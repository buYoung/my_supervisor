//! `my-supervisor` desktop host (Tauri v2). Embeds the shared core in-process
//! (DD-002: no daemon spawn). The production UI ↔ core path is `tauri invoke`;
//! a test-only devBridge mirrors the same facade over loopback HTTP so vibe-
//! coding test automation can drive operations without `invoke`. Parity holds
//! because both the invoke handlers and the devBridge routes call the SAME
//! `OperationsFacade` instance.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tokio::net::TcpListener;

use my_supervisor_app_daemon::{build_runtime, data_dir, DEFAULT_BIND_PORT};
use my_supervisor_application::{AppError, ConvertTarget, OperationsFacade, RestartOutcome};
use my_supervisor_infra_http::mapping::{
    daemon_info_to_dto, job_config_to_job, job_run_to_dto, job_view_to_dto, log_line_to_dto,
    log_page_to_dto, process_config_to_spec, process_status_to_dto,
};
use my_supervisor_infra_http::Assembled;
use my_supervisor_shared::api::{
    ConvertTargetDto, DaemonStatusDto, JobConfigDto, JobListDto, JobRunListDto, LogsResponseDto,
    ProcessConfigDto, ProcessListDto, ProcessStatusDto,
};

/// Serializable command error carrying the API §5 code.
#[derive(Debug, Serialize)]
struct CmdError {
    code: String,
    message: String,
}

impl From<AppError> for CmdError {
    fn from(e: AppError) -> Self {
        CmdError {
            code: e.code().to_string(),
            message: e.to_string(),
        }
    }
}

type CmdResult<T> = Result<T, CmdError>;
type Facade<'a> = State<'a, Arc<OperationsFacade>>;

// --- invoke handlers (production UI transport) ------------------------------
// Each is a thin adapter over the shared facade + the infra/http mapping — the
// SAME facade the devBridge routes call. No domain logic here (parity).

#[tauri::command]
async fn cmd_list_processes(facade: Facade<'_>) -> CmdResult<ProcessListDto> {
    let processes = facade
        .list_processes()
        .await?
        .into_iter()
        .map(process_status_to_dto)
        .collect();
    Ok(ProcessListDto { processes })
}

#[tauri::command]
async fn cmd_get_process(facade: Facade<'_>, name: String) -> CmdResult<ProcessStatusDto> {
    Ok(process_status_to_dto(facade.get_process(&name).await?))
}

#[tauri::command]
async fn cmd_add_process(
    facade: Facade<'_>,
    config: ProcessConfigDto,
) -> CmdResult<ProcessStatusDto> {
    let status = facade.add_process(process_config_to_spec(config)).await?;
    Ok(process_status_to_dto(status))
}

#[tauri::command]
async fn cmd_start_process(facade: Facade<'_>, name: String) -> CmdResult<()> {
    facade.start_process(&name).await.map_err(Into::into)
}

#[tauri::command]
async fn cmd_stop_process(facade: Facade<'_>, name: String, force: bool) -> CmdResult<()> {
    facade.stop_process(&name, force).await.map_err(Into::into)
}

#[tauri::command]
async fn cmd_restart_process(facade: Facade<'_>, name: String) -> CmdResult<serde_json::Value> {
    match facade.restart_process(&name).await? {
        RestartOutcome::Accepted => Ok(serde_json::json!({ "noop": false })),
        RestartOutcome::Noop { reason } => {
            Ok(serde_json::json!({ "noop": true, "reason": reason }))
        }
    }
}

#[tauri::command]
async fn cmd_remove_process(facade: Facade<'_>, name: String, force: bool) -> CmdResult<()> {
    facade
        .remove_process(&name, force)
        .await
        .map_err(Into::into)
}

#[tauri::command]
async fn cmd_convert_process(
    facade: Facade<'_>,
    name: String,
    to: ConvertTargetDto,
    unit_name: Option<String>,
    auto_start: Option<bool>,
) -> CmdResult<ProcessStatusDto> {
    let target = match to {
        ConvertTargetDto::Direct => ConvertTarget::Direct,
        ConvertTargetDto::SystemRegistered => ConvertTarget::SystemRegistered,
    };
    let status = facade
        .convert_process(&name, target, unit_name, auto_start.unwrap_or(false))
        .await?;
    Ok(process_status_to_dto(status))
}

#[tauri::command]
async fn cmd_process_logs(
    facade: Facade<'_>,
    name: String,
    tail: usize,
) -> CmdResult<LogsResponseDto> {
    Ok(log_page_to_dto(
        facade.process_logs(&name, tail, None).await?,
    ))
}

