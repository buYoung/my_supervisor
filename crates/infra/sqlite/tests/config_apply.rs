use std::collections::BTreeMap;

use chrono::Utc;
use my_supervisor_core::domain::{
    ApplyMode, ChildHandle, ConfigApplyJournal, ConfigApplyStage, ConfigDiff, ConfigSnapshot,
    ConfigTargetDirectStart, DependencyFailurePolicy, DependencySignature, Job, JobDeletionJournal,
    JobDeletionStage, JobId, JobRun, JobRunId, JobRunState, JobTrigger, LogRetention,
    OverlapPolicy, ProcessSpec, TriggeredBy,
};
use my_supervisor_core::ports::{
    CleanupTicket, JobRepository, StateRepository, TransientCleanupStage, TransientTerminalEvent,
};
use my_supervisor_infra_sqlite::SqliteStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::Row;

fn process(name: &str) -> ProcessSpec {
    ProcessSpec::new(name, "/bin/echo")
}

fn job(name: &str) -> Job {
    Job {
        id: JobId::new(),
        name: name.into(),
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
    }
}

#[tokio::test]
async fn config_snapshot_apply_and_journal_restore_are_atomic() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let previous = ConfigSnapshot {
        processes: vec![process("old")],
        jobs: vec![job("old-job")],
        running_direct_processes: vec![],
    };
    store.apply_config_snapshot(&previous).await.unwrap();
    let target = ConfigSnapshot {
        processes: vec![process("new")],
        jobs: vec![job("new-job")],
        running_direct_processes: vec![],
    };
    let apply_id = uuid::Uuid::new_v4();
    store
        .create_config_apply_journal(&ConfigApplyJournal {
            apply_id,
            previous: previous.clone(),
            target: target.clone(),
            diff: ConfigDiff::default(),
            stage: ConfigApplyStage::Prepared,
            compensation_error: None,
            target_direct_starts: Vec::new(),
        })
        .await
        .unwrap();
    store.apply_config_snapshot(&target).await.unwrap();
    assert!(store.get_spec("old").await.unwrap().is_none());
    assert!(store.get_job("new-job").await.unwrap().is_some());

    let restored = store.restore_config_apply_snapshot(apply_id).await.unwrap();
    assert_eq!(restored, previous);
    assert!(store.get_spec("old").await.unwrap().is_some());
    assert!(store.get_spec("new").await.unwrap().is_none());
    assert!(store.get_job("old-job").await.unwrap().is_some());
    assert!(store.get_job("new-job").await.unwrap().is_none());
    store.clear_config_apply_journal(apply_id).await.unwrap();
    assert!(store
        .list_incomplete_config_applies()
        .await
        .unwrap()
        .is_empty());
    let _ = ApplyMode::Merge;
}

