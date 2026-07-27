//! `Scheduler` — evaluates cron/interval/one-shot triggers and emits "run now"
//! events to the application. Dependency triggers are propagated separately by
//! observing run completion (DD-028).

use crate::domain::{Job, JobTrigger, ScheduleOccurrence};
use crate::ports::error::SchedulerError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Emitted when a job's trigger fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleEvent {
    pub job_name: String,
    pub scheduled_at: DateTime<Utc>,
    pub occurrence: ScheduleOccurrence,
}

/// Scheduler state that can be restored without reconstructing timer timing
/// from an already-mutated config definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerSnapshot {
    pub entries: Vec<ScheduledJob>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledJob {
    pub name: String,
    pub trigger: JobTrigger,
}

#[async_trait]
pub trait Scheduler: Send + Sync {
    /// Arm (or re-arm) the timer for a job. Re-registering replaces the prior arming.
    async fn register(&self, job_name: &str, trigger: &JobTrigger) -> Result<(), SchedulerError>;
    /// Full schedule metadata is additive so legacy scheduler test doubles can
    /// retain their trigger-only implementation while production adapters
    /// attach the durable occurrence identity to emitted events.
    async fn register_job(&self, job: &Job) -> Result<(), SchedulerError> {
        self.register(&job.name, &job.trigger).await
    }
    /// Disarm a job's timer.
    async fn unregister(&self, job_name: &str) -> Result<(), SchedulerError>;
    async fn snapshot(&self) -> Result<SchedulerSnapshot, SchedulerError>;
    async fn restore(&self, snapshot: &SchedulerSnapshot) -> Result<(), SchedulerError>;
    /// Pure computation of the next fire time after `after` (None for `DependsOn`).
    fn next_run(&self, trigger: &JobTrigger, after: DateTime<Utc>) -> Option<DateTime<Utc>>;
    /// Pure bounded preview. Implementations must not register timers or emit
    /// events while calculating candidates.
    fn preview(
        &self,
        job: &Job,
        after: DateTime<Utc>,
        count: u16,
    ) -> Result<Vec<DateTime<Utc>>, SchedulerError> {
        if count > 100 {
            return Err(SchedulerError::PreviewBounded("count exceeds 100".into()));
        }
        let mut candidates = Vec::with_capacity(count as usize);
        let mut cursor = after;
        for _ in 0..count {
            let Some(next) = self.next_run(&job.trigger, cursor) else {
                break;
            };
            if next - after > chrono::Duration::days(365 * 5) {
                return Err(SchedulerError::PreviewBounded(
                    "search horizon exceeds 5 years".into(),
                ));
            }
            candidates.push(next);
            cursor = next;
        }
        Ok(candidates)
    }
    /// Wait for the next fire event. The scheduler retains every event until
    /// the single application consumer receives it.
    async fn next_event(&self) -> Option<ScheduleEvent>;
}