#[tauri::command]
async fn cmd_list_jobs(facade: Facade<'_>) -> CmdResult<JobListDto> {
    let jobs = facade
        .list_jobs()
        .await?
        .into_iter()
        .map(job_view_to_dto)
        .collect();
    Ok(JobListDto { jobs })
}

#[tauri::command]
async fn cmd_add_job(facade: Facade<'_>, config: JobConfigDto) -> CmdResult<serde_json::Value> {
    let view = facade.add_job(job_config_to_job(config)).await?;
    serde_json::to_value(job_view_to_dto(view)).map_err(|e| CmdError {
        code: "internal_error".into(),
        message: e.to_string(),
    })
}

#[tauri::command]
async fn cmd_remove_job(facade: Facade<'_>, name: String, force: bool) -> CmdResult<()> {
    facade.delete_job(&name, force).await.map_err(Into::into)
}

#[tauri::command]
async fn cmd_trigger_job(facade: Facade<'_>, name: String) -> CmdResult<serde_json::Value> {
    let run_id = facade.trigger_job(&name).await?;
    Ok(serde_json::json!({ "run_id": run_id.0.to_string() }))
}

#[tauri::command]
async fn cmd_list_runs(facade: Facade<'_>, name: String, limit: usize) -> CmdResult<JobRunListDto> {
    let runs = facade.list_runs(&name, limit).await?;
    Ok(JobRunListDto {
        runs: runs.iter().map(job_run_to_dto).collect(),
        truncated: false,
    })
}

#[tauri::command]
async fn cmd_daemon_status(facade: Facade<'_>) -> CmdResult<DaemonStatusDto> {
    Ok(daemon_info_to_dto(facade.daemon_status().await?))
}

/// Start forwarding a process's logs to the `process-log:{name}` Tauri event
/// channel (the invoke adapter's follow path).
#[tauri::command]
async fn cmd_follow_logs(app: AppHandle, facade: Facade<'_>, name: String) -> CmdResult<()> {
    let mut rx = facade.subscribe_process_logs(&name).await?;
    let channel = format!("process-log:{name}");
    tauri::async_runtime::spawn(async move {
        while let Ok(line) = rx.recv().await {
            let _ = app.emit(&channel, log_line_to_dto(&line));
        }
    });
    Ok(())
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            // Build the wired core in-process, mount the test-only devBridge,
            // and manage the facade for the invoke handlers.
            let assembled: Assembled =
                tauri::async_runtime::block_on(async { build_host().await })?;
            let facade = assembled.facade.clone();

            tauri::async_runtime::block_on(async {
                facade.bootstrap().await.ok();
            });
            tauri::async_runtime::spawn(facade.clone().run_scheduler_loop());

            let data_dir = data_dir();
            let port = std::env::var("MSV_DEVBRIDGE_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_BIND_PORT);
            tauri::async_runtime::spawn(run_devbridge(assembled.router, port, data_dir));

            app.manage(facade);
            build_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Close-to-tray: hide instead of quitting so supervision continues.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            cmd_list_processes,
            cmd_get_process,
            cmd_add_process,
            cmd_start_process,
            cmd_stop_process,
            cmd_restart_process,
            cmd_remove_process,
            cmd_convert_process,
            cmd_process_logs,
            cmd_list_jobs,
            cmd_add_job,
            cmd_remove_job,
            cmd_trigger_job,
            cmd_list_runs,
            cmd_daemon_status,
            cmd_follow_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running my-supervisor desktop");
}

/// Tray icon with show / quit; closing the window only hides it.
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Serve the operations Router on loopback for test automation; write the
/// discovery file so an out-of-process harness can find the port.
async fn run_devbridge(router: axum::Router, port: u16, data_dir: PathBuf) {
    let addr = format!("127.0.0.1:{port}");
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("devBridge bind failed on {addr}: {e}");
            return;
        }
    };
    let base_url = format!("http://{addr}");
    let discovery = data_dir.join("devbridge.json");
    let _ = tokio::fs::create_dir_all(&data_dir).await;
    let _ = tokio::fs::write(&discovery, format!("{{\"base_url\":\"{base_url}\"}}")).await;
    tracing::info!("devBridge (test-only) listening on {base_url}");
    let _ = axum::serve(listener, router).await;
    let _ = tokio::fs::remove_file(&discovery).await;
}

/// In-process composition shared with the daemon runtime.
async fn build_host() -> anyhow::Result<Assembled> {
    build_runtime().await
}
