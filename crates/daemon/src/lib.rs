//! Shared daemon runtime assembly for local hosts.
//!
//! This crate owns the backend wiring used by the CLI defaults, the desktop
//! host, and the thin `msv-daemon` launcher. Cargo still owns Rust dependency
//! resolution and workspace task boundaries.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use my_supervisor_application::{AppDeps, DaemonMeta};
use my_supervisor_config::TomlConfigSource;
use my_supervisor_core::ports::{
    LifecycleController, LogSink, ProcessServiceRegistrar, RealClock, ShutdownSignaler,
};
use my_supervisor_infra_http::{assemble, Assembled};
use my_supervisor_infra_logging::InMemoryLogSink;
use my_supervisor_infra_scheduler::TokioScheduler;
use my_supervisor_infra_sqlite::SqliteStore;

pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1:9876";
pub const DEFAULT_BIND_PORT: u16 = 9876;
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:9876";

/// Private, typed failure controls used only by the debug daemon integration
/// host.  They are deliberately not represented in config, HTTP DTOs, or a
/// child `ProcessSpec`; a test must possess the private Unix socket selected
/// by the launcher to arm one.
#[derive(Clone)]
pub struct DaemonTestControls {
    store: Arc<SqliteStore>,
}

impl DaemonTestControls {
    pub fn apply_line(&self, line: &str) -> Result<(), String> {
        let mut fields = line.split_whitespace();
        let command = fields.next().ok_or_else(|| "missing control command".to_string())?;
        let count = fields
            .next()
            .ok_or_else(|| "missing failure count".to_string())?
            .parse::<u32>()
            .map_err(|_| "failure count must be an unsigned integer".to_string())?;
        if fields.next().is_some() {
            return Err("unexpected control arguments".into());
        }
        match command {
            "terminal_ack" => self.store.fail_next_transient_terminal_acknowledgements(count),
            "deletion_rollback_direction" => self
                .store
                .fail_next_job_deletion_rollback_direction_updates(count),
            "deletion_cancellation" => self.store.fail_next_job_deletion_cancellations(count),
            "deletion_rows" => self.store.fail_next_job_deletion_row_commits(count),
            "deletion_clear" => self.store.fail_next_job_deletion_journal_clears(count),
            "config_snapshot" => self.store.fail_next_config_snapshot_commits(count),
            _ => return Err(format!("unknown control command: {command}")),
        }
        Ok(())
    }
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("my-supervisor")
}

pub async fn build_runtime() -> anyhow::Result<Assembled> {
    Ok(assemble(build_deps().await?))
}

pub async fn build_deps() -> anyhow::Result<AppDeps> {
    let base = data_dir();
    let config_path: PathBuf = dirs::config_dir()
        .map(|path| path.join("my-supervisor").join("config.toml"))
        .unwrap_or_else(|| base.join("config.toml"));
    build_deps_with_runtime_helpers(base, config_path).await
}

/// Build the production adapter graph with caller-supplied persistent paths.
/// Hosts keep using [`build_deps`]; integration hosts can use this to isolate
/// one daemon instance without changing the selected platform adapters.
pub async fn build_deps_with_paths(
    base: PathBuf,
    config_path: PathBuf,
) -> anyhow::Result<AppDeps> {
    build_deps_with_helpers(base, config_path, None).await
}

/// Assemble an isolated daemon plus its private integration-test controls.
/// Production hosts keep using `build_runtime` / `build_deps` and never expose
/// the concrete SQLite adapter that these controls require.
pub async fn build_test_runtime_with_paths(
    base: PathBuf,
    config_path: PathBuf,
) -> anyhow::Result<(Assembled, DaemonTestControls)> {
    let (deps, store) = build_deps_with_helpers_and_store(base, config_path, None).await?;
    Ok((assemble(deps), DaemonTestControls { store }))
}

async fn build_deps_with_runtime_helpers(
    base: PathBuf,
    config_path: PathBuf,
) -> anyhow::Result<AppDeps> {
    #[cfg(target_os = "macos")]
    let detached_helpers = Some(detached_helper_paths_for_current_runtime()?);
    #[cfg(not(target_os = "macos"))]
    let detached_helpers = None;
    build_deps_with_helpers(base, config_path, detached_helpers).await
}

