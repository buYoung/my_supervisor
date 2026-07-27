//! Port traits — the seams `application` depends on and adapters implement.

pub mod clock;
pub mod config_source;
pub mod error;
pub mod job_runner;
pub mod lifecycle;
pub mod log_sink;
pub mod observability;
pub mod registrar;
pub mod repository;
pub mod scheduler;
pub mod shutdown;

pub use clock::{RealClock, SystemClock};
pub use config_source::ConfigSource;
pub use error::{ConfigError, LogError, RegistrarError, RepoError, RunnerError, SchedulerError};
pub use job_runner::{JobRunner, RunExecutionControl};
pub use lifecycle::{
    Aliveness, CheckOutcome, CleanupTicket, GuardError, LifecycleController,
    OwnedGroupResourceUsage, ProbeError, ReapError, SpawnError, TransientCleanupStage,
    TransientCompletion, TransientOutcome, WatchRegistrationId,
};
pub use log_sink::{LogSink, LogTail};
pub use observability::{AlertDelivery, ObservabilityRepository};
pub use registrar::ProcessServiceRegistrar;
pub use repository::{JobRepository, StateRepository, TransientTerminalEvent};
pub use scheduler::{ScheduleEvent, ScheduledJob, Scheduler, SchedulerSnapshot};
pub use shutdown::{ShutdownSignaler, SignalError};
