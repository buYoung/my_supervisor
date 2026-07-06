//! WebSocket handlers: per-process log follow, per-run log follow, and the
//! global event stream. The process-logs route doubles as the REST `GET`
//! endpoint — a non-upgrade request returns the JSON tail.

use std::convert::Infallible;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::broadcast;

use my_supervisor_application::DomainEvent;
use my_supervisor_core::domain::LogLine;

use crate::error::HttpError;
use crate::handlers::{Facade, LogQuery};
use crate::mapping::{log_line_to_dto, log_page_to_dto, process_state_to_dto};

/// Optional WS upgrade extractor. axum 0.8's `Option<T>` requires
/// `OptionalFromRequestParts`, which `WebSocketUpgrade` does not implement, so
/// we wrap it: present on an upgrade request, `None` for a plain `GET`.
pub struct MaybeWs(pub Option<WebSocketUpgrade>);

impl<S> FromRequestParts<S> for MaybeWs
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Ok(MaybeWs(
            WebSocketUpgrade::from_request_parts(parts, state)
                .await
                .ok(),
        ))
    }
}

/// `GET /api/v1/processes/{name}/logs` — JSON tail, or WS follow on upgrade.
pub async fn process_logs(
    State(f): State<Facade>,
    Path(name): Path<String>,
    Query(q): Query<LogQuery>,
    MaybeWs(upgrade): MaybeWs,
) -> Result<Response, HttpError> {
    if let Some(upgrade) = upgrade {
        let rx = f.subscribe_process_logs(&name).await?;
        Ok(upgrade.on_upgrade(move |socket| forward_logs(socket, rx)))
    } else {
        let page = f
            .process_logs(&name, q.tail.unwrap_or(100), q.since)
            .await?;
        Ok(Json(log_page_to_dto(page)).into_response())
    }
}

/// `WS /api/v1/jobs/{name}/runs/{run_id}/logs` — follow a run's output.
pub async fn run_logs(
    State(f): State<Facade>,
    Path((name, run_id)): Path<(String, String)>,
    upgrade: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    let rid = crate::handlers::parse_run_id(&run_id)?;
    // Confirm the run exists so a bad id closes cleanly rather than hanging.
    f.get_run(&name, &rid).await?;
    let rx = f.subscribe_run_logs(rid);
    Ok(upgrade.on_upgrade(move |socket| forward_logs(socket, rx)))
}

/// `WS /api/v1/events` — global event stream.
pub async fn events(State(f): State<Facade>, upgrade: WebSocketUpgrade) -> Response {
    let rx = f.subscribe_events();
    upgrade.on_upgrade(move |socket| forward_events(socket, rx))
}

async fn forward_logs(socket: WebSocket, mut rx: broadcast::Receiver<LogLine>) {
    let (mut sender, mut receiver) = socket.split();
    loop {
        tokio::select! {
            incoming = rx.recv() => match incoming {
                Ok(line) => {
                    let payload = serde_json::to_string(&log_line_to_dto(&line)).unwrap_or_default();
                    if sender.send(Message::Text(payload.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    let frame = json!({ "type": "log.dropped", "payload": { "count": count } });
                    let _ = sender.send(Message::Text(frame.to_string().into())).await;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            client = receiver.next() => match client {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            },
        }
    }
}

async fn forward_events(socket: WebSocket, mut rx: broadcast::Receiver<DomainEvent>) {
    let (mut sender, mut receiver) = socket.split();
    loop {
        tokio::select! {
            incoming = rx.recv() => match incoming {
                Ok(event) => {
                    let payload = serde_json::to_string(&event_to_wire(&event)).unwrap_or_default();
                    if sender.send(Message::Text(payload.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            client = receiver.next() => match client {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            },
        }
    }
}

/// Map a `DomainEvent` onto the `{type, timestamp, payload}` wire envelope.
fn event_to_wire(event: &DomainEvent) -> serde_json::Value {
    let (event_type, payload) = match event {
        DomainEvent::ProcessStateChanged { name, from, to } => (
            "process.state_changed",
            json!({
                "name": name,
                "from": process_state_to_dto(*from),
                "to": process_state_to_dto(*to),
            }),
        ),
        DomainEvent::JobRunScheduled { name, run_id } => (
            "job.run_scheduled",
            json!({ "name": name, "run_id": run_id.0.to_string() }),
        ),
        DomainEvent::JobRunStarted { name, run_id } => (
            "job.run_started",
            json!({ "name": name, "run_id": run_id.0.to_string() }),
        ),
        DomainEvent::JobRunSucceeded {
            name,
            run_id,
            exit_code,
        } => (
            "job.run_succeeded",
            json!({ "name": name, "run_id": run_id.0.to_string(), "exit_code": exit_code }),
        ),
        DomainEvent::JobRunFailed {
            name,
            run_id,
            exit_code,
        } => (
            "job.run_failed",
            json!({ "name": name, "run_id": run_id.0.to_string(), "exit_code": exit_code }),
        ),
        DomainEvent::JobRunSkipped {
            name,
            run_id,
            reason,
        } => (
            "job.run_skipped",
            json!({ "name": name, "run_id": run_id.0.to_string(), "reason": reason }),
        ),
    };
    json!({
        "type": event_type,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "payload": payload,
    })
}
