//! REST route handlers. Each is a thin adapter: decode → call the facade →
//! map the result onto a DTO. No domain logic here (parity invariant).

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;

use my_supervisor_application::{ConvertTarget, OperationsFacade, RestartOutcome};
use my_supervisor_core::domain::JobRunId;
use my_supervisor_shared::api::{
    ConvertRequestDto, ConvertTargetDto, JobConfigDto, JobListDto, JobRunListDto, ProcessConfigDto,
    ProcessListDto, RestartNoopDto,
};

use crate::error::HttpError;
use crate::mapping::{
    daemon_info_to_dto, job_config_to_job, job_run_to_dto, job_view_to_dto, process_config_to_spec,
    process_status_to_dto,
};

pub type Facade = Arc<OperationsFacade>;

#[derive(Debug, Default, Deserialize)]
pub struct ForceQuery {
    #[serde(default)]
    pub force: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct LogQuery {
    pub tail: Option<usize>,
    pub since: Option<DateTime<Utc>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RunsQuery {
    pub limit: Option<usize>,
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

pub async fn list_runs(
    State(f): State<Facade>,
    Path(name): Path<String>,
    Query(q): Query<RunsQuery>,
) -> Result<Response, HttpError> {
    let limit = q.limit.unwrap_or(50).min(500);
    let runs = f.list_runs(&name, limit).await?;
    let dto = JobRunListDto {
        runs: runs.iter().map(job_run_to_dto).collect(),
        truncated: false,
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

pub async fn reload(State(f): State<Facade>) -> Result<StatusCode, HttpError> {
    f.reload().await?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn shutdown(State(f): State<Facade>) -> StatusCode {
    f.request_shutdown();
    StatusCode::ACCEPTED
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
