//! `my-supervisor-infra-sqlite` — `StateRepository` + `JobRepository` over
//! SQLite (WAL). One `SqliteStore` implements both; the host injects it into
//! both `AppDeps` slots.

mod observability;
mod repr;

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

use my_supervisor_core::domain::process::RuntimeHandleCleanup;
use my_supervisor_core::domain::{
    AlertSeverity, CheckPolicy, ConfigApplyJournal, ConfigApplyStage, ConfigSnapshot,
    ConfigTargetDirectStart, DependencySignature, DurableScheduleOccurrence, GuardSnapshot, Job,
    JobDeletionJournal, JobDeletionStage, JobId, JobRun, JobRunId, JobRunState, LifecycleMode,
    LogRetention, ManagementMode, MemoryPolicy, OperatorEvent, ProcessDefinitionId,
    ProcessInstance, ProcessInstanceId, ProcessOperation, ProcessSpec, RestartPolicy,
    RollingPolicy, RunLogCleanup, ScheduleAdmission, ScheduleFinalization, ScheduleOccurrence,
    ScheduleOccurrenceState, ShutdownPolicy, ShutdownSignal, WatchPolicy,
};
use my_supervisor_core::ports::error::RepoError;
use my_supervisor_core::ports::{
    CleanupTicket, JobRepository, StateRepository, TransientCleanupStage, TransientTerminalEvent,
};

use repr::{TriggerRepr, TriggeredByRepr};

fn backend<E: std::fmt::Display>(e: E) -> RepoError {
    RepoError::Backend(e.to_string())
}

fn dt_to_str(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

fn str_to_dt(s: &str) -> Result<DateTime<Utc>, RepoError> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(backend)
}

fn opt_dt_to_str(dt: &Option<DateTime<Utc>>) -> Option<String> {
    dt.as_ref().map(dt_to_str)
}

fn opt_str_to_dt(s: Option<String>) -> Result<Option<DateTime<Utc>>, RepoError> {
    s.map(|s| str_to_dt(&s)).transpose()
}

fn json_string<T: serde::Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn run_state_to_str(s: JobRunState) -> &'static str {
    match s {
        JobRunState::Pending => "pending",
        JobRunState::Running => "running",
        JobRunState::Succeeded => "succeeded",
        JobRunState::Failed => "failed",
        JobRunState::TimedOut => "timed_out",
        JobRunState::Cancelled => "cancelled",
        JobRunState::Skipped => "skipped",
    }
}

fn str_to_run_state(s: &str) -> JobRunState {
    match s {
        "running" => JobRunState::Running,
        "succeeded" => JobRunState::Succeeded,
        "failed" => JobRunState::Failed,
        "timed_out" => JobRunState::TimedOut,
        "cancelled" => JobRunState::Cancelled,
        "skipped" => JobRunState::Skipped,
        _ => JobRunState::Pending,
    }
}

fn str_to_occurrence_state(state: &str) -> ScheduleOccurrenceState {
    match state {
        "queued" => ScheduleOccurrenceState::Queued,
        "running" => ScheduleOccurrenceState::Running,
        "retry_pending" => ScheduleOccurrenceState::RetryPending,
        "finalized" => ScheduleOccurrenceState::Finalized,
        _ => ScheduleOccurrenceState::Claimed,
    }
}

fn cleanup_stage_to_str(stage: TransientCleanupStage) -> &'static str {
    match stage {
        TransientCleanupStage::TerminateGroup => "terminate_group",
        TransientCleanupStage::WaitLeader => "wait_leader",
        TransientCleanupStage::JoinPumps => "join_pumps",
        TransientCleanupStage::SealLog => "seal_log",
        TransientCleanupStage::PersistTerminal => "persist_terminal",
    }
}

fn str_to_cleanup_stage(stage: &str) -> TransientCleanupStage {
    match stage {
        "wait_leader" => TransientCleanupStage::WaitLeader,
        "join_pumps" => TransientCleanupStage::JoinPumps,
        "seal_log" => TransientCleanupStage::SealLog,
        "persist_terminal" => TransientCleanupStage::PersistTerminal,
        _ => TransientCleanupStage::TerminateGroup,
    }
}

fn deletion_stage_to_str(stage: JobDeletionStage) -> &'static str {
    match stage {
        JobDeletionStage::Prepared => "prepared",
        JobDeletionStage::DispatchFrozen => "dispatch_frozen",
        JobDeletionStage::SchedulerUnregistered => "scheduler_unregistered",
        JobDeletionStage::RollbackRequired => "rollback_required",
        JobDeletionStage::CancellationStarted => "cancellation_started",
        JobDeletionStage::RunsDraining => "runs_draining",
        JobDeletionStage::RowsDeleted => "rows_deleted",
        JobDeletionStage::LogsCleaning => "logs_cleaning",
        JobDeletionStage::Completed => "completed",
    }
}

fn str_to_deletion_stage(stage: &str) -> JobDeletionStage {
    match stage {
        "dispatch_frozen" => JobDeletionStage::DispatchFrozen,
        "scheduler_unregistered" => JobDeletionStage::SchedulerUnregistered,
        "rollback_required" => JobDeletionStage::RollbackRequired,
        "cancellation_started" => JobDeletionStage::CancellationStarted,
        "runs_draining" => JobDeletionStage::RunsDraining,
        "rows_deleted" => JobDeletionStage::RowsDeleted,
        "logs_cleaning" => JobDeletionStage::LogsCleaning,
        "completed" => JobDeletionStage::Completed,
        _ => JobDeletionStage::Prepared,
    }
}

fn job_deletion_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<JobDeletionJournal, RepoError> {
    let job: Job =
        serde_json::from_str(&row.try_get::<String, _>("job_snapshot").map_err(backend)?)
            .map_err(backend)?;
    let run_ids: Vec<JobRunId> =
        serde_json::from_str(&row.try_get::<String, _>("run_ids").map_err(backend)?)
            .map_err(backend)?;
    Ok(JobDeletionJournal {
        deletion_id: uuid::Uuid::parse_str(
            &row.try_get::<String, _>("deletion_id").map_err(backend)?,
        )
        .map_err(backend)?,
        job,
        stage: str_to_deletion_stage(&row.try_get::<String, _>("stage").map_err(backend)?),
        run_ids,
        last_error: row.try_get("last_error").map_err(backend)?,
    })
}

pub struct SqliteStore {
    pool: SqlitePool,
    transient_cleanup_enqueue_failures: AtomicU32,
    terminal_run_commit_failures: AtomicU32,
    transient_terminal_ack_failures: AtomicU32,
    job_deletion_rollback_direction_failures: AtomicU32,
    job_deletion_cancellation_failures: AtomicU32,
    job_deletion_row_commit_failures: AtomicU32,
    job_deletion_clear_failures: AtomicU32,
    config_snapshot_commit_failures: AtomicU32,
}

