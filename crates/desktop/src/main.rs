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

#[cfg(debug_assertions)]
use my_supervisor_app_daemon::build_runtime;
use my_supervisor_app_daemon::{data_dir, DEFAULT_BIND_PORT};
use my_supervisor_application::{AppError, ConvertTarget, OperationsFacade, RestartOutcome};
use my_supervisor_core::domain::JobRunId;
use my_supervisor_infra_http::mapping::{
    alert_episode_to_dto, daemon_info_to_dto, delivery_attempt_to_dto, job_config_to_job,
    job_preview_to_dto, job_run_to_dto, job_view_to_dto, log_line_to_dto, log_page_to_dto,
    metric_sample_to_dto, observability_page_to_dto, operator_event_to_dto, process_config_to_spec,
    process_operation_to_dto, process_status_to_dto,
};
use my_supervisor_infra_http::{event_to_wire, Assembled, AuthVerifier};
use my_supervisor_shared::api::{
    AlertEpisodeDto, AlertRuleDto, AlertSeverityDto, ConvertTargetDto, DaemonStatusDto,
    DeliveryAttemptDto, JobConfigDto, JobListDto, JobPageDto, JobPreviewDto, JobPreviewRequestDto,
    JobRunDto, JobRunListDto, JobStatusDto, LogsResponseDto, MetricSampleDto, ObservabilityPageDto,
    OperatorEventDto, ProcessConfigDto, ProcessInstancesDto, ProcessListDto, ProcessOperationDto,
    ProcessPageDto, ProcessStatusDto, UpsertAlertRuleRequestDto,
};

/// Build-time package-security evidence retained in the release Mach-O for
/// artifact verification. It records no credentials and does not assert that
/// an unsigned candidate has a hardened runtime or embedded entitlements.
#[used]
static MSV_SECURITY_CONTRACT_MARKER: &str = concat!(env!("MSV_SECURITY_CONTRACT_MARKER"), "\0");

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
async fn cmd_list_processes_page(
    facade: Facade<'_>,
    cursor: Option<String>,
    high_watermark: Option<String>,
    limit: Option<usize>,
) -> CmdResult<ProcessPageDto> {
    let limit = limit.unwrap_or(50);
    if limit == 0 || limit > 200 {
        return Err(CmdError {
            code: "invalid_request".into(),
            message: "limit must be between 1 and 200".into(),
        });
    }
    let page = facade
        .list_processes_page(cursor.as_deref(), high_watermark.as_deref(), limit)
        .await?;
    let partial = !page.failed_partitions.is_empty();
    Ok(ProcessPageDto {
        processes: page
            .records
            .into_iter()
            .map(process_status_to_dto)
            .collect(),
        next_cursor: page.next_cursor,
        high_watermark: page.high_watermark,
        partial,
        failed_partitions: page.failed_partitions,
    })
}

#[tauri::command]
async fn cmd_get_process(facade: Facade<'_>, name: String) -> CmdResult<ProcessStatusDto> {
    Ok(process_status_to_dto(facade.get_process(&name).await?))
}

#[tauri::command]
async fn process_instances(facade: Facade<'_>, name: String) -> CmdResult<ProcessInstancesDto> {
    let (desired_instances, instances) = facade.process_instances(&name).await?;
    Ok(ProcessInstancesDto {
        name,
        desired_instances,
        instances: instances
            .into_iter()
            .map(
                |instance| my_supervisor_shared::api::ProcessInstanceStatusDto {
                    instance_id: instance.instance_id.0,
                    ordinal: instance.ordinal,
                    generation: instance.generation,
                    state: my_supervisor_infra_http::mapping::process_state_to_dto(instance.state),
                    pid: instance.pid,
                    restart_count: instance.restart_count,
                    started_at: instance.started_at,
                    cpu_percent: instance.cpu_percent,
                    memory_bytes: instance.memory_bytes,
                },
            )
            .collect(),
    })
}

#[tauri::command]
async fn scale_process(
    facade: Facade<'_>,
    name: String,
    instances: u16,
    operation_id: Option<String>,
) -> CmdResult<ProcessOperationDto> {
    let operation_id = operation_id
        .map(|value| {
            uuid::Uuid::parse_str(&value).map_err(|_| CmdError {
                code: "invalid_request".into(),
                message: "operation_id must be a UUID".into(),
            })
        })
        .transpose()?;
    Ok(process_operation_to_dto(
        facade.scale_process(&name, instances, operation_id).await?,
    ))
}

