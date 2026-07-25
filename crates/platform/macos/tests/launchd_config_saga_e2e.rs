#![cfg(target_os = "macos")]

use std::sync::Arc;
use std::time::Duration;

use my_supervisor_application::{AppDeps, DaemonMeta, OperationsFacade};
use my_supervisor_config::TomlConfigSource;
use my_supervisor_core::domain::{ApplyMode, LifecycleMode, LoadedConfig, ManagementMode, ProcessSpec};
use my_supervisor_core::ports::{JobRepository, LogSink, ProcessServiceRegistrar, RealClock};
use my_supervisor_infra_logging::InMemoryLogSink;
use my_supervisor_infra_scheduler::TokioScheduler;
use my_supervisor_infra_sqlite::SqliteStore;
use my_supervisor_platform_macos::{
    LaunchdAgentProcess, LaunchdTestControls, MacLifecycle, UnixShutdown,
};

fn temporary_directory() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("my-supervisor-launchd-config-saga-{}", uuid::Uuid::new_v4()))
}

async fn wait_for_pid(
    registrar: &LaunchdAgentProcess,
    label: &str,
) -> Result<u32, my_supervisor_core::ports::RegistrarError> {
    for _ in 0..40 {
        if let Some(pid) = registrar.query_pid(label).await? {
            return Ok(pid);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(my_supervisor_core::ports::RegistrarError::RegistrationFailed(
        "launchd unit did not publish a PID".into(),
    ))
}

async fn has_candidate_plist(agents_dir: &std::path::Path, label: &str) -> bool {
    let mut entries = tokio::fs::read_dir(agents_dir)
        .await
        .expect("LaunchAgents directory is readable");
    let prefix = format!(".{label}.");
    while let Some(entry) = entries.next_entry().await.expect("LaunchAgents entry reads") {
        let file_name = entry.file_name();
        if file_name.to_string_lossy().starts_with(&prefix) {
            return true;
        }
    }
    false
}

async fn facade(
    directory: &std::path::Path,
    registrar: Arc<dyn ProcessServiceRegistrar>,
) -> (Arc<OperationsFacade>, Arc<SqliteStore>) {
    let log_dir = directory.join("logs");
    tokio::fs::create_dir_all(&log_dir).await.expect("log directory creates");
    let log_sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(log_dir.clone()));
    let store = Arc::new(SqliteStore::connect(directory.join("state.db")).await.expect("SQLite opens"));
    let facade = OperationsFacade::new(AppDeps {
        lifecycle: Arc::new(MacLifecycle::new(log_sink.clone(), log_dir.clone())),
        shutdown: Arc::new(UnixShutdown::new()),
        registrar,
        state_repo: store.clone(),
        job_repo: store.clone(),
        scheduler: Arc::new(TokioScheduler::new()),
        log_sink,
        clock: Arc::new(RealClock),
        config: Arc::new(TomlConfigSource::new(directory.join("config.toml"))),
        meta: DaemonMeta::new(directory.join("config.toml"), log_dir),
    });
    (facade, store)
}

fn system_registered_spec(name: &str, unit_name: String) -> ProcessSpec {
    let mut spec = ProcessSpec::new(name, "/bin/sh");
    spec.args = vec!["-c".into(), "sleep 30".into()];
    spec.management_mode = ManagementMode::SystemRegistered { unit_name };
    spec.lifecycle = LifecycleMode::Tied;
    spec
}

#[tokio::test]
#[ignore = "requires an interactive macOS GUI launchd session"]
async fn application_config_prepare_failure_preserves_existing_unit_and_cleans_temporary_plists() {
    let directory = temporary_directory();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let label = format!("com.my-supervisor.config-saga.{}", uuid::Uuid::new_v4());
    // A slash makes the generated plist parent absent.  It exercises the
    // registrar's real write failure before the old unit may be removed.
    let failing_label = format!("{label}/prepare-failure");
    let spec = system_registered_spec("config-saga", label.clone());
    let failing_spec = system_registered_spec("config-saga", failing_label.clone());
    let registrar = LaunchdAgentProcess::new(directory.clone());
    let home = std::env::var("HOME").expect("GUI-session HOME is available");
    let plist = std::path::PathBuf::from(home)
        .join("Library/LaunchAgents")
        .join(format!("{label}.plist"));
    let failed_plist = std::path::PathBuf::from(std::env::var("HOME").unwrap())
        .join("Library/LaunchAgents")
        .join(format!("{failing_label}.plist"));
    // SAFETY: getuid is a pure libc query.
    let target = format!("gui/{}/{}", unsafe { libc::getuid() }, label);

    let facade = facade(
        &directory,
        Arc::new(LaunchdAgentProcess::new(directory.join("logs"))),
    )
    .await
    .0;
    let result = async {
        facade
            .apply_config(LoadedConfig { processes: vec![spec], jobs: Vec::new() }, ApplyMode::Replace, false)
            .await
            .map_err(|error| my_supervisor_core::ports::RegistrarError::RegistrationFailed(error.to_string()))?;
        registrar.start(&label).await?;
        let original_pid = wait_for_pid(&registrar, &label).await?;

        assert!(facade
            .apply_config(LoadedConfig { processes: vec![failing_spec], jobs: Vec::new() }, ApplyMode::Replace, false)
            .await
            .is_err());
        assert_eq!(wait_for_pid(&registrar, &label).await?, original_pid);
        assert!(plist.exists(), "prepare failure removed the existing unit plist");
        assert!(!failed_plist.exists(), "failed prepare left a candidate plist");
        Ok::<(), my_supervisor_core::ports::RegistrarError>(())
    }
    .await;

    let cleanup = facade.apply_config(LoadedConfig::default(), ApplyMode::Replace, false).await;
    let print_status = tokio::process::Command::new("launchctl")
        .args(["print", &target])
        .status()
        .await
        .expect("launchctl print starts");
    let plist_removed = !plist.exists();
    let failed_plist_removed = !failed_plist.exists();
    let _ = tokio::fs::remove_dir_all(&directory).await;

    result.expect("application config prepare failure preserves the existing unit");
    cleanup.expect("application config cleanup succeeds");
    assert!(!print_status.success(), "removed launchd unit remains registered");
    assert!(plist_removed, "removed launchd plist remains on disk");
    assert!(failed_plist_removed, "failed candidate plist remains on disk");
}

#[tokio::test]
#[ignore = "requires an interactive macOS GUI launchd session"]
async fn system_registered_replace_forward_recovers_after_old_unit_removal_and_start_failure() {
    let directory = temporary_directory();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let old_label = format!("com.my-supervisor.config-saga.old.{}", uuid::Uuid::new_v4());
    let target_label = format!("com.my-supervisor.config-saga.target.{}", uuid::Uuid::new_v4());
    let home = std::env::var("HOME").expect("GUI-session HOME is available");
    let agents_dir = std::path::PathBuf::from(&home).join("Library/LaunchAgents");
    let old_plist = agents_dir.join(format!("{old_label}.plist"));
    let target_plist = agents_dir.join(format!("{target_label}.plist"));
    // SAFETY: getuid is a pure libc query.
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let old_target = format!("{domain}/{old_label}");
    let target_target = format!("{domain}/{target_label}");
    let controls = Arc::new(LaunchdTestControls::default());
    let registrar = Arc::new(LaunchdAgentProcess::with_test_controls(
        directory.join("logs"),
        controls.clone(),
    ));
    let (first_facade, store) = facade(&directory, registrar.clone()).await;
    let mut old_spec = system_registered_spec("old-config-saga", old_label.clone());
    old_spec.autostart = true;
    let mut target_spec = system_registered_spec("target-config-saga", target_label.clone());
    target_spec.autostart = true;

    let result = async {
        first_facade
            .apply_config(
                LoadedConfig { processes: vec![old_spec], jobs: Vec::new() },
                ApplyMode::Replace,
                false,
            )
            .await
            .expect("old SystemRegistered unit starts");
        let old_pid = wait_for_pid(&registrar, &old_label).await?;
        controls.fail_next_start();

        let error = first_facade
            .apply_config(
                LoadedConfig { processes: vec![target_spec.clone()], jobs: Vec::new() },
                ApplyMode::Replace,
                false,
            )
            .await
            .expect_err("one-shot target start failure crosses the old-unit boundary");
        assert_eq!(error.code(), "config_recovery_required");
        let journals = store.list_incomplete_config_applies().await
            .expect("forward recovery journal remains durable");
        assert_eq!(journals.len(), 1);
        assert_eq!(journals[0].stage, my_supervisor_core::domain::ConfigApplyStage::ForwardRecovery);
        assert!(registrar.query_pid(&old_label).await.is_err(), "old unit survived the forward boundary");
        assert!(old_pid > 0, "old unit never had a live PID");
        assert!(!old_plist.exists(), "old plist survived the forward boundary");
        assert!(target_plist.exists(), "prepared target plist is missing");
        assert!(!has_candidate_plist(&agents_dir, &target_label).await, "candidate temporary plist remains");
        Ok::<(), my_supervisor_core::ports::RegistrarError>(())
    }
    .await;

    // Recreate the facade against the same SQLite file to exercise the daemon
    // bootstrap recovery boundary without inheriting any in-memory state.
    drop(first_facade);
    let (restarted_facade, restarted_store) = facade(&directory, registrar.clone()).await;
    let recovery = restarted_facade.recover_incomplete_config_apply().await;
    let target_pid = wait_for_pid(&registrar, &target_label).await;
    let cleanup = async {
        restarted_facade
            .apply_config(LoadedConfig::default(), ApplyMode::Replace, false)
            .await
            .map_err(|error| my_supervisor_core::ports::RegistrarError::RegistrationFailed(error.to_string()))?;
        Ok::<(), my_supervisor_core::ports::RegistrarError>(())
    }
    .await;
    let old_status = tokio::process::Command::new("launchctl")
        .args(["print", &old_target])
        .status()
        .await
        .expect("old launchctl print starts");
    let target_status = tokio::process::Command::new("launchctl")
        .args(["print", &target_target])
        .status()
        .await
        .expect("target launchctl print starts");
    let journals_cleared = restarted_store.list_incomplete_config_applies().await
        .expect("recovery journal lookup succeeds")
        .is_empty();
    let plists_removed = !old_plist.exists()
        && !target_plist.exists()
        && !has_candidate_plist(&agents_dir, &old_label).await
        && !has_candidate_plist(&agents_dir, &target_label).await;
    let directory_cleanup = tokio::fs::remove_dir_all(&directory).await;

    result.expect("old unit removal and injected target-start failure are observed");
    recovery.expect("same-DB facade restart forward-recovers the target unit");
    target_pid.expect("forward recovery did not start the target unit");
    cleanup.expect("target cleanup succeeds");
    assert!(!old_status.success(), "old launchd unit remains registered after recovery cleanup");
    assert!(!target_status.success(), "target launchd unit remains registered after cleanup");
    assert!(journals_cleared, "config recovery journal remains after forward recovery");
    assert!(plists_removed, "launchd plist or candidate temporary plist remains after cleanup");
    directory_cleanup.expect("test directory cleanup succeeds");
}
