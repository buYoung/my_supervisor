//! Domain entities and value objects. No wire-format or OS concerns leak here.

pub mod config;
pub mod job;
pub mod log;
pub mod observability;
pub mod process;

pub use config::{
    ApplyMode, ConfigApplyJournal, ConfigApplyResult, ConfigApplyStage, ConfigDiff, ConfigSnapshot,
    ConfigTargetDirectStart, DependencySignature, LoadedConfig,
};
pub use job::{
    AdmissionPolicy, DependencyFailurePolicy, DurableScheduleOccurrence, Job, JobDeletionJournal,
    JobDeletionStage, JobId, JobRun, JobRunId, JobRunState, JobTrigger, LogRetention,
    MisfirePolicy, OverlapPolicy, QueueOverflowPolicy, RetryPolicy, ScheduleAdmission,
    ScheduleFinalization, ScheduleOccurrence, ScheduleOccurrenceState, TriggeredBy,
};
pub use log::{LogLine, LogStream, RunLogCleanup};
pub use observability::{
    AlertEpisode, AlertRule, AlertSeverity, AlertState, DeliveryAttempt, DeliveryCandidate,
    DeliverySubmission, MetricSample, ObservabilityPage, OperatorEvent,
};
pub use process::{
    CheckKind, CheckPolicy, ChildHandle, GuardRestartCause, GuardSnapshot, GuardState, GuardStatus,
    LifecycleMode, ManagementMode, MemoryPolicy, ProcessDefinitionId, ProcessInstance,
    ProcessInstanceId, ProcessInstanceStatus, ProcessOperation, ProcessOperationInstanceOutcome,
    ProcessOperationInstanceState, ProcessOperationKind, ProcessResourceUsage, ProcessSpec,
    ProcessState, ProcessStatus, RestartPolicy, RollingPolicy, ShutdownPolicy, ShutdownSignal,
    WatchPolicy,
};
