//! `JobRunner` — executes a Job as a transient process and records the result.
//! Implemented in `application` by composing `LifecycleController` + `LogSink`.

use async_trait::async_trait;

use crate::domain::{Job, JobRun, JobRunId, TriggeredBy};
use crate::ports::error::RunnerError;

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
    ) -> Result<JobRun, RunnerError>;
}
