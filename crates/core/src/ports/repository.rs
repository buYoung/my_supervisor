//! Persistence ports. `StateRepository` owns the process registry (specs +
//! restart counters); `JobRepository` owns job definitions and run history.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::{
    ChildHandle, ConfigApplyJournal, ConfigApplyStage, ConfigSnapshot, ConfigTargetDirectStart, DependencySignature, Job,
    JobRun, JobDeletionJournal, JobDeletionStage, JobRunId, JobRunState, ProcessSpec, RunLogCleanup,
};
use crate::domain::process::RuntimeHandleCleanup;
use crate::ports::lifecycle::CleanupTicket;
use crate::ports::error::RepoError;

/// Persisted terminal notification for a cleanup ticket.  It is deliberately
/// a core value rather than an application `DomainEvent` so repository
/// implementations can commit it with the terminal Run row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransientTerminalEvent {
    pub cleanup_id: uuid::Uuid,
    /// Stable identity for every retry of this terminal notification.
    pub event_id: uuid::Uuid,
    /// The terminal transition time, retained across delivery retries.
    pub occurred_at: DateTime<Utc>,
    pub job_name: String,
    pub run_id: JobRunId,
    pub state: JobRunState,
    pub exit_code: Option<i32>,
}

/// Durable process registry. Survives daemon restart so the managed-process
/// list is remembered; live runtime status is held in memory by the supervisor.
#[async_trait]
pub trait StateRepository: Send + Sync {
    async fn list_specs(&self) -> Result<Vec<ProcessSpec>, RepoError>;
    async fn get_spec(&self, name: &str) -> Result<Option<ProcessSpec>, RepoError>;
    async fn save_spec(&self, spec: &ProcessSpec) -> Result<(), RepoError>;
    async fn delete_spec(&self, name: &str) -> Result<(), RepoError>;
    async fn get_restart_count(&self, name: &str) -> Result<u32, RepoError>;
    async fn set_restart_count(&self, name: &str, count: u32) -> Result<(), RepoError>;
    async fn get_runtime_handle(&self, name: &str) -> Result<Option<ChildHandle>, RepoError>;
    async fn set_runtime_handle(
        &self,
        name: &str,
        handle: Option<&ChildHandle>,
    ) -> Result<(), RepoError>;
    /// Queue retryable cleanup after an already-reaped process could not clear
    /// its durable handle.  The identity is part of the record so a later
    /// retry cannot erase a replacement process.
    async fn enqueue_runtime_handle_cleanup(
        &self,
        name: &str,
        handle: &ChildHandle,
        error: &str,
    ) -> Result<(), RepoError>;
    async fn pending_runtime_handle_cleanup(
        &self,
        limit: usize,
    ) -> Result<Vec<RuntimeHandleCleanup>, RepoError>;
    /// Clear a durable handle only when it still has the queued identity.
    /// Returns `false` when a replacement handle has taken its place.
    async fn clear_runtime_handle_if_matches(
        &self,
        cleanup: &RuntimeHandleCleanup,
    ) -> Result<bool, RepoError>;
    async fn complete_runtime_handle_cleanup(&self, name: &str) -> Result<(), RepoError>;
}

