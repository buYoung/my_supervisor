//! WebSocket handlers: per-process log follow, per-run log follow, and the
//! global event stream. The process-logs route doubles as the REST `GET`
//! endpoint — a non-upgrade request returns the JSON tail.

use std::convert::Infallible;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::broadcast;

use my_supervisor_application::views::LogPage;
use my_supervisor_application::{DomainEvent, PublishedEvent};
use my_supervisor_core::domain::LogLine;

use crate::auth::{AuthSession, AuthVerifier};
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
    Extension(session): Extension<AuthSession>,
    Extension(auth): Extension<AuthVerifier>,
) -> Result<Response, HttpError> {
    if let Some(upgrade) = upgrade {
        let rx = f.subscribe_process_logs(&name).await?;
        // Subscribe before taking the snapshot.  The high-watermark filter in
        // `forward_logs` then turns this into one no-gap boundary.
        let page = process_log_page(&f, &name, &q).await?;
        let generation = auth.generation_receiver();
        Ok(upgrade.on_upgrade(move |socket| {
            forward_logs(socket, page, rx, generation, session.generation)
        }))
    } else {
        let page = process_log_page(&f, &name, &q).await?;
        Ok(Json(log_page_to_dto(page)).into_response())
    }
}

/// `WS /api/v1/jobs/{name}/runs/{run_id}/logs` — follow a run's output.
pub async fn run_logs(
    State(f): State<Facade>,
    Path((name, run_id)): Path<(String, String)>,
    Query(q): Query<LogQuery>,
    MaybeWs(upgrade): MaybeWs,
    Extension(session): Extension<AuthSession>,
    Extension(auth): Extension<AuthVerifier>,
) -> Result<Response, HttpError> {
    let rid = crate::handlers::parse_run_id(&run_id)?;
    // Confirm the run exists so a bad id closes cleanly rather than hanging.
    f.get_run(&name, &rid).await?;
    if let Some(upgrade) = upgrade {
        let rx = f.subscribe_run_logs(rid);
        let page = run_log_page(&f, &name, rid, &q).await?;
        let generation = auth.generation_receiver();
        Ok(upgrade.on_upgrade(move |socket| {
            forward_logs(socket, page, rx, generation, session.generation)
        }))
    } else {
        Ok(Json(log_page_to_dto(run_log_page(&f, &name, rid, &q).await?)).into_response())
    }
}

/// `WS /api/v1/events` — global event stream.
pub async fn events(
    State(f): State<Facade>,
    Extension(auth): Extension<AuthVerifier>,
    Extension(session): Extension<AuthSession>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let rx = f.subscribe_events();
    let generation = auth.generation_receiver();
    let delivery_facade = f.clone();
    let response = upgrade
        .on_upgrade(move |socket| forward_events(socket, rx, generation, session.generation));
    tokio::spawn(async move {
        delivery_facade.retry_pending_terminal_events().await;
    });
    response
}

async fn process_log_page(
    facade: &Facade,
    name: &str,
    query: &LogQuery,
) -> Result<LogPage, HttpError> {
    let page = facade
        .process_logs_with_cursor(
            name,
            query.tail.unwrap_or(100).min(10_000),
            query.since,
            query.after_sequence,
        )
        .await?;
    Ok(page)
}

async fn run_log_page(
    facade: &Facade,
    name: &str,
    run_id: my_supervisor_core::domain::JobRunId,
    query: &LogQuery,
) -> Result<LogPage, HttpError> {
    Ok(facade
        .run_logs(
            name,
            run_id,
            query.tail.unwrap_or(100).min(10_000),
            query.since,
            query.after_sequence,
        )
        .await?)
}