impl SqliteStore {
    /// Open (creating if missing) the SQLite database at `path` and ensure schema.
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self, RepoError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .map_err(backend)?;
        let store = SqliteStore {
            pool,
            transient_cleanup_enqueue_failures: AtomicU32::new(0),
            terminal_run_commit_failures: AtomicU32::new(0),
            transient_terminal_ack_failures: AtomicU32::new(0),
            job_deletion_rollback_direction_failures: AtomicU32::new(0),
            job_deletion_cancellation_failures: AtomicU32::new(0),
            job_deletion_row_commit_failures: AtomicU32::new(0),
            job_deletion_clear_failures: AtomicU32::new(0),
            config_snapshot_commit_failures: AtomicU32::new(0),
        };
        store.migrate().await?;
        Ok(store)
    }

    /// In-memory store for tests / ephemeral hosts.
    pub async fn connect_in_memory() -> Result<Self, RepoError> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(backend)?
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(backend)?;
        let store = SqliteStore {
            pool,
            transient_cleanup_enqueue_failures: AtomicU32::new(0),
            terminal_run_commit_failures: AtomicU32::new(0),
            transient_terminal_ack_failures: AtomicU32::new(0),
            job_deletion_rollback_direction_failures: AtomicU32::new(0),
            job_deletion_cancellation_failures: AtomicU32::new(0),
            job_deletion_row_commit_failures: AtomicU32::new(0),
            job_deletion_clear_failures: AtomicU32::new(0),
            config_snapshot_commit_failures: AtomicU32::new(0),
        };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&self) -> Result<(), RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let ddl = r#"
            CREATE TABLE IF NOT EXISTS process_specs (
                name          TEXT PRIMARY KEY,
                command       TEXT NOT NULL,
                args          TEXT NOT NULL,
                cwd           TEXT,
                env           TEXT NOT NULL,
                mode          TEXT NOT NULL,
                unit_name     TEXT,
                lifecycle     TEXT NOT NULL,
                autostart     INTEGER NOT NULL,
                restart_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS jobs (
                name                  TEXT PRIMARY KEY,
                id                    TEXT NOT NULL,
                command               TEXT NOT NULL,
                args                  TEXT NOT NULL,
                cwd                   TEXT,
                env                   TEXT NOT NULL,
                trigger               TEXT NOT NULL,
                on_overlap            TEXT NOT NULL,
                on_dependency_failure TEXT NOT NULL,
                timeout_sec           INTEGER,
                UNIQUE(name, id)
            );
            CREATE TABLE IF NOT EXISTS job_runs (
                run_id       TEXT PRIMARY KEY,
                job_name     TEXT NOT NULL,
                job_id       TEXT NOT NULL,
                triggered_by TEXT NOT NULL,
                scheduled_at TEXT NOT NULL,
                started_at   TEXT,
                ended_at     TEXT,
                exit_code    INTEGER,
                state        TEXT NOT NULL,
                FOREIGN KEY(job_name, job_id) REFERENCES jobs(name, id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_job_runs_job
                ON job_runs(job_name, scheduled_at DESC);
        "#;
        for statement in ddl.split(';') {
            let trimmed = statement.trim();
            if trimmed.is_empty() {
                continue;
            }
            sqlx::query(trimmed)
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
        }
        self.ensure_process_spec_column(
            &mut transaction,
            "restart_enabled",
            "INTEGER NOT NULL DEFAULT 1",
        )
        .await?;
        self.ensure_process_spec_column(&mut transaction, "restart_max_retries", "INTEGER")
            .await?;
        self.ensure_process_spec_column(
            &mut transaction,
            "restart_backoff_initial_ms",
            "INTEGER NOT NULL DEFAULT 1000",
        )
        .await?;
        self.ensure_process_spec_column(
            &mut transaction,
            "restart_backoff_max_ms",
            "INTEGER NOT NULL DEFAULT 60000",
        )
        .await?;
        self.ensure_process_spec_column(
            &mut transaction,
            "restart_backoff_multiplier",
            "INTEGER NOT NULL DEFAULT 2",
        )
        .await?;
        self.ensure_process_spec_column(
            &mut transaction,
            "restart_jitter",
            "INTEGER NOT NULL DEFAULT 1",
        )
        .await?;
        self.ensure_process_spec_column(
            &mut transaction,
            "restart_reset_after_ms",
            "INTEGER NOT NULL DEFAULT 60000",
        )
        .await?;
        self.ensure_process_spec_column(&mut transaction, "runtime_process_id", "TEXT")
            .await?;
        self.ensure_process_spec_column(&mut transaction, "runtime_pid", "INTEGER")
            .await?;
        self.ensure_process_spec_column(&mut transaction, "runtime_pgid", "INTEGER")
            .await?;
        self.ensure_process_spec_column(&mut transaction, "runtime_generation", "TEXT")
            .await?;
        self.ensure_process_spec_column(&mut transaction, "runtime_started_at", "TEXT")
            .await?;
        self.ensure_process_spec_column(
            &mut transaction,
            "shutdown_signal",
            "TEXT NOT NULL DEFAULT 'term'",
        )
        .await?;
        self.ensure_process_spec_column(
            &mut transaction,
            "shutdown_grace_period_ms",
            "INTEGER NOT NULL DEFAULT 10000",
        )
        .await?;
        self.ensure_process_spec_column(
            &mut transaction,
            "definition_id",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        for (column, definition) in [
            ("instances", "INTEGER NOT NULL DEFAULT 1"),
            ("watch_policy", "TEXT"),
            ("memory_policy", "TEXT"),
            ("liveness_policy", "TEXT"),
            ("readiness_policy", "TEXT"),
            ("rolling_policy", "TEXT"),
        ] {
            self.ensure_process_spec_column(&mut transaction, column, definition)
                .await?;
        }
        self.backfill_process_definition_ids(&mut transaction)
            .await?;
        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS process_specs_definition_id_idx ON process_specs(definition_id)",
        )
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS process_instances (\
             instance_id TEXT PRIMARY KEY, definition_id TEXT NOT NULL, ordinal INTEGER NOT NULL CHECK (ordinal >= 0), \
             generation INTEGER NOT NULL CHECK (generation >= 1), active INTEGER NOT NULL DEFAULT 1, \
             FOREIGN KEY(definition_id) REFERENCES process_specs(definition_id) ON DELETE CASCADE)",
        )
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS process_instances_active_ordinal_idx \
             ON process_instances(definition_id, ordinal) WHERE active = 1",
        )
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        for (column, definition) in [
            ("runtime_process_id", "TEXT"),
            ("runtime_pid", "INTEGER"),
            ("runtime_pgid", "INTEGER"),
            ("runtime_generation", "TEXT"),
            ("runtime_started_at", "TEXT"),
            ("restart_count", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            self.ensure_table_column(&mut transaction, "process_instances", column, definition)
                .await?;
        }
        self.backfill_legacy_process_instances(&mut transaction)
            .await?;
        // A legacy definition owned one durable name-keyed handle.  Copy it
        // once to the ordinal-zero compatibility slot before facade recovery
        // starts reading instance-owned handles. Existing instance evidence
        // always wins, which makes reopen/partial migration idempotent.
        sqlx::query(
            "UPDATE process_instances AS instance SET \
             runtime_process_id = (SELECT runtime_process_id FROM process_specs WHERE definition_id = instance.definition_id), \
             runtime_pid = (SELECT runtime_pid FROM process_specs WHERE definition_id = instance.definition_id), \
             runtime_pgid = (SELECT runtime_pgid FROM process_specs WHERE definition_id = instance.definition_id), \
             runtime_generation = (SELECT runtime_generation FROM process_specs WHERE definition_id = instance.definition_id), \
             runtime_started_at = (SELECT runtime_started_at FROM process_specs WHERE definition_id = instance.definition_id), \
             restart_count = (SELECT restart_count FROM process_specs WHERE definition_id = instance.definition_id) \
             WHERE instance.ordinal = 0 AND instance.active = 1 AND instance.runtime_process_id IS NULL \
             AND EXISTS (SELECT 1 FROM process_specs WHERE definition_id = instance.definition_id AND runtime_process_id IS NOT NULL)",
        )
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_versions (name TEXT PRIMARY KEY, version INTEGER NOT NULL)",
        )
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        sqlx::query(
            "INSERT INTO schema_versions(name, version) VALUES('process_instances', 2) \
             ON CONFLICT(name) DO UPDATE SET version = excluded.version",
        )
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        self.ensure_table_column(
            &mut transaction,
            "jobs",
            "log_retention_max_runs",
            "INTEGER",
        )
        .await?;
        self.ensure_table_column(
            &mut transaction,
            "jobs",
            "log_retention_max_age_days",
            "INTEGER",
        )
        .await?;
        self.ensure_table_column(
            &mut transaction,
            "jobs",
            "timezone",
            "TEXT NOT NULL DEFAULT 'UTC'",
        )
        .await?;
        self.ensure_table_column(
            &mut transaction,
            "jobs",
            "schedule_revision",
            "INTEGER NOT NULL DEFAULT 0",
        )
        .await?;
        self.ensure_table_column(
            &mut transaction,
            "jobs",
            "trigger_id",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        self.ensure_table_column(
            &mut transaction,
            "jobs",
            "misfire_policy",
            "TEXT NOT NULL DEFAULT '\"skip\"'",
        )
        .await?;
        self.ensure_table_column(
            &mut transaction,
            "jobs",
            "retry_policy",
            "TEXT NOT NULL DEFAULT '{}' ",
        )
        .await?;
        self.ensure_table_column(
            &mut transaction,
            "jobs",
            "admission_policy",
            "TEXT NOT NULL DEFAULT '{}' ",
        )
        .await?;
        sqlx::query("UPDATE jobs SET trigger_id = id WHERE trigger_id = ''")
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        self.ensure_table_column(&mut transaction, "job_runs", "occurrence", "TEXT")
            .await?;
        self.ensure_table_column(
            &mut transaction,
            "job_runs",
            "original_scheduled_at",
            "TEXT",
        )
        .await?;
        sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_job_runs_occurrence ON job_runs(job_id, original_scheduled_at, occurrence) WHERE occurrence IS NOT NULL")
            .execute(&mut *transaction).await.map_err(backend)?;
        self.migrate_job_runs_to_job_identity(&mut transaction)
            .await?;
        for statement in [
            "CREATE TABLE IF NOT EXISTS schedule_cursors (job_id TEXT NOT NULL, trigger_id TEXT NOT NULL, schedule_revision INTEGER NOT NULL, cursor_at TEXT NOT NULL, PRIMARY KEY(job_id, trigger_id, schedule_revision))",
            "CREATE TABLE IF NOT EXISTS schedule_occurrences (job_id TEXT NOT NULL, job_name TEXT NOT NULL, trigger_id TEXT NOT NULL, schedule_revision INTEGER NOT NULL, scheduled_at TEXT NOT NULL, occurrence TEXT NOT NULL, state TEXT NOT NULL, next_attempt_at TEXT NOT NULL, run_id TEXT, final_state TEXT, PRIMARY KEY(job_id, trigger_id, schedule_revision, scheduled_at), FOREIGN KEY(job_name, job_id) REFERENCES jobs(name, id) ON DELETE CASCADE)",
            "CREATE INDEX IF NOT EXISTS idx_schedule_occurrences_due ON schedule_occurrences(state, next_attempt_at)",
        ] {
            sqlx::query(statement).execute(&mut *transaction).await.map_err(backend)?;
        }
        for statement in [
            "CREATE TABLE IF NOT EXISTS config_apply_journal (apply_id TEXT PRIMARY KEY, previous_snapshot TEXT NOT NULL, target_snapshot TEXT NOT NULL, diff TEXT NOT NULL, stage TEXT NOT NULL, compensation_error TEXT, target_direct_starts TEXT NOT NULL DEFAULT '[]')",
            "CREATE TABLE IF NOT EXISTS dependency_signatures (job_name TEXT PRIMARY KEY, signature TEXT NOT NULL)",
            "CREATE TABLE IF NOT EXISTS run_log_cleanup (run_id TEXT PRIMARY KEY, attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT)",
            "CREATE TABLE IF NOT EXISTS runtime_handle_cleanup (name TEXT PRIMARY KEY, process_id TEXT NOT NULL, generation TEXT, attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT)",
            "CREATE TABLE IF NOT EXISTS transient_cleanup (cleanup_id TEXT PRIMARY KEY, job_id TEXT NOT NULL, job_name TEXT NOT NULL, run_id TEXT NOT NULL UNIQUE, process_id TEXT NOT NULL, pid INTEGER NOT NULL, pgid INTEGER, generation TEXT, started_at TEXT NOT NULL, stage TEXT NOT NULL, attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT, intended_terminal_state TEXT NOT NULL, outcome_started_at TEXT, outcome_ended_at TEXT, outcome_exit_code INTEGER)",
            "CREATE TABLE IF NOT EXISTS transient_terminal_outbox (cleanup_id TEXT PRIMARY KEY, event_id TEXT NOT NULL UNIQUE, occurred_at TEXT NOT NULL, job_name TEXT NOT NULL, run_id TEXT NOT NULL, state TEXT NOT NULL, exit_code INTEGER)",
            "CREATE TABLE IF NOT EXISTS job_deletion_journal (job_name TEXT PRIMARY KEY, deletion_id TEXT NOT NULL UNIQUE, job_snapshot TEXT NOT NULL, stage TEXT NOT NULL, run_ids TEXT NOT NULL DEFAULT '[]', last_error TEXT)",
            "CREATE TABLE IF NOT EXISTS process_guard_snapshots (name TEXT PRIMARY KEY, snapshot TEXT NOT NULL, FOREIGN KEY(name) REFERENCES process_specs(name) ON DELETE CASCADE)",
            "CREATE TABLE IF NOT EXISTS process_instance_guard_snapshots (name TEXT NOT NULL, instance_id TEXT NOT NULL, snapshot TEXT NOT NULL, PRIMARY KEY(name, instance_id), FOREIGN KEY(name) REFERENCES process_specs(name) ON DELETE CASCADE, FOREIGN KEY(instance_id) REFERENCES process_instances(instance_id) ON DELETE CASCADE)",
            "CREATE TABLE IF NOT EXISTS process_operations (operation_id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL, operation_json TEXT NOT NULL, FOREIGN KEY(name) REFERENCES process_specs(name) ON DELETE CASCADE)",
            "CREATE TABLE IF NOT EXISTS observability_rules (id TEXT PRIMARY KEY, payload TEXT NOT NULL, deleted_at TEXT)",
            "CREATE TABLE IF NOT EXISTS observability_events (id TEXT PRIMARY KEY, occurred_at TEXT NOT NULL, transition_key TEXT NOT NULL UNIQUE, payload TEXT NOT NULL)",
            "CREATE INDEX IF NOT EXISTS observability_events_order_idx ON observability_events(occurred_at, id)",
            "CREATE TABLE IF NOT EXISTS observability_metrics (id TEXT PRIMARY KEY, occurred_at TEXT NOT NULL, source TEXT NOT NULL, payload TEXT NOT NULL)",
            "CREATE INDEX IF NOT EXISTS observability_metrics_order_idx ON observability_metrics(occurred_at, id)",
            "CREATE TABLE IF NOT EXISTS observability_metric_minutes (bucket_start TEXT NOT NULL, source TEXT NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(bucket_start, source))",
            "CREATE INDEX IF NOT EXISTS observability_metric_minutes_order_idx ON observability_metric_minutes(bucket_start, source)",
            "CREATE TABLE IF NOT EXISTS observability_alerts (id TEXT PRIMARY KEY, opened_at TEXT NOT NULL, payload TEXT NOT NULL)",
            "CREATE INDEX IF NOT EXISTS observability_alerts_order_idx ON observability_alerts(opened_at, id)",
            "CREATE TABLE IF NOT EXISTS observability_delivery_attempts (id TEXT PRIMARY KEY, alert_id TEXT NOT NULL, occurred_at TEXT NOT NULL, payload TEXT NOT NULL)",
            "CREATE INDEX IF NOT EXISTS observability_delivery_order_idx ON observability_delivery_attempts(occurred_at, id)",
            "CREATE TABLE IF NOT EXISTS observability_episode_keys (dedupe_key TEXT PRIMARY KEY, alert_id TEXT NOT NULL UNIQUE)",
            "CREATE TABLE IF NOT EXISTS observability_delivery_candidates (id TEXT PRIMARY KEY, alert_id TEXT NOT NULL, kind TEXT NOT NULL, payload TEXT NOT NULL, attempt_count INTEGER NOT NULL, created_at TEXT NOT NULL, lease_owner TEXT, lease_until TEXT)",
            "CREATE INDEX IF NOT EXISTS observability_delivery_candidates_claim_idx ON observability_delivery_candidates(lease_until, created_at)",
        ] {
            sqlx::query(statement).execute(&mut *transaction).await.map_err(backend)?;
        }
        self.ensure_table_column(
            &mut transaction,
            "config_apply_journal",
            "target_direct_starts",
            "TEXT NOT NULL DEFAULT '[]'",
        )
        .await?;
        self.ensure_table_column(
            &mut transaction,
            "transient_cleanup",
            "outcome_started_at",
            "TEXT",
        )
        .await?;
        self.ensure_table_column(
            &mut transaction,
            "transient_cleanup",
            "outcome_ended_at",
            "TEXT",
        )
        .await?;
        self.ensure_table_column(
            &mut transaction,
            "transient_cleanup",
            "outcome_exit_code",
            "INTEGER",
        )
        .await?;
        self.ensure_table_column(
            &mut transaction,
            "transient_terminal_outbox",
            "event_id",
            "TEXT",
        )
        .await?;
        self.ensure_table_column(
            &mut transaction,
            "transient_terminal_outbox",
            "occurred_at",
            "TEXT",
        )
        .await?;
        // Existing rows predate durable event identity.  Their cleanup ID was
        // already stable, so it is a safe idempotency key for every replay.
        sqlx::query("UPDATE transient_terminal_outbox SET event_id = cleanup_id WHERE event_id IS NULL OR event_id = ''")
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        let migration_time = dt_to_str(&Utc::now());
        sqlx::query(
            "UPDATE transient_terminal_outbox SET occurred_at = COALESCE((SELECT ended_at FROM job_runs WHERE job_runs.run_id = transient_terminal_outbox.run_id), ?) WHERE occurred_at IS NULL OR occurred_at = ''",
        )
        .bind(migration_time)
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS transient_terminal_outbox_event_id_idx ON transient_terminal_outbox(event_id)")
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        let foreign_key_violations = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *transaction)
            .await
            .map_err(backend)?;
        if !foreign_key_violations.is_empty() {
            return Err(RepoError::Backend(
                "schema migration left foreign-key violations".into(),
            ));
        }
        transaction.commit().await.map_err(backend)?;
        Ok(())
    }

    /// Test/host failure seam for proving that a transient child remains owned
    /// until its durable cleanup handoff succeeds.  It is inert unless a host
    /// explicitly arms it.
    pub fn fail_next_transient_cleanup_enqueues(&self, count: u32) {
        self.transient_cleanup_enqueue_failures
            .store(count, Ordering::SeqCst);
    }

    /// Test-only failure seam immediately before a terminal Run/outbox commit.
    /// It proves a startup cancellation cannot leave a terminal row without its
    /// matching external-delivery record.
    pub fn fail_next_terminal_run_commits(&self, count: u32) {
        self.terminal_run_commit_failures
            .store(count, Ordering::SeqCst);
    }

    /// Test-only failure seam immediately before the delivery acknowledgement
    /// transaction.  It leaves both durable records available for replay.
    pub fn fail_next_transient_terminal_acknowledgements(&self, count: u32) {
        self.transient_terminal_ack_failures
            .store(count, Ordering::SeqCst);
    }

    /// Test-only failure seam for recording the reversible rollback direction.
    /// Recovery must still restore dispatch even when this durable marker cannot
    /// be written, because the surviving pre-cancellation stage is rollback-only.
    pub fn fail_next_job_deletion_rollback_direction_updates(&self, count: u32) {
        self.job_deletion_rollback_direction_failures
            .store(count, Ordering::SeqCst);
    }

    /// Test-only failure seam for the reversible queued-cancellation commit.
    pub fn fail_next_job_deletion_cancellations(&self, count: u32) {
        self.job_deletion_cancellation_failures
            .store(count, Ordering::SeqCst);
    }

    /// Test-only failure seam immediately before the atomic row-deletion
    /// commit.  It proves no caller can observe deleted rows without the
    /// matching durable journal and log-cleanup records.
    pub fn fail_next_job_deletion_row_commits(&self, count: u32) {
        self.job_deletion_row_commit_failures
            .store(count, Ordering::SeqCst);
    }

    /// Test-only failure seam for the final rollback-journal removal.
    pub fn fail_next_job_deletion_journal_clears(&self, count: u32) {
        self.job_deletion_clear_failures
            .store(count, Ordering::SeqCst);
    }

    /// Test-only failure seam immediately before the target config snapshot
    /// transaction.  It leaves the durable `ForwardRecovery` journal intact
    /// so a real daemon restart must converge toward the target snapshot.
    pub fn fail_next_config_snapshot_commits(&self, count: u32) {
        self.config_snapshot_commit_failures
            .store(count, Ordering::SeqCst);
    }

    async fn save_spec_transaction(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        spec: &ProcessSpec,
    ) -> Result<(), RepoError> {
        let (mode, unit_name) = match &spec.management_mode {
            ManagementMode::Direct => ("direct", None),
            ManagementMode::SystemRegistered { unit_name } => {
                ("system_registered", Some(unit_name.clone()))
            }
        };
        let lifecycle = match spec.lifecycle {
            LifecycleMode::Tied => "tied",
            LifecycleMode::Detached => "detached",
        };
        let shutdown_signal = match spec.shutdown.signal {
            ShutdownSignal::Term => "term",
            ShutdownSignal::Int => "int",
            ShutdownSignal::Kill => "kill",
        };
        sqlx::query(
            r#"INSERT INTO process_specs
                (definition_id, name, command, args, cwd, env, mode, unit_name, lifecycle, autostart,
                 restart_enabled, restart_max_retries, restart_backoff_initial_ms,
                 restart_backoff_max_ms, restart_backoff_multiplier, restart_jitter,
                 restart_reset_after_ms, shutdown_signal, shutdown_grace_period_ms, instances,
                 watch_policy, memory_policy, liveness_policy, readiness_policy, rolling_policy, restart_count)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                       COALESCE((SELECT restart_count FROM process_specs WHERE name = ?), 0))
               ON CONFLICT(name) DO UPDATE SET command=excluded.command,args=excluded.args,cwd=excluded.cwd,
                 env=excluded.env,mode=excluded.mode,unit_name=excluded.unit_name,lifecycle=excluded.lifecycle,
                 autostart=excluded.autostart,restart_enabled=excluded.restart_enabled,
                 restart_max_retries=excluded.restart_max_retries,restart_backoff_initial_ms=excluded.restart_backoff_initial_ms,
                 restart_backoff_max_ms=excluded.restart_backoff_max_ms,restart_backoff_multiplier=excluded.restart_backoff_multiplier,
                 restart_jitter=excluded.restart_jitter,restart_reset_after_ms=excluded.restart_reset_after_ms,
                 shutdown_signal=excluded.shutdown_signal,shutdown_grace_period_ms=excluded.shutdown_grace_period_ms,
                 instances=excluded.instances,watch_policy=excluded.watch_policy,memory_policy=excluded.memory_policy,
                 liveness_policy=excluded.liveness_policy,readiness_policy=excluded.readiness_policy,
                 rolling_policy=excluded.rolling_policy"#,
        ).bind(spec.definition_id.0.to_string()).bind(&spec.name).bind(&spec.command).bind(json_string(&spec.args))
            .bind(spec.cwd.as_ref().map(|path| path.display().to_string())).bind(json_string(&spec.env))
            .bind(mode).bind(unit_name).bind(lifecycle).bind(spec.autostart as i64)
            .bind(spec.restart.enabled as i64).bind(spec.restart.max_retries.map(i64::from))
            .bind(spec.restart.backoff_initial.as_millis().min(i64::MAX as u128) as i64)
            .bind(spec.restart.backoff_max.as_millis().min(i64::MAX as u128) as i64)
            .bind(i64::from(spec.restart.backoff_multiplier)).bind(spec.restart.jitter as i64)
            .bind(spec.restart.reset_after.as_millis().min(i64::MAX as u128) as i64)
            .bind(shutdown_signal).bind(spec.shutdown.grace_period.as_millis().min(i64::MAX as u128) as i64)
            .bind(i64::from(spec.instances)).bind(spec.watch.as_ref().map(json_string))
            .bind(spec.memory.as_ref().map(json_string)).bind(spec.liveness.as_ref().map(json_string))
            .bind(spec.readiness.as_ref().map(json_string)).bind(spec.rolling.as_ref().map(json_string))
            .bind(&spec.name).execute(&mut **transaction).await.map_err(backend)?;
        let definition_id: String =
            sqlx::query("SELECT definition_id FROM process_specs WHERE name = ?")
                .bind(&spec.name)
                .fetch_one(&mut **transaction)
                .await
                .map_err(backend)?
                .try_get("definition_id")
                .map_err(backend)?;
        Self::sync_process_instances_transaction(transaction, &definition_id, spec.instances)
            .await?;
        Ok(())
    }

    async fn sync_process_instances_transaction(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        definition_id: &str,
        instances: u16,
    ) -> Result<(), RepoError> {
        // Do not retire live excess slots while a config transaction merely
        // changes desired count. The facade must first stop and identity-clear
        // the owned child, then retire the highest ordinal under the group lock.
        for ordinal in 0..instances {
            let active: Option<i64> = sqlx::query("SELECT active FROM process_instances WHERE definition_id = ? AND ordinal = ? AND active = 1")
                .bind(definition_id).bind(i64::from(ordinal)).fetch_optional(&mut **transaction).await.map_err(backend)?
                .map(|row| row.try_get("active").map_err(backend)).transpose()?;
            if active.is_none() {
                sqlx::query("INSERT INTO process_instances(instance_id, definition_id, ordinal, generation, active) VALUES (?, ?, ?, 1, 1)")
                    .bind(ProcessInstanceId::new().0.to_string()).bind(definition_id).bind(i64::from(ordinal))
                    .execute(&mut **transaction).await.map_err(backend)?;
            }
        }
        Ok(())
    }

    async fn backfill_process_definition_ids(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
    ) -> Result<(), RepoError> {
        let rows = sqlx::query("SELECT name, definition_id FROM process_specs WHERE definition_id IS NULL OR definition_id = ''")
            .fetch_all(&mut **transaction).await.map_err(backend)?;
        for row in rows {
            let name: String = row.try_get("name").map_err(backend)?;
            sqlx::query("UPDATE process_specs SET definition_id = ? WHERE name = ?")
                .bind(ProcessDefinitionId::from_legacy_name(&name).0.to_string())
                .bind(name)
                .execute(&mut **transaction)
                .await
                .map_err(backend)?;
        }
        Ok(())
    }

    async fn backfill_legacy_process_instances(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
    ) -> Result<(), RepoError> {
        let definitions = sqlx::query("SELECT definition_id FROM process_specs")
            .fetch_all(&mut **transaction)
            .await
            .map_err(backend)?;
        for row in definitions {
            let definition_id: String = row.try_get("definition_id").map_err(backend)?;
            // Only definitions without any historical slots are legacy rows.
            // A retired ordinal zero must remain retired across reopen; a later
            // explicit allocation receives a new, non-reused instance ID.
            let exists =
                sqlx::query("SELECT 1 FROM process_instances WHERE definition_id = ? LIMIT 1")
                    .bind(&definition_id)
                    .fetch_optional(&mut **transaction)
                    .await
                    .map_err(backend)?
                    .is_some();
            if !exists {
                sqlx::query("INSERT INTO process_instances(instance_id, definition_id, ordinal, generation, active) VALUES (?, ?, 0, 1, 1)")
                    .bind(ProcessInstanceId::new().0.to_string()).bind(definition_id)
                    .execute(&mut **transaction).await.map_err(backend)?;
            }
        }
        Ok(())
    }

    async fn save_job_transaction(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        job: &Job,
    ) -> Result<(), RepoError> {
        let on_overlap = match job.on_overlap {
            my_supervisor_core::domain::OverlapPolicy::Skip => "skip",
            my_supervisor_core::domain::OverlapPolicy::Queue => "queue",
            my_supervisor_core::domain::OverlapPolicy::Parallel => "parallel",
        };
        let on_dependency_failure = match job.on_dependency_failure {
            my_supervisor_core::domain::DependencyFailurePolicy::Skip => "skip",
            my_supervisor_core::domain::DependencyFailurePolicy::RunAnyway => "run_anyway",
        };
        sqlx::query(r#"INSERT INTO jobs (name,id,command,args,cwd,env,trigger,on_overlap,on_dependency_failure,timeout_sec,log_retention_max_runs,log_retention_max_age_days,timezone,schedule_revision,trigger_id,misfire_policy,retry_policy,admission_policy)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(name) DO UPDATE SET command=excluded.command,args=excluded.args,cwd=excluded.cwd,env=excluded.env,
            trigger=excluded.trigger,on_overlap=excluded.on_overlap,on_dependency_failure=excluded.on_dependency_failure,
            timeout_sec=excluded.timeout_sec,log_retention_max_runs=excluded.log_retention_max_runs,log_retention_max_age_days=excluded.log_retention_max_age_days,
            timezone=excluded.timezone,schedule_revision=excluded.schedule_revision,trigger_id=excluded.trigger_id,misfire_policy=excluded.misfire_policy,retry_policy=excluded.retry_policy,admission_policy=excluded.admission_policy"#)
            .bind(&job.name).bind(job.id.0.to_string()).bind(&job.command).bind(json_string(&job.args))
            .bind(job.cwd.as_ref().map(|path| path.display().to_string())).bind(json_string(&job.env))
            .bind(json_string(&TriggerRepr::from(&job.trigger))).bind(on_overlap).bind(on_dependency_failure)
            .bind(job.timeout.map(|duration| duration.as_secs() as i64))
            .bind(job.log_retention.max_runs.map(i64::from)).bind(job.log_retention.max_age_days.map(i64::from))
            .bind(&job.timezone).bind(job.schedule_revision as i64).bind(job.trigger_id.to_string())
            .bind(json_string(&job.misfire_policy)).bind(json_string(&job.retry_policy)).bind(json_string(&job.admission))
            .execute(&mut **transaction).await.map_err(backend)?;
        Ok(())
    }

    /// Rebuild the historic name-only run table atomically.  Invalid historic
    /// rows are not discarded: they remain inspectable in `orphan_job_runs`.
    async fn migrate_job_runs_to_job_identity(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
    ) -> Result<(), RepoError> {
        sqlx::query("CREATE TABLE IF NOT EXISTS schema_versions (name TEXT PRIMARY KEY, version INTEGER NOT NULL)")
            .execute(&mut **transaction)
            .await
            .map_err(backend)?;
        sqlx::query("CREATE TABLE IF NOT EXISTS orphan_job_runs (run_id TEXT PRIMARY KEY, payload TEXT NOT NULL, migration_reason TEXT NOT NULL)")
            .execute(&mut **transaction)
            .await
            .map_err(backend)?;
        let columns = sqlx::query("PRAGMA table_info(job_runs)")
            .fetch_all(&mut **transaction)
            .await
            .map_err(backend)?;
        let has_job_id = columns.iter().any(|row| {
            row.try_get::<String, _>("name")
                .map(|name| name == "job_id")
                .unwrap_or(false)
        });
        let has_parent_identity_index = self.has_jobs_identity_index(transaction).await?;
        let has_run_identity_foreign_key =
            self.has_job_runs_identity_foreign_key(transaction).await?;
        if has_job_id && has_parent_identity_index && has_run_identity_foreign_key {
            sqlx::query("INSERT INTO schema_versions(name, version) VALUES('job_runs_job_identity', 1) ON CONFLICT(name) DO UPDATE SET version = excluded.version")
                .execute(&mut **transaction)
                .await
                .map_err(backend)?;
            return Ok(());
        }

        sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_name_id ON jobs(name, id)")
            .execute(&mut **transaction)
            .await
            .map_err(backend)?;
        sqlx::query(
            "CREATE TABLE job_runs_rebuilt (\
                run_id TEXT PRIMARY KEY,\
                job_name TEXT NOT NULL,\
                job_id TEXT NOT NULL,\
                triggered_by TEXT NOT NULL,\
                scheduled_at TEXT NOT NULL,\
                started_at TEXT,\
                ended_at TEXT,\
                exit_code INTEGER,\
                state TEXT NOT NULL,\
                FOREIGN KEY(job_name, job_id) REFERENCES jobs(name, id) ON DELETE CASCADE\
            )",
        )
        .execute(&mut **transaction)
        .await
        .map_err(backend)?;
        let orphan_query = if has_job_id {
            "INSERT OR IGNORE INTO orphan_job_runs(run_id, payload, migration_reason) \
             SELECT r.run_id, json_object('job_name', r.job_name, 'job_id', r.job_id, 'triggered_by', r.triggered_by, 'scheduled_at', r.scheduled_at, 'started_at', r.started_at, 'ended_at', r.ended_at, 'exit_code', r.exit_code, 'state', r.state), 'missing_or_stale_parent_job' \
             FROM job_runs r LEFT JOIN jobs j ON j.name = r.job_name AND j.id = r.job_id WHERE j.name IS NULL"
        } else {
            "INSERT OR IGNORE INTO orphan_job_runs(run_id, payload, migration_reason) \
             SELECT r.run_id, json_object('job_name', r.job_name, 'triggered_by', r.triggered_by, 'scheduled_at', r.scheduled_at, 'started_at', r.started_at, 'ended_at', r.ended_at, 'exit_code', r.exit_code, 'state', r.state), 'missing_parent_job' \
             FROM job_runs r LEFT JOIN jobs j ON j.name = r.job_name WHERE j.name IS NULL"
        };
        sqlx::query(orphan_query)
            .execute(&mut **transaction)
            .await
            .map_err(backend)?;
        let copy_query = if has_job_id {
            "INSERT INTO job_runs_rebuilt(run_id, job_name, job_id, triggered_by, scheduled_at, started_at, ended_at, exit_code, state) \
             SELECT r.run_id, r.job_name, r.job_id, r.triggered_by, r.scheduled_at, r.started_at, r.ended_at, r.exit_code, r.state \
             FROM job_runs r JOIN jobs j ON j.name = r.job_name AND j.id = r.job_id"
        } else {
            "INSERT INTO job_runs_rebuilt(run_id, job_name, job_id, triggered_by, scheduled_at, started_at, ended_at, exit_code, state) \
             SELECT r.run_id, r.job_name, j.id, r.triggered_by, r.scheduled_at, r.started_at, r.ended_at, r.exit_code, r.state \
             FROM job_runs r JOIN jobs j ON j.name = r.job_name"
        };
        sqlx::query(copy_query)
            .execute(&mut **transaction)
            .await
            .map_err(backend)?;
        sqlx::query("DROP TABLE job_runs")
            .execute(&mut **transaction)
            .await
            .map_err(backend)?;
        sqlx::query("ALTER TABLE job_runs_rebuilt RENAME TO job_runs")
            .execute(&mut **transaction)
            .await
            .map_err(backend)?;
        sqlx::query("CREATE INDEX idx_job_runs_job ON job_runs(job_name, scheduled_at DESC)")
            .execute(&mut **transaction)
            .await
            .map_err(backend)?;
        sqlx::query("INSERT INTO schema_versions(name, version) VALUES('job_runs_job_identity', 1) ON CONFLICT(name) DO UPDATE SET version = excluded.version")
            .execute(&mut **transaction)
            .await
            .map_err(backend)?;
        let foreign_key_violations = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut **transaction)
            .await
            .map_err(backend)?;
        if foreign_key_violations.is_empty() {
            Ok(())
        } else {
            Err(RepoError::Backend(
                "job_runs migration left foreign-key violations".into(),
            ))
        }
    }

    async fn has_jobs_identity_index(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
    ) -> Result<bool, RepoError> {
        let indexes = sqlx::query("PRAGMA index_list(jobs)")
            .fetch_all(&mut **transaction)
            .await
            .map_err(backend)?;
        for index in indexes {
            let is_unique = index.try_get::<i64, _>("unique").map_err(backend)? != 0;
            if !is_unique {
                continue;
            }
            let index_name: String = index.try_get("name").map_err(backend)?;
            let columns = sqlx::query(&format!("PRAGMA index_info({index_name})"))
                .fetch_all(&mut **transaction)
                .await
                .map_err(backend)?;
            let column_names = columns
                .iter()
                .map(|column| column.try_get::<String, _>("name"))
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)?;
            if column_names == ["name", "id"] {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn has_job_runs_identity_foreign_key(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
    ) -> Result<bool, RepoError> {
        let foreign_keys = sqlx::query("PRAGMA foreign_key_list(job_runs)")
            .fetch_all(&mut **transaction)
            .await
            .map_err(backend)?;
        let mut expected_columns = std::collections::BTreeMap::new();
        for foreign_key in foreign_keys {
            let table: String = foreign_key.try_get("table").map_err(backend)?;
            if table != "jobs" {
                continue;
            }
            let sequence: i64 = foreign_key.try_get("seq").map_err(backend)?;
            let from: String = foreign_key.try_get("from").map_err(backend)?;
            let to: String = foreign_key.try_get("to").map_err(backend)?;
            expected_columns.insert(sequence, (from, to));
        }
        Ok(
            expected_columns.get(&0) == Some(&("job_name".into(), "name".into()))
                && expected_columns.get(&1) == Some(&("job_id".into(), "id".into())),
        )
    }

    async fn ensure_process_spec_column(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        column_name: &str,
        definition: &str,
    ) -> Result<(), RepoError> {
        self.ensure_table_column(transaction, "process_specs", column_name, definition)
            .await
    }

    async fn ensure_table_column(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        table_name: &str,
        column_name: &str,
        definition: &str,
    ) -> Result<(), RepoError> {
        let columns = sqlx::query(&format!("PRAGMA table_info({table_name})"))
            .fetch_all(&mut **transaction)
            .await
            .map_err(backend)?;
        let exists = columns.iter().any(|row| {
            row.try_get::<String, _>("name")
                .map(|name| name == column_name)
                .unwrap_or(false)
        });
        if !exists {
            let statement =
                format!("ALTER TABLE {table_name} ADD COLUMN {column_name} {definition}");
            sqlx::query(&statement)
                .execute(&mut **transaction)
                .await
                .map_err(backend)?;
        }
        Ok(())
    }
}

fn parse_json_vec(s: &str) -> Vec<String> {
    serde_json::from_str(s).unwrap_or_default()
}

fn parse_json_map(s: &str) -> BTreeMap<String, String> {
    serde_json::from_str(s).unwrap_or_default()
}

fn parse_json_option<T: DeserializeOwned>(value: Option<String>) -> Result<Option<T>, RepoError> {
    value
        .map(|value| serde_json::from_str(&value).map_err(backend))
        .transpose()
}

fn spec_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ProcessSpec, RepoError> {
    let definition_id: String = row.try_get("definition_id").map_err(backend)?;
    let mode: String = row.try_get("mode").map_err(backend)?;
    let unit_name: Option<String> = row.try_get("unit_name").map_err(backend)?;
    let management_mode = match mode.as_str() {
        "system_registered" => ManagementMode::SystemRegistered {
            unit_name: unit_name.unwrap_or_default(),
        },
        _ => ManagementMode::Direct,
    };
    let lifecycle: String = row.try_get("lifecycle").map_err(backend)?;
    let args: String = row.try_get("args").map_err(backend)?;
    let env: String = row.try_get("env").map_err(backend)?;
    let cwd: Option<String> = row.try_get("cwd").map_err(backend)?;
    let autostart: i64 = row.try_get("autostart").map_err(backend)?;
    let restart_enabled: i64 = row.try_get("restart_enabled").map_err(backend)?;
    let restart_max_retries: Option<i64> = row.try_get("restart_max_retries").map_err(backend)?;
    let restart_backoff_initial_ms: i64 =
        row.try_get("restart_backoff_initial_ms").map_err(backend)?;
    let restart_backoff_max_ms: i64 = row.try_get("restart_backoff_max_ms").map_err(backend)?;
    let restart_backoff_multiplier: i64 =
        row.try_get("restart_backoff_multiplier").map_err(backend)?;
    let restart_jitter: i64 = row.try_get("restart_jitter").map_err(backend)?;
    let restart_reset_after_ms: i64 = row.try_get("restart_reset_after_ms").map_err(backend)?;
    let shutdown_signal: String = row.try_get("shutdown_signal").map_err(backend)?;
    let shutdown_grace_period_ms: i64 = row.try_get("shutdown_grace_period_ms").map_err(backend)?;
    let instances: i64 = row.try_get("instances").map_err(backend)?;
    let watch_policy: Option<String> = row.try_get("watch_policy").map_err(backend)?;
    let memory_policy: Option<String> = row.try_get("memory_policy").map_err(backend)?;
    let liveness_policy: Option<String> = row.try_get("liveness_policy").map_err(backend)?;
    let readiness_policy: Option<String> = row.try_get("readiness_policy").map_err(backend)?;
    let rolling_policy: Option<String> = row.try_get("rolling_policy").map_err(backend)?;

    Ok(ProcessSpec {
        definition_id: ProcessDefinitionId(uuid::Uuid::parse_str(&definition_id).map_err(backend)?),
        name: row.try_get("name").map_err(backend)?,
        command: row.try_get("command").map_err(backend)?,
        args: parse_json_vec(&args),
        cwd: cwd.map(std::path::PathBuf::from),
        env: parse_json_map(&env),
        management_mode,
        lifecycle: if lifecycle == "detached" {
            LifecycleMode::Detached
        } else {
            LifecycleMode::Tied
        },
        autostart: autostart != 0,
        restart: RestartPolicy {
            enabled: restart_enabled != 0,
            max_retries: restart_max_retries.map(|value| value.max(0) as u32),
            backoff_initial: std::time::Duration::from_millis(
                restart_backoff_initial_ms.max(0) as u64
            ),
            backoff_max: std::time::Duration::from_millis(restart_backoff_max_ms.max(0) as u64),
            backoff_multiplier: restart_backoff_multiplier.max(1) as u32,
            jitter: restart_jitter != 0,
            reset_after: std::time::Duration::from_millis(restart_reset_after_ms.max(0) as u64),
        },
        shutdown: ShutdownPolicy {
            signal: match shutdown_signal.as_str() {
                "int" => ShutdownSignal::Int,
                "kill" => ShutdownSignal::Kill,
                _ => ShutdownSignal::Term,
            },
            grace_period: std::time::Duration::from_millis(shutdown_grace_period_ms.max(0) as u64),
        },
        instances: u16::try_from(instances).unwrap_or(1),
        watch: parse_json_option::<WatchPolicy>(watch_policy)?,
        memory: parse_json_option::<MemoryPolicy>(memory_policy)?,
        liveness: parse_json_option::<CheckPolicy>(liveness_policy)?,
        readiness: parse_json_option::<CheckPolicy>(readiness_policy)?,
        rolling: parse_json_option::<RollingPolicy>(rolling_policy)?,
    })
}

fn job_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Job, RepoError> {
    let id: String = row.try_get("id").map_err(backend)?;
    let args: String = row.try_get("args").map_err(backend)?;
    let env: String = row.try_get("env").map_err(backend)?;
    let cwd: Option<String> = row.try_get("cwd").map_err(backend)?;
    let trigger: String = row.try_get("trigger").map_err(backend)?;
    let on_overlap: String = row.try_get("on_overlap").map_err(backend)?;
    let on_dep: String = row.try_get("on_dependency_failure").map_err(backend)?;
    let timeout: Option<i64> = row.try_get("timeout_sec").map_err(backend)?;
    let log_retention_max_runs: Option<i64> =
        row.try_get("log_retention_max_runs").map_err(backend)?;
    let log_retention_max_age_days: Option<i64> =
        row.try_get("log_retention_max_age_days").map_err(backend)?;

    let trigger_repr: TriggerRepr = serde_json::from_str(&trigger).map_err(backend)?;

    Ok(Job {
        id: JobId(uuid::Uuid::parse_str(&id).unwrap_or_default()),
        name: row.try_get("name").map_err(backend)?,
        command: row.try_get("command").map_err(backend)?,
        args: parse_json_vec(&args),
        cwd: cwd.map(std::path::PathBuf::from),
        env: parse_json_map(&env),
        trigger: trigger_repr.into(),
        on_overlap: match on_overlap.as_str() {
            "queue" => my_supervisor_core::domain::OverlapPolicy::Queue,
            "parallel" => my_supervisor_core::domain::OverlapPolicy::Parallel,
            _ => my_supervisor_core::domain::OverlapPolicy::Skip,
        },
        on_dependency_failure: match on_dep.as_str() {
            "run_anyway" => my_supervisor_core::domain::DependencyFailurePolicy::RunAnyway,
            _ => my_supervisor_core::domain::DependencyFailurePolicy::Skip,
        },
        timeout: timeout.map(|s| std::time::Duration::from_secs(s as u64)),
        log_retention: LogRetention {
            max_runs: log_retention_max_runs.map(|value| value.max(0) as u32),
            max_age_days: log_retention_max_age_days.map(|value| value.max(0) as u32),
        },
        timezone: row
            .try_get::<String, _>("timezone")
            .unwrap_or_else(|_| "UTC".to_string()),
        schedule_revision: row
            .try_get::<i64, _>("schedule_revision")
            .unwrap_or(0)
            .max(0) as u64,
        trigger_id: row
            .try_get::<String, _>("trigger_id")
            .ok()
            .and_then(|value| uuid::Uuid::parse_str(&value).ok())
            .unwrap_or_default(),
        misfire_policy: row
            .try_get::<String, _>("misfire_policy")
            .ok()
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default(),
        retry_policy: row
            .try_get::<String, _>("retry_policy")
            .ok()
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default(),
        admission: row
            .try_get::<String, _>("admission_policy")
            .ok()
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_else(|| {
                my_supervisor_core::domain::AdmissionPolicy::legacy(match on_overlap.as_str() {
                    "queue" => my_supervisor_core::domain::OverlapPolicy::Queue,
                    "parallel" => my_supervisor_core::domain::OverlapPolicy::Parallel,
                    _ => my_supervisor_core::domain::OverlapPolicy::Skip,
                })
            }),
    })
}

fn run_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<JobRun, RepoError> {
    let run_id: String = row.try_get("run_id").map_err(backend)?;
    let triggered_by: String = row.try_get("triggered_by").map_err(backend)?;
    let scheduled_at: String = row.try_get("scheduled_at").map_err(backend)?;
    let started_at: Option<String> = row.try_get("started_at").map_err(backend)?;
    let ended_at: Option<String> = row.try_get("ended_at").map_err(backend)?;
    let exit_code: Option<i64> = row.try_get("exit_code").map_err(backend)?;
    let state: String = row.try_get("state").map_err(backend)?;

    let triggered_repr: TriggeredByRepr = serde_json::from_str(&triggered_by).map_err(backend)?;

    Ok(JobRun {
        run_id: JobRunId(uuid::Uuid::parse_str(&run_id).unwrap_or_default()),
        job_name: row.try_get("job_name").map_err(backend)?,
        job_id: JobId(
            uuid::Uuid::parse_str(&row.try_get::<String, _>("job_id").map_err(backend)?)
                .map_err(backend)?,
        ),
        triggered_by: triggered_repr.into_domain(),
        scheduled_at: str_to_dt(&scheduled_at)?,
        started_at: opt_str_to_dt(started_at)?,
        ended_at: opt_str_to_dt(ended_at)?,
        exit_code: exit_code.map(|c| c as i32),
        state: str_to_run_state(&state),
        occurrence: row
            .try_get::<Option<String>, _>("occurrence")
            .ok()
            .flatten()
            .and_then(|value| serde_json::from_str(&value).ok()),
        original_scheduled_at: row
            .try_get::<Option<String>, _>("original_scheduled_at")
            .ok()
            .flatten()
            .as_deref()
            .map(str_to_dt)
            .transpose()?,
    })
}

fn durable_occurrence_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<DurableScheduleOccurrence, RepoError> {
    let occurrence: ScheduleOccurrence =
        serde_json::from_str(&row.try_get::<String, _>("occurrence").map_err(backend)?)
            .map_err(backend)?;
    Ok(DurableScheduleOccurrence {
        job_id: JobId(
            uuid::Uuid::parse_str(&row.try_get::<String, _>("job_id").map_err(backend)?)
                .map_err(backend)?,
        ),
        job_name: row.try_get("job_name").map_err(backend)?,
        occurrence,
        state: str_to_occurrence_state(&row.try_get::<String, _>("state").map_err(backend)?),
        next_attempt_at: str_to_dt(
            &row.try_get::<String, _>("next_attempt_at")
                .map_err(backend)?,
        )?,
        run_id: row
            .try_get::<Option<String>, _>("run_id")
            .map_err(backend)?
            .map(|value| uuid::Uuid::parse_str(&value).map(JobRunId).map_err(backend))
            .transpose()?,
        final_state: row
            .try_get::<Option<String>, _>("final_state")
            .map_err(backend)?
            .map(|value| str_to_run_state(&value)),
    })
}

#[async_trait]
impl StateRepository for SqliteStore {
    async fn list_specs(&self) -> Result<Vec<ProcessSpec>, RepoError> {
        let rows = sqlx::query("SELECT * FROM process_specs ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        rows.iter().map(spec_from_row).collect()
    }

    async fn list_specs_page(
        &self,
        cursor: Option<&str>,
        high_watermark: Option<&str>,
        limit: usize,
    ) -> Result<my_supervisor_core::ports::repository::BoundedPage<ProcessSpec>, RepoError> {
        let high_watermark = match high_watermark {
            Some(value) => value.to_owned(),
            None => {
                sqlx::query_scalar::<_, String>("SELECT COALESCE(MAX(name), '') FROM process_specs")
                    .fetch_one(&self.pool)
                    .await
                    .map_err(backend)?
            }
        };
        let cursor = cursor.unwrap_or("");
        let rows = sqlx::query(
            "SELECT * FROM process_specs WHERE name > ? AND name <= ? ORDER BY name LIMIT ?",
        )
        .bind(cursor)
        .bind(&high_watermark)
        .bind((limit + 1) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        let mut records: Vec<_> = rows.iter().map(spec_from_row).collect::<Result<_, _>>()?;
        let has_more = records.len() > limit;
        records.truncate(limit);
        Ok(my_supervisor_core::ports::repository::BoundedPage {
            next_cursor: has_more
                .then(|| records.last().map(|spec| spec.name.clone()))
                .flatten(),
            records,
            high_watermark,
            failed_partitions: Vec::new(),
        })
    }

    async fn get_spec(&self, name: &str) -> Result<Option<ProcessSpec>, RepoError> {
        let row = sqlx::query("SELECT * FROM process_specs WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.as_ref().map(spec_from_row).transpose()
    }

    async fn save_spec(&self, spec: &ProcessSpec) -> Result<(), RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        Self::save_spec_transaction(&mut transaction, spec).await?;
        transaction.commit().await.map_err(backend)
    }

    async fn delete_spec(&self, name: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM process_specs WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn list_process_instances(
        &self,
        definition_id: ProcessDefinitionId,
    ) -> Result<Vec<ProcessInstance>, RepoError> {
        let rows = sqlx::query(
            "SELECT instance_id, definition_id, ordinal, generation FROM process_instances \
             WHERE definition_id = ? AND active = 1 ORDER BY ordinal",
        )
        .bind(definition_id.0.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.iter()
            .map(|row| {
                Ok(ProcessInstance {
                    id: ProcessInstanceId(
                        uuid::Uuid::parse_str(
                            &row.try_get::<String, _>("instance_id").map_err(backend)?,
                        )
                        .map_err(backend)?,
                    ),
                    definition_id: ProcessDefinitionId(
                        uuid::Uuid::parse_str(
                            &row.try_get::<String, _>("definition_id").map_err(backend)?,
                        )
                        .map_err(backend)?,
                    ),
                    ordinal: u16::try_from(row.try_get::<i64, _>("ordinal").map_err(backend)?)
                        .map_err(backend)?,
                    generation: u64::try_from(
                        row.try_get::<i64, _>("generation").map_err(backend)?,
                    )
                    .map_err(backend)?,
                })
            })
            .collect()
    }

    async fn allocate_process_instance(
        &self,
        definition_id: ProcessDefinitionId,
        ordinal: u16,
    ) -> Result<ProcessInstance, RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let exists = sqlx::query("SELECT 1 FROM process_specs WHERE definition_id = ?")
            .bind(definition_id.0.to_string())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(backend)?
            .is_some();
        if !exists {
            return Err(RepoError::NotFound(format!(
                "process definition {}",
                definition_id.0
            )));
        }
        let active = sqlx::query("SELECT 1 FROM process_instances WHERE definition_id = ? AND ordinal = ? AND active = 1")
            .bind(definition_id.0.to_string())
            .bind(i64::from(ordinal))
            .fetch_optional(&mut *transaction)
            .await
            .map_err(backend)?
            .is_some();
        if active {
            return Err(RepoError::Conflict(format!(
                "active process ordinal {ordinal}"
            )));
        }
        let instance = ProcessInstance {
            id: ProcessInstanceId::new(),
            definition_id,
            ordinal,
            generation: 1,
        };
        sqlx::query("INSERT INTO process_instances(instance_id, definition_id, ordinal, generation, active) VALUES (?, ?, ?, ?, 1)")
            .bind(instance.id.0.to_string()).bind(instance.definition_id.0.to_string())
            .bind(i64::from(instance.ordinal)).bind(i64::try_from(instance.generation).map_err(backend)?)
            .execute(&mut *transaction).await.map_err(backend)?;
        transaction.commit().await.map_err(backend)?;
        Ok(instance)
    }

    async fn retire_process_instance(
        &self,
        instance_id: ProcessInstanceId,
    ) -> Result<(), RepoError> {
        sqlx::query("UPDATE process_instances SET active = 0 WHERE instance_id = ? AND active = 1")
            .bind(instance_id.0.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn promote_process_instance(
        &self,
        retired: &ProcessInstance,
        replacement: &ProcessInstance,
    ) -> Result<Option<ProcessInstance>, RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let retired_result = sqlx::query(
            "UPDATE process_instances SET active = 0 WHERE instance_id = ? AND definition_id = ? AND ordinal = ? AND generation = ? AND active = 1",
        )
        .bind(retired.id.0.to_string())
        .bind(retired.definition_id.0.to_string())
        .bind(i64::from(retired.ordinal))
        .bind(i64::try_from(retired.generation).map_err(backend)?)
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        if retired_result.rows_affected() != 1 {
            return Ok(None);
        }
        let replacement_result = sqlx::query(
            "UPDATE process_instances SET ordinal = ? WHERE instance_id = ? AND definition_id = ? AND ordinal = ? AND generation = ? AND active = 1",
        )
        .bind(i64::from(retired.ordinal))
        .bind(replacement.id.0.to_string())
        .bind(replacement.definition_id.0.to_string())
        .bind(i64::from(replacement.ordinal))
        .bind(i64::try_from(replacement.generation).map_err(backend)?)
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        if replacement_result.rows_affected() != 1 {
            transaction.rollback().await.map_err(backend)?;
            return Ok(None);
        }
        transaction.commit().await.map_err(backend)?;
        Ok(Some(ProcessInstance {
            ordinal: retired.ordinal,
            ..replacement.clone()
        }))
    }

    async fn get_process_instance_runtime_handle(
        &self,
        instance: &ProcessInstance,
    ) -> Result<Option<my_supervisor_core::domain::ChildHandle>, RepoError> {
        let row = sqlx::query(
            "SELECT runtime_process_id, runtime_pid, runtime_pgid, runtime_generation, runtime_started_at \
             FROM process_instances WHERE instance_id = ? AND generation = ? AND active = 1",
        )
        .bind(instance.id.0.to_string())
        .bind(i64::try_from(instance.generation).map_err(backend)?)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let mut process_id: Option<String> = row.try_get("runtime_process_id").map_err(backend)?;
        let mut pid: Option<i64> = row.try_get("runtime_pid").map_err(backend)?;
        let mut pgid: Option<i64> = row.try_get("runtime_pgid").map_err(backend)?;
        let mut generation: Option<String> = row.try_get("runtime_generation").map_err(backend)?;
        let mut started_at: Option<String> = row.try_get("runtime_started_at").map_err(backend)?;
        // The name-keyed ordinal-zero row remains readable for callers that
        // wrote it before (or concurrently with) the additive migration.
        if process_id.is_none() && instance.ordinal == 0 {
            if let Some(legacy) = sqlx::query(
                "SELECT runtime_process_id, runtime_pid, runtime_pgid, runtime_generation, runtime_started_at \
                 FROM process_specs WHERE definition_id = ?",
            )
            .bind(instance.definition_id.0.to_string())
            .fetch_optional(&self.pool).await.map_err(backend)? {
                process_id = legacy.try_get("runtime_process_id").map_err(backend)?;
                pid = legacy.try_get("runtime_pid").map_err(backend)?;
                pgid = legacy.try_get("runtime_pgid").map_err(backend)?;
                generation = legacy.try_get("runtime_generation").map_err(backend)?;
                started_at = legacy.try_get("runtime_started_at").map_err(backend)?;
            }
        }
        match (process_id, pid, started_at) {
            (Some(process_id), Some(pid), Some(started_at)) => {
                Ok(Some(my_supervisor_core::domain::ChildHandle {
                    process_id: uuid::Uuid::parse_str(&process_id).map_err(backend)?,
                    pid: u32::try_from(pid).map_err(backend)?,
                    pgid: pgid.map(u32::try_from).transpose().map_err(backend)?,
                    generation,
                    started_at: str_to_dt(&started_at)?,
                }))
            }
            _ => Ok(None),
        }
    }

    async fn set_process_instance_runtime_handle(
        &self,
        instance: &ProcessInstance,
        handle: Option<&my_supervisor_core::domain::ChildHandle>,
    ) -> Result<bool, RepoError> {
        let (process_id, pid, pgid, native_generation, started_at) = match handle {
            Some(handle) => (
                Some(handle.process_id.to_string()),
                Some(i64::from(handle.pid)),
                handle.pgid.map(i64::from),
                handle.generation.clone(),
                Some(dt_to_str(&handle.started_at)),
            ),
            None => (None, None, None, None, None),
        };
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let result = sqlx::query(
            "UPDATE process_instances SET runtime_process_id = ?, runtime_pid = ?, runtime_pgid = ?, \
             runtime_generation = ?, runtime_started_at = ? \
             WHERE instance_id = ? AND generation = ? AND active = 1",
        )
        .bind(process_id).bind(pid).bind(pgid).bind(native_generation).bind(started_at)
        .bind(instance.id.0.to_string())
        .bind(i64::try_from(instance.generation).map_err(backend)?)
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        Ok(result.rows_affected() == 1)
    }

    async fn clear_process_instance_runtime_handle_if_matches(
        &self,
        instance: &ProcessInstance,
        handle: &my_supervisor_core::domain::ChildHandle,
    ) -> Result<bool, RepoError> {
        let result = sqlx::query(
            "UPDATE process_instances SET runtime_process_id = NULL, runtime_pid = NULL, runtime_pgid = NULL, \
             runtime_generation = NULL, runtime_started_at = NULL \
             WHERE instance_id = ? AND generation = ? AND active = 1 AND runtime_process_id = ? \
             AND runtime_generation IS ?",
        )
        .bind(instance.id.0.to_string())
        .bind(i64::try_from(instance.generation).map_err(backend)?)
        .bind(handle.process_id.to_string())
        .bind(&handle.generation)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(result.rows_affected() == 1)
    }

    async fn advance_process_instance_generation(
        &self,
        instance: &ProcessInstance,
    ) -> Result<Option<ProcessInstance>, RepoError> {
        let next_generation = instance
            .generation
            .checked_add(1)
            .ok_or_else(|| RepoError::Backend("process instance generation overflow".into()))?;
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let result = sqlx::query(
            "UPDATE process_instances SET generation = ?, runtime_process_id = NULL, runtime_pid = NULL, \
             runtime_pgid = NULL, runtime_generation = NULL, runtime_started_at = NULL, restart_count = 0 \
             WHERE instance_id = ? AND generation = ? AND active = 1",
        )
        .bind(i64::try_from(next_generation).map_err(backend)?)
        .bind(instance.id.0.to_string())
        .bind(i64::try_from(instance.generation).map_err(backend)?)
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        if result.rows_affected() != 1 {
            return Ok(None);
        }
        observability::insert_operator_event(
            &mut transaction,
            Utc::now(),
            format!("process-instance:{}", instance.id.0),
            "process_generation_advanced".into(),
            AlertSeverity::Info,
            format!(
                "process instance generation advanced from {} to {next_generation}",
                instance.generation
            ),
            format!("process-generation:{}:{next_generation}", instance.id.0),
        )
        .await?;
        transaction.commit().await.map_err(backend)?;
        Ok(Some(ProcessInstance {
            id: instance.id,
            definition_id: instance.definition_id,
            ordinal: instance.ordinal,
            generation: next_generation,
        }))
    }

    async fn get_process_instance_restart_count(
        &self,
        instance: &ProcessInstance,
    ) -> Result<u32, RepoError> {
        let row = sqlx::query("SELECT restart_count FROM process_instances WHERE instance_id = ? AND generation = ? AND active = 1")
            .bind(instance.id.0.to_string())
            .bind(i64::try_from(instance.generation).map_err(backend)?)
            .fetch_optional(&self.pool).await.map_err(backend)?;
        row.map(|row| {
            row.try_get::<i64, _>("restart_count")
                .map(|count| count.max(0) as u32)
                .map_err(backend)
        })
        .transpose()
        .map(|count| count.unwrap_or(0))
    }

    async fn set_process_instance_restart_count(
        &self,
        instance: &ProcessInstance,
        count: u32,
    ) -> Result<bool, RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let result = sqlx::query("UPDATE process_instances SET restart_count = ? WHERE instance_id = ? AND generation = ? AND active = 1")
            .bind(i64::from(count))
            .bind(instance.id.0.to_string())
            .bind(i64::try_from(instance.generation).map_err(backend)?)
            .execute(&mut *transaction).await.map_err(backend)?;
        if result.rows_affected() != 1 {
            return Ok(false);
        }
        observability::insert_operator_event(
            &mut transaction,
            Utc::now(),
            format!("process-instance:{}", instance.id.0),
            "process_restart_scheduled".into(),
            AlertSeverity::Warning,
            format!("process instance restart attempt {count} was scheduled"),
            format!(
                "process-restart:{}:{}:{count}",
                instance.id.0, instance.generation
            ),
        )
        .await?;
        transaction.commit().await.map_err(backend)?;
        Ok(true)
    }

    async fn get_restart_count(&self, name: &str) -> Result<u32, RepoError> {
        let row = sqlx::query("SELECT restart_count FROM process_specs WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        match row {
            Some(row) => {
                let count: i64 = row.try_get("restart_count").map_err(backend)?;
                Ok(count.max(0) as u32)
            }
            None => Ok(0),
        }
    }

    async fn set_restart_count(&self, name: &str, count: u32) -> Result<(), RepoError> {
        sqlx::query("UPDATE process_specs SET restart_count = ? WHERE name = ?")
            .bind(count as i64)
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn get_runtime_handle(
        &self,
        name: &str,
    ) -> Result<Option<my_supervisor_core::domain::ChildHandle>, RepoError> {
        let row = sqlx::query(
            "SELECT runtime_process_id, runtime_pid, runtime_pgid, runtime_generation, runtime_started_at FROM process_specs WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let process_id: Option<String> = row.try_get("runtime_process_id").map_err(backend)?;
        let pid: Option<i64> = row.try_get("runtime_pid").map_err(backend)?;
        let pgid: Option<i64> = row.try_get("runtime_pgid").map_err(backend)?;
        let generation: Option<String> = row.try_get("runtime_generation").map_err(backend)?;
        let started_at: Option<String> = row.try_get("runtime_started_at").map_err(backend)?;
        match (process_id, pid, started_at) {
            (Some(process_id), Some(pid), Some(started_at)) => {
                Ok(Some(my_supervisor_core::domain::ChildHandle {
                    process_id: uuid::Uuid::parse_str(&process_id).map_err(backend)?,
                    pid: u32::try_from(pid).map_err(backend)?,
                    pgid: pgid.map(u32::try_from).transpose().map_err(backend)?,
                    generation,
                    started_at: str_to_dt(&started_at)?,
                }))
            }
            _ => Ok(None),
        }
    }

    async fn set_runtime_handle(
        &self,
        name: &str,
        handle: Option<&my_supervisor_core::domain::ChildHandle>,
    ) -> Result<(), RepoError> {
        let (process_id, pid, pgid, generation, started_at) = match handle {
            Some(handle) => (
                Some(handle.process_id.to_string()),
                Some(i64::from(handle.pid)),
                handle.pgid.map(i64::from),
                handle.generation.clone(),
                Some(dt_to_str(&handle.started_at)),
            ),
            None => (None, None, None, None, None),
        };
        sqlx::query(
            "UPDATE process_specs SET runtime_process_id = ?, runtime_pid = ?, runtime_pgid = ?, runtime_generation = ?, runtime_started_at = ? WHERE name = ?",
        )
        .bind(process_id)
        .bind(pid)
        .bind(pgid)
        .bind(generation)
        .bind(started_at)
        .bind(name)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn enqueue_runtime_handle_cleanup(
        &self,
        name: &str,
        handle: &my_supervisor_core::domain::ChildHandle,
        error: &str,
    ) -> Result<(), RepoError> {
        sqlx::query(
            "INSERT INTO runtime_handle_cleanup(name, process_id, generation, attempts, last_error) \
             VALUES (?, ?, ?, 1, ?) \
             ON CONFLICT(name) DO UPDATE SET process_id = excluded.process_id, generation = excluded.generation, \
             attempts = runtime_handle_cleanup.attempts + 1, last_error = excluded.last_error",
        )
        .bind(name)
        .bind(handle.process_id.to_string())
        .bind(&handle.generation)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn pending_runtime_handle_cleanup(
        &self,
        limit: usize,
    ) -> Result<Vec<RuntimeHandleCleanup>, RepoError> {
        let rows = sqlx::query(
            "SELECT name, process_id, generation, attempts, last_error \
             FROM runtime_handle_cleanup ORDER BY name LIMIT ?",
        )
        .bind(limit.min(i64::MAX as usize) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.iter()
            .map(|row| {
                Ok(RuntimeHandleCleanup {
                    name: row.try_get("name").map_err(backend)?,
                    process_id: uuid::Uuid::parse_str(
                        &row.try_get::<String, _>("process_id").map_err(backend)?,
                    )
                    .map_err(backend)?,
                    generation: row.try_get("generation").map_err(backend)?,
                    attempts: row.try_get::<i64, _>("attempts").map_err(backend)?.max(0) as u32,
                    last_error: row.try_get("last_error").map_err(backend)?,
                })
            })
            .collect()
    }

    async fn clear_runtime_handle_if_matches(
        &self,
        cleanup: &RuntimeHandleCleanup,
    ) -> Result<bool, RepoError> {
        let result = sqlx::query(
            "UPDATE process_specs SET runtime_process_id = NULL, runtime_pid = NULL, runtime_pgid = NULL, \
             runtime_generation = NULL, runtime_started_at = NULL \
             WHERE name = ? AND runtime_process_id = ? AND runtime_generation IS ?",
        )
        .bind(&cleanup.name)
        .bind(cleanup.process_id.to_string())
        .bind(&cleanup.generation)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(result.rows_affected() == 1)
    }

    async fn complete_runtime_handle_cleanup(&self, name: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM runtime_handle_cleanup WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn latest_guard_snapshot(&self, name: &str) -> Result<Option<GuardSnapshot>, RepoError> {
        let row = sqlx::query("SELECT snapshot FROM process_guard_snapshots WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.map(|row| {
            let snapshot: String = row.try_get("snapshot").map_err(backend)?;
            serde_json::from_str(&snapshot).map_err(backend)
        })
        .transpose()
    }

    async fn upsert_guard_snapshot(
        &self,
        name: &str,
        snapshot: &GuardSnapshot,
    ) -> Result<(), RepoError> {
        sqlx::query(
            "INSERT INTO process_guard_snapshots(name, snapshot) VALUES (?, ?) \
             ON CONFLICT(name) DO UPDATE SET snapshot = excluded.snapshot",
        )
        .bind(name)
        .bind(json_string(snapshot))
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn latest_process_instance_guard_snapshot(
        &self,
        name: &str,
        instance: &ProcessInstance,
    ) -> Result<Option<GuardSnapshot>, RepoError> {
        let row = sqlx::query(
            "SELECT snapshot FROM process_instance_guard_snapshots WHERE name = ? AND instance_id = ?",
        )
        .bind(name)
        .bind(instance.id.0.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(|row| {
            let snapshot: String = row.try_get("snapshot").map_err(backend)?;
            serde_json::from_str(&snapshot).map_err(backend)
        })
        .transpose()
    }

    async fn upsert_process_instance_guard_snapshot(
        &self,
        name: &str,
        instance: &ProcessInstance,
        snapshot: &GuardSnapshot,
    ) -> Result<(), RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let previous = sqlx::query("SELECT snapshot FROM process_instance_guard_snapshots WHERE name = ? AND instance_id = ?")
            .bind(name).bind(instance.id.0.to_string()).fetch_optional(&mut *transaction).await.map_err(backend)?
            .map(|row| serde_json::from_str::<GuardSnapshot>(&row.try_get::<String, _>("snapshot").map_err(backend)?).map_err(backend))
            .transpose()?;
        sqlx::query(
            "INSERT INTO process_instance_guard_snapshots(name, instance_id, snapshot) VALUES (?, ?, ?) \
             ON CONFLICT(name, instance_id) DO UPDATE SET snapshot = excluded.snapshot",
        )
        .bind(name)
        .bind(instance.id.0.to_string())
        .bind(json_string(snapshot))
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        if let Some(cause) = snapshot.last_restart_cause.filter(|cause| {
            previous
                .as_ref()
                .and_then(|previous| previous.last_restart_cause)
                != Some(*cause)
        }) {
            observability::insert_operator_event(
                &mut transaction,
                snapshot.observed_at,
                format!("process:{name}"),
                "guard_restart_intent".into(),
                AlertSeverity::Warning,
                format!("guard requested restart because {cause:?}"),
                format!(
                    "guard-restart:{}:{}:{}",
                    instance.id.0,
                    instance.generation,
                    snapshot.observed_at.to_rfc3339()
                ),
            )
            .await?;
        }
        transaction.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn get_process_operation(
        &self,
        operation_id: uuid::Uuid,
    ) -> Result<Option<ProcessOperation>, RepoError> {
        let row =
            sqlx::query("SELECT operation_json FROM process_operations WHERE operation_id = ?")
                .bind(operation_id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(backend)?;
        row.map(|row| {
            serde_json::from_str(
                &row.try_get::<String, _>("operation_json")
                    .map_err(backend)?,
            )
            .map_err(backend)
        })
        .transpose()
    }

    async fn save_process_operation(&self, operation: &ProcessOperation) -> Result<(), RepoError> {
        let kind = match operation.kind {
            my_supervisor_core::domain::ProcessOperationKind::Scale => "scale",
            my_supervisor_core::domain::ProcessOperationKind::RollingRestart => "rolling_restart",
        };
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let result = sqlx::query(
            "INSERT INTO process_operations(operation_id, name, kind, operation_json) VALUES (?, ?, ?, ?) \
             ON CONFLICT(operation_id) DO UPDATE SET operation_json = excluded.operation_json \
             WHERE process_operations.name = excluded.name AND process_operations.kind = excluded.kind",
        )
        .bind(operation.operation_id.to_string())
        .bind(&operation.name)
        .bind(kind)
        .bind(json_string(operation))
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::Conflict(
                "operation ID belongs to a different process operation".into(),
            ));
        }
        observability::insert_operator_event(
            &mut transaction,
            Utc::now(),
            format!("process:{}", operation.name),
            format!("process_operation_{kind}"),
            AlertSeverity::Info,
            format!(
                "process operation {} entered {}",
                operation.operation_id, operation.phase
            ),
            format!(
                "process-operation:{}:{}:{}",
                operation.operation_id, operation.phase, operation.completed
            ),
        )
        .await?;
        transaction.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn list_incomplete_process_operations(&self) -> Result<Vec<ProcessOperation>, RepoError> {
        let rows = sqlx::query("SELECT operation_json FROM process_operations")
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        rows.into_iter()
            .map(|row| {
                serde_json::from_str::<ProcessOperation>(
                    &row.try_get::<String, _>("operation_json")
                        .map_err(backend)?,
                )
                .map_err(backend)
            })
            .filter(|operation| match operation {
                Ok(operation) => !operation.completed,
                Err(_) => true,
            })
            .collect()
    }

    async fn observability_upsert_rule(
        &self,
        rule: &my_supervisor_core::domain::AlertRule,
    ) -> Result<(), RepoError> {
        my_supervisor_core::ports::ObservabilityRepository::upsert_alert_rule(self, rule).await
    }
    async fn observability_delete_rule(
        &self,
        id: uuid::Uuid,
        deleted_at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        my_supervisor_core::ports::ObservabilityRepository::delete_alert_rule(self, id, deleted_at)
            .await
    }
    async fn observability_list_rules(
        &self,
        limit: usize,
    ) -> Result<Vec<my_supervisor_core::domain::AlertRule>, RepoError> {
        my_supervisor_core::ports::ObservabilityRepository::list_alert_rules(self, limit).await
    }
    async fn observability_list_events(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<
        my_supervisor_core::domain::ObservabilityPage<my_supervisor_core::domain::OperatorEvent>,
        RepoError,
    > {
        my_supervisor_core::ports::ObservabilityRepository::list_events(self, cursor, limit).await
    }
    async fn observability_record_metric(
        &self,
        sample: &my_supervisor_core::domain::MetricSample,
    ) -> Result<(), RepoError> {
        my_supervisor_core::ports::ObservabilityRepository::record_metric(self, sample).await
    }
    async fn observability_maintain(&self, now: DateTime<Utc>) -> Result<(), RepoError> {
        my_supervisor_core::ports::ObservabilityRepository::maintain(self, now).await
    }
    async fn observability_upsert_alert_episode(
        &self,
        episode: &my_supervisor_core::domain::AlertEpisode,
        dedupe_key: &str,
    ) -> Result<bool, RepoError> {
        my_supervisor_core::ports::ObservabilityRepository::upsert_alert_episode(
            self, episode, dedupe_key,
        )
        .await
    }
    async fn observability_resolve_alert_episode(
        &self,
        episode: &my_supervisor_core::domain::AlertEpisode,
    ) -> Result<bool, RepoError> {
        my_supervisor_core::ports::ObservabilityRepository::resolve_alert_episode(self, episode)
            .await
    }
    async fn observability_enqueue_delivery_candidate(
        &self,
        candidate: &my_supervisor_core::domain::DeliveryCandidate,
    ) -> Result<(), RepoError> {
        my_supervisor_core::ports::ObservabilityRepository::enqueue_delivery_candidate(
            self, candidate,
        )
        .await
    }
    async fn observability_claim_delivery_candidates(
        &self,
        owner: &str,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<my_supervisor_core::domain::DeliveryCandidate>, RepoError> {
        my_supervisor_core::ports::ObservabilityRepository::claim_delivery_candidates(
            self,
            owner,
            now,
            lease_until,
            limit,
        )
        .await
    }
    async fn observability_finish_delivery_candidate(
        &self,
        candidate: &my_supervisor_core::domain::DeliveryCandidate,
        submission: &my_supervisor_core::domain::DeliverySubmission,
        occurred_at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        my_supervisor_core::ports::ObservabilityRepository::finish_delivery_candidate(
            self,
            candidate,
            submission,
            occurred_at,
        )
        .await
    }
    async fn observability_cancel_delivery_candidates_for_alert(
        &self,
        alert_id: uuid::Uuid,
        occurred_at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        my_supervisor_core::ports::ObservabilityRepository::cancel_delivery_candidates_for_alert(
            self,
            alert_id,
            occurred_at,
        )
        .await
    }
    async fn observability_list_metrics(
        &self,
        source: Option<&str>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<
        my_supervisor_core::domain::ObservabilityPage<my_supervisor_core::domain::MetricSample>,
        RepoError,
    > {
        my_supervisor_core::ports::ObservabilityRepository::list_metrics(
            self, source, cursor, limit,
        )
        .await
    }
    async fn observability_list_alerts(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<
        my_supervisor_core::domain::ObservabilityPage<my_supervisor_core::domain::AlertEpisode>,
        RepoError,
    > {
        my_supervisor_core::ports::ObservabilityRepository::list_alerts(self, cursor, limit).await
    }
    async fn observability_acknowledge_alert(
        &self,
        id: uuid::Uuid,
        at: DateTime<Utc>,
    ) -> Result<bool, RepoError> {
        my_supervisor_core::ports::ObservabilityRepository::acknowledge_alert(self, id, at).await
    }
    async fn observability_list_delivery_attempts(
        &self,
        alert_id: Option<uuid::Uuid>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<
        my_supervisor_core::domain::ObservabilityPage<my_supervisor_core::domain::DeliveryAttempt>,
        RepoError,
    > {
        my_supervisor_core::ports::ObservabilityRepository::list_delivery_attempts(
            self, alert_id, cursor, limit,
        )
        .await
    }
}

#[async_trait]
impl JobRepository for SqliteStore {
    async fn list_jobs(&self) -> Result<Vec<Job>, RepoError> {
        let rows = sqlx::query("SELECT * FROM jobs ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        rows.iter().map(job_from_row).collect()
    }

    async fn list_jobs_page(
        &self,
        cursor: Option<&str>,
        high_watermark: Option<&str>,
        limit: usize,
    ) -> Result<my_supervisor_core::ports::repository::BoundedPage<Job>, RepoError> {
        let high_watermark = match high_watermark {
            Some(value) => value.to_owned(),
            None => sqlx::query_scalar::<_, String>("SELECT COALESCE(MAX(name), '') FROM jobs")
                .fetch_one(&self.pool)
                .await
                .map_err(backend)?,
        };
        let cursor = cursor.unwrap_or("");
        let rows =
            sqlx::query("SELECT * FROM jobs WHERE name > ? AND name <= ? ORDER BY name LIMIT ?")
                .bind(cursor)
                .bind(&high_watermark)
                .bind((limit + 1) as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(backend)?;
        let mut records: Vec<_> = rows.iter().map(job_from_row).collect::<Result<_, _>>()?;
        let has_more = records.len() > limit;
        records.truncate(limit);
        Ok(my_supervisor_core::ports::repository::BoundedPage {
            next_cursor: has_more
                .then(|| records.last().map(|job| job.name.clone()))
                .flatten(),
            records,
            high_watermark,
            failed_partitions: Vec::new(),
        })
    }

    async fn get_job(&self, name: &str) -> Result<Option<Job>, RepoError> {
        let row = sqlx::query("SELECT * FROM jobs WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.as_ref().map(job_from_row).transpose()
    }

    async fn save_job(&self, job: &Job) -> Result<(), RepoError> {
        let on_overlap = match job.on_overlap {
            my_supervisor_core::domain::OverlapPolicy::Skip => "skip",
            my_supervisor_core::domain::OverlapPolicy::Queue => "queue",
            my_supervisor_core::domain::OverlapPolicy::Parallel => "parallel",
        };
        let on_dep = match job.on_dependency_failure {
            my_supervisor_core::domain::DependencyFailurePolicy::Skip => "skip",
            my_supervisor_core::domain::DependencyFailurePolicy::RunAnyway => "run_anyway",
        };
        let _ = sqlx::query(
            r#"INSERT INTO jobs
                (name, id, command, args, cwd, env, trigger, on_overlap, on_dependency_failure,
                 timeout_sec, log_retention_max_runs, log_retention_max_age_days, timezone, schedule_revision, trigger_id, misfire_policy, retry_policy, admission_policy)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(name) DO UPDATE SET
                command = excluded.command,
                args = excluded.args,
                cwd = excluded.cwd,
                env = excluded.env,
                trigger = excluded.trigger,
                on_overlap = excluded.on_overlap,
                on_dependency_failure = excluded.on_dependency_failure,
                timeout_sec = excluded.timeout_sec,
                log_retention_max_runs = excluded.log_retention_max_runs,
                log_retention_max_age_days = excluded.log_retention_max_age_days, timezone = excluded.timezone, schedule_revision = excluded.schedule_revision, trigger_id = excluded.trigger_id, misfire_policy = excluded.misfire_policy, retry_policy = excluded.retry_policy, admission_policy = excluded.admission_policy"#,
        )
        .bind(&job.name)
        .bind(job.id.0.to_string())
        .bind(&job.command)
        .bind(json_string(&job.args))
        .bind(job.cwd.as_ref().map(|p| p.display().to_string()))
        .bind(json_string(&job.env))
        .bind(json_string(&TriggerRepr::from(&job.trigger)))
        .bind(on_overlap)
        .bind(on_dep)
        .bind(job.timeout.map(|d| d.as_secs() as i64))
        .bind(job.log_retention.max_runs.map(i64::from))
        .bind(job.log_retention.max_age_days.map(i64::from))
        .bind(&job.timezone).bind(job.schedule_revision as i64).bind(job.trigger_id.to_string())
        .bind(json_string(&job.misfire_policy)).bind(json_string(&job.retry_policy)).bind(json_string(&job.admission))
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn delete_job(&self, name: &str) -> Result<Vec<JobRunId>, RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let rows = sqlx::query("SELECT run_id FROM job_runs WHERE job_name = ?")
            .bind(name)
            .fetch_all(&mut *transaction)
            .await
            .map_err(backend)?;
        let run_ids = rows
            .iter()
            .map(|row| {
                uuid::Uuid::parse_str(row.try_get::<&str, _>("run_id").map_err(backend)?)
                    .map(JobRunId)
                    .map_err(backend)
            })
            .collect::<Result<Vec<_>, _>>()?;
        for run_id in &run_ids {
            sqlx::query("INSERT INTO run_log_cleanup(run_id, attempts, last_error) VALUES (?, 0, NULL) ON CONFLICT(run_id) DO NOTHING")
                .bind(run_id.0.to_string())
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
        }
        sqlx::query("DELETE FROM job_runs WHERE job_name = ?")
            .bind(name)
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        sqlx::query("DELETE FROM jobs WHERE name = ?")
            .bind(name)
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        sqlx::query("DELETE FROM dependency_signatures WHERE job_name = ?")
            .bind(name)
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        transaction.commit().await.map_err(backend)?;
        Ok(run_ids)
    }

    async fn commit_job_deletion_rows(
        &self,
        deletion_id: uuid::Uuid,
        job_name: &str,
    ) -> Result<Vec<JobRunId>, RepoError> {
        if self
            .job_deletion_row_commit_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                count.checked_sub(1)
            })
            .is_ok()
        {
            return Err(RepoError::Backend(
                "injected job deletion row commit failure".into(),
            ));
        }

        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let journal = sqlx::query(
            "SELECT stage FROM job_deletion_journal WHERE deletion_id = ? AND job_name = ?",
        )
        .bind(deletion_id.to_string())
        .bind(job_name)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(backend)?;
        let Some(journal) = journal else {
            return Err(RepoError::Conflict(
                "job deletion journal is missing".into(),
            ));
        };
        let stage = journal.try_get::<String, _>("stage").map_err(backend)?;
        if stage != deletion_stage_to_str(JobDeletionStage::RunsDraining) {
            return Err(RepoError::Conflict(
                "job deletion journal is not ready for row deletion".into(),
            ));
        }

        let rows = sqlx::query("SELECT run_id FROM job_runs WHERE job_name = ? ORDER BY run_id")
            .bind(job_name)
            .fetch_all(&mut *transaction)
            .await
            .map_err(backend)?;
        let run_ids = rows
            .iter()
            .map(|row| {
                uuid::Uuid::parse_str(row.try_get::<&str, _>("run_id").map_err(backend)?)
                    .map(JobRunId)
                    .map_err(backend)
            })
            .collect::<Result<Vec<_>, _>>()?;
        for run_id in &run_ids {
            sqlx::query("INSERT INTO run_log_cleanup(run_id, attempts, last_error) VALUES (?, 0, NULL) ON CONFLICT(run_id) DO NOTHING")
                .bind(run_id.0.to_string())
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
        }
        sqlx::query("DELETE FROM job_runs WHERE job_name = ?")
            .bind(job_name)
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        sqlx::query("DELETE FROM jobs WHERE name = ?")
            .bind(job_name)
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        sqlx::query("DELETE FROM dependency_signatures WHERE job_name = ?")
            .bind(job_name)
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        let result = sqlx::query(
            "UPDATE job_deletion_journal SET stage = ?, run_ids = ?, last_error = NULL WHERE deletion_id = ? AND job_name = ? AND stage = ?",
        )
        .bind(deletion_stage_to_str(JobDeletionStage::RowsDeleted))
        .bind(json_string(&run_ids))
        .bind(deletion_id.to_string())
        .bind(job_name)
        .bind(deletion_stage_to_str(JobDeletionStage::RunsDraining))
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        if result.rows_affected() != 1 {
            return Err(RepoError::Conflict(
                "job deletion journal changed before row deletion".into(),
            ));
        }
        transaction.commit().await.map_err(backend)?;
        Ok(run_ids)
    }

    async fn create_job_deletion_journal(
        &self,
        journal: &JobDeletionJournal,
    ) -> Result<(), RepoError> {
        sqlx::query("INSERT INTO job_deletion_journal(job_name, deletion_id, job_snapshot, stage, run_ids, last_error) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(job_name) DO NOTHING")
            .bind(&journal.job.name)
            .bind(journal.deletion_id.to_string())
            .bind(json_string(&journal.job))
            .bind(deletion_stage_to_str(journal.stage))
            .bind(json_string(&journal.run_ids))
            .bind(&journal.last_error)
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn get_job_deletion_journal(
        &self,
        name: &str,
    ) -> Result<Option<JobDeletionJournal>, RepoError> {
        let row = sqlx::query("SELECT * FROM job_deletion_journal WHERE job_name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.as_ref().map(job_deletion_from_row).transpose()
    }

    async fn list_incomplete_job_deletions(&self) -> Result<Vec<JobDeletionJournal>, RepoError> {
        let rows = sqlx::query("SELECT * FROM job_deletion_journal ORDER BY job_name")
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        rows.iter().map(job_deletion_from_row).collect()
    }

    async fn update_job_deletion_journal(
        &self,
        deletion_id: uuid::Uuid,
        stage: JobDeletionStage,
        run_ids: Option<&[JobRunId]>,
        error: Option<&str>,
    ) -> Result<(), RepoError> {
        if stage == JobDeletionStage::RollbackRequired
            && self
                .job_deletion_rollback_direction_failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                    count.checked_sub(1)
                })
                .is_ok()
        {
            return Err(RepoError::Backend(
                "injected job deletion rollback direction failure".into(),
            ));
        }
        let run_ids = run_ids.map(json_string);
        let result = sqlx::query("UPDATE job_deletion_journal SET stage = ?, run_ids = COALESCE(?, run_ids), last_error = ? WHERE deletion_id = ?")
            .bind(deletion_stage_to_str(stage))
            .bind(run_ids)
            .bind(error)
            .bind(deletion_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::Conflict(
                "job deletion journal is missing".into(),
            ));
        }
        Ok(())
    }

    async fn cancel_queued_runs_for_job_deletion(
        &self,
        deletion_id: uuid::Uuid,
        job_name: &str,
        terminal_events: &[TransientTerminalEvent],
    ) -> Result<(), RepoError> {
        if self
            .job_deletion_cancellation_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                count.checked_sub(1)
            })
            .is_ok()
        {
            return Err(RepoError::Backend(
                "injected job deletion cancellation failure".into(),
            ));
        }

        let mut transaction = self.pool.begin().await.map_err(backend)?;
        for event in terminal_events {
            if event.job_name != job_name || event.state != JobRunState::Cancelled {
                return Err(RepoError::Conflict(
                    "invalid queued deletion terminal event".into(),
                ));
            }
            let result = sqlx::query(
                "UPDATE job_runs SET state = ?, ended_at = ?, exit_code = ? WHERE run_id = ? AND job_name = ? AND state = ?",
            )
            .bind(run_state_to_str(JobRunState::Cancelled))
            .bind(dt_to_str(&event.occurred_at))
            .bind(event.exit_code.map(i64::from))
            .bind(event.run_id.0.to_string())
            .bind(job_name)
            .bind(run_state_to_str(JobRunState::Pending))
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
            if result.rows_affected() != 1 {
                return Err(RepoError::Conflict(
                    "queued run changed before deletion cancellation".into(),
                ));
            }
            sqlx::query(
                "INSERT INTO transient_terminal_outbox(cleanup_id, event_id, occurred_at, job_name, run_id, state, exit_code) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(cleanup_id) DO NOTHING",
            )
            .bind(event.cleanup_id.to_string())
            .bind(event.event_id.to_string())
            .bind(dt_to_str(&event.occurred_at))
            .bind(&event.job_name)
            .bind(event.run_id.0.to_string())
            .bind(run_state_to_str(event.state))
            .bind(event.exit_code.map(i64::from))
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        }
        let result = sqlx::query(
            "UPDATE job_deletion_journal SET stage = ?, last_error = NULL WHERE deletion_id = ? AND job_name = ? AND stage = ?",
        )
        .bind(deletion_stage_to_str(JobDeletionStage::CancellationStarted))
        .bind(deletion_id.to_string())
        .bind(job_name)
        .bind(deletion_stage_to_str(JobDeletionStage::SchedulerUnregistered))
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        if result.rows_affected() != 1 {
            return Err(RepoError::Conflict(
                "job deletion journal is not ready for cancellation".into(),
            ));
        }
        transaction.commit().await.map_err(backend)
    }

    async fn clear_job_deletion_journal(&self, deletion_id: uuid::Uuid) -> Result<(), RepoError> {
        if self
            .job_deletion_clear_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                count.checked_sub(1)
            })
            .is_ok()
        {
            return Err(RepoError::Backend(
                "injected job deletion journal clear failure".into(),
            ));
        }
        sqlx::query("DELETE FROM job_deletion_journal WHERE deletion_id = ?")
            .bind(deletion_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn save_run(&self, run: &JobRun) -> Result<(), RepoError> {
        let parent_exists = sqlx::query("SELECT 1 FROM jobs WHERE name = ? AND id = ?")
            .bind(&run.job_name)
            .bind(run.job_id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?
            .is_some();
        if !parent_exists {
            return Err(RepoError::Conflict(
                "run parent job identity is missing or stale".into(),
            ));
        }
        let result = sqlx::query(
            r#"INSERT INTO job_runs
                (run_id, job_name, job_id, triggered_by, scheduled_at, started_at, ended_at, exit_code, state, occurrence, original_scheduled_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(run_id) DO UPDATE SET
                started_at = excluded.started_at,
                ended_at = excluded.ended_at,
                exit_code = excluded.exit_code,
                state = excluded.state, occurrence = excluded.occurrence, original_scheduled_at = excluded.original_scheduled_at
               WHERE job_runs.job_name = excluded.job_name AND job_runs.job_id = excluded.job_id"#,
        )
        .bind(run.run_id.0.to_string())
        .bind(&run.job_name)
        .bind(run.job_id.0.to_string())
        .bind(json_string(&TriggeredByRepr::from(&run.triggered_by)))
        .bind(dt_to_str(&run.scheduled_at))
        .bind(opt_dt_to_str(&run.started_at))
        .bind(opt_dt_to_str(&run.ended_at))
        .bind(run.exit_code.map(|c| c as i64))
        .bind(run_state_to_str(run.state))
        .bind(run.occurrence.as_ref().map(json_string))
        .bind(opt_dt_to_str(&run.original_scheduled_at))
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::Conflict("run identity is stale".into()));
        }
        Ok(())
    }

    async fn claim_schedule_occurrence(
        &self,
        job: &Job,
        occurrence: &ScheduleOccurrence,
        now: DateTime<Utc>,
    ) -> Result<DurableScheduleOccurrence, RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let inserted = sqlx::query(
            "INSERT INTO schedule_occurrences(job_id, job_name, trigger_id, schedule_revision, scheduled_at, occurrence, state, next_attempt_at, run_id, final_state) \
             VALUES (?, ?, ?, ?, ?, ?, 'claimed', ?, NULL, NULL) \
             ON CONFLICT(job_id, trigger_id, schedule_revision, scheduled_at) DO NOTHING",
        )
        .bind(job.id.0.to_string()).bind(&job.name).bind(occurrence.trigger_id.to_string())
        .bind(occurrence.schedule_revision as i64).bind(dt_to_str(&occurrence.scheduled_at))
        .bind(json_string(occurrence)).bind(dt_to_str(&now))
        .execute(&mut *transaction).await.map_err(backend)?.rows_affected() == 1;
        sqlx::query(
            "INSERT INTO schedule_cursors(job_id, trigger_id, schedule_revision, cursor_at) VALUES (?, ?, ?, ?) \
             ON CONFLICT(job_id, trigger_id, schedule_revision) DO UPDATE SET cursor_at = \
             CASE WHEN excluded.cursor_at > schedule_cursors.cursor_at THEN excluded.cursor_at ELSE schedule_cursors.cursor_at END",
        )
        .bind(job.id.0.to_string()).bind(occurrence.trigger_id.to_string())
        .bind(occurrence.schedule_revision as i64).bind(dt_to_str(&occurrence.scheduled_at))
        .execute(&mut *transaction).await.map_err(backend)?;
        if inserted {
            observability::insert_operator_event(
                &mut transaction,
                now,
                format!("job:{}", job.name),
                "schedule_occurrence_claimed".into(),
                AlertSeverity::Info,
                format!(
                    "scheduled occurrence {} was claimed",
                    occurrence.scheduled_at
                ),
                format!(
                    "schedule-claim:{}:{}:{}:{}",
                    job.id.0,
                    occurrence.trigger_id,
                    occurrence.schedule_revision,
                    occurrence.scheduled_at.to_rfc3339()
                ),
            )
            .await?;
        }
        let row = sqlx::query(
            "SELECT * FROM schedule_occurrences WHERE job_id = ? AND trigger_id = ? AND schedule_revision = ? AND scheduled_at = ?",
        )
        .bind(job.id.0.to_string()).bind(occurrence.trigger_id.to_string())
        .bind(occurrence.schedule_revision as i64).bind(dt_to_str(&occurrence.scheduled_at))
        .fetch_one(&mut *transaction).await.map_err(backend)?;
        let durable = durable_occurrence_from_row(&row)?;
        transaction.commit().await.map_err(backend)?;
        Ok(durable)
    }

    async fn schedule_cursor(&self, job: &Job) -> Result<Option<DateTime<Utc>>, RepoError> {
        let value: Option<String> = sqlx::query_scalar(
            "SELECT cursor_at FROM schedule_cursors WHERE job_id = ? AND trigger_id = ? AND schedule_revision = ?",
        )
        .bind(job.id.0.to_string()).bind(job.trigger_id.to_string()).bind(job.schedule_revision as i64)
        .fetch_optional(&self.pool).await.map_err(backend)?;
        value.map(|value| str_to_dt(&value)).transpose()
    }

    async fn list_due_schedule_occurrences(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<DurableScheduleOccurrence>, RepoError> {
        let rows = sqlx::query(
            "SELECT * FROM schedule_occurrences WHERE state IN ('claimed', 'queued', 'retry_pending') AND next_attempt_at <= ? ORDER BY scheduled_at ASC LIMIT ?",
        )
        .bind(dt_to_str(&now)).bind(limit as i64).fetch_all(&self.pool).await.map_err(backend)?;
        rows.iter().map(durable_occurrence_from_row).collect()
    }

    async fn admit_schedule_occurrence(
        &self,
        job: &Job,
        occurrence: &ScheduleOccurrence,
        now: DateTime<Utc>,
    ) -> Result<ScheduleAdmission, RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let row = sqlx::query(
            "SELECT * FROM schedule_occurrences WHERE job_id = ? AND trigger_id = ? AND schedule_revision = ? AND scheduled_at = ?",
        )
        .bind(job.id.0.to_string()).bind(occurrence.trigger_id.to_string())
        .bind(occurrence.schedule_revision as i64).bind(dt_to_str(&occurrence.scheduled_at))
        .fetch_optional(&mut *transaction).await.map_err(backend)?
        .ok_or_else(|| RepoError::NotFound("schedule occurrence".into()))?;
        let durable = durable_occurrence_from_row(&row)?;
        if durable.state == ScheduleOccurrenceState::Finalized {
            transaction.commit().await.map_err(backend)?;
            return Ok(ScheduleAdmission::Finalized(durable));
        }
        if durable.state == ScheduleOccurrenceState::Running {
            transaction.commit().await.map_err(backend)?;
            return Ok(ScheduleAdmission::Queued(durable));
        }
        let running: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM schedule_occurrences WHERE job_id = ? AND state = 'running'",
        )
        .bind(job.id.0.to_string())
        .fetch_one(&mut *transaction)
        .await
        .map_err(backend)?;
        if running >= i64::from(job.admission.max_concurrency) {
            let queued: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM schedule_occurrences WHERE job_id = ? AND state IN ('claimed', 'queued', 'retry_pending')",
            ).bind(job.id.0.to_string()).fetch_one(&mut *transaction).await.map_err(backend)?;
            if queued > i64::from(job.admission.max_queue) {
                let run_id = JobRunId::new();
                sqlx::query(
                    "UPDATE schedule_occurrences SET state = 'finalized', run_id = ?, final_state = 'skipped' WHERE job_id = ? AND trigger_id = ? AND schedule_revision = ? AND scheduled_at = ?",
                ).bind(run_id.0.to_string()).bind(job.id.0.to_string()).bind(occurrence.trigger_id.to_string())
                    .bind(occurrence.schedule_revision as i64).bind(dt_to_str(&occurrence.scheduled_at))
                    .execute(&mut *transaction).await.map_err(backend)?;
                sqlx::query(
                    "INSERT INTO job_runs(run_id, job_name, job_id, triggered_by, scheduled_at, started_at, ended_at, exit_code, state, occurrence, original_scheduled_at) VALUES (?, ?, ?, ?, ?, NULL, ?, NULL, 'skipped', ?, ?)",
                ).bind(run_id.0.to_string()).bind(&job.name).bind(job.id.0.to_string())
                    .bind(json_string(&TriggeredByRepr::from(&my_supervisor_core::domain::TriggeredBy::Scheduled { occurrence: occurrence.clone() })))
                    .bind(dt_to_str(&occurrence.scheduled_at)).bind(dt_to_str(&now)).bind(json_string(occurrence)).bind(dt_to_str(&occurrence.scheduled_at))
                    .execute(&mut *transaction).await.map_err(backend)?;
                observability::insert_operator_event(
                    &mut transaction,
                    now,
                    format!("job:{}", job.name),
                    "schedule_admission_overflow".into(),
                    AlertSeverity::Warning,
                    format!(
                        "scheduled occurrence {} was skipped by admission overflow",
                        occurrence.scheduled_at
                    ),
                    format!("schedule-overflow:{}", run_id.0),
                )
                .await?;
                let row = sqlx::query("SELECT * FROM schedule_occurrences WHERE job_id = ? AND trigger_id = ? AND schedule_revision = ? AND scheduled_at = ?")
                    .bind(job.id.0.to_string()).bind(occurrence.trigger_id.to_string()).bind(occurrence.schedule_revision as i64).bind(dt_to_str(&occurrence.scheduled_at))
                    .fetch_one(&mut *transaction).await.map_err(backend)?;
                let durable = durable_occurrence_from_row(&row)?;
                transaction.commit().await.map_err(backend)?;
                return Ok(ScheduleAdmission::Finalized(durable));
            }
            sqlx::query("UPDATE schedule_occurrences SET state = 'queued' WHERE job_id = ? AND trigger_id = ? AND schedule_revision = ? AND scheduled_at = ?")
                .bind(job.id.0.to_string()).bind(occurrence.trigger_id.to_string()).bind(occurrence.schedule_revision as i64).bind(dt_to_str(&occurrence.scheduled_at))
                .execute(&mut *transaction).await.map_err(backend)?;
            observability::insert_operator_event(
                &mut transaction,
                now,
                format!("job:{}", job.name),
                "schedule_occurrence_queued".into(),
                AlertSeverity::Info,
                format!(
                    "scheduled occurrence {} was queued",
                    occurrence.scheduled_at
                ),
                format!(
                    "schedule-queued:{}:{}:{}:{}",
                    job.id.0,
                    occurrence.trigger_id,
                    occurrence.schedule_revision,
                    occurrence.scheduled_at.to_rfc3339()
                ),
            )
            .await?;
            let row = sqlx::query("SELECT * FROM schedule_occurrences WHERE job_id = ? AND trigger_id = ? AND schedule_revision = ? AND scheduled_at = ?")
                .bind(job.id.0.to_string()).bind(occurrence.trigger_id.to_string()).bind(occurrence.schedule_revision as i64).bind(dt_to_str(&occurrence.scheduled_at))
                .fetch_one(&mut *transaction).await.map_err(backend)?;
            let durable = durable_occurrence_from_row(&row)?;
            transaction.commit().await.map_err(backend)?;
            return Ok(ScheduleAdmission::Queued(durable));
        }
        let run_id = JobRunId::new();
        sqlx::query("UPDATE schedule_occurrences SET state = 'running', run_id = ? WHERE job_id = ? AND trigger_id = ? AND schedule_revision = ? AND scheduled_at = ?")
            .bind(run_id.0.to_string()).bind(job.id.0.to_string()).bind(occurrence.trigger_id.to_string()).bind(occurrence.schedule_revision as i64).bind(dt_to_str(&occurrence.scheduled_at))
            .execute(&mut *transaction).await.map_err(backend)?;
        sqlx::query(
            "INSERT INTO job_runs(run_id, job_name, job_id, triggered_by, scheduled_at, started_at, ended_at, exit_code, state, occurrence, original_scheduled_at) VALUES (?, ?, ?, ?, ?, NULL, NULL, NULL, 'pending', ?, ?)",
        ).bind(run_id.0.to_string()).bind(&job.name).bind(job.id.0.to_string())
            .bind(json_string(&TriggeredByRepr::from(&my_supervisor_core::domain::TriggeredBy::Scheduled { occurrence: occurrence.clone() })))
            .bind(dt_to_str(&occurrence.scheduled_at)).bind(json_string(occurrence)).bind(dt_to_str(&occurrence.scheduled_at))
            .execute(&mut *transaction).await.map_err(backend)?;
        observability::insert_operator_event(
            &mut transaction,
            now,
            format!("job:{}", job.name),
            "schedule_occurrence_started".into(),
            AlertSeverity::Info,
            format!(
                "scheduled occurrence {} was admitted",
                occurrence.scheduled_at
            ),
            format!("schedule-start:{}", run_id.0),
        )
        .await?;
        let row = sqlx::query("SELECT * FROM schedule_occurrences WHERE job_id = ? AND trigger_id = ? AND schedule_revision = ? AND scheduled_at = ?")
            .bind(job.id.0.to_string()).bind(occurrence.trigger_id.to_string()).bind(occurrence.schedule_revision as i64).bind(dt_to_str(&occurrence.scheduled_at))
            .fetch_one(&mut *transaction).await.map_err(backend)?;
        let durable = durable_occurrence_from_row(&row)?;
        transaction.commit().await.map_err(backend)?;
        Ok(ScheduleAdmission::Start(durable))
    }

    async fn finalize_schedule_attempt(
        &self,
        job: &Job,
        run: &JobRun,
        now: DateTime<Utc>,
    ) -> Result<ScheduleFinalization, RepoError> {
        let Some(occurrence) = &run.occurrence else {
            return Err(RepoError::NotFound("scheduled occurrence".into()));
        };
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let row = sqlx::query("SELECT * FROM schedule_occurrences WHERE job_id = ? AND trigger_id = ? AND schedule_revision = ? AND scheduled_at = ? AND run_id = ?")
            .bind(job.id.0.to_string()).bind(occurrence.trigger_id.to_string()).bind(occurrence.schedule_revision as i64).bind(dt_to_str(&occurrence.scheduled_at)).bind(run.run_id.0.to_string())
            .fetch_one(&mut *transaction).await.map_err(backend)?;
        let mut durable = durable_occurrence_from_row(&row)?;
        let can_retry = run.state == JobRunState::Failed
            && durable.occurrence.attempt < job.retry_policy.max_attempts;
        if can_retry {
            let exponent = u32::from(durable.occurrence.attempt.saturating_sub(1));
            let multiplier = u64::from(job.retry_policy.multiplier.max(1)).saturating_pow(exponent);
            let delay = job
                .retry_policy
                .initial_backoff
                .as_secs()
                .saturating_mul(multiplier)
                .min(job.retry_policy.max_backoff.as_secs());
            let jitter =
                delay.saturating_mul(u64::from(job.retry_policy.jitter_percent.min(100))) / 100;
            let width = jitter.saturating_mul(2).saturating_add(1);
            let offset = if width == 0 {
                0
            } else {
                run.run_id.0.as_u128() as u64 % width
            };
            let retry_at = now
                + chrono::Duration::seconds(
                    delay.saturating_sub(jitter).saturating_add(offset) as i64
                );
            durable.occurrence.attempt = durable.occurrence.attempt.saturating_add(1);
            sqlx::query("UPDATE schedule_occurrences SET occurrence = ?, state = 'retry_pending', next_attempt_at = ?, run_id = NULL, final_state = NULL WHERE job_id = ? AND trigger_id = ? AND schedule_revision = ? AND scheduled_at = ? AND run_id = ?")
                .bind(json_string(&durable.occurrence)).bind(dt_to_str(&retry_at)).bind(job.id.0.to_string()).bind(occurrence.trigger_id.to_string()).bind(occurrence.schedule_revision as i64).bind(dt_to_str(&occurrence.scheduled_at)).bind(run.run_id.0.to_string())
                .execute(&mut *transaction).await.map_err(backend)?;
            durable.state = ScheduleOccurrenceState::RetryPending;
            durable.next_attempt_at = retry_at;
            durable.run_id = None;
            observability::insert_operator_event(
                &mut transaction,
                now,
                format!("job:{}", job.name),
                "schedule_attempt_retry_pending".into(),
                AlertSeverity::Warning,
                format!("scheduled run {} will retry", run.run_id.0),
                format!("schedule-retry:{}", run.run_id.0),
            )
            .await?;
            transaction.commit().await.map_err(backend)?;
            Ok(ScheduleFinalization::Retry(durable))
        } else {
            sqlx::query("UPDATE schedule_occurrences SET state = 'finalized', final_state = ? WHERE job_id = ? AND trigger_id = ? AND schedule_revision = ? AND scheduled_at = ? AND run_id = ?")
                .bind(run_state_to_str(run.state)).bind(job.id.0.to_string()).bind(occurrence.trigger_id.to_string()).bind(occurrence.schedule_revision as i64).bind(dt_to_str(&occurrence.scheduled_at)).bind(run.run_id.0.to_string())
                .execute(&mut *transaction).await.map_err(backend)?;
            durable.state = ScheduleOccurrenceState::Finalized;
            durable.final_state = Some(run.state);
            observability::insert_operator_event(
                &mut transaction,
                now,
                format!("job:{}", job.name),
                "schedule_attempt_finalized".into(),
                if run.state == JobRunState::Succeeded {
                    AlertSeverity::Info
                } else {
                    AlertSeverity::Warning
                },
                format!(
                    "scheduled run {} finalized as {}",
                    run.run_id.0,
                    run_state_to_str(run.state)
                ),
                format!("schedule-finalized:{}", run.run_id.0),
            )
            .await?;
            transaction.commit().await.map_err(backend)?;
            Ok(ScheduleFinalization::Finalized(durable))
        }
    }

    async fn recover_schedule_occurrences(&self, now: DateTime<Utc>) -> Result<(), RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let rows = sqlx::query("SELECT job_id,job_name,trigger_id,schedule_revision,scheduled_at,run_id FROM schedule_occurrences WHERE state = 'running'")
            .fetch_all(&mut *transaction).await.map_err(backend)?;
        sqlx::query("UPDATE schedule_occurrences SET state = 'retry_pending', next_attempt_at = ?, run_id = NULL WHERE state = 'running'")
            .bind(dt_to_str(&now)).execute(&mut *transaction).await.map_err(backend)?;
        for row in rows {
            let job_id: String = row.try_get("job_id").map_err(backend)?;
            let job_name: String = row.try_get("job_name").map_err(backend)?;
            let trigger_id: String = row.try_get("trigger_id").map_err(backend)?;
            let revision: i64 = row.try_get("schedule_revision").map_err(backend)?;
            let scheduled_at: String = row.try_get("scheduled_at").map_err(backend)?;
            let run_id: Option<String> = row.try_get("run_id").map_err(backend)?;
            observability::insert_operator_event(
                &mut transaction,
                now,
                format!("job:{job_name}"),
                "schedule_occurrence_recovered".into(),
                AlertSeverity::Warning,
                format!("running scheduled occurrence {scheduled_at} recovered for retry"),
                format!(
                    "schedule-recovery:{job_id}:{trigger_id}:{revision}:{scheduled_at}:{}",
                    run_id.unwrap_or_default()
                ),
            )
            .await?;
        }
        transaction.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn is_durable_schedule_run(&self, run_id: JobRunId) -> Result<bool, RepoError> {
        Ok(sqlx::query("SELECT 1 FROM schedule_occurrences WHERE run_id = ? OR EXISTS (SELECT 1 FROM job_runs WHERE run_id = ? AND occurrence IS NOT NULL)")
            .bind(run_id.0.to_string()).bind(run_id.0.to_string()).fetch_optional(&self.pool).await.map_err(backend)?.is_some())
    }

    async fn commit_terminal_run_with_event(
        &self,
        run: &JobRun,
        event: &TransientTerminalEvent,
    ) -> Result<(), RepoError> {
        if self
            .terminal_run_commit_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                count.checked_sub(1)
            })
            .is_ok()
        {
            return Err(RepoError::Backend(
                "injected terminal run commit failure".into(),
            ));
        }
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let result = sqlx::query(
            "UPDATE job_runs SET started_at = ?, ended_at = ?, exit_code = ?, state = ? WHERE run_id = ? AND job_name = ? AND job_id = ?",
        )
        .bind(opt_dt_to_str(&run.started_at))
        .bind(opt_dt_to_str(&run.ended_at))
        .bind(run.exit_code.map(i64::from))
        .bind(run_state_to_str(run.state))
        .bind(run.run_id.0.to_string())
        .bind(&run.job_name)
        .bind(run.job_id.0.to_string())
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::Conflict(
                "run parent job identity is missing or stale".into(),
            ));
        }
        sqlx::query(
            "INSERT INTO transient_terminal_outbox(cleanup_id, event_id, occurred_at, job_name, run_id, state, exit_code) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(cleanup_id) DO NOTHING",
        )
        .bind(event.cleanup_id.to_string())
        .bind(event.event_id.to_string())
        .bind(dt_to_str(&event.occurred_at))
        .bind(&event.job_name)
        .bind(event.run_id.0.to_string())
        .bind(run_state_to_str(event.state))
        .bind(event.exit_code.map(i64::from))
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        let operator_event = OperatorEvent {
            id: event.event_id,
            occurred_at: event.occurred_at,
            source: format!("job:{}", event.job_name),
            kind: format!("job_run_{}", run_state_to_str(event.state)),
            severity: if matches!(event.state, JobRunState::Succeeded) {
                AlertSeverity::Info
            } else {
                AlertSeverity::Warning
            },
            message: format!(
                "job '{}' run {} completed as {}",
                event.job_name,
                event.run_id.0,
                run_state_to_str(event.state)
            ),
            transition_key: format!("terminal-run:{}", event.event_id),
        };
        sqlx::query("INSERT INTO observability_events(id, occurred_at, transition_key, payload) VALUES (?, ?, ?, ?) ON CONFLICT(transition_key) DO NOTHING")
            .bind(operator_event.id.to_string())
            .bind(dt_to_str(&operator_event.occurred_at))
            .bind(&operator_event.transition_key)
            .bind(json_string(&operator_event))
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        transaction.commit().await.map_err(backend)
    }

    async fn list_runs(&self, job_name: &str, limit: usize) -> Result<Vec<JobRun>, RepoError> {
        let rows = sqlx::query(
            "SELECT * FROM job_runs WHERE job_name = ? ORDER BY scheduled_at DESC LIMIT ?",
        )
        .bind(job_name)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.iter().map(run_from_row).collect()
    }

    async fn list_runs_filtered(
        &self,
        job_name: &str,
        state: Option<JobRunState>,
        since: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<JobRun>, RepoError> {
        let mut statement = String::from("SELECT * FROM job_runs WHERE job_name = ?");
        if state.is_some() {
            statement.push_str(" AND state = ?");
        }
        if since.is_some() {
            statement.push_str(" AND COALESCE(started_at, scheduled_at) >= ?");
        }
        statement.push_str(" ORDER BY scheduled_at DESC LIMIT ?");
        let mut query = sqlx::query(&statement).bind(job_name);
        if let Some(state) = state {
            query = query.bind(run_state_to_str(state));
        }
        if let Some(since) = since {
            query = query.bind(dt_to_str(&since));
        }
        let rows = query
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        rows.iter().map(run_from_row).collect()
    }

    async fn get_run(
        &self,
        job_name: &str,
        run_id: &JobRunId,
    ) -> Result<Option<JobRun>, RepoError> {
        let row = sqlx::query("SELECT * FROM job_runs WHERE job_name = ? AND run_id = ?")
            .bind(job_name)
            .bind(run_id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.as_ref().map(run_from_row).transpose()
    }

    async fn prune_runs(
        &self,
        job_name: &str,
        max_runs: Option<u32>,
        older_than: Option<DateTime<Utc>>,
    ) -> Result<Vec<JobRunId>, RepoError> {
        if max_runs.is_none() && older_than.is_none() {
            return Ok(Vec::new());
        }
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let rows = sqlx::query(
            r#"SELECT run_id, scheduled_at, ended_at
               FROM job_runs
               WHERE job_name = ? AND state IN ('succeeded', 'failed', 'timed_out', 'cancelled', 'skipped')
               ORDER BY scheduled_at DESC"#,
        )
        .bind(job_name)
        .fetch_all(&mut *transaction)
        .await
        .map_err(backend)?;

        let mut removed = Vec::new();
        for (position, row) in rows.iter().enumerate() {
            let scheduled_at = str_to_dt(row.try_get("scheduled_at").map_err(backend)?)?;
            let ended_at: Option<String> = row.try_get("ended_at").map_err(backend)?;
            let retention_time = ended_at
                .as_deref()
                .map(str_to_dt)
                .transpose()?
                .unwrap_or(scheduled_at);
            let exceeds_count = max_runs.is_some_and(|limit| position >= limit as usize);
            let exceeds_age = older_than.is_some_and(|cutoff| retention_time < cutoff);
            if !exceeds_count && !exceeds_age {
                continue;
            }
            let run_id = JobRunId(
                uuid::Uuid::parse_str(row.try_get::<&str, _>("run_id").map_err(backend)?)
                    .map_err(backend)?,
            );
            sqlx::query("DELETE FROM job_runs WHERE run_id = ?")
                .bind(run_id.0.to_string())
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
            sqlx::query("INSERT INTO run_log_cleanup(run_id, attempts, last_error) VALUES (?, 0, NULL) ON CONFLICT(run_id) DO NOTHING")
                .bind(run_id.0.to_string())
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
            removed.push(run_id);
        }
        transaction.commit().await.map_err(backend)?;
        Ok(removed)
    }

    async fn pending_run_log_cleanup(&self, limit: usize) -> Result<Vec<RunLogCleanup>, RepoError> {
        let rows = sqlx::query("SELECT run_id, attempts, last_error FROM run_log_cleanup ORDER BY attempts, run_id LIMIT ?")
            .bind(limit as i64).fetch_all(&self.pool).await.map_err(backend)?;
        rows.into_iter()
            .map(|row| {
                let run_id =
                    uuid::Uuid::parse_str(row.try_get::<&str, _>("run_id").map_err(backend)?)
                        .map(JobRunId)
                        .map_err(backend)?;
                Ok(RunLogCleanup {
                    run_id,
                    attempts: row.try_get::<i64, _>("attempts").map_err(backend)? as u32,
                    last_error: row.try_get("last_error").map_err(backend)?,
                })
            })
            .collect()
    }

    async fn complete_run_log_cleanup(&self, run_id: JobRunId) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM run_log_cleanup WHERE run_id = ?")
            .bind(run_id.0.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn fail_run_log_cleanup(&self, run_id: JobRunId, error: &str) -> Result<(), RepoError> {
        sqlx::query(
            "UPDATE run_log_cleanup SET attempts = attempts + 1, last_error = ? WHERE run_id = ?",
        )
        .bind(error)
        .bind(run_id.0.to_string())
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn enqueue_run_log_cleanup(&self, run_id: JobRunId) -> Result<(), RepoError> {
        sqlx::query("INSERT INTO run_log_cleanup(run_id, attempts, last_error) VALUES (?, 0, NULL) ON CONFLICT(run_id) DO NOTHING")
            .bind(run_id.0.to_string()).execute(&self.pool).await.map_err(backend)?;
        Ok(())
    }

    async fn enqueue_transient_cleanup(&self, ticket: &CleanupTicket) -> Result<(), RepoError> {
        if self
            .transient_cleanup_enqueue_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                count.checked_sub(1)
            })
            .is_ok()
        {
            return Err(RepoError::Backend(
                "injected transient cleanup enqueue failure".into(),
            ));
        }
        sqlx::query(
            "INSERT INTO transient_cleanup(cleanup_id, job_id, job_name, run_id, process_id, pid, pgid, generation, started_at, stage, attempts, last_error, intended_terminal_state, outcome_started_at, outcome_ended_at, outcome_exit_code) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(run_id) DO UPDATE SET stage = excluded.stage, attempts = transient_cleanup.attempts + 1, last_error = excluded.last_error, outcome_started_at = excluded.outcome_started_at, outcome_ended_at = excluded.outcome_ended_at, outcome_exit_code = excluded.outcome_exit_code",
        )
        .bind(ticket.cleanup_id.to_string())
        .bind(ticket.job_id.0.to_string())
        .bind(&ticket.job_name)
        .bind(ticket.run_id.0.to_string())
        .bind(ticket.child.process_id.to_string())
        .bind(i64::from(ticket.child.pid))
        .bind(ticket.child.pgid.map(i64::from))
        .bind(&ticket.child.generation)
        .bind(dt_to_str(&ticket.child.started_at))
        .bind(cleanup_stage_to_str(ticket.stage))
        .bind(i64::from(ticket.attempts))
        .bind(&ticket.last_error)
        .bind(run_state_to_str(ticket.intended_terminal_state))
        .bind(dt_to_str(&ticket.outcome.started_at))
        .bind(dt_to_str(&ticket.outcome.ended_at))
        .bind(ticket.outcome.exit_code.map(i64::from))
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn pending_transient_cleanup(
        &self,
        limit: usize,
    ) -> Result<Vec<CleanupTicket>, RepoError> {
        let rows = sqlx::query(
            "SELECT cleanup_id, job_id, job_name, run_id, process_id, pid, pgid, generation, started_at, stage, attempts, last_error, intended_terminal_state, outcome_started_at, outcome_ended_at, outcome_exit_code \
             FROM transient_cleanup ORDER BY attempts, cleanup_id LIMIT ?",
        )
        .bind(limit.min(i64::MAX as usize) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.iter()
            .map(|row| {
                Ok(CleanupTicket {
                    cleanup_id: uuid::Uuid::parse_str(
                        &row.try_get::<String, _>("cleanup_id").map_err(backend)?,
                    )
                    .map_err(backend)?,
                    job_id: JobId(
                        uuid::Uuid::parse_str(
                            &row.try_get::<String, _>("job_id").map_err(backend)?,
                        )
                        .map_err(backend)?,
                    ),
                    job_name: row.try_get("job_name").map_err(backend)?,
                    run_id: JobRunId(
                        uuid::Uuid::parse_str(
                            &row.try_get::<String, _>("run_id").map_err(backend)?,
                        )
                        .map_err(backend)?,
                    ),
                    child: my_supervisor_core::domain::ChildHandle {
                        process_id: uuid::Uuid::parse_str(
                            &row.try_get::<String, _>("process_id").map_err(backend)?,
                        )
                        .map_err(backend)?,
                        pid: row.try_get::<i64, _>("pid").map_err(backend)?.max(0) as u32,
                        pgid: row
                            .try_get::<Option<i64>, _>("pgid")
                            .map_err(backend)?
                            .map(|value| value.max(0) as u32),
                        generation: row.try_get("generation").map_err(backend)?,
                        started_at: str_to_dt(
                            &row.try_get::<String, _>("started_at").map_err(backend)?,
                        )?,
                    },
                    stage: str_to_cleanup_stage(
                        &row.try_get::<String, _>("stage").map_err(backend)?,
                    ),
                    attempts: row.try_get::<i64, _>("attempts").map_err(backend)?.max(0) as u32,
                    last_error: row.try_get("last_error").map_err(backend)?,
                    intended_terminal_state: str_to_run_state(
                        &row.try_get::<String, _>("intended_terminal_state")
                            .map_err(backend)?,
                    ),
                    outcome: my_supervisor_core::ports::TransientOutcome {
                        // Historic tickets did not retain terminal data; retain a
                        // conservative fallback only for those migrated records.
                        started_at: row
                            .try_get::<Option<String>, _>("outcome_started_at")
                            .map_err(backend)?
                            .map(|value| str_to_dt(&value))
                            .transpose()?
                            .unwrap_or_else(|| {
                                row.try_get::<String, _>("started_at")
                                    .ok()
                                    .and_then(|value| str_to_dt(&value).ok())
                                    .unwrap_or_else(Utc::now)
                            }),
                        ended_at: row
                            .try_get::<Option<String>, _>("outcome_ended_at")
                            .map_err(backend)?
                            .map(|value| str_to_dt(&value))
                            .transpose()?
                            .unwrap_or_else(Utc::now),
                        exit_code: row
                            .try_get::<Option<i64>, _>("outcome_exit_code")
                            .map_err(backend)?
                            .map(|value| value as i32),
                    },
                })
            })
            .collect()
    }

    async fn update_transient_cleanup(
        &self,
        ticket: &CleanupTicket,
        stage: TransientCleanupStage,
        error: Option<&str>,
    ) -> Result<(), RepoError> {
        sqlx::query(
            "UPDATE transient_cleanup SET stage = ?, attempts = attempts + 1, last_error = ? WHERE cleanup_id = ?",
        )
        .bind(cleanup_stage_to_str(stage))
        .bind(error)
        .bind(ticket.cleanup_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn complete_transient_cleanup(&self, cleanup_id: uuid::Uuid) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM transient_cleanup WHERE cleanup_id = ?")
            .bind(cleanup_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn commit_transient_cleanup_terminal(
        &self,
        ticket: &CleanupTicket,
        run: &JobRun,
    ) -> Result<(), RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let result = sqlx::query(
            "UPDATE job_runs SET started_at = ?, ended_at = ?, exit_code = ?, state = ? WHERE run_id = ? AND job_name = ? AND job_id = ?",
        )
        .bind(opt_dt_to_str(&run.started_at))
        .bind(opt_dt_to_str(&run.ended_at))
        .bind(run.exit_code.map(i64::from))
        .bind(run_state_to_str(run.state))
        .bind(run.run_id.0.to_string())
        .bind(&run.job_name)
        .bind(run.job_id.0.to_string())
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::Conflict(
                "run parent job identity is missing or stale".into(),
            ));
        }
        sqlx::query(
            "INSERT INTO transient_terminal_outbox(cleanup_id, event_id, occurred_at, job_name, run_id, state, exit_code) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(cleanup_id) DO NOTHING",
        )
        .bind(ticket.cleanup_id.to_string())
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(dt_to_str(&ticket.outcome.ended_at))
        .bind(&ticket.job_name)
        .bind(ticket.run_id.0.to_string())
        .bind(run_state_to_str(run.state))
        .bind(run.exit_code.map(i64::from))
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        let operator_event = OperatorEvent {
            id: ticket.cleanup_id,
            occurred_at: ticket.outcome.ended_at,
            source: format!("job:{}", ticket.job_name),
            kind: format!("job_run_{}", run_state_to_str(run.state)),
            severity: if matches!(run.state, JobRunState::Succeeded) {
                AlertSeverity::Info
            } else {
                AlertSeverity::Warning
            },
            message: format!(
                "job '{}' transient run {} completed as {}",
                ticket.job_name,
                ticket.run_id.0,
                run_state_to_str(run.state)
            ),
            transition_key: format!("transient-terminal:{}", ticket.cleanup_id),
        };
        sqlx::query("INSERT INTO observability_events(id, occurred_at, transition_key, payload) VALUES (?, ?, ?, ?) ON CONFLICT(transition_key) DO NOTHING")
            .bind(operator_event.id.to_string())
            .bind(dt_to_str(&operator_event.occurred_at))
            .bind(&operator_event.transition_key)
            .bind(json_string(&operator_event))
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        transaction.commit().await.map_err(backend)
    }

    async fn pending_transient_terminal_events(
        &self,
        limit: usize,
    ) -> Result<Vec<TransientTerminalEvent>, RepoError> {
        let rows = sqlx::query(
            "SELECT cleanup_id, event_id, occurred_at, job_name, run_id, state, exit_code FROM transient_terminal_outbox ORDER BY occurred_at, event_id LIMIT ?",
        )
        .bind(limit.min(i64::MAX as usize) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter()
            .map(|row| {
                Ok(TransientTerminalEvent {
                    cleanup_id: uuid::Uuid::parse_str(
                        &row.try_get::<String, _>("cleanup_id").map_err(backend)?,
                    )
                    .map_err(backend)?,
                    event_id: uuid::Uuid::parse_str(
                        &row.try_get::<String, _>("event_id").map_err(backend)?,
                    )
                    .map_err(backend)?,
                    occurred_at: str_to_dt(
                        &row.try_get::<String, _>("occurred_at").map_err(backend)?,
                    )?,
                    job_name: row.try_get("job_name").map_err(backend)?,
                    run_id: JobRunId(
                        uuid::Uuid::parse_str(
                            &row.try_get::<String, _>("run_id").map_err(backend)?,
                        )
                        .map_err(backend)?,
                    ),
                    state: str_to_run_state(&row.try_get::<String, _>("state").map_err(backend)?),
                    exit_code: row
                        .try_get::<Option<i64>, _>("exit_code")
                        .map_err(backend)?
                        .map(|value| value as i32),
                })
            })
            .collect()
    }

    async fn acknowledge_transient_terminal_event(
        &self,
        event_id: uuid::Uuid,
        cleanup_id: uuid::Uuid,
    ) -> Result<(), RepoError> {
        if self
            .transient_terminal_ack_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(RepoError::Backend(
                "injected transient terminal acknowledgement failure".into(),
            ));
        }
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let deleted = sqlx::query(
            "DELETE FROM transient_terminal_outbox WHERE cleanup_id = ? AND event_id = ?",
        )
        .bind(cleanup_id.to_string())
        .bind(event_id.to_string())
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        if deleted.rows_affected() > 0 {
            sqlx::query("DELETE FROM transient_cleanup WHERE cleanup_id = ?")
                .bind(cleanup_id.to_string())
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
        }
        transaction.commit().await.map_err(backend)
    }

    async fn apply_config_snapshot(&self, snapshot: &ConfigSnapshot) -> Result<(), RepoError> {
        if self
            .config_snapshot_commit_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                count.checked_sub(1)
            })
            .is_ok()
        {
            return Err(RepoError::Backend(
                "injected config snapshot commit failure".into(),
            ));
        }
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let process_names: Vec<String> = sqlx::query("SELECT name FROM process_specs")
            .fetch_all(&mut *transaction)
            .await
            .map_err(backend)?
            .into_iter()
            .map(|row| row.try_get("name").map_err(backend))
            .collect::<Result<_, _>>()?;
        let target_processes: std::collections::HashSet<&str> = snapshot
            .processes
            .iter()
            .map(|spec| spec.name.as_str())
            .collect();
        for name in process_names
            .into_iter()
            .filter(|name| !target_processes.contains(name.as_str()))
        {
            sqlx::query("DELETE FROM process_specs WHERE name = ?")
                .bind(name)
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
        }
        for spec in &snapshot.processes {
            Self::save_spec_transaction(&mut transaction, spec).await?;
        }

        let job_names: Vec<String> = sqlx::query("SELECT name FROM jobs")
            .fetch_all(&mut *transaction)
            .await
            .map_err(backend)?
            .into_iter()
            .map(|row| row.try_get("name").map_err(backend))
            .collect::<Result<_, _>>()?;
        let target_jobs: std::collections::HashSet<&str> =
            snapshot.jobs.iter().map(|job| job.name.as_str()).collect();
        for name in job_names
            .into_iter()
            .filter(|name| !target_jobs.contains(name.as_str()))
        {
            let removed_runs = sqlx::query("SELECT run_id FROM job_runs WHERE job_name = ?")
                .bind(&name)
                .fetch_all(&mut *transaction)
                .await
                .map_err(backend)?;
            for row in removed_runs {
                let run_id: String = row.try_get("run_id").map_err(backend)?;
                sqlx::query("INSERT INTO run_log_cleanup(run_id, attempts, last_error) VALUES (?, 0, NULL) ON CONFLICT(run_id) DO NOTHING")
                    .bind(run_id).execute(&mut *transaction).await.map_err(backend)?;
            }
            sqlx::query("DELETE FROM job_runs WHERE job_name = ?")
                .bind(&name)
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
            sqlx::query("DELETE FROM jobs WHERE name = ?")
                .bind(&name)
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
            sqlx::query("DELETE FROM dependency_signatures WHERE job_name = ?")
                .bind(&name)
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
        }
        for job in &snapshot.jobs {
            Self::save_job_transaction(&mut transaction, job).await?;
        }
        transaction.commit().await.map_err(backend)
    }

    async fn create_config_apply_journal(
        &self,
        journal: &ConfigApplyJournal,
    ) -> Result<(), RepoError> {
        sqlx::query("INSERT INTO config_apply_journal(apply_id, previous_snapshot, target_snapshot, diff, stage, compensation_error, target_direct_starts) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(journal.apply_id.to_string())
            .bind(serde_json::to_string(&journal.previous).map_err(backend)?)
            .bind(serde_json::to_string(&journal.target).map_err(backend)?)
            .bind(serde_json::to_string(&journal.diff).map_err(backend)?)
            .bind(serde_json::to_string(&journal.stage).map_err(backend)?)
            .bind(&journal.compensation_error)
            .bind(serde_json::to_string(&journal.target_direct_starts).map_err(backend)?)
            .execute(&self.pool).await.map_err(backend)?;
        Ok(())
    }

    async fn set_config_apply_stage(
        &self,
        apply_id: uuid::Uuid,
        stage: ConfigApplyStage,
        compensation_error: Option<&str>,
    ) -> Result<(), RepoError> {
        sqlx::query(
            "UPDATE config_apply_journal SET stage = ?, compensation_error = ? WHERE apply_id = ?",
        )
        .bind(serde_json::to_string(&stage).map_err(backend)?)
        .bind(compensation_error)
        .bind(apply_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn record_config_target_direct_start(
        &self,
        apply_id: uuid::Uuid,
        start: &ConfigTargetDirectStart,
    ) -> Result<(), RepoError> {
        let row =
            sqlx::query("SELECT target_direct_starts FROM config_apply_journal WHERE apply_id = ?")
                .bind(apply_id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(backend)?
                .ok_or_else(|| RepoError::NotFound(format!("config apply {apply_id}")))?;
        let mut starts: Vec<ConfigTargetDirectStart> = serde_json::from_str(
            &row.try_get::<String, _>("target_direct_starts")
                .map_err(backend)?,
        )
        .map_err(backend)?;
        if let Some(existing) = starts
            .iter_mut()
            .find(|existing| existing.name == start.name)
        {
            *existing = start.clone();
        } else {
            starts.push(start.clone());
        }
        sqlx::query("UPDATE config_apply_journal SET target_direct_starts = ? WHERE apply_id = ?")
            .bind(serde_json::to_string(&starts).map_err(backend)?)
            .bind(apply_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn list_incomplete_config_applies(&self) -> Result<Vec<ConfigApplyJournal>, RepoError> {
        let rows = sqlx::query("SELECT * FROM config_apply_journal ORDER BY rowid")
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        rows.into_iter()
            .map(|row| {
                Ok(ConfigApplyJournal {
                    apply_id: uuid::Uuid::parse_str(
                        &row.try_get::<String, _>("apply_id").map_err(backend)?,
                    )
                    .map_err(backend)?,
                    previous: serde_json::from_str(
                        &row.try_get::<String, _>("previous_snapshot")
                            .map_err(backend)?,
                    )
                    .map_err(backend)?,
                    target: serde_json::from_str(
                        &row.try_get::<String, _>("target_snapshot")
                            .map_err(backend)?,
                    )
                    .map_err(backend)?,
                    diff: serde_json::from_str(&row.try_get::<String, _>("diff").map_err(backend)?)
                        .map_err(backend)?,
                    stage: serde_json::from_str(
                        &row.try_get::<String, _>("stage").map_err(backend)?,
                    )
                    .map_err(backend)?,
                    compensation_error: row.try_get("compensation_error").map_err(backend)?,
                    target_direct_starts: serde_json::from_str(
                        &row.try_get::<String, _>("target_direct_starts")
                            .map_err(backend)?,
                    )
                    .map_err(backend)?,
                })
            })
            .collect()
    }

    async fn restore_config_apply_snapshot(
        &self,
        apply_id: uuid::Uuid,
    ) -> Result<ConfigSnapshot, RepoError> {
        let row =
            sqlx::query("SELECT previous_snapshot FROM config_apply_journal WHERE apply_id = ?")
                .bind(apply_id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(backend)?
                .ok_or_else(|| RepoError::NotFound(format!("config apply {apply_id}")))?;
        let snapshot: ConfigSnapshot = serde_json::from_str(
            &row.try_get::<String, _>("previous_snapshot")
                .map_err(backend)?,
        )
        .map_err(backend)?;
        self.apply_config_snapshot(&snapshot).await?;
        Ok(snapshot)
    }

    async fn clear_config_apply_journal(&self, apply_id: uuid::Uuid) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM config_apply_journal WHERE apply_id = ?")
            .bind(apply_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn claim_dependency_run(
        &self,
        job_name: &str,
        signature: &DependencySignature,
        run: &JobRun,
    ) -> Result<bool, RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let encoded_signature = serde_json::to_string(signature).map_err(backend)?;
        let previous =
            sqlx::query("SELECT signature FROM dependency_signatures WHERE job_name = ?")
                .bind(job_name)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(backend)?;
        if previous.as_ref().is_some_and(|row| {
            row.try_get::<String, _>("signature").ok().as_deref()
                == Some(encoded_signature.as_str())
        }) {
            transaction.rollback().await.map_err(backend)?;
            return Ok(false);
        }
        sqlx::query("INSERT INTO dependency_signatures(job_name, signature) VALUES (?, ?) ON CONFLICT(job_name) DO UPDATE SET signature = excluded.signature")
            .bind(job_name).bind(encoded_signature).execute(&mut *transaction).await.map_err(backend)?;
        sqlx::query("INSERT INTO job_runs(run_id, job_name, job_id, triggered_by, scheduled_at, started_at, ended_at, exit_code, state) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(run.run_id.0.to_string()).bind(&run.job_name).bind(run.job_id.0.to_string())
            .bind(json_string(&TriggeredByRepr::from(&run.triggered_by))).bind(dt_to_str(&run.scheduled_at))
            .bind(opt_dt_to_str(&run.started_at)).bind(opt_dt_to_str(&run.ended_at)).bind(run.exit_code.map(i64::from)).bind(run_state_to_str(run.state))
            .execute(&mut *transaction).await.map_err(backend)?;
        transaction.commit().await.map_err(backend)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStore;
    use chrono::{Duration, Utc};
    use my_supervisor_core::domain::{
        DependencyFailurePolicy, Job, JobId, JobRun, JobRunId, JobRunState, JobTrigger,
        LogRetention, OverlapPolicy, TriggeredBy,
    };
    use my_supervisor_core::ports::JobRepository;
    use std::collections::BTreeMap;

    fn completed_run(job: &Job, scheduled_at: chrono::DateTime<Utc>) -> JobRun {
        JobRun {
            run_id: JobRunId::new(),
            job_name: job.name.clone(),
            job_id: job.id,
            triggered_by: TriggeredBy::Manual,
            scheduled_at,
            started_at: Some(scheduled_at),
            ended_at: Some(scheduled_at),
            exit_code: Some(0),
            state: JobRunState::Succeeded,
            occurrence: None,
            original_scheduled_at: None,
        }
    }

    #[tokio::test]
    async fn prune_runs_applies_age_without_deleting_recent_runs() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let now = Utc::now();
        let job = Job {
            id: JobId::new(),
            name: "cleanup".into(),
            command: "/bin/true".into(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            trigger: JobTrigger::Interval(std::time::Duration::from_secs(60)),
            on_overlap: OverlapPolicy::Skip,
            on_dependency_failure: DependencyFailurePolicy::Skip,
            timeout: None,
            log_retention: LogRetention::default(),
            timezone: "UTC".into(),
            schedule_revision: 0,
            trigger_id: uuid::Uuid::new_v4(),
            misfire_policy: Default::default(),
            retry_policy: Default::default(),
            admission: Default::default(),
        };
        store.save_job(&job).await.unwrap();
        let old_run = completed_run(&job, now - Duration::days(10));
        let recent_run = completed_run(&job, now);
        store.save_run(&old_run).await.unwrap();
        store.save_run(&recent_run).await.unwrap();
        let removed = store
            .prune_runs("cleanup", None, Some(now - Duration::days(7)))
            .await
            .unwrap();
        assert_eq!(removed, vec![old_run.run_id]);
        assert!(store
            .get_run("cleanup", &recent_run.run_id)
            .await
            .unwrap()
            .is_some());
        let deleted_run_ids = store.delete_job("cleanup").await.unwrap();
        assert_eq!(deleted_run_ids, vec![recent_run.run_id]);
        assert!(store
            .get_run("cleanup", &recent_run.run_id)
            .await
            .unwrap()
            .is_none());
    }
}
