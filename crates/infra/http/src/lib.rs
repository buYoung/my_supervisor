//! `my-supervisor-infra-http` — the operations axum Router and the host
//! assembly entry point. The route manifest below is the frozen contract that
//! children 02/03/04/05 build against; it is pinned to `docs/API.md` for the
//! in-scope operations surface. The `convert` route (child 06) and the
//! `/api/v1/rules*` family (Phase 2) are intentionally absent — not stubbed.

mod error;
mod handlers;
pub mod mapping;
mod ws;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::CorsLayer;

use my_supervisor_application::{AppDeps, OperationsFacade};

pub use error::HttpError;
pub use ws::event_to_wire;

/// The assembled host artifacts: the unbound Router and the facade. Each host
/// owns the bind address/port (daemon `127.0.0.1:9876`; the Tauri devBridge its
/// own loopback port).
pub struct Assembled {
    pub router: Router,
    pub facade: Arc<OperationsFacade>,
}

/// Build the wired facade and operations Router from injected adapters.
pub fn assemble(deps: AppDeps) -> Assembled {
    let facade = OperationsFacade::new(deps);
    let router = build_router(facade.clone());
    Assembled { router, facade }
}

/// The enumerated operations route manifest (the frozen contract of record).
pub fn build_router(facade: Arc<OperationsFacade>) -> Router {
    Router::new()
        .route("/api/v1/health", get(handlers::health))
        // Processes
        .route(
            "/api/v1/processes",
            get(handlers::list_processes).post(handlers::create_process),
        )
        .route(
            "/api/v1/processes/{name}",
            get(handlers::get_process).delete(handlers::delete_process),
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
        .route("/api/v1/daemon/recovery", get(handlers::recovery_diagnostics))
        .route("/api/v1/daemon/reload", post(handlers::reload))
        .route(
            "/api/v1/daemon/config/validate",
            post(handlers::validate_config),
        )
        .route(
            "/api/v1/daemon/config/apply",
            post(handlers::apply_config),
        )
        .route("/api/v1/daemon/shutdown", post(handlers::shutdown))
        // Events
        .route("/api/v1/events", get(ws::events))
        // Permissive CORS is safe here: the surface is loopback-only, single-user,
        // no-auth (DD-011). It lets the standalone browser UI (a different origin,
        // e.g. the Vite dev server) reach the daemon; the Tauri invoke path and the
        // CLI never need it.
        .layer(CorsLayer::permissive())
        .with_state(facade)
}