async fn forward_logs(
    socket: WebSocket,
    page: LogPage,
    mut rx: broadcast::Receiver<LogLine>,
    mut generation: tokio::sync::watch::Receiver<u64>,
    authenticated_generation: u64,
) {
    let (mut sender, mut receiver) = socket.split();
    let mut last_sequence = page.lines.last().map(|line| line.sequence).unwrap_or(0);
    let high_watermark = page.high_watermark;
    for line in page.lines {
        let payload = serde_json::to_string(&log_line_to_dto(&line)).unwrap_or_default();
        if sender.send(Message::Text(payload.into())).await.is_err() {
            return;
        }
    }
    loop {
        tokio::select! {
            incoming = rx.recv() => match incoming {
                Ok(line) => {
                    if line.sequence != 0
                        && (line.sequence <= high_watermark || line.sequence <= last_sequence)
                    {
                        continue;
                    }
                    let payload = serde_json::to_string(&log_line_to_dto(&line)).unwrap_or_default();
                    if sender.send(Message::Text(payload.into())).await.is_err() {
                        break;
                    }
                    last_sequence = last_sequence.max(line.sequence);
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    let frame = json!({ "type": "log.dropped", "payload": {
                        "count": count,
                        "after_sequence": last_sequence,
                    } });
                    let _ = sender.send(Message::Text(frame.to_string().into())).await;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            client = receiver.next() => match client {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            },
            changed = generation.changed() => {
                if changed.is_err() || *generation.borrow() != authenticated_generation {
                    break;
                }
            },
        }
    }
}

async fn forward_events(
    socket: WebSocket,
    mut rx: broadcast::Receiver<PublishedEvent>,
    mut generation: tokio::sync::watch::Receiver<u64>,
    authenticated_generation: u64,
) {
    let (mut sender, mut receiver) = socket.split();
    loop {
        tokio::select! {
            incoming = rx.recv() => match incoming {
                Ok(event) => {
                    let payload = serde_json::to_string(&event_to_wire(&event)).unwrap_or_default();
                    if sender.send(Message::Text(payload.into())).await.is_err() {
                        break;
                    }
                    // Only an actual external socket write may release a
                    // durable terminal outbox record; internal subscribers do
                    // not complete this receipt.
                    event.complete_delivery();
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            client = receiver.next() => match client {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            },
            changed = generation.changed() => {
                if changed.is_err() || *generation.borrow() != authenticated_generation {
                    break;
                }
            },
        }
    }
}

/// Map a published event onto the `{type, timestamp, payload}` wire envelope.
/// Convert a published event to the shared public envelope. Desktop Tauri
/// forwarding reuses this exact mapping so its event IDs and timestamps remain
/// transport-equivalent to the HTTP WebSocket stream.
pub fn event_to_wire(published: &PublishedEvent) -> serde_json::Value {
    let (event_type, payload) = match &published.event {
        DomainEvent::ProcessStateChanged {
            name,
            from,
            to,
            definition_id,
            instance_id,
            generation,
        } => (
            "process.state_changed",
            json!({
                "name": name,
                "from": process_state_to_dto(*from),
                "to": process_state_to_dto(*to),
                "definition_id": definition_id.0.to_string(),
                "instance_id": instance_id.map(|id| id.0.to_string()),
                "generation": generation,
            }),
        ),
        DomainEvent::ProcessGuardChanged {
            name,
            definition_id,
            guard,
        } => (
            "process.guard_changed",
            json!({
                "name": name,
                "definition_id": definition_id.0.to_string(),
                "guard": crate::mapping::guard_status_to_dto(guard.clone()),
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
        DomainEvent::JobRunTimedOut { name, run_id } => (
            "job.run_timed_out",
            json!({ "name": name, "run_id": run_id.0.to_string() }),
        ),
        DomainEvent::JobRunCancelled { name, run_id } => (
            "job.run_cancelled",
            json!({ "name": name, "run_id": run_id.0.to_string() }),
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
    let mut envelope = json!({
        "type": event_type,
        "timestamp": published.occurred_at.unwrap_or_else(chrono::Utc::now).to_rfc3339(),
        "payload": payload,
    });
    if let Some(event_id) = published.event_id {
        envelope["event_id"] = json!(event_id.to_string());
    }
    envelope
}
