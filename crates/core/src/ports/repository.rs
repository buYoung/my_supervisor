//! Persistence ports. `StateRepository` owns the process registry (specs +
//! restart counters); `JobRepository` owns job definitions and run history.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::process::RuntimeHandleCleanup;
use crate::domain::{
    ChildHandle, ConfigApplyJournal, ConfigApplyStage, ConfigSnapshot, ConfigTargetDirectStart,
    DependencySignature, DurableScheduleOccurrence, GuardSnapshot, Job, JobDeletionJournal,
    JobDeletionStage, JobRun, JobRunId, JobRunState, ProcessDefinitionId, ProcessInstance,
    ProcessInstanceId, ProcessOperation, ProcessSpec, RunLogCleanup, ScheduleAdmission,
    ScheduleFinalization, ScheduleOccurrence,
};
use crate::ports::error::RepoError;
use crate::ports::lifecycle::CleanupTicket;

/// A bounded, snapshot-aware repository read.  The values held in the cursor
/// and watermark are storage ordering keys; transports turn them into opaque
/// public cursors instead of exposing a database ordering contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedPage<T> {
    pub records: Vec<T>,
    pub next_cursor: Option<String>,
    pub high_watermark: String,
    /// Debug/test-only partition faults are surfaced additively so operators
    /// can distinguish a complete empty result from a partial snapshot.
    pub failed_partitions: Vec<String>,
}

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
    /// Additive observability capability. Existing state-only test adapters
    /// retain their behavior; production adapters override these methods.
    async fn observability_upsert_rule(
        &self,
        _rule: &crate::domain::AlertRule,
    ) -> Result<(), RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_delete_rule(
        &self,
        _id: uuid::Uuid,
        _deleted_at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_list_rules(
        &self,
        _limit: usize,
    ) -> Result<Vec<crate::domain::AlertRule>, RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_list_events(
        &self,
        _cursor: Option<&str>,
        _limit: usize,
    ) -> Result<crate::domain::ObservabilityPage<crate::domain::OperatorEvent>, RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_record_metric(
        &self,
        _sample: &crate::domain::MetricSample,
    ) -> Result<(), RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    /// Runs one bounded observability retention/downsampling pass.  The
    /// daemon-owned coordinator supplies `now`; this does not take ownership
    /// of process/job cancellation or caller options.
    async fn observability_maintain(&self, _now: DateTime<Utc>) -> Result<(), RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_upsert_alert_episode(
        &self,
        _episode: &crate::domain::AlertEpisode,
        _dedupe_key: &str,
    ) -> Result<bool, RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_resolve_alert_episode(
        &self,
        _episode: &crate::domain::AlertEpisode,
    ) -> Result<bool, RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_enqueue_delivery_candidate(
        &self,
        _candidate: &crate::domain::DeliveryCandidate,
    ) -> Result<(), RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_claim_delivery_candidates(
        &self,
        _owner: &str,
        _now: DateTime<Utc>,
        _lease_until: DateTime<Utc>,
        _limit: usize,
    ) -> Result<Vec<crate::domain::DeliveryCandidate>, RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_finish_delivery_candidate(
        &self,
        _candidate: &crate::domain::DeliveryCandidate,
        _submission: &crate::domain::DeliverySubmission,
        _occurred_at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_cancel_delivery_candidates_for_alert(
        &self,
        _alert_id: uuid::Uuid,
        _occurred_at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_list_metrics(
        &self,
        _source: Option<&str>,
        _cursor: Option<&str>,
        _limit: usize,
    ) -> Result<crate::domain::ObservabilityPage<crate::domain::MetricSample>, RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_list_alerts(
        &self,
        _cursor: Option<&str>,
        _limit: usize,
    ) -> Result<crate::domain::ObservabilityPage<crate::domain::AlertEpisode>, RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_acknowledge_alert(
        &self,
        _id: uuid::Uuid,
        _at: DateTime<Utc>,
    ) -> Result<bool, RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn observability_list_delivery_attempts(
        &self,
        _alert_id: Option<uuid::Uuid>,
        _cursor: Option<&str>,
        _limit: usize,
    ) -> Result<crate::domain::ObservabilityPage<crate::domain::DeliveryAttempt>, RepoError> {
        Err(RepoError::Backend(
            "observability persistence is not supported".into(),
        ))
    }
    async fn list_specs(&self) -> Result<Vec<ProcessSpec>, RepoError>;
    /// Reads a stable name-ordered slice.  The default keeps lightweight test
    /// repositories source-compatible; durable adapters override it with a
    /// storage-bounded query.
    async fn list_specs_page(
        &self,
        cursor: Option<&str>,
        high_watermark: Option<&str>,
        limit: usize,
    ) -> Result<BoundedPage<ProcessSpec>, RepoError> {
        let mut specs = self.list_specs().await?;
        specs.sort_by(|left, right| left.name.cmp(&right.name));
        let high_watermark = high_watermark
            .map(str::to_owned)
            .or_else(|| specs.last().map(|spec| spec.name.clone()))
            .unwrap_or_default();
        specs.retain(|spec| {
            cursor.is_none_or(|cursor| spec.name.as_str() > cursor) && spec.name <= high_watermark
        });
        let has_more = specs.len() > limit;
        specs.truncate(limit);
        Ok(BoundedPage {
            next_cursor: has_more
                .then(|| specs.last().map(|spec| spec.name.clone()))
                .flatten(),
            records: specs,
            high_watermark,
            failed_partitions: Vec::new(),
        })
    }
    async fn get_spec(&self, name: &str) -> Result<Option<ProcessSpec>, RepoError>;
    async fn save_spec(&self, spec: &ProcessSpec) -> Result<(), RepoError>;
    async fn delete_spec(&self, name: &str) -> Result<(), RepoError>;
    /// Return active durable slots in ordinal order for one definition.
    async fn list_process_instances(
        &self,
        _definition_id: ProcessDefinitionId,
    ) -> Result<Vec<ProcessInstance>, RepoError> {
        Err(RepoError::Backend(
            "process instance persistence is not supported".into(),
        ))
    }
    /// Allocate a fresh non-reused ID for an inactive ordinal.
    async fn allocate_process_instance(
        &self,
        _definition_id: ProcessDefinitionId,
        _ordinal: u16,
    ) -> Result<ProcessInstance, RepoError> {
        Err(RepoError::Backend(
            "process instance persistence is not supported".into(),
        ))
    }
    /// Retire a slot without making its identity available for reuse.
    async fn retire_process_instance(
        &self,
        _instance_id: ProcessInstanceId,
    ) -> Result<(), RepoError> {
        Err(RepoError::Backend(
            "process instance persistence is not supported".into(),
        ))
    }
    /// Atomically retire an old, already-drained slot and promote a verified
    /// surge replacement into its ordinal. The replacement keeps its UUID.
    async fn promote_process_instance(
        &self,
        _retired: &ProcessInstance,
        _replacement: &ProcessInstance,
    ) -> Result<Option<ProcessInstance>, RepoError> {
        Err(RepoError::Backend(
            "process instance persistence is not supported".into(),
        ))
    }
    /// Read the actual native child handle for this exact slot generation.
    /// New multi-instance callers must not fall back to the name-keyed legacy
    /// handle, which only remains for ordinal-zero compatibility.
    async fn get_process_instance_runtime_handle(
        &self,
        _instance: &ProcessInstance,
    ) -> Result<Option<ChildHandle>, RepoError> {
        Err(RepoError::Backend(
            "process instance runtime persistence is not supported".into(),
        ))
    }
    /// Persist a handle only while the stable slot and logical generation still
    /// match. Returns false when a newer generation owns the slot.
    async fn set_process_instance_runtime_handle(
        &self,
        _instance: &ProcessInstance,
        _handle: Option<&ChildHandle>,
    ) -> Result<bool, RepoError> {
        Err(RepoError::Backend(
            "process instance runtime persistence is not supported".into(),
        ))
    }
    /// Clear only the exact native identity belonging to this logical slot
    /// generation. This prevents stale sibling cleanup from erasing a replacement.
    async fn clear_process_instance_runtime_handle_if_matches(
        &self,
        _instance: &ProcessInstance,
        _handle: &ChildHandle,
    ) -> Result<bool, RepoError> {
        Err(RepoError::Backend(
            "process instance runtime persistence is not supported".into(),
        ))
    }
    /// Advance one still-active slot after its prior generation is gone.
    async fn advance_process_instance_generation(
        &self,
        _instance: &ProcessInstance,
    ) -> Result<Option<ProcessInstance>, RepoError> {
        Err(RepoError::Backend(
            "process instance runtime persistence is not supported".into(),
        ))
    }
    async fn get_process_instance_restart_count(
        &self,
        _instance: &ProcessInstance,
    ) -> Result<u32, RepoError> {
        Ok(0)
    }
    async fn set_process_instance_restart_count(
        &self,
        _instance: &ProcessInstance,
        _count: u32,
    ) -> Result<bool, RepoError> {
        Ok(false)
    }
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
    /// Latest bounded runtime-guard evidence. The durable snapshot is only
    /// historical evidence after a daemon restart; application ownership
    /// decides when it becomes fresh again.
    async fn latest_guard_snapshot(&self, _name: &str) -> Result<Option<GuardSnapshot>, RepoError> {
        Ok(None)
    }
    async fn upsert_guard_snapshot(
        &self,
        _name: &str,
        _snapshot: &GuardSnapshot,
    ) -> Result<(), RepoError> {
        Ok(())
    }
    async fn latest_process_instance_guard_snapshot(
        &self,
        _name: &str,
        _instance: &ProcessInstance,
    ) -> Result<Option<GuardSnapshot>, RepoError> {
        Ok(None)
    }
    async fn upsert_process_instance_guard_snapshot(
        &self,
        _name: &str,
        _instance: &ProcessInstance,
        _snapshot: &GuardSnapshot,
    ) -> Result<(), RepoError> {
        Ok(())
    }
    /// Return a completed or in-progress operation by its caller supplied
    /// idempotency key.  The name/kind check remains an application concern.
    async fn get_process_operation(
        &self,
        _operation_id: uuid::Uuid,
    ) -> Result<Option<ProcessOperation>, RepoError> {
        Err(RepoError::Backend(
            "process operation persistence is not supported".into(),
        ))
    }
    /// Persist the complete operation state atomically. Implementations must
    /// not replace a record with a different name or operation kind.
    async fn save_process_operation(&self, _operation: &ProcessOperation) -> Result<(), RepoError> {
        Err(RepoError::Backend(
            "process operation persistence is not supported".into(),
        ))
    }
    /// Incomplete records are recovered as explicit fail-closed outcomes at
    /// daemon bootstrap; they are never silently resumed without the original
    /// caller's cancellation context.
    async fn list_incomplete_process_operations(&self) -> Result<Vec<ProcessOperation>, RepoError> {
        Err(RepoError::Backend(
            "process operation persistence is not supported".into(),
        ))
    }
}

/// Job definitions plus run history.
#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn list_jobs(&self) -> Result<Vec<Job>, RepoError>;
    /// See `StateRepository::list_specs_page` for the compatibility rationale.
    async fn list_jobs_page(
        &self,
        cursor: Option<&str>,
        high_watermark: Option<&str>,
        limit: usize,
    ) -> Result<BoundedPage<Job>, RepoError> {
        let mut jobs = self.list_jobs().await?;
        jobs.sort_by(|left, right| left.name.cmp(&right.name));
        let high_watermark = high_watermark
            .map(str::to_owned)
            .or_else(|| jobs.last().map(|job| job.name.clone()))
            .unwrap_or_default();
        jobs.retain(|job| {
            cursor.is_none_or(|cursor| job.name.as_str() > cursor) && job.name <= high_watermark
        });
        let has_more = jobs.len() > limit;
        jobs.truncate(limit);
        Ok(BoundedPage {
            next_cursor: has_more
                .then(|| jobs.last().map(|job| job.name.clone()))
                .flatten(),
            records: jobs,
            high_watermark,
            failed_partitions: Vec::new(),
        })
    }
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
    async fn create_job_deletion_journal(
        &self,
        journal: &JobDeletionJournal,
    ) -> Result<(), RepoError>;
    async fn get_job_deletion_journal(
        &self,
        name: &str,
    ) -> Result<Option<JobDeletionJournal>, RepoError>;
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
    /// Atomically inserts the logical occurrence (if needed) and advances the
    /// schedule cursor in the same durable transaction.
    async fn claim_schedule_occurrence(
        &self,
        _job: &Job,
        _occurrence: &ScheduleOccurrence,
        _now: DateTime<Utc>,
    ) -> Result<DurableScheduleOccurrence, RepoError> {
        Err(RepoError::Backend(
            "durable schedule occurrence persistence is not supported".into(),
        ))
    }
    async fn schedule_cursor(&self, _job: &Job) -> Result<Option<DateTime<Utc>>, RepoError> {
        Ok(None)
    }
    /// Returns only durable work whose persisted absolute retry time is due.
    async fn list_due_schedule_occurrences(
        &self,
        _now: DateTime<Utc>,
        _limit: usize,
    ) -> Result<Vec<DurableScheduleOccurrence>, RepoError> {
        Ok(Vec::new())
    }
    /// Applies per-job admission before creating a child.  Queue/overflow
    /// decisions are durable and therefore survive a daemon restart.
    async fn admit_schedule_occurrence(
        &self,
        _job: &Job,
        _occurrence: &ScheduleOccurrence,
        _now: DateTime<Utc>,
    ) -> Result<ScheduleAdmission, RepoError> {
        Err(RepoError::Backend(
            "durable schedule admission is not supported".into(),
        ))
    }
    /// Completes one attempt after the existing terminal Run/outbox commit.
    /// A retry retains the logical occurrence and only creates a new attempt.
    async fn finalize_schedule_attempt(
        &self,
        _job: &Job,
        _run: &JobRun,
        _now: DateTime<Utc>,
    ) -> Result<ScheduleFinalization, RepoError> {
        Err(RepoError::Backend(
            "durable schedule finalization is not supported".into(),
        ))
    }
    /// Running children are not recoverable after host restart.  Convert them
    /// back to a durable retry/claim without manufacturing a new occurrence.
    async fn recover_schedule_occurrences(&self, _now: DateTime<Utc>) -> Result<(), RepoError> {
        Ok(())
    }
    async fn is_durable_schedule_run(&self, _run_id: JobRunId) -> Result<bool, RepoError> {
        Ok(false)
    }
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
        runs.retain(|run| {
            since.is_none_or(|since| run.started_at.unwrap_or(run.scheduled_at) >= since)
        });
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
    async fn pending_transient_cleanup(
        &self,
        limit: usize,
    ) -> Result<Vec<CleanupTicket>, RepoError>;
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
    async fn create_config_apply_journal(
        &self,
        journal: &ConfigApplyJournal,
    ) -> Result<(), RepoError>;
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
    async fn restore_config_apply_snapshot(
        &self,
        apply_id: uuid::Uuid,
    ) -> Result<ConfigSnapshot, RepoError>;
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