/// Job definitions plus run history.
#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn list_jobs(&self) -> Result<Vec<Job>, RepoError>;
    async fn get_job(&self, name: &str) -> Result<Option<Job>, RepoError>;
    async fn save_job(&self, job: &Job) -> Result<(), RepoError>;
    /// Delete a job and its run history, returning run IDs whose log files can be removed.
    async fn delete_job(&self, name: &str) -> Result<Vec<JobRunId>, RepoError>;
    /// Crosses the irreversible row-deletion boundary for a durable deletion
    /// journal.  The adapter must atomically enqueue every Run log cleanup,
    /// delete the Job/Run/dependency rows, and advance this exact journal to
    /// `RowsDeleted` with the stable Run-ID set.
    async fn commit_job_deletion_rows(
        &self,
        deletion_id: uuid::Uuid,
        job_name: &str,
    ) -> Result<Vec<JobRunId>, RepoError>;
    /// The deletion journal commits each external-effect boundary.  A journal
    /// is keyed by the original Job identity, not merely its reusable name.
    async fn create_job_deletion_journal(&self, journal: &JobDeletionJournal) -> Result<(), RepoError>;
    async fn get_job_deletion_journal(&self, name: &str) -> Result<Option<JobDeletionJournal>, RepoError>;
    async fn list_incomplete_job_deletions(&self) -> Result<Vec<JobDeletionJournal>, RepoError>;
    async fn update_job_deletion_journal(
        &self,
        deletion_id: uuid::Uuid,
        stage: JobDeletionStage,
        run_ids: Option<&[JobRunId]>,
        error: Option<&str>,
    ) -> Result<(), RepoError>;
    /// Atomically marks every queued Run terminal, inserts its durable external
    /// event, and crosses the destructive cancellation boundary.  If this
    /// fails, neither the Run rows, outbox records, nor journal may advance.
    async fn cancel_queued_runs_for_job_deletion(
        &self,
        deletion_id: uuid::Uuid,
        job_name: &str,
        terminal_events: &[TransientTerminalEvent],
    ) -> Result<(), RepoError>;
    async fn clear_job_deletion_journal(&self, deletion_id: uuid::Uuid) -> Result<(), RepoError>;
    async fn save_run(&self, run: &JobRun) -> Result<(), RepoError>;
    /// Atomically persist a terminal Run and the durable external-delivery
    /// record. Internal scheduler notifications are intentionally outside this
    /// port; they do not acknowledge the external outbox.
    async fn commit_terminal_run_with_event(
        &self,
        run: &JobRun,
        event: &TransientTerminalEvent,
    ) -> Result<(), RepoError>;
    async fn list_runs(&self, job_name: &str, limit: usize) -> Result<Vec<JobRun>, RepoError>;
    /// Applies the query predicates before the result limit.  The default is
    /// correct for non-SQL test repositories; durable adapters should push the
    /// predicates down to their storage engine.
    async fn list_runs_filtered(
        &self,
        job_name: &str,
        state: Option<JobRunState>,
        since: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<JobRun>, RepoError> {
        let mut runs = self.list_runs(job_name, usize::MAX).await?;
        runs.retain(|run| state.is_none_or(|state| run.state == state));
        runs.retain(|run| since.is_none_or(|since| run.started_at.unwrap_or(run.scheduled_at) >= since));
        runs.truncate(limit);
        Ok(runs)
    }
    async fn get_run(&self, job_name: &str, run_id: &JobRunId)
        -> Result<Option<JobRun>, RepoError>;
    /// Delete terminal runs exceeding either retention bound and return their IDs.
    async fn prune_runs(
        &self,
        job_name: &str,
        max_runs: Option<u32>,
        older_than: Option<DateTime<Utc>>,
    ) -> Result<Vec<JobRunId>, RepoError>;
    /// Lists durable journal removals that were committed with Run deletion.
    async fn pending_run_log_cleanup(&self, limit: usize) -> Result<Vec<RunLogCleanup>, RepoError>;
    async fn complete_run_log_cleanup(&self, run_id: JobRunId) -> Result<(), RepoError>;
    async fn fail_run_log_cleanup(&self, run_id: JobRunId, error: &str) -> Result<(), RepoError>;
    /// Used by startup reconciliation for a file with no remaining Run row.
    async fn enqueue_run_log_cleanup(&self, run_id: JobRunId) -> Result<(), RepoError>;
    /// Cleanup tickets are durable independently of an adapter's in-memory
    /// child/pump ownership so restart reconciliation can resume them.
    async fn enqueue_transient_cleanup(&self, ticket: &CleanupTicket) -> Result<(), RepoError>;
    async fn pending_transient_cleanup(&self, limit: usize) -> Result<Vec<CleanupTicket>, RepoError>;
    async fn update_transient_cleanup(
        &self,
        ticket: &CleanupTicket,
        stage: crate::ports::lifecycle::TransientCleanupStage,
        error: Option<&str>,
    ) -> Result<(), RepoError>;
    async fn complete_transient_cleanup(&self, cleanup_id: uuid::Uuid) -> Result<(), RepoError>;
    /// Atomically persist the original terminal outcome and its event outbox
    /// record.  The ticket remains until the outbox has been delivered.
    async fn commit_transient_cleanup_terminal(
        &self,
        ticket: &CleanupTicket,
        run: &JobRun,
    ) -> Result<(), RepoError>;
    async fn pending_transient_terminal_events(
        &self,
        limit: usize,
    ) -> Result<Vec<TransientTerminalEvent>, RepoError>;
    /// Acknowledging delivery also removes the corresponding ticket so a
    /// restart can never lose a committed terminal transition before delivery.
    async fn acknowledge_transient_terminal_event(
        &self,
        event_id: uuid::Uuid,
        cleanup_id: uuid::Uuid,
    ) -> Result<(), RepoError>;

    /// Persist one complete config snapshot in a single database transaction.
    /// The application performs external side effects around this atomic
    /// boundary and uses the journal methods below for crash recovery.
    async fn apply_config_snapshot(&self, snapshot: &ConfigSnapshot) -> Result<(), RepoError>;
    async fn create_config_apply_journal(&self, journal: &ConfigApplyJournal) -> Result<(), RepoError>;
    async fn set_config_apply_stage(
        &self,
        apply_id: uuid::Uuid,
        stage: ConfigApplyStage,
        compensation_error: Option<&str>,
    ) -> Result<(), RepoError>;
    /// Records target Direct start intent before spawn and its verified native
    /// generation afterwards.  The complete journal remains the recovery
    /// source of truth across daemon restart.
    async fn record_config_target_direct_start(
        &self,
        apply_id: uuid::Uuid,
        start: &ConfigTargetDirectStart,
    ) -> Result<(), RepoError>;
    async fn list_incomplete_config_applies(&self) -> Result<Vec<ConfigApplyJournal>, RepoError>;
    async fn restore_config_apply_snapshot(&self, apply_id: uuid::Uuid) -> Result<ConfigSnapshot, RepoError>;
    async fn clear_config_apply_journal(&self, apply_id: uuid::Uuid) -> Result<(), RepoError>;

    /// Atomically record a dependency signature and its pending downstream run.
    /// Returns false when the signature was already consumed.
    async fn claim_dependency_run(
        &self,
        job_name: &str,
        signature: &DependencySignature,
        run: &JobRun,
    ) -> Result<bool, RepoError>;
}
