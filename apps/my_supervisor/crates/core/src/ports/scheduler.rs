//! `Scheduler` — evaluates cron/interval/one-shot triggers and emits "run now"
//! events to the application. Dependency triggers are propagated separately by
//! observing run completion (DD-028).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::broadcast;

use crate::domain::JobTrigger;
use crate::ports::error::SchedulerError;

/// Emitted when a job's trigger fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleEvent {
    pub job_name: String,
    pub scheduled_at: DateTime<Utc>,
}

#[async_trait]
pub trait Scheduler: Send + Sync {
    /// Arm (or re-arm) the timer for a job. Re-registering replaces the prior arming.
    async fn register(&self, job_name: &str, trigger: &JobTrigger) -> Result<(), SchedulerError>;
    /// Disarm a job's timer.
    async fn unregister(&self, job_name: &str) -> Result<(), SchedulerError>;
    /// Pure computation of the next fire time after `after` (None for `DependsOn`).
    fn next_run(&self, trigger: &JobTrigger, after: DateTime<Utc>) -> Option<DateTime<Utc>>;
    /// Subscribe to fire events.
    fn subscribe(&self) -> broadcast::Receiver<ScheduleEvent>;
}
