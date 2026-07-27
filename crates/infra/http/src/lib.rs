//! `my-supervisor-infra-http` — the operations axum Router and the host
//! assembly entry point. The route manifest below is the frozen contract that
//! children 02/03/04/05 build against; it is pinned to `docs/API.md` for the
//! in-scope operations surface. The `convert` route (child 06) and the
//! `/api/v1/rules*` family (Phase 2) are intentionally absent — not stubbed.

mod auth;
mod error;
mod handlers;
pub mod mapping;
mod ws;

use std::sync::Arc;

use axum::http::{header, HeaderValue, Method};
use axum::middleware;
use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};

use my_supervisor_application::{AppDeps, OperationsFacade};

pub use auth::{AuthVerifier, MaintenanceHandlers};
pub use error::HttpError;
pub use ws::event_to_wire;

/// The assembled host artifacts: the unbound Router and the facade. Each host
/// owns the bind address/port (daemon `127.0.0.1:9876`; the Tauri devBridge its
/// own loopback port).
pub struct Assembled {
    pub router: Router,
    pub facade: Arc<OperationsFacade>,
    /// Retains the authenticated owner lifetime for embedded hosts that do not
    /// expose the HTTP router.
    pub auth: AuthVerifier,
}

/// Build the wired facade and operations Router from injected adapters.
pub fn assemble(deps: AppDeps, auth: AuthVerifier) -> Assembled {
    let facade = OperationsFacade::new(deps);
    let router = build_router(facade.clone(), auth.clone());
    Assembled {
        router,
        facade,
        auth,
    }
}

/// The enumerated operations route manifest (the frozen contract of record).
pub fn build_router(facade: Arc<OperationsFacade>, auth: AuthVerifier) -> Router {
    Router::new()
        .route("/api/v1/health", get(handlers::health))
        // Additive native-to-browser debug-session exchange. Both routes stay
        // behind the same auth middleware as the operations surface.
        .route(
            "/api/v1/session/bootstrap",
            post(handlers::bootstrap_session),
        )
        .route("/api/v1/session/logout", post(handlers::logout_session))
        // Processes
        .route(
            "/api/v1/processes",
            get(handlers::list_processes).post(handlers::create_process),
        )
        .route("/api/v1/processes/page", get(handlers::list_processes_page))
        .route(
            "/api/v1/processes/{name}",
            get(handlers::get_process).delete(handlers::delete_process),
        )
        .route(
            "/api/v1/processes/{name}/instances",
            get(handlers::process_instances),
        )
        .route(
            "/api/v1/processes/{name}/scale",
            post(handlers::scale_process),
        )
        .route(
            "/api/v1/processes/{name}/rolling-restart",
            post(handlers::rolling_restart_process),
        )
        .route(
            "/api/v1/processes/{name}/start",
            post(handlers::start_process),
        )
        .route(
            "/api/v1/processes/{name}/stop",
            post(handlers::stop_process),
        )
        .route(
            "/api/v1/processes/{name}/restart",
            post(handlers::restart_process),
        )
        // Added by child 06 (macOS SystemRegistered convert flow).
        .route(
            "/api/v1/processes/{name}/convert",
            post(handlers::convert_process),
        )
        .route("/api/v1/processes/{name}/logs", get(ws::process_logs))
        // Jobs
        .route(
            "/api/v1/jobs",
            get(handlers::list_jobs).post(handlers::create_job),
        )
        .route("/api/v1/jobs/page", get(handlers::list_jobs_page))
        .route("/api/v1/jobs/preview", post(handlers::preview_job))
        .route(
            "/api/v1/jobs/{name}",
            get(handlers::get_job)
                .patch(handlers::update_job)
                .delete(handlers::delete_job),
        )
        .route("/api/v1/jobs/{name}/trigger", post(handlers::trigger_job))
        .route("/api/v1/jobs/{name}/runs", get(handlers::list_runs))
        .route("/api/v1/jobs/{name}/runs/{run_id}", get(handlers::get_run))
        .route(
            "/api/v1/jobs/{name}/runs/{run_id}/cancel",
            post(handlers::cancel_run),
        )
        .route("/api/v1/jobs/{name}/runs/{run_id}/logs", get(ws::run_logs))
        // Daemon
        .route("/api/v1/daemon/status", get(handlers::daemon_status))
        .route(
            "/api/v1/daemon/recovery",
            get(handlers::recovery_diagnostics),
        )
        .route("/api/v1/daemon/reload", post(handlers::reload))
        .route(
            "/api/v1/daemon/config/validate",
            post(handlers::validate_config),
        )
        .route("/api/v1/daemon/config/apply", post(handlers::apply_config))
        .route("/api/v1/daemon/shutdown", post(handlers::shutdown))
        .route("/api/v1/service/rotate-token", post(handlers::rotate_token))
        .route("/api/v1/service/backup", post(handlers::backup))
        .route("/api/v1/service/upgrade", post(handlers::upgrade))
        .route("/api/v1/service/rollback", post(handlers::rollback))
        // Events
        .route("/api/v1/events", get(ws::events))
        // Bounded durable observability records. These are additive to the
        // live WebSocket event stream above.
        .route(
            "/api/v1/observability/rules",
            get(handlers::list_alert_rules).put(handlers::upsert_alert_rule),
        )
        .route(
            "/api/v1/observability/rules/{id}",
            delete(handlers::delete_alert_rule),
        )
        .route(
            "/api/v1/observability/events",
            get(handlers::list_operator_events),
        )
        .route(
            "/api/v1/observability/metrics",
            get(handlers::list_metric_samples),
        )
        .route(
            "/api/v1/observability/alerts",
            get(handlers::list_alert_episodes),
        )
        .route(
            "/api/v1/observability/alerts/{id}/ack",
            post(handlers::acknowledge_alert_episode),
        )
        .route(
            "/api/v1/observability/deliveries",
            get(handlers::list_delivery_attempts),
        )
        .layer(middleware::from_fn_with_state(auth, auth::require_bearer))
        .layer(loopback_cors())
        .with_state(facade)
}

fn loopback_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin: &HeaderValue, _| {
            let Ok(origin) = origin.to_str() else {
                return false;
            };
            origin == "http://localhost"
                || origin == "http://127.0.0.1"
                || origin.starts_with("http://localhost:")
                || origin.starts_with("http://127.0.0.1:")
        }))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::HeaderName::from_static("idempotency-key"),
            header::HeaderName::from_static("x-csrf-token"),
        ])
}
