//! Application-level error. Carries enough to pick an `API.md` §5 code and HTTP
//! status, but holds no axum/HTTP types — the facade stays transport-agnostic.

use thiserror::Error;

use my_supervisor_core::ports::error::{
    ConfigError, RegistrarError, RepoError, RunnerError, SchedulerError,
};
use my_supervisor_core::ports::lifecycle::SpawnError;
use my_supervisor_core::ports::shutdown::SignalError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Process,
    Job,
    Run,
}

impl ResourceKind {
    fn label(self) -> &'static str {
        match self {
            ResourceKind::Process => "process",
            ResourceKind::Job => "job",
            ResourceKind::Run => "run",
        }
    }

    fn not_found_code(self) -> &'static str {
        match self {
            ResourceKind::Process => "process_not_found",
            ResourceKind::Job => "job_not_found",
            ResourceKind::Run => "run_not_found",
        }
    }
}

/// 409-class conflicts, each mapping to a distinct §5 code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictReason {
    AlreadyRunning,
    NotRunning,
    CrashLoop,
    NameConflict,
    JobNameConflict,
    HasDependents,
    RunAlreadyFinished,
}

impl ConflictReason {
    fn code(self) -> &'static str {
        match self {
            ConflictReason::AlreadyRunning => "already_running",
            ConflictReason::NotRunning => "not_running",
            ConflictReason::CrashLoop => "crash_loop_detected",
            ConflictReason::NameConflict => "name_conflict",
            ConflictReason::JobNameConflict => "job_name_conflict",
            ConflictReason::HasDependents => "has_dependents",
            ConflictReason::RunAlreadyFinished => "run_already_finished",
        }
    }

    fn message(self) -> &'static str {
        match self {
            ConflictReason::AlreadyRunning => "process is already running",
            ConflictReason::NotRunning => "process is not running",
            ConflictReason::CrashLoop => "process is in a crash loop",
            ConflictReason::NameConflict => "a process with this name already exists",
            ConflictReason::JobNameConflict => "a job with this name already exists",
            ConflictReason::HasDependents => "job has downstream dependents",
            ConflictReason::RunAlreadyFinished => "run is already finished",
        }
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{} '{name}' not found", kind.label())]
    NotFound { kind: ResourceKind, name: String },
    #[error("{}", reason.message())]
    Conflict { reason: ConflictReason },
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("invalid cron expression: {0}")]
    InvalidCron(String),
    #[error("dependency cycle detected")]
    CycleDetected,
    #[error("spawn failed: {0}")]
    SpawnFailed(String),
    #[error("service registration failed: {0}")]
    ServiceRegistrationFailed(String),
    #[error("unit name conflict: {0}")]
    UnitNameConflict(String),
    #[error("not supported on this platform: {0}")]
    NotSupported(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    pub fn not_found(kind: ResourceKind, name: impl Into<String>) -> Self {
        AppError::NotFound {
            kind,
            name: name.into(),
        }
    }

    pub fn conflict(reason: ConflictReason) -> Self {
        AppError::Conflict { reason }
    }

    /// Stable machine-readable code (`API.md` §5).
    pub fn code(&self) -> &'static str {
        match self {
            AppError::NotFound { kind, .. } => kind.not_found_code(),
            AppError::Conflict { reason } => reason.code(),
            AppError::InvalidRequest(_) => "invalid_request",
            AppError::InvalidConfig(_) => "invalid_config",
            AppError::InvalidCron(_) => "invalid_cron_expression",
            AppError::CycleDetected => "cycle_detected",
            AppError::SpawnFailed(_) => "spawn_failed",
            AppError::ServiceRegistrationFailed(_) => "service_registration_failed",
            AppError::UnitNameConflict(_) => "unit_name_conflict",
            AppError::NotSupported(_) => "not_supported_on_platform",
            AppError::Internal(_) => "internal_error",
        }
    }

    /// HTTP status the http adapter should emit for this error.
    pub fn http_status(&self) -> u16 {
        match self {
            AppError::NotFound { .. } => 404,
            AppError::Conflict { .. } | AppError::UnitNameConflict(_) => 409,
            AppError::InvalidRequest(_)
            | AppError::InvalidConfig(_)
            | AppError::InvalidCron(_) => 400,
            AppError::CycleDetected => 422,
            AppError::NotSupported(_) => 422,
            AppError::SpawnFailed(_)
            | AppError::ServiceRegistrationFailed(_)
            | AppError::Internal(_) => 500,
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

// --- Port error conversions -------------------------------------------------

impl From<RepoError> for AppError {
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::NotFound(m) => AppError::Internal(format!("repository: {m}")),
            RepoError::Conflict(m) => AppError::InvalidRequest(m),
            RepoError::Backend(m) => AppError::Internal(format!("repository: {m}")),
        }
    }
}

impl From<SpawnError> for AppError {
    fn from(e: SpawnError) -> Self {
        AppError::SpawnFailed(e.to_string())
    }
}

impl From<SignalError> for AppError {
    fn from(e: SignalError) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl From<SchedulerError> for AppError {
    fn from(e: SchedulerError) -> Self {
        match e {
            SchedulerError::InvalidCron(m) => AppError::InvalidCron(m),
            SchedulerError::Backend(m) => AppError::Internal(format!("scheduler: {m}")),
        }
    }
}

impl From<RunnerError> for AppError {
    fn from(e: RunnerError) -> Self {
        AppError::Internal(format!("runner: {e}"))
    }
}

impl From<ConfigError> for AppError {
    fn from(e: ConfigError) -> Self {
        match e {
            ConfigError::Invalid(m) => AppError::InvalidConfig(m),
            other => AppError::Internal(other.to_string()),
        }
    }
}

impl From<RegistrarError> for AppError {
    fn from(e: RegistrarError) -> Self {
        match e {
            RegistrarError::NotSupported => {
                AppError::NotSupported("SystemRegistered mode".to_string())
            }
            RegistrarError::UnitNameConflict(u) => AppError::UnitNameConflict(u),
            RegistrarError::RegistrationFailed(m) => AppError::ServiceRegistrationFailed(m),
            RegistrarError::NotFound(u) => AppError::not_found(ResourceKind::Process, u),
        }
    }
}
