//! `JobRunner` — executes a Job as a transient process and records the result.
//! Implemented in `application` by composing `LifecycleController` + `LogSink`.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::watch;

use crate::domain::{ChildHandle, Job, JobRun, JobRunId, TriggeredBy};
use crate::ports::error::RunnerError;

/// Facade-owned controls for one active run.  The port keeps the runner
/// transport-independent while allowing the facade to own cancellation and
/// the verified child handle for the entire execution.
#[async_trait]
pub trait RunExecutionControl: Send + Sync {
    async fn publish_child(&self, child: ChildHandle);
    fn child(&self) -> Option<ChildHandle>;
    fn cancellation(&self) -> watch::Receiver<bool>;
    fn should_persist_terminal(&self) -> bool;
}

#[async_trait]
pub trait JobRunner: Send + Sync {
    /// Run the job to completion (or timeout) under the given run id and return
    /// the finalized run. The caller pre-generates `run_id` so it can be
    /// surfaced immediately (e.g. a `Location` header) before completion.
    async fn run(
        &self,
        job: &Job,
        triggered_by: TriggeredBy,
        run_id: JobRunId,
        control: Arc<dyn RunExecutionControl>,
    ) -> Result<JobRun, RunnerError>;
}