#[tauri::command]
async fn rolling_restart_process(
    facade: Facade<'_>,
    name: String,
    operation_id: Option<String>,
) -> CmdResult<ProcessOperationDto> {
    let operation_id = operation_id
        .map(|value| {
            uuid::Uuid::parse_str(&value).map_err(|_| CmdError {
                code: "invalid_request".into(),
                message: "operation_id must be a UUID".into(),
            })
        })
        .transpose()?;
    Ok(process_operation_to_dto(
        facade.rolling_restart_process(&name, operation_id).await?,
    ))
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
async fn cmd_list_jobs_page(
    facade: Facade<'_>,
    cursor: Option<String>,
    high_watermark: Option<String>,
    limit: Option<usize>,
) -> CmdResult<JobPageDto> {
    let limit = limit.unwrap_or(50);
    if limit == 0 || limit > 200 {
        return Err(CmdError {
            code: "invalid_request".into(),
            message: "limit must be between 1 and 200".into(),
        });
    }
    let page = facade
        .list_jobs_page(cursor.as_deref(), high_watermark.as_deref(), limit)
        .await?;
    let partial = !page.failed_partitions.is_empty();
    Ok(JobPageDto {
        jobs: page.records.into_iter().map(job_view_to_dto).collect(),
        next_cursor: page.next_cursor,
        high_watermark: page.high_watermark,
        partial,
        failed_partitions: page.failed_partitions,
    })
}

#[tauri::command]
async fn cmd_get_job(facade: Facade<'_>, name: String) -> CmdResult<JobStatusDto> {
    Ok(job_view_to_dto(facade.get_job(&name).await?))
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
async fn cmd_update_job(
    facade: Facade<'_>,
    name: String,
    config: JobConfigDto,
) -> CmdResult<JobStatusDto> {
    Ok(job_view_to_dto(
        facade.update_job(&name, job_config_to_job(config)).await?,
    ))
}

#[tauri::command]
async fn cmd_preview_job(
    facade: Facade<'_>,
    request: JobPreviewRequestDto,
) -> CmdResult<JobPreviewDto> {
    Ok(job_preview_to_dto(
        facade
            .preview_job(job_config_to_job(request.config), request.at, request.count)
            .await?,
    ))
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

fn command_run_id(value: String) -> CmdResult<JobRunId> {
    uuid::Uuid::parse_str(&value)
        .map(JobRunId)
        .map_err(|_| CmdError {
            code: "invalid_request".into(),
            message: "run_id must be a UUID".into(),
        })
}

#[tauri::command]
async fn cmd_get_run(facade: Facade<'_>, name: String, run_id: String) -> CmdResult<JobRunDto> {
    Ok(job_run_to_dto(
        &facade.get_run(&name, &command_run_id(run_id)?).await?,
    ))
}

#[tauri::command]
async fn cmd_cancel_run(facade: Facade<'_>, name: String, run_id: String) -> CmdResult<()> {
    facade
        .cancel_run(&name, command_run_id(run_id)?)
        .await
        .map_err(Into::into)
}

#[tauri::command]
async fn cmd_daemon_status(facade: Facade<'_>) -> CmdResult<DaemonStatusDto> {
    Ok(daemon_info_to_dto(facade.daemon_status().await?))
}

#[tauri::command]
async fn cmd_list_operator_events(
    facade: Facade<'_>,
    cursor: Option<String>,
    limit: usize,
) -> CmdResult<ObservabilityPageDto<OperatorEventDto>> {
    Ok(observability_page_to_dto(
        facade
            .list_operator_events(cursor.as_deref(), limit)
            .await?,
        operator_event_to_dto,
    ))
}
#[tauri::command]
async fn cmd_list_metric_samples(
    facade: Facade<'_>,
    source: Option<String>,
    cursor: Option<String>,
    limit: usize,
) -> CmdResult<ObservabilityPageDto<MetricSampleDto>> {
    Ok(observability_page_to_dto(
        facade
            .list_metric_samples(source.as_deref(), cursor.as_deref(), limit)
            .await?,
        metric_sample_to_dto,
    ))
}
#[tauri::command]
async fn cmd_list_alert_episodes(
    facade: Facade<'_>,
    cursor: Option<String>,
    limit: usize,
) -> CmdResult<ObservabilityPageDto<AlertEpisodeDto>> {
    Ok(observability_page_to_dto(
        facade.list_alert_episodes(cursor.as_deref(), limit).await?,
        alert_episode_to_dto,
    ))
}
#[tauri::command]
async fn cmd_list_delivery_attempts(
    facade: Facade<'_>,
    cursor: Option<String>,
    limit: usize,
) -> CmdResult<ObservabilityPageDto<DeliveryAttemptDto>> {
    Ok(observability_page_to_dto(
        facade
            .list_delivery_attempts(None, cursor.as_deref(), limit)
            .await?,
        delivery_attempt_to_dto,
    ))
}
#[tauri::command]
async fn cmd_list_alert_rules(facade: Facade<'_>, limit: usize) -> CmdResult<Vec<AlertRuleDto>> {
    Ok(facade
        .list_alert_rules(limit)
        .await?
        .into_iter()
        .map(my_supervisor_infra_http::mapping::alert_rule_to_dto)
        .collect())
}
#[tauri::command]
async fn cmd_upsert_alert_rule(
    facade: Facade<'_>,
    request: UpsertAlertRuleRequestDto,
) -> CmdResult<AlertRuleDto> {
    if request.name.trim().is_empty() || request.condition.trim().is_empty() {
        return Err(CmdError {
            code: "invalid_request".into(),
            message: "rule name and condition are required".into(),
        });
    }
    let now = chrono::Utc::now();
    let severity = match request.severity {
        AlertSeverityDto::Info => my_supervisor_core::domain::AlertSeverity::Info,
        AlertSeverityDto::Warning => my_supervisor_core::domain::AlertSeverity::Warning,
        AlertSeverityDto::Critical => my_supervisor_core::domain::AlertSeverity::Critical,
    };
    Ok(my_supervisor_infra_http::mapping::alert_rule_to_dto(
        facade
            .save_alert_rule(my_supervisor_core::domain::AlertRule {
                id: request.id.unwrap_or_else(uuid::Uuid::new_v4),
                name: request.name,
                condition: request.condition,
                severity,
                cooldown_seconds: request.cooldown_seconds,
                enabled: request.enabled,
                created_at: now,
                updated_at: now,
            })
            .await?,
    ))
}
#[tauri::command]
async fn cmd_acknowledge_alert_episode(facade: Facade<'_>, id: uuid::Uuid) -> CmdResult<bool> {
    Ok(facade.acknowledge_alert_episode(id).await?)
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

/// Forward global events through Tauri's renderer transport. A durable terminal
/// event is acknowledged only after Tauri accepts this external emit; the
/// renderer still de-duplicates repeated stable IDs in session memory.
fn start_event_forwarder(app: AppHandle, facade: Arc<OperationsFacade>) {
    let mut events = facade.subscribe_events();
    tauri::async_runtime::spawn(async move {
        while let Ok(event) = events.recv().await {
            if app.emit("global-event", event_to_wire(&event)).is_ok() {
                event.complete_delivery();
            }
        }
    });
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
            // Keep the development-owner lock alive for the full desktop
            // process even if the optional test devBridge is not serving.
            app.manage::<AuthVerifier>(assembled.auth.clone());

            tauri::async_runtime::block_on(async {
                facade.bootstrap().await.ok();
            });
            tauri::async_runtime::spawn(facade.clone().run_scheduler_loop());
            tauri::async_runtime::spawn(facade.clone().run_process_supervisor_loop());
            start_event_forwarder(app.handle().clone(), facade.clone());

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
            cmd_list_processes_page,
            cmd_get_process,
            process_instances,
            scale_process,
            rolling_restart_process,
            cmd_add_process,
            cmd_start_process,
            cmd_stop_process,
            cmd_restart_process,
            cmd_remove_process,
            cmd_convert_process,
            cmd_process_logs,
            cmd_list_jobs,
            cmd_list_jobs_page,
            cmd_get_job,
            cmd_add_job,
            cmd_update_job,
            cmd_preview_job,
            cmd_remove_job,
            cmd_trigger_job,
            cmd_list_runs,
            cmd_get_run,
            cmd_cancel_run,
            cmd_daemon_status,
            cmd_list_operator_events,
            cmd_list_metric_samples,
            cmd_list_alert_episodes,
            cmd_list_delivery_attempts,
            cmd_list_alert_rules,
            cmd_upsert_alert_rule,
            cmd_acknowledge_alert_episode,
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

/// In-process composition is available only to the explicit development host.
#[cfg(debug_assertions)]
async fn build_host() -> anyhow::Result<Assembled> {
    if std::env::var("MSV_DESKTOP_EMBEDDED_DEV").as_deref() != Ok("1") {
        anyhow::bail!("desktop embedded owner requires MSV_DESKTOP_EMBEDDED_DEV=1; installed client mode is owned by U19");
    }
    build_runtime().await
}

/// Release candidates deliberately reject the development-only embedded host.
#[cfg(not(debug_assertions))]
async fn build_host() -> anyhow::Result<Assembled> {
    anyhow::bail!("desktop embedded owner is unavailable outside explicit development mode");
}
