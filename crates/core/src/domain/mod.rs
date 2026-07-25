//! Domain entities and value objects. No wire-format or OS concerns leak here.

pub mod config;
pub mod job;
pub mod log;
pub mod process;

pub use config::{
    ApplyMode, ConfigApplyJournal, ConfigApplyResult, ConfigApplyStage, ConfigDiff, ConfigSnapshot,
    ConfigTargetDirectStart,
    DependencySignature, LoadedConfig,
};
pub use job::{
    DependencyFailurePolicy, Job, JobDeletionJournal, JobDeletionStage, JobId, JobRun, JobRunId, JobRunState, JobTrigger, LogRetention,
    OverlapPolicy, TriggeredBy,
};
pub use log::{LogLine, LogStream, RunLogCleanup};
pub use process::{
    ChildHandle, LifecycleMode, ManagementMode, ProcessResourceUsage, ProcessSpec, ProcessState,
    ProcessStatus,
    RestartPolicy, ShutdownPolicy, ShutdownSignal,
};
