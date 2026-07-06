//! `msv-daemon` — a thin launcher around the shared daemon runtime.

use std::sync::Arc;

use anyhow::Context;
use my_supervisor_app_daemon::{build_runtime, DEFAULT_BIND_ADDR};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let assembled = build_runtime().await?;
    let facade = assembled.facade;

    facade
        .bootstrap()
        .await
        .context("bootstrap (config load + scheduler arm + autostart)")?;
    tokio::spawn(facade.clone().run_scheduler_loop());

    let listener = TcpListener::bind(DEFAULT_BIND_ADDR)
        .await
        .with_context(|| format!("binding {DEFAULT_BIND_ADDR}"))?;
    info!("msv-daemon listening on http://{DEFAULT_BIND_ADDR}");

    let shutdown = facade.shutdown_signal();
    axum::serve(listener, assembled.router)
        .with_graceful_shutdown(wait_for_shutdown(shutdown))
        .await
        .context("http server")?;

    info!("shutting down; reaping tied children");
    facade.shutdown_children().await;
    Ok(())
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
