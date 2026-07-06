//! Shared daemon runtime assembly for local hosts.
//!
//! This crate owns the backend wiring used by the CLI defaults, the desktop
//! host, and the thin `msv-daemon` launcher. Cargo still owns Rust dependency
//! resolution; Turborepo only orchestrates the workspace tasks.

use std::path::PathBuf;
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
    tokio::fs::create_dir_all(&base).await.ok();
    let log_dir = base.join("logs");
    tokio::fs::create_dir_all(&log_dir).await.ok();
    let db_path = base.join("state.db");

    let config_path: PathBuf = dirs::config_dir()
        .map(|path| path.join("my-supervisor").join("config.toml"))
        .unwrap_or_else(|| base.join("config.toml"));

    let log_sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::new());
    let (lifecycle, shutdown) = platform_adapters(log_sink.clone());
    let registrar = process_service_registrar(log_dir.clone());

    let store = Arc::new(
        SqliteStore::connect(&db_path)
            .await
            .with_context(|| format!("opening sqlite at {}", db_path.display()))?,
    );

    Ok(AppDeps {
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
    })
}

#[cfg(target_os = "macos")]
fn platform_adapters(
    log_sink: Arc<dyn LogSink>,
) -> (Arc<dyn LifecycleController>, Arc<dyn ShutdownSignaler>) {
    use my_supervisor_platform_macos::{MacLifecycle, UnixShutdown};
    (
        Arc::new(MacLifecycle::new(log_sink)),
        Arc::new(UnixShutdown::new()),
    )
}

#[cfg(not(target_os = "macos"))]
fn platform_adapters(
    _log_sink: Arc<dyn LogSink>,
) -> (Arc<dyn LifecycleController>, Arc<dyn ShutdownSignaler>) {
    unimplemented!("daemon runtime currently supports macOS only (Linux/Windows deferred)")
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
