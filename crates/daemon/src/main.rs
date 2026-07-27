//! `msv-daemon` — a thin launcher around the shared daemon runtime.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use my_supervisor_app_daemon::{
    build_runtime, build_test_runtime_with_paths, DaemonTestControls, DEFAULT_BIND_ADDR,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UnixListener};
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

    let launch = LaunchOptions::from_debug_environment()?;
    let (assembled, controls) = match &launch.test_root {
        Some(root) => {
            let config_path = launch
                .test_config_path
                .clone()
                .unwrap_or_else(|| root.join("config.toml"));
            let (assembled, controls) = build_test_runtime_with_paths(
                root.clone(),
                config_path,
                format!("http://{}", launch.bind_addr),
            )
            .await?;
            (assembled, Some(controls))
        }
        None => (build_runtime().await?, None),
    };
    if let (Some(socket_path), Some(controls)) = (&launch.control_socket, controls) {
        tokio::spawn(serve_test_controls(socket_path.clone(), controls));
    }
    let facade = assembled.facade;

    facade
        .bootstrap()
        .await
        .context("bootstrap (config load + scheduler arm + autostart)")?;
    let scheduler = tokio::spawn(facade.clone().run_scheduler_loop());
    let supervisor = tokio::spawn(facade.clone().run_process_supervisor_loop());
    #[cfg(target_os = "macos")]
    let observability = tokio::spawn(facade.clone().run_observability_loop(Arc::new(
        my_supervisor_platform_macos::NotificationCenterDelivery::new(),
    )));

    let listener = TcpListener::bind(launch.bind_addr)
        .await
        .with_context(|| format!("binding {}", launch.bind_addr))?;
    info!(address = %launch.bind_addr, "msv-daemon listening");

    let shutdown = facade.shutdown_signal();
    axum::serve(listener, assembled.router)
        .with_graceful_shutdown(wait_for_shutdown(shutdown))
        .await
        .context("http server")?;

    info!("shutting down; draining runs and reaping tied children");
    facade.shutdown_all().await.context("shutdown drain")?;
    scheduler.await.context("scheduler join")?;
    supervisor.await.context("process supervisor join")?;
    #[cfg(target_os = "macos")]
    observability.await.context("observability worker join")?;
    Ok(())
}

struct LaunchOptions {
    bind_addr: SocketAddr,
    test_root: Option<PathBuf>,
    test_config_path: Option<PathBuf>,
    control_socket: Option<PathBuf>,
}

impl LaunchOptions {
    fn from_debug_environment() -> anyhow::Result<Self> {
        #[cfg(debug_assertions)]
        {
            let test_root = std::env::var_os("MSV_DAEMON_TEST_DATA_DIR").map(PathBuf::from);
            let test_config_path =
                std::env::var_os("MSV_DAEMON_TEST_CONFIG_PATH").map(PathBuf::from);
            let control_socket =
                std::env::var_os("MSV_DAEMON_TEST_CONTROL_SOCKET").map(PathBuf::from);
            if (test_config_path.is_some() || control_socket.is_some()) && test_root.is_none() {
                anyhow::bail!("MSV_DAEMON_TEST_* controls require MSV_DAEMON_TEST_DATA_DIR");
            }
            let bind_addr = parse_loopback_bind_addr(
                &std::env::var("MSV_DAEMON_TEST_BIND_ADDR")
                    .unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string()),
            )?;
            Ok(Self {
                bind_addr,
                test_root,
                test_config_path,
                control_socket,
            })
        }
        #[cfg(not(debug_assertions))]
        {
            if std::env::vars_os()
                .any(|(key, _)| key.to_string_lossy().starts_with("MSV_DAEMON_TEST_"))
            {
                anyhow::bail!("MSV_DAEMON_TEST_* controls are unavailable outside debug builds");
            }
            Ok(Self {
                bind_addr: parse_loopback_bind_addr(DEFAULT_BIND_ADDR)?,
                test_root: None,
                test_config_path: None,
                control_socket: None,
            })
        }
    }
}

fn parse_loopback_bind_addr(value: &str) -> anyhow::Result<SocketAddr> {
    let bind_addr = value
        .parse::<SocketAddr>()
        .with_context(|| format!("parsing daemon bind address {value}"))?;
    if !bind_addr.ip().is_loopback() {
        anyhow::bail!("daemon bind address must be loopback, got {bind_addr}");
    }
    Ok(bind_addr)
}

async fn serve_test_controls(socket_path: PathBuf, controls: DaemonTestControls) {
    let _ = tokio::fs::remove_file(&socket_path).await;
    let Some(parent) = socket_path.parent() else {
        tracing::error!(path = %socket_path.display(), "test control socket has no parent directory");
        return;
    };
    if let Err(error) = tokio::fs::create_dir_all(parent).await {
        tracing::error!(%error, path = %parent.display(), "creating test control directory failed");
        return;
    }
    let listener = match UnixListener::bind(&socket_path) {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%error, path = %socket_path.display(), "binding test control socket failed");
            return;
        }
    };
    #[cfg(unix)]
    if let Err(error) = std::fs::set_permissions(
        &socket_path,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    ) {
        tracing::error!(%error, path = %socket_path.display(), "securing test control socket failed");
        return;
    }
    loop {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        let controls = controls.clone();
        tokio::spawn(async move {
            let mut bytes = [0_u8; 256];
            let size = match stream.read(&mut bytes).await {
                Ok(size) => size,
                Err(_) => return,
            };
            let response = std::str::from_utf8(&bytes[..size])
                .map_err(|_| "control command must be UTF-8".to_string())
                .and_then(|line| controls.apply_line(line.trim()))
                .map(|_| "ok\n".to_string())
                .unwrap_or_else(|error| format!("error: {error}\n"));
            let _ = stream.write_all(response.as_bytes()).await;
        });
    }
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
