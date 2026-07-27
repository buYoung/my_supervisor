//! REST route handlers. Each is a thin adapter: decode → call the facade →
//! map the result onto a DTO. No domain logic here (parity invariant).

use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;

use my_supervisor_application::{ConvertTarget, OperationsFacade, RestartOutcome};
use my_supervisor_core::domain::{JobRunId, JobRunState};
use my_supervisor_shared::api::{AlertSeverityDto, UpsertAlertRuleRequestDto};
use my_supervisor_shared::api::{
    ConfigApplyResultDto, ConvertRequestDto, ConvertTargetDto, JobConfigDto, JobListDto,
    JobPageDto, JobPreviewDto, JobPreviewRequestDto, JobRunListDto, JobRunStateDto,
    ProcessConfigDto, ProcessInstancesDto, ProcessListDto, ProcessPageDto, RestartNoopDto,
    RollingRestartRequestDto, ScaleProcessRequestDto, SessionBootstrapDto,
};
use my_supervisor_shared::config::ConfigApplyRequestDto;

use crate::auth::{AuthSession, AuthVerifier};
use crate::error::HttpError;
use crate::mapping::{
    config_apply_mode_to_domain, config_apply_result_to_dto, daemon_info_to_dto,
    file_config_to_loaded, job_config_to_job, job_preview_to_dto, job_run_to_dto, job_view_to_dto,
    process_config_to_spec, process_operation_to_dto, process_status_to_dto,
    recovery_diagnostics_to_dto,
};

pub type Facade = Arc<OperationsFacade>;

/// Native-only bootstrap endpoint. The bearer is accepted by middleware but
/// never copied into the response; the renderer receives only a CSRF nonce
/// while the opaque id is set as an HttpOnly loopback cookie.
pub async fn bootstrap_session(
    Extension(session): Extension<AuthSession>,
    Extension(auth): Extension<AuthVerifier>,
) -> Result<Response, HttpError> {
    if session.is_browser_session() {
        return Err(HttpError(
            my_supervisor_application::AppError::InvalidRequest(
                "session bootstrap requires native bearer authentication".into(),
            ),
        ));
    }
    let (id, dto): (String, SessionBootstrapDto) = auth.bootstrap_session(session.generation);
    let cookie = format!("msv_session={id}; HttpOnly; SameSite=Strict; Max-Age=600; Path=/api/v1");
    let mut response = Json(dto).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::try_from(cookie).expect("static cookie shape"),
    );
    Ok(response)
}

pub async fn logout_session(
    Extension(auth): Extension<AuthVerifier>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    auth.logout_session(headers.get(header::COOKIE));
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "msv_session=; HttpOnly; SameSite=Strict; Max-Age=0; Path=/api/v1",
        ),
    );
    Ok(response)
}

