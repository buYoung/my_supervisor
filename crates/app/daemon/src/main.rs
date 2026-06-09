//! `msv-daemon` — the headless host. Embeds the shared core, binds the
//! operations Router on `127.0.0.1:9876` (DD-011, loopback no-auth), and
//! handles graceful shutdown (SIGINT/SIGTERM or `POST /daemon/shutdown`),
//! reaping tied children so none are orphaned. The only `#[cfg(target_os)]` in
//! the app lives here, in the DI assembly (DD-018).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use my_supervisor_application::{AppDeps, DaemonMeta};
use my_supervisor_config::TomlConfigSource;
use my_supervisor_core::ports::{
    LifecycleController, LogSink, ProcessServiceRegistrar, RealClock, ShutdownSignaler,
};
use my_supervisor_infra_http::assemble;
use my_supervisor_infra_logging::InMemoryLogSink;
use my_supervisor_infra_scheduler::TokioScheduler;
use my_supervisor_infra_sqlite::SqliteStore;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tracing::info;
use tracing_subscriber::EnvFilter;

const BIND_ADDR: &str = "127.0.0.1:9876";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let deps = build_deps().await?;
    let assembled = assemble(deps);
    let facade = assembled.facade;

    facade
        .bootstrap()
        .await
        .context("bootstrap (config load + scheduler arm + autostart)")?;
    tokio::spawn(facade.clone().run_scheduler_loop());

    let listener = TcpListener::bind(BIND_ADDR)
        .await
        .with_context(|| format!("binding {BIND_ADDR}"))?;
    info!("msv-daemon listening on http://{BIND_ADDR}");

    let shutdown = facade.shutdown_signal();
    axum::serve(listener, assembled.router)
        .with_graceful_shutdown(wait_for_shutdown(shutdown))
        .await
        .context("http server")?;

    info!("shutting down; reaping tied children");
    facade.shutdown_children().await;
    Ok(())
}

/// Build the wired adapters. This is the composition root — the only place
/// `#[cfg(target_os)]` selects platform adapters.
async fn build_deps() -> anyhow::Result<AppDeps> {
    let base = dirs::data_dir()
        .context("locating the application data directory")?
        .join("my-supervisor");
    tokio::fs::create_dir_all(&base).await.ok();
    let log_dir = base.join("logs");
    tokio::fs::create_dir_all(&log_dir).await.ok();
    let db_path = base.join("state.db");

    let config_path: PathBuf = dirs::config_dir()
        .map(|p| p.join("my-supervisor").join("config.toml"))
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
    compile_error!("app/daemon currently supports macOS only (Linux/Windows deferred)");
    unreachable!()
}

/// Per-process SystemRegistered registrar — the macOS launchd adapter (child
/// 06); the null registrar elsewhere so the Direct path still builds.
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

/// Resolve when a shutdown is requested via signal or the HTTP endpoint.
async fn wait_for_shutdown(notify: Arc<Notify>) {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("received SIGINT"),
        _ = terminate => info!("received SIGTERM"),
        _ = notify.notified() => info!("received shutdown request"),
    }
}
