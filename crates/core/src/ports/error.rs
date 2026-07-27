//! Error types returned by port traits. Adapters map their concrete failures
//! onto these; the application layer maps these onto API error codes.

use thiserror::Error;

/// Persistence-layer failure (`StateRepository`, `JobRepository`).
#[derive(Debug, Error)]
pub enum RepoError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("backend error: {0}")]
    Backend(String),
}

/// Scheduler registration / evaluation failure.
#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("invalid cron expression: {0}")]
    InvalidCron(String),
    #[error("invalid or unavailable timezone: {0}")]
    InvalidTimezone(String),
    #[error("schedule preview is bounded or unsatisfiable: {0}")]
    PreviewBounded(String),
    #[error("scheduler error: {0}")]
    Backend(String),
}

/// Job execution failure raised by `JobRunner`.
#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("failed to launch job '{0}'")]
    Launch(String),
    #[error("runner error: {0}")]
    Backend(String),
    #[error("job child could not be reaped: {0}")]
    Unreaped(String),
}

/// Durable log-journal failure.  A failed append must not be announced to
/// subscribers because consumers use the source sequence as their cursor.
#[derive(Debug, Error)]
pub enum LogError {
    #[error("log source is sealed: {0}")]
    Sealed(String),
    #[error("log storage error: {0}")]
    Storage(String),
}

/// Config load / validation failure.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config not found at {0}")]
    NotFound(String),
    #[error("invalid config: {0}")]
    Invalid(String),
    #[error("io error: {0}")]
    Io(String),
}

/// OS service-manager registration failure (`ProcessServiceRegistrar`).
#[derive(Debug, Error)]
pub enum RegistrarError {
    #[error("not supported on this platform")]
    NotSupported,
    #[error("unit name conflict: {0}")]
    UnitNameConflict(String),
    #[error("service registration failed: {0}")]
    RegistrationFailed(String),
    #[error("unit not found: {0}")]
    NotFound(String),
}
