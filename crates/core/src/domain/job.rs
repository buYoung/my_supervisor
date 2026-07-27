//! Job (batch scheduler) domain entities. A Job is a definition; a JobRun is a
//! single execution instance. Distinct lifecycle from a supervised Process
//! (run → expected exit), hence a separate entity (DD-023).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identity of a Job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub Uuid);

impl JobId {
    pub fn new() -> Self {
        JobId(Uuid::new_v4())
    }
}

impl Default for JobId {
    fn default() -> Self {
        JobId::new()
    }
}

/// Stable identity of a single Job execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobRunId(pub Uuid);

impl JobRunId {
    pub fn new() -> Self {
        JobRunId(Uuid::new_v4())
    }
}

impl Default for JobRunId {
    fn default() -> Self {
        JobRunId::new()
    }
}

/// Durable forward-recovery state for a destructive Job removal.  The journal
/// retains the original definition because a process restart can occur after
/// dispatch has been frozen but before the database rows are removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobDeletionStage {
    Prepared,
    DispatchFrozen,
    SchedulerUnregistered,
    /// A failure before cancellation must converge by restoring dispatch, not
    /// by retrying the destructive path after a restart.
    RollbackRequired,
    CancellationStarted,
    RunsDraining,
    RowsDeleted,
    LogsCleaning,
    Completed,
}

impl JobDeletionStage {
    pub fn is_irreversible(self) -> bool {
        matches!(
            self,
            Self::CancellationStarted
                | Self::RunsDraining
                | Self::RowsDeleted
                | Self::LogsCleaning
                | Self::Completed
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobDeletionJournal {
    pub deletion_id: Uuid,
    pub job: Job,
    pub stage: JobDeletionStage,
    /// Persisted before the Job/Run rows are committed away so log cleanup can
    /// resume after a crash without rediscovering a potentially reused name.
    pub run_ids: Vec<JobRunId>,
    pub last_error: Option<String>,
}

/// One-of trigger that decides when a Job runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobTrigger {
    /// 5-field cron expression, e.g. `0 */6 * * *`.
    Cron(String),
    /// Fixed interval between runs.
    Interval(Duration),
    /// Single scheduled run at an absolute time.
    OneShot(DateTime<Utc>),
    /// Run when all named upstream jobs succeed (AND semantics, on-success).
    DependsOn(Vec<String>),
}

/// What happens when a trigger fires while a prior run is still in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OverlapPolicy {
    #[default]
    Skip,
    Queue,
    Parallel,
}

/// Missed timer behavior. Legacy definitions retain `Skip`; newly supplied
/// schedules default to one bounded recovery run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MisfirePolicy {
    #[default]
    Skip,
    RunOnce,
    CatchUp {
        max_occurrences: u16,
        max_age: Duration,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum QueueOverflowPolicy {
    #[default]
    RejectNew,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Includes the initial attempt. `1` disables retry.
    pub max_attempts: u16,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub multiplier: u8,
    /// Percentage in the inclusive range 0..=100.
    pub jitter_percent: u8,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 1,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(300),
            multiplier: 2,
            jitter_percent: 20,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionPolicy {
    pub max_concurrency: u16,
    pub max_queue: u16,
    pub overflow: QueueOverflowPolicy,
}

impl AdmissionPolicy {
    pub fn legacy(overlap: OverlapPolicy) -> Self {
        match overlap {
            OverlapPolicy::Skip => Self {
                max_concurrency: 1,
                max_queue: 0,
                overflow: QueueOverflowPolicy::Skip,
            },
            OverlapPolicy::Queue => Self {
                max_concurrency: 1,
                max_queue: 1024,
                overflow: QueueOverflowPolicy::RejectNew,
            },
            OverlapPolicy::Parallel => Self {
                max_concurrency: 32,
                max_queue: 1024,
                overflow: QueueOverflowPolicy::RejectNew,
            },
        }
    }
}

impl Default for AdmissionPolicy {
    fn default() -> Self {
        Self::legacy(OverlapPolicy::Skip)
    }
}

/// Stable identity of one logical timer occurrence. Attempt/run IDs remain
/// separate so retry state can be added without rewriting run history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleOccurrence {
    pub trigger_id: Uuid,
    pub schedule_revision: u64,
    pub scheduled_at: DateTime<Utc>,
    pub attempt: u16,
}

/// Persisted state for one logical scheduled occurrence.  A timer notification
/// is only a candidate: this ledger is the restart-safe delivery authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleOccurrenceState {
    Claimed,
    Queued,
    Running,
    RetryPending,
    Finalized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableScheduleOccurrence {
    pub job_id: JobId,
    pub job_name: String,
    pub occurrence: ScheduleOccurrence,
    pub state: ScheduleOccurrenceState,
    /// Absolute retry time calculated once and persisted with the occurrence.
    pub next_attempt_at: DateTime<Utc>,
    pub run_id: Option<JobRunId>,
    pub final_state: Option<JobRunState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleAdmission {
    Start(DurableScheduleOccurrence),
    Queued(DurableScheduleOccurrence),
    Finalized(DurableScheduleOccurrence),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleFinalization {
    Retry(DurableScheduleOccurrence),
    Finalized(DurableScheduleOccurrence),
}

/// What happens when an upstream dependency failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DependencyFailurePolicy {
    #[default]
    Skip,
    RunAnyway,
}

/// Run-log retention bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LogRetention {
    pub max_runs: Option<u32>,
    pub max_age_days: Option<u32>,
}

/// A registered batch-job definition. Mirrors a single `[[job]]` config block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub trigger: JobTrigger,
    pub on_overlap: OverlapPolicy,
    pub on_dependency_failure: DependencyFailurePolicy,
    pub timeout: Option<Duration>,
    pub log_retention: LogRetention,
    /// Explicit IANA zone for cron interpretation. Legacy rows are UTC.
    #[serde(default = "legacy_timezone")]
    pub timezone: String,
    #[serde(default)]
    pub schedule_revision: u64,
    #[serde(default = "Uuid::new_v4")]
    pub trigger_id: Uuid,
    #[serde(default)]
    pub misfire_policy: MisfirePolicy,
    #[serde(default)]
    pub retry_policy: RetryPolicy,
    #[serde(default)]
    pub admission: AdmissionPolicy,
}

fn legacy_timezone() -> String {
    "UTC".to_string()
}

/// State machine for a single run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobRunState {
    Pending,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
    Skipped,
}

impl JobRunState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobRunState::Succeeded
                | JobRunState::Failed
                | JobRunState::TimedOut
                | JobRunState::Cancelled
                | JobRunState::Skipped
        )
    }
}

/// What caused a run to be created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggeredBy {
    Schedule,
    /// New schedule dispatches retain the event identity while legacy rows
    /// continue to deserialize as `Schedule`.
    Scheduled {
        occurrence: ScheduleOccurrence,
    },
    Manual,
    Dependency {
        upstream_run_id: JobRunId,
    },
}

/// A single execution instance of a Job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRun {
    pub run_id: JobRunId,
    pub job_name: String,
    /// Identity of the definition that created this run.  Names may be reused
    /// after deletion, so a late runner must never attach to a replacement job.
    pub job_id: JobId,
    pub triggered_by: TriggeredBy,
    pub scheduled_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub state: JobRunState,
    /// Absent for legacy/manual/dependency runs.
    #[serde(default)]
    pub occurrence: Option<ScheduleOccurrence>,
    #[serde(default)]
    pub original_scheduled_at: Option<DateTime<Utc>>,
}