#[derive(Debug, Default, Deserialize)]
pub struct ForceQuery {
    #[serde(default)]
    pub force: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct LogQuery {
    pub tail: Option<usize>,
    pub since: Option<DateTime<Utc>>,
    pub after_sequence: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RunsQuery {
    pub limit: Option<usize>,
    pub since: Option<DateTime<Utc>>,
    pub state: Option<JobRunStateDto>,
}
#[derive(Debug, Default, Deserialize)]
pub struct PageQuery {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub high_watermark: Option<String>,
}
#[derive(Debug, Default, Deserialize)]
pub struct ObservabilityQuery {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub source: Option<String>,
    pub alert_id: Option<uuid::Uuid>,
}

fn observability_limit(limit: Option<usize>) -> Result<usize, HttpError> {
    let value = limit.unwrap_or(100);
    if value == 0 || value > 500 {
        return Err(HttpError(
            my_supervisor_application::AppError::InvalidRequest(
                "limit must be between 1 and 500".into(),
            ),
        ));
    }
    Ok(value)
}
fn page_limit(limit: Option<usize>) -> Result<usize, HttpError> {
    let value = limit.unwrap_or(50);
    if value == 0 || value > 200 {
        return Err(HttpError(
            my_supervisor_application::AppError::InvalidRequest(
                "limit must be between 1 and 200".into(),
            ),
        ));
    }
    Ok(value)
}

// Resource names are encoded before crossing the public query boundary so
// callers cannot infer or depend on SQLite's ordering representation.
fn encode_page_key(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn decode_page_key(value: Option<&str>) -> Result<Option<String>, HttpError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() % 2 != 0 {
        return Err(HttpError(
            my_supervisor_application::AppError::InvalidRequest("invalid page cursor".into()),
        ));
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| {
            HttpError(my_supervisor_application::AppError::InvalidRequest(
                "invalid page cursor".into(),
            ))
        })?;
    String::from_utf8(bytes).map(Some).map_err(|_| {
        HttpError(my_supervisor_application::AppError::InvalidRequest(
            "invalid page cursor".into(),
        ))
    })
}
fn severity_from_dto(value: AlertSeverityDto) -> my_supervisor_core::domain::AlertSeverity {
    match value {
        AlertSeverityDto::Info => my_supervisor_core::domain::AlertSeverity::Info,
        AlertSeverityDto::Warning => my_supervisor_core::domain::AlertSeverity::Warning,
        AlertSeverityDto::Critical => my_supervisor_core::domain::AlertSeverity::Critical,
    }
}

pub async fn list_alert_rules(
    State(f): State<Facade>,
    Query(q): Query<ObservabilityQuery>,
) -> Result<Response, HttpError> {
    Ok(Json(
        f.list_alert_rules(observability_limit(q.limit)?)
            .await?
            .into_iter()
            .map(crate::mapping::alert_rule_to_dto)
            .collect::<Vec<_>>(),
    )
    .into_response())
}
pub async fn upsert_alert_rule(
    State(f): State<Facade>,
    Json(dto): Json<UpsertAlertRuleRequestDto>,
) -> Result<Response, HttpError> {
    if dto.name.trim().is_empty() || dto.condition.trim().is_empty() {
        return Err(HttpError(
            my_supervisor_application::AppError::InvalidRequest(
                "rule name and condition are required".into(),
            ),
        ));
    }
    let now = chrono::Utc::now();
    let rule = my_supervisor_core::domain::AlertRule {
        id: dto.id.unwrap_or_else(uuid::Uuid::new_v4),
        name: dto.name,
        condition: dto.condition,
        severity: severity_from_dto(dto.severity),
        cooldown_seconds: dto.cooldown_seconds,
        enabled: dto.enabled,
        created_at: now,
        updated_at: now,
    };
    Ok(Json(crate::mapping::alert_rule_to_dto(
        f.save_alert_rule(rule).await?,
    ))
    .into_response())
}
pub async fn delete_alert_rule(
    State(f): State<Facade>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Response, HttpError> {
    f.delete_alert_rule(id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
pub async fn list_operator_events(
    State(f): State<Facade>,
    Query(q): Query<ObservabilityQuery>,
) -> Result<Response, HttpError> {
    Ok(Json(crate::mapping::observability_page_to_dto(
        f.list_operator_events(q.cursor.as_deref(), observability_limit(q.limit)?)
            .await?,
        crate::mapping::operator_event_to_dto,
    ))
    .into_response())
}
pub async fn list_metric_samples(
    State(f): State<Facade>,
    Query(q): Query<ObservabilityQuery>,
) -> Result<Response, HttpError> {
    Ok(Json(crate::mapping::observability_page_to_dto(
        f.list_metric_samples(
            q.source.as_deref(),
            q.cursor.as_deref(),
            observability_limit(q.limit)?,
        )
        .await?,
        crate::mapping::metric_sample_to_dto,
    ))
    .into_response())
}
pub async fn list_alert_episodes(
    State(f): State<Facade>,
    Query(q): Query<ObservabilityQuery>,
) -> Result<Response, HttpError> {
    Ok(Json(crate::mapping::observability_page_to_dto(
        f.list_alert_episodes(q.cursor.as_deref(), observability_limit(q.limit)?)
            .await?,
        crate::mapping::alert_episode_to_dto,
    ))
    .into_response())
}
pub async fn acknowledge_alert_episode(
    State(f): State<Facade>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Response, HttpError> {
    Ok(
        Json(serde_json::json!({"acknowledged":f.acknowledge_alert_episode(id).await?}))
            .into_response(),
    )
}
pub async fn list_delivery_attempts(
    State(f): State<Facade>,
    Query(q): Query<ObservabilityQuery>,
) -> Result<Response, HttpError> {
    Ok(Json(crate::mapping::observability_page_to_dto(
        f.list_delivery_attempts(
            q.alert_id,
            q.cursor.as_deref(),
            observability_limit(q.limit)?,
        )
        .await?,
        crate::mapping::delivery_attempt_to_dto,
    ))
    .into_response())
}

// --- Processes -------------------------------------------------------------

pub async fn list_processes(State(f): State<Facade>) -> Result<Response, HttpError> {
    let processes = f
        .list_processes()
        .await?
        .into_iter()
        .map(process_status_to_dto)
        .collect();
    Ok(Json(ProcessListDto { processes }).into_response())
}

/// Additive bounded page; the legacy `/processes` response remains unchanged.
pub async fn list_processes_page(
    State(f): State<Facade>,
    Query(q): Query<PageQuery>,
) -> Result<Response, HttpError> {
    let page = f
        .list_processes_page(
            decode_page_key(q.cursor.as_deref())?.as_deref(),
            decode_page_key(q.high_watermark.as_deref())?.as_deref(),
            page_limit(q.limit)?,
        )
        .await?;
    Ok(Json(ProcessPageDto {
        processes: page
            .records
            .into_iter()
            .map(process_status_to_dto)
            .collect(),
        next_cursor: page.next_cursor.as_deref().map(encode_page_key),
        high_watermark: encode_page_key(&page.high_watermark),
        partial: !page.failed_partitions.is_empty(),
        failed_partitions: page.failed_partitions,
    })
    .into_response())
}

pub async fn create_process(
    State(f): State<Facade>,
    Json(dto): Json<ProcessConfigDto>,
) -> Result<Response, HttpError> {
    let status = f.add_process(process_config_to_spec(dto)).await?;
    Ok((StatusCode::CREATED, Json(process_status_to_dto(status))).into_response())
}

pub async fn get_process(
    State(f): State<Facade>,
    Path(name): Path<String>,
) -> Result<Response, HttpError> {
    let status = f.get_process(&name).await?;
    Ok(Json(process_status_to_dto(status)).into_response())
}

pub async fn process_instances(
    State(f): State<Facade>,
    Path(name): Path<String>,
) -> Result<Response, HttpError> {
    let (desired_instances, instances) = f.process_instances(&name).await?;
    Ok(Json(ProcessInstancesDto {
        name,
        desired_instances,
        instances: instances
            .into_iter()
            .map(
                |instance| my_supervisor_shared::api::ProcessInstanceStatusDto {
                    instance_id: instance.instance_id.0,
                    ordinal: instance.ordinal,
                    generation: instance.generation,
                    state: crate::mapping::process_state_to_dto(instance.state),
                    pid: instance.pid,
                    restart_count: instance.restart_count,
                    started_at: instance.started_at,
                    cpu_percent: instance.cpu_percent,
                    memory_bytes: instance.memory_bytes,
                },
            )
            .collect(),
    })
    .into_response())
}

fn request_operation_id(
    headers: &HeaderMap,
    body: Option<uuid::Uuid>,
) -> Result<Option<uuid::Uuid>, HttpError> {
    let header_id = headers
        .get("Idempotency-Key")
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            uuid::Uuid::parse_str(value).map_err(|_| {
                HttpError(my_supervisor_application::AppError::InvalidRequest(
                    "Idempotency-Key must be a UUID".into(),
                ))
            })
        })
        .transpose()?;
    if let (Some(header_id), Some(body_id)) = (header_id, body) {
        if header_id != body_id {
            return Err(HttpError(
                my_supervisor_application::AppError::InvalidRequest(
                    "operation_id and Idempotency-Key must match".into(),
                ),
            ));
        }
    }
    Ok(header_id.or(body))
}

pub async fn scale_process(
    State(f): State<Facade>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(dto): Json<ScaleProcessRequestDto>,
) -> Result<Response, HttpError> {
    let operation = f
        .scale_process(
            &name,
            dto.instances,
            request_operation_id(&headers, dto.operation_id)?,
        )
        .await?;
    Ok(Json(process_operation_to_dto(operation)).into_response())
}

pub async fn rolling_restart_process(
    State(f): State<Facade>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(dto): Json<RollingRestartRequestDto>,
) -> Result<Response, HttpError> {
    let operation = f
        .rolling_restart_process(&name, request_operation_id(&headers, dto.operation_id)?)
        .await?;
    Ok(Json(process_operation_to_dto(operation)).into_response())
}

pub async fn delete_process(
    State(f): State<Facade>,
    Path(name): Path<String>,
    Query(q): Query<ForceQuery>,
) -> Result<StatusCode, HttpError> {
    f.remove_process(&name, q.force.unwrap_or(false)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn start_process(
    State(f): State<Facade>,
    Path(name): Path<String>,
) -> Result<StatusCode, HttpError> {
    f.start_process(&name).await?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn stop_process(
    State(f): State<Facade>,
    Path(name): Path<String>,
    Query(q): Query<ForceQuery>,
) -> Result<StatusCode, HttpError> {
    f.stop_process(&name, q.force.unwrap_or(false)).await?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn restart_process(
    State(f): State<Facade>,
    Path(name): Path<String>,
) -> Result<Response, HttpError> {
    match f.restart_process(&name).await? {
        RestartOutcome::Accepted => Ok(StatusCode::ACCEPTED.into_response()),
        RestartOutcome::Noop { reason } => {
            Ok((StatusCode::OK, Json(RestartNoopDto { noop: true, reason })).into_response())
        }
    }
}

pub async fn convert_process(
    State(f): State<Facade>,
    Path(name): Path<String>,
    Json(dto): Json<ConvertRequestDto>,
) -> Result<Response, HttpError> {
    let target = match dto.to {
        ConvertTargetDto::Direct => ConvertTarget::Direct,
        ConvertTargetDto::SystemRegistered => ConvertTarget::SystemRegistered,
    };
    let status = f
        .convert_process(
            &name,
            target,
            dto.unit_name,
            dto.auto_start.unwrap_or(false),
        )
        .await?;
    Ok(Json(process_status_to_dto(status)).into_response())
}

// --- Jobs ------------------------------------------------------------------

pub async fn list_jobs(State(f): State<Facade>) -> Result<Response, HttpError> {
    let jobs = f
        .list_jobs()
        .await?
        .into_iter()
        .map(job_view_to_dto)
        .collect();
    Ok(Json(JobListDto { jobs }).into_response())
}

/// Additive bounded page; callers retain the old `/jobs` list contract.
pub async fn list_jobs_page(
    State(f): State<Facade>,
    Query(q): Query<PageQuery>,
) -> Result<Response, HttpError> {
    let page = f
        .list_jobs_page(
            decode_page_key(q.cursor.as_deref())?.as_deref(),
            decode_page_key(q.high_watermark.as_deref())?.as_deref(),
            page_limit(q.limit)?,
        )
        .await?;
    Ok(Json(JobPageDto {
        jobs: page.records.into_iter().map(job_view_to_dto).collect(),
        next_cursor: page.next_cursor.as_deref().map(encode_page_key),
        high_watermark: encode_page_key(&page.high_watermark),
        partial: !page.failed_partitions.is_empty(),
        failed_partitions: page.failed_partitions,
    })
    .into_response())
}

pub async fn preview_job(
    State(f): State<Facade>,
    Json(dto): Json<JobPreviewRequestDto>,
) -> Result<Response, HttpError> {
    let preview: JobPreviewDto = job_preview_to_dto(
        f.preview_job(job_config_to_job(dto.config), dto.at, dto.count)
            .await?,
    );
    Ok(Json(preview).into_response())
}

pub async fn create_job(
    State(f): State<Facade>,
    Json(dto): Json<JobConfigDto>,
) -> Result<Response, HttpError> {
    let view = f.add_job(job_config_to_job(dto)).await?;
    Ok((StatusCode::CREATED, Json(job_view_to_dto(view))).into_response())
}

pub async fn get_job(
    State(f): State<Facade>,
    Path(name): Path<String>,
) -> Result<Response, HttpError> {
    let view = f.get_job(&name).await?;
    Ok(Json(job_view_to_dto(view)).into_response())
}

pub async fn update_job(
    State(f): State<Facade>,
    Path(name): Path<String>,
    Json(dto): Json<JobConfigDto>,
) -> Result<Response, HttpError> {
    let view = f.update_job(&name, job_config_to_job(dto)).await?;
    Ok(Json(job_view_to_dto(view)).into_response())
}

pub async fn delete_job(
    State(f): State<Facade>,
    Path(name): Path<String>,
    Query(q): Query<ForceQuery>,
) -> Result<StatusCode, HttpError> {
    f.delete_job(&name, q.force.unwrap_or(false)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn trigger_job(
    State(f): State<Facade>,
    Path(name): Path<String>,
) -> Result<Response, HttpError> {
    let run_id = f.trigger_job(&name).await?;
    let location = format!("/api/v1/jobs/{name}/runs/{}", run_id.0);
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(json!({ "run_id": run_id.0.to_string() })),
    )
        .into_response())
}

pub async fn cancel_run(
    State(f): State<Facade>,
    Path((name, run_id)): Path<(String, String)>,
) -> Result<StatusCode, HttpError> {
    f.cancel_run(&name, parse_run_id(&run_id)?).await?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn list_runs(
    State(f): State<Facade>,
    Path(name): Path<String>,
    Query(q): Query<RunsQuery>,
) -> Result<Response, HttpError> {
    let limit = q.limit.unwrap_or(50).min(500);
    let state = q.state.map(|state| match state {
        JobRunStateDto::Pending => JobRunState::Pending,
        JobRunStateDto::Running => JobRunState::Running,
        JobRunStateDto::Succeeded => JobRunState::Succeeded,
        JobRunStateDto::Failed => JobRunState::Failed,
        JobRunStateDto::TimedOut => JobRunState::TimedOut,
        JobRunStateDto::Cancelled => JobRunState::Cancelled,
        JobRunStateDto::Skipped => JobRunState::Skipped,
    });
    let mut runs = f
        .list_runs_filtered(&name, state, q.since, limit.saturating_add(1))
        .await?;
    let truncated = runs.len() > limit;
    runs.truncate(limit);
    let dto = JobRunListDto {
        runs: runs.iter().map(job_run_to_dto).collect(),
        truncated,
    };
    Ok(Json(dto).into_response())
}

pub async fn get_run(
    State(f): State<Facade>,
    Path((name, run_id)): Path<(String, String)>,
) -> Result<Response, HttpError> {
    let rid = parse_run_id(&run_id)?;
    let run = f.get_run(&name, &rid).await?;
    Ok(Json(job_run_to_dto(&run)).into_response())
}

// --- Daemon ----------------------------------------------------------------

pub async fn daemon_status(State(f): State<Facade>) -> Result<Response, HttpError> {
    let info = f.daemon_status().await?;
    Ok(Json(daemon_info_to_dto(info)).into_response())
}

pub async fn recovery_diagnostics(State(f): State<Facade>) -> Result<Response, HttpError> {
    let diagnostics = f.recovery_diagnostics().await?;
    Ok(Json(recovery_diagnostics_to_dto(diagnostics)).into_response())
}

pub async fn reload(State(f): State<Facade>) -> Result<StatusCode, HttpError> {
    f.reload().await?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn validate_config(
    State(f): State<Facade>,
    Json(request): Json<ConfigApplyRequestDto>,
) -> Result<Json<ConfigApplyResultDto>, HttpError> {
    let result = f
        .validate_config(
            &file_config_to_loaded(request.config),
            config_apply_mode_to_domain(request.mode),
        )
        .await?;
    Ok(Json(config_apply_result_to_dto(result)))
}

pub async fn apply_config(
    State(f): State<Facade>,
    Json(request): Json<ConfigApplyRequestDto>,
) -> Result<Json<ConfigApplyResultDto>, HttpError> {
    let result = f
        .apply_config(
            file_config_to_loaded(request.config),
            config_apply_mode_to_domain(request.mode),
            request.dry_run,
        )
        .await?;
    Ok(Json(config_apply_result_to_dto(result)))
}

pub async fn shutdown(State(f): State<Facade>) -> StatusCode {
    f.request_shutdown();
    StatusCode::ACCEPTED
}

fn maintenance_error(message: String) -> Response {
    (
        StatusCode::CONFLICT,
        Json(my_supervisor_shared::error::ErrorBody::new(
            "maintenance_unavailable",
            message,
        )),
    )
        .into_response()
}

pub async fn rotate_token(Extension(auth): Extension<crate::AuthVerifier>) -> Response {
    match auth.rotate_token() {
        Ok(result) => Json(result).into_response(),
        Err(message) => maintenance_error(message),
    }
}

pub async fn backup(Extension(auth): Extension<crate::AuthVerifier>) -> Response {
    match auth.backup() {
        Ok(result) => Json(result).into_response(),
        Err(message) => maintenance_error(message),
    }
}

pub async fn upgrade(Extension(auth): Extension<crate::AuthVerifier>) -> Response {
    match auth.upgrade() {
        Ok(result) => Json(result).into_response(),
        Err(message) => maintenance_error(message),
    }
}

pub async fn rollback(Extension(auth): Extension<crate::AuthVerifier>) -> Response {
    match auth.rollback() {
        Ok(result) => Json(result).into_response(),
        Err(message) => maintenance_error(message),
    }
}

pub async fn health() -> Response {
    Json(json!({ "status": "ok" })).into_response()
}

/// Parse a run id, surfacing a `run_not_found` rather than a 400 on a bad uuid.
pub fn parse_run_id(raw: &str) -> Result<JobRunId, HttpError> {
    uuid::Uuid::parse_str(raw).map(JobRunId).map_err(|_| {
        HttpError(my_supervisor_application::AppError::not_found(
            my_supervisor_application::ResourceKind::Run,
            raw,
        ))
    })
}