#[tokio::test]
async fn config_target_start_intent_and_generation_survive_reopen() {
    let directory = std::env::temp_dir().join(format!(
        "my-supervisor-config-target-start-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("state.db");
    let target = process("target");
    let apply_id = uuid::Uuid::new_v4();
    let store = SqliteStore::connect(&database).await.unwrap();
    store
        .create_config_apply_journal(&ConfigApplyJournal {
            apply_id,
            previous: ConfigSnapshot {
                processes: vec![target.clone()],
                jobs: Vec::new(),
                running_direct_processes: vec![target.name.clone()],
            },
            target: ConfigSnapshot {
                processes: vec![target.clone()],
                jobs: Vec::new(),
                running_direct_processes: vec![target.name.clone()],
            },
            diff: ConfigDiff::default(),
            stage: ConfigApplyStage::ForwardRecovery,
            compensation_error: Some("target started before simulated crash".into()),
            target_direct_starts: Vec::new(),
        })
        .await
        .unwrap();
    store
        .record_config_target_direct_start(
            apply_id,
            &ConfigTargetDirectStart {
                name: target.name.clone(),
                spec: target.clone(),
                expected_generation: Some("macos-libproc:1:2".into()),
            },
        )
        .await
        .unwrap();
    drop(store);

    let reopened = SqliteStore::connect(&database).await.unwrap();
    let journal = reopened
        .list_incomplete_config_applies()
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        journal.target_direct_starts,
        vec![ConfigTargetDirectStart {
            name: target.name.clone(),
            spec: target,
            expected_generation: Some("macos-libproc:1:2".into()),
        }]
    );
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn dependency_signature_claim_creates_one_pending_run() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let downstream = job("downstream");
    store.save_job(&downstream).await.unwrap();
    let signature = DependencySignature {
        run_ids: vec![JobRunId::new()],
    };
    let run = JobRun {
        run_id: JobRunId::new(),
        job_name: downstream.name.clone(),
        job_id: downstream.id,
        triggered_by: TriggeredBy::Manual,
        scheduled_at: Utc::now(),
        started_at: None,
        ended_at: None,
        exit_code: None,
        state: JobRunState::Pending,
        occurrence: None,
        original_scheduled_at: None,
    };
    assert!(store
        .claim_dependency_run(&downstream.name, &signature, &run)
        .await
        .unwrap());
    assert!(!store
        .claim_dependency_run(&downstream.name, &signature, &run)
        .await
        .unwrap());
    assert_eq!(
        store.list_runs(&downstream.name, 10).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn transient_cleanup_ticket_is_durable_and_stage_updates_are_idempotent() {
    let directory = std::env::temp_dir().join(format!(
        "my-supervisor-terminal-outbox-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("state.db");
    let store = SqliteStore::connect(&database).await.unwrap();
    let parent = job("cleanup-parent");
    store.save_job(&parent).await.unwrap();
    let run = JobRun {
        run_id: JobRunId::new(),
        job_name: parent.name.clone(),
        job_id: parent.id,
        triggered_by: TriggeredBy::Manual,
        scheduled_at: Utc::now(),
        started_at: Some(Utc::now()),
        ended_at: None,
        exit_code: None,
        state: JobRunState::Running,
        occurrence: None,
        original_scheduled_at: None,
    };
    store.save_run(&run).await.unwrap();
    let ticket = CleanupTicket {
        cleanup_id: uuid::Uuid::new_v4(),
        job_id: parent.id,
        job_name: parent.name.clone(),
        run_id: run.run_id,
        child: ChildHandle {
            process_id: uuid::Uuid::new_v4(),
            pid: 4242,
            pgid: Some(4242),
            generation: Some("test-generation".into()),
            started_at: Utc::now(),
        },
        stage: TransientCleanupStage::JoinPumps,
        attempts: 0,
        last_error: Some("first failure".into()),
        intended_terminal_state: JobRunState::Cancelled,
        outcome: my_supervisor_core::ports::TransientOutcome {
            started_at: Utc::now(),
            ended_at: Utc::now(),
            exit_code: Some(23),
        },
    };
    store.enqueue_transient_cleanup(&ticket).await.unwrap();
    store.enqueue_transient_cleanup(&ticket).await.unwrap();
    let pending = store.pending_transient_cleanup(10).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].stage, TransientCleanupStage::JoinPumps);
    assert_eq!(pending[0].attempts, 1);
    store
        .update_transient_cleanup(&pending[0], TransientCleanupStage::PersistTerminal, None)
        .await
        .unwrap();
    let pending = store.pending_transient_cleanup(10).await.unwrap();
    assert_eq!(pending[0].stage, TransientCleanupStage::PersistTerminal);
    let mut terminal_run = run.clone();
    terminal_run.state = JobRunState::Cancelled;
    terminal_run.started_at = Some(ticket.outcome.started_at);
    terminal_run.ended_at = Some(ticket.outcome.ended_at);
    terminal_run.exit_code = ticket.outcome.exit_code;
    store
        .commit_transient_cleanup_terminal(&ticket, &terminal_run)
        .await
        .unwrap();
    let persisted_run = store
        .get_run(&parent.name, &run.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted_run.started_at, terminal_run.started_at);
    assert_eq!(persisted_run.ended_at, terminal_run.ended_at);
    assert_eq!(persisted_run.exit_code, Some(23));
    let events = store.pending_transient_terminal_events(10).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].state, JobRunState::Cancelled);
    assert_eq!(events[0].exit_code, Some(23));
    let event_id = events[0].event_id;
    let occurred_at = events[0].occurred_at;
    store.fail_next_transient_terminal_acknowledgements(1);
    assert!(store
        .acknowledge_transient_terminal_event(event_id, ticket.cleanup_id)
        .await
        .is_err());
    assert_eq!(store.pending_transient_cleanup(10).await.unwrap().len(), 1);
    assert_eq!(
        store
            .pending_transient_terminal_events(10)
            .await
            .unwrap()
            .len(),
        1
    );
    drop(store);

    // A process crash after a successful external write but before its ack
    // replays the exact same durable identity after the database reopens.
    let reopened = SqliteStore::connect(&database).await.unwrap();
    let replay = reopened
        .pending_transient_terminal_events(10)
        .await
        .unwrap();
    assert_eq!(replay.len(), 1);
    assert_eq!(replay[0].event_id, event_id);
    assert_eq!(replay[0].occurred_at, occurred_at);
    reopened
        .acknowledge_transient_terminal_event(event_id, ticket.cleanup_id)
        .await
        .unwrap();
    assert!(reopened
        .pending_transient_cleanup(10)
        .await
        .unwrap()
        .is_empty());
    assert!(reopened
        .pending_transient_terminal_events(10)
        .await
        .unwrap()
        .is_empty());
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn job_deletion_journal_survives_stage_updates_and_clears_after_completion() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let deleted_job = job("deletion-journal");
    store.save_job(&deleted_job).await.unwrap();
    let journal = JobDeletionJournal {
        deletion_id: uuid::Uuid::new_v4(),
        job: deleted_job.clone(),
        stage: JobDeletionStage::Prepared,
        run_ids: Vec::new(),
        last_error: None,
    };
    store.create_job_deletion_journal(&journal).await.unwrap();
    store.create_job_deletion_journal(&journal).await.unwrap();
    let persisted = store
        .get_job_deletion_journal(&deleted_job.name)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.deletion_id, journal.deletion_id);
    let run_id = JobRunId::new();
    store
        .update_job_deletion_journal(
            journal.deletion_id,
            JobDeletionStage::RowsDeleted,
            Some(&[run_id]),
            Some("db retry"),
        )
        .await
        .unwrap();
    let pending = store.list_incomplete_job_deletions().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].stage, JobDeletionStage::RowsDeleted);
    assert_eq!(pending[0].run_ids, vec![run_id]);
    assert_eq!(pending[0].last_error.as_deref(), Some("db retry"));
    store
        .clear_job_deletion_journal(journal.deletion_id)
        .await
        .unwrap();
    assert!(store
        .list_incomplete_job_deletions()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn job_row_deletion_commits_rows_log_cleanup_and_journal_together_across_reopen() {
    let directory = std::env::temp_dir().join(format!(
        "my-supervisor-deletion-commit-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("state.db");
    let store = SqliteStore::connect(&database).await.unwrap();
    let deleting_job = job("atomic-row-delete");
    let run_ids = [JobRunId::new(), JobRunId::new()];
    store.save_job(&deleting_job).await.unwrap();
    for run_id in run_ids {
        store
            .save_run(&JobRun {
                run_id,
                job_name: deleting_job.name.clone(),
                job_id: deleting_job.id,
                triggered_by: TriggeredBy::Manual,
                scheduled_at: Utc::now(),
                started_at: Some(Utc::now()),
                ended_at: Some(Utc::now()),
                exit_code: Some(0),
                state: JobRunState::Succeeded,
                occurrence: None,
                original_scheduled_at: None,
            })
            .await
            .unwrap();
    }
    let journal = JobDeletionJournal {
        deletion_id: uuid::Uuid::new_v4(),
        job: deleting_job.clone(),
        stage: JobDeletionStage::RunsDraining,
        run_ids: Vec::new(),
        last_error: None,
    };
    store.create_job_deletion_journal(&journal).await.unwrap();

    store.fail_next_job_deletion_row_commits(1);
    assert!(store
        .commit_job_deletion_rows(journal.deletion_id, &deleting_job.name)
        .await
        .is_err());
    assert!(store.get_job(&deleting_job.name).await.unwrap().is_some());
    assert_eq!(
        store.list_runs(&deleting_job.name, 10).await.unwrap().len(),
        run_ids.len()
    );
    assert_eq!(
        store
            .get_job_deletion_journal(&deleting_job.name)
            .await
            .unwrap()
            .unwrap()
            .stage,
        JobDeletionStage::RunsDraining,
    );
    assert!(store.pending_run_log_cleanup(10).await.unwrap().is_empty());

    let committed_run_ids = store
        .commit_job_deletion_rows(journal.deletion_id, &deleting_job.name)
        .await
        .unwrap();
    assert_eq!(committed_run_ids.len(), run_ids.len());
    drop(store);

    let reopened = SqliteStore::connect(&database).await.unwrap();
    assert!(reopened
        .get_job(&deleting_job.name)
        .await
        .unwrap()
        .is_none());
    assert!(reopened
        .list_runs(&deleting_job.name, 10)
        .await
        .unwrap()
        .is_empty());
    let persisted = reopened
        .get_job_deletion_journal(&deleting_job.name)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.stage, JobDeletionStage::RowsDeleted);
    assert_eq!(persisted.run_ids, committed_run_ids);
    let pending = reopened.pending_run_log_cleanup(10).await.unwrap();
    assert_eq!(pending.len(), run_ids.len());
    for cleanup in pending {
        reopened
            .complete_run_log_cleanup(cleanup.run_id)
            .await
            .unwrap();
    }
    reopened
        .clear_job_deletion_journal(journal.deletion_id)
        .await
        .unwrap();
    assert!(reopened
        .pending_run_log_cleanup(10)
        .await
        .unwrap()
        .is_empty());
    assert!(reopened
        .list_incomplete_job_deletions()
        .await
        .unwrap()
        .is_empty());
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn queued_job_deletion_cancellation_is_atomic_with_its_journal_boundary() {
    let directory = std::env::temp_dir().join(format!(
        "my-supervisor-delete-terminal-outbox-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("state.db");
    let store = SqliteStore::connect(&database).await.unwrap();
    let deleting_job = job("atomic-queued-cancellation");
    store.save_job(&deleting_job).await.unwrap();
    let queued_runs = [JobRunId::new(), JobRunId::new()];
    for run_id in queued_runs {
        store
            .save_run(&JobRun {
                run_id,
                job_name: deleting_job.name.clone(),
                job_id: deleting_job.id,
                triggered_by: TriggeredBy::Manual,
                scheduled_at: Utc::now(),
                started_at: None,
                ended_at: None,
                exit_code: None,
                state: JobRunState::Pending,
                occurrence: None,
                original_scheduled_at: None,
            })
            .await
            .unwrap();
    }
    let journal = JobDeletionJournal {
        deletion_id: uuid::Uuid::new_v4(),
        job: deleting_job.clone(),
        stage: JobDeletionStage::SchedulerUnregistered,
        run_ids: Vec::new(),
        last_error: None,
    };
    store.create_job_deletion_journal(&journal).await.unwrap();
    let terminal_events = queued_runs
        .iter()
        .map(|run_id| TransientTerminalEvent {
            cleanup_id: uuid::Uuid::new_v4(),
            event_id: uuid::Uuid::new_v4(),
            occurred_at: Utc::now(),
            job_name: deleting_job.name.clone(),
            run_id: *run_id,
            state: JobRunState::Cancelled,
            exit_code: None,
        })
        .collect::<Vec<_>>();

    store.fail_next_job_deletion_cancellations(1);
    assert!(store
        .cancel_queued_runs_for_job_deletion(
            journal.deletion_id,
            &deleting_job.name,
            &terminal_events,
        )
        .await
        .is_err());
    assert_eq!(
        store
            .get_job_deletion_journal(&deleting_job.name)
            .await
            .unwrap()
            .unwrap()
            .stage,
        JobDeletionStage::SchedulerUnregistered,
    );
    for run_id in queued_runs {
        assert_eq!(
            store
                .get_run(&deleting_job.name, &run_id)
                .await
                .unwrap()
                .unwrap()
                .state,
            JobRunState::Pending,
        );
    }
    assert!(store
        .pending_transient_terminal_events(10)
        .await
        .unwrap()
        .is_empty());
    drop(store);

    let reopened = SqliteStore::connect(&database).await.unwrap();
    reopened
        .cancel_queued_runs_for_job_deletion(
            journal.deletion_id,
            &deleting_job.name,
            &terminal_events,
        )
        .await
        .unwrap();
    assert_eq!(
        reopened
            .get_job_deletion_journal(&deleting_job.name)
            .await
            .unwrap()
            .unwrap()
            .stage,
        JobDeletionStage::CancellationStarted,
    );
    for run_id in queued_runs {
        let run = reopened
            .get_run(&deleting_job.name, &run_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(run.state, JobRunState::Cancelled);
        assert!(run.ended_at.is_some());
    }
    assert_eq!(
        reopened
            .pending_transient_terminal_events(10)
            .await
            .unwrap(),
        terminal_events
    );
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn migration_rebuilds_job_runs_when_job_id_exists_without_the_composite_fk() {
    let path = std::env::temp_dir().join(format!(
        "my-supervisor-fk-migration-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    sqlx::query("CREATE TABLE jobs (name TEXT PRIMARY KEY, id TEXT NOT NULL)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE job_runs (run_id TEXT PRIMARY KEY, job_name TEXT NOT NULL, job_id TEXT NOT NULL, triggered_by TEXT NOT NULL, scheduled_at TEXT NOT NULL, started_at TEXT, ended_at TEXT, exit_code INTEGER, state TEXT NOT NULL)").execute(&pool).await.unwrap();
    let job_id = JobId::new();
    sqlx::query("INSERT INTO jobs(name, id) VALUES ('legacy', ?)")
        .bind(job_id.0.to_string())
        .execute(&pool)
        .await
        .unwrap();
    let valid_run_id = JobRunId::new();
    let stale_run_id = JobRunId::new();
    for (run_id, run_job_id) in [(valid_run_id, job_id), (stale_run_id, JobId::new())] {
        sqlx::query("INSERT INTO job_runs(run_id, job_name, job_id, triggered_by, scheduled_at, state) VALUES (?, 'legacy', ?, ?, ?, 'pending')")
            .bind(run_id.0.to_string()).bind(run_job_id.0.to_string()).bind(r#"{"type":"manual"}"#).bind(Utc::now().to_rfc3339()).execute(&pool).await.unwrap();
    }
    pool.close().await;

    let store = SqliteStore::connect(&path).await.unwrap();
    assert!(store
        .get_run("legacy", &valid_run_id)
        .await
        .unwrap()
        .is_some());
    assert!(store
        .get_run("legacy", &stale_run_id)
        .await
        .unwrap()
        .is_none());
    drop(store);

    let inspected = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    let foreign_keys = sqlx::query("PRAGMA foreign_key_list(job_runs)")
        .fetch_all(&inspected)
        .await
        .unwrap();
    assert!(foreign_keys.iter().any(|row| {
        row.try_get::<String, _>("from").ok().as_deref() == Some("job_name")
            && row.try_get::<String, _>("to").ok().as_deref() == Some("name")
    }));
    assert!(foreign_keys.iter().any(|row| {
        row.try_get::<String, _>("from").ok().as_deref() == Some("job_id")
            && row.try_get::<String, _>("to").ok().as_deref() == Some("id")
    }));
    assert!(sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&inspected)
        .await
        .unwrap()
        .is_empty());
    inspected.close().await;
    tokio::fs::remove_file(path).await.unwrap();
}