async fn build_deps_with_helpers(
    base: PathBuf,
    config_path: PathBuf,
    #[cfg(target_os = "macos")] detached_helpers: Option<my_supervisor_platform_macos::DetachedHelperPaths>,
    #[cfg(not(target_os = "macos"))] _detached_helpers: Option<()>,
) -> anyhow::Result<AppDeps> {
    Ok(build_deps_with_helpers_and_store(
        base,
        config_path,
        #[cfg(target_os = "macos")]
        detached_helpers,
        #[cfg(not(target_os = "macos"))]
        _detached_helpers,
    )
    .await?
    .0)
}

async fn build_deps_with_helpers_and_store(
    base: PathBuf,
    config_path: PathBuf,
    #[cfg(target_os = "macos")] detached_helpers: Option<my_supervisor_platform_macos::DetachedHelperPaths>,
    #[cfg(not(target_os = "macos"))] _detached_helpers: Option<()>,
) -> anyhow::Result<(AppDeps, Arc<SqliteStore>)> {
    tokio::fs::create_dir_all(&base).await.ok();
    let log_dir = base.join("logs");
    tokio::fs::create_dir_all(&log_dir).await.ok();
    let db_path = base.join("state.db");

    let log_sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(log_dir.clone()));
    let (lifecycle, shutdown) = platform_adapters(
        log_sink.clone(),
        log_dir.clone(),
        #[cfg(target_os = "macos")]
        detached_helpers,
    );
    let registrar = process_service_registrar(log_dir.clone());

    let store = Arc::new(
        SqliteStore::connect(&db_path)
            .await
            .with_context(|| format!("opening sqlite at {}", db_path.display()))?,
    );

    let deps = AppDeps {
        lifecycle,
        shutdown,
        registrar,
        state_repo: store.clone(),
        job_repo: store.clone(),
        scheduler: Arc::new(TokioScheduler::new()),
        log_sink,
        clock: Arc::new(RealClock),
        config: Arc::new(TomlConfigSource::new(config_path.clone())),
        meta: DaemonMeta::new(config_path, log_dir),
    };
    Ok((deps, store))
}

#[cfg(target_os = "macos")]
fn platform_adapters(
    log_sink: Arc<dyn LogSink>,
    log_dir: PathBuf,
    detached_helpers: Option<my_supervisor_platform_macos::DetachedHelperPaths>,
) -> (Arc<dyn LifecycleController>, Arc<dyn ShutdownSignaler>) {
    use my_supervisor_platform_macos::{MacLifecycle, UnixShutdown};
    let lifecycle = match detached_helpers {
        Some(detached_helpers) => MacLifecycle::with_detached_helpers(log_sink, log_dir, detached_helpers),
        None => MacLifecycle::new(log_sink, log_dir),
    };
    (
        Arc::new(lifecycle),
        Arc::new(UnixShutdown::new()),
    )
}

#[cfg(not(target_os = "macos"))]
fn platform_adapters(
    _log_sink: Arc<dyn LogSink>,
    _log_dir: PathBuf,
) -> (Arc<dyn LifecycleController>, Arc<dyn ShutdownSignaler>) {
    unimplemented!("daemon runtime currently supports macOS only (Linux/Windows deferred)")
}

#[cfg(target_os = "macos")]
fn detached_helper_paths_for_current_runtime() -> anyhow::Result<my_supervisor_platform_macos::DetachedHelperPaths> {
    let executable = std::env::current_exe().context("resolving the runtime executable for detached helpers")?;
    detached_helper_paths_from_executable(&executable)
}

#[cfg(target_os = "macos")]
fn detached_helper_paths_from_executable(
    executable: &Path,
) -> anyhow::Result<my_supervisor_platform_macos::DetachedHelperPaths> {
    let helper_dir = executable.parent().ok_or_else(|| anyhow::anyhow!(
        "runtime executable {} has no parent directory for detached helpers",
        executable.display()
    ))?;
    my_supervisor_platform_macos::DetachedHelperPaths::new(
        helper_dir.join("msv-log-proxy"),
        helper_dir.join("msv-group-reaper"),
    )
    .map_err(anyhow::Error::msg)
}

#[cfg(target_os = "macos")]
fn process_service_registrar(log_dir: PathBuf) -> Arc<dyn ProcessServiceRegistrar> {
    Arc::new(my_supervisor_platform_macos::LaunchdAgentProcess::new(
        log_dir,
    ))
}

#[cfg(not(target_os = "macos"))]
fn process_service_registrar(_log_dir: PathBuf) -> Arc<dyn ProcessServiceRegistrar> {
    Arc::new(my_supervisor_application::NullProcessServiceRegistrar)
}
