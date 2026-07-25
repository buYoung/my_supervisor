#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use my_supervisor_application::{AppDeps, DaemonMeta, NullProcessServiceRegistrar, OperationsFacade};
use my_supervisor_config::TomlConfigSource;
use my_supervisor_core::domain::{
    DependencyFailurePolicy, Job, JobId, JobRunState, JobTrigger, LogRetention, OverlapPolicy,
};
use my_supervisor_core::ports::{LogSink, RealClock};
use my_supervisor_infra_logging::InMemoryLogSink;
use my_supervisor_infra_scheduler::TokioScheduler;
use my_supervisor_infra_sqlite::SqliteStore;
use my_supervisor_platform_macos::{MacLifecycle, UnixShutdown};

fn long_running_job(name: &str, overlap: OverlapPolicy) -> Job {
    Job {
        id: JobId::new(),
        name: name.into(),
        command: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            "trap 'exit 0' TERM; sleep 30 & wait".into(),
        ],
        cwd: None,
        env: BTreeMap::new(),
        trigger: JobTrigger::Interval(Duration::from_secs(60)),
        on_overlap: overlap,
        on_dependency_failure: DependencyFailurePolicy::Skip,
        timeout: None,
        log_retention: LogRetention::default(),
    }
}

async fn facade() -> (Arc<OperationsFacade>, PathBuf) {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("my-supervisor-run-shutdown-{}-{nonce}", std::process::id()));
    let log_dir = root.join("logs");
    tokio::fs::create_dir_all(&log_dir).await.unwrap();
    let store = Arc::new(SqliteStore::connect(&root.join("state.db")).await.unwrap());
    let log_sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(log_dir.clone()));
    let dependencies = AppDeps {
        lifecycle: Arc::new(MacLifecycle::new(log_sink.clone(), log_dir.clone())),
        shutdown: Arc::new(UnixShutdown::new()),
        registrar: Arc::new(NullProcessServiceRegistrar),
        state_repo: store.clone(),
        job_repo: store,
        scheduler: Arc::new(TokioScheduler::new()),
        log_sink,
        clock: Arc::new(RealClock),
        config: Arc::new(TomlConfigSource::new(root.join("config.toml"))),
        meta: DaemonMeta::new(root.join("config.toml"), log_dir),
    };
    (OperationsFacade::new(dependencies), root)
}

#[tokio::test]
async fn force_delete_and_shutdown_wait_for_real_active_runs() {
    let (facade, root) = facade().await;
    facade
        .add_job(long_running_job("force-delete", OverlapPolicy::Parallel))
        .await
        .unwrap();
    let first = facade.trigger_job("force-delete").await.unwrap();
    let second = facade.trigger_job("force-delete").await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let error = facade.delete_job("force-delete", false).await.unwrap_err();
    assert_eq!(error.code(), "job_has_active_runs");
    facade.delete_job("force-delete", true).await.unwrap();
    assert!(facade.get_job("force-delete").await.is_err());
    assert!(facade.get_run("force-delete", &first).await.is_err());
    assert!(facade.get_run("force-delete", &second).await.is_err());

    facade
        .add_job(long_running_job("shutdown", OverlapPolicy::Queue))
        .await
        .unwrap();
    let active = facade.trigger_job("shutdown").await.unwrap();
    let queued = facade.trigger_job("shutdown").await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    facade.shutdown_all().await.unwrap();
    assert_eq!(facade.get_run("shutdown", &active).await.unwrap().state, JobRunState::Cancelled);
    assert_eq!(facade.get_run("shutdown", &queued).await.unwrap().state, JobRunState::Cancelled);

    tokio::fs::remove_dir_all(root).await.unwrap();
}
