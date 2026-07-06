//! Domain entities and value objects. No wire-format or OS concerns leak here.

pub mod config;
pub mod job;
pub mod log;
pub mod process;

pub use config::LoadedConfig;
pub use job::{
    DependencyFailurePolicy, Job, JobId, JobRun, JobRunId, JobRunState, JobTrigger, LogRetention,
    OverlapPolicy, TriggeredBy,
};
pub use log::{LogLine, LogStream};
pub use process::{
    ChildHandle, LifecycleMode, ManagementMode, ProcessSpec, ProcessState, ProcessStatus,
    RestartPolicy, ShutdownPolicy, ShutdownSignal,
};
