use chrono::{Duration, Utc};
use my_supervisor_core::domain::{
    DependencyFailurePolicy, Job, JobId, JobRun, JobRunId, JobRunState, JobTrigger, LogRetention,
    OverlapPolicy, TriggeredBy,
};
use my_supervisor_core::ports::{JobRepository, StateRepository};
use my_supervisor_infra_sqlite::SqliteStore;
use std::collections::BTreeMap;

fn job() -> Job {
    Job { id: JobId::new(), name: "cleanup".into(), command: "/bin/true".into(), args: vec![], cwd: None, env: BTreeMap::new(), trigger: JobTrigger::Interval(std::time::Duration::from_secs(60)), on_overlap: OverlapPolicy::Skip, on_dependency_failure: DependencyFailurePolicy::Skip, timeout: None, log_retention: LogRetention::default() }
}

#[tokio::test]
async fn deleting_run_history_commits_cleanup_queue_with_the_row_delete() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let job = job();
    store.save_job(&job).await.unwrap();
    let run_id = JobRunId::new();
    store.save_run(&JobRun { run_id, job_name: job.name.clone(), job_id: job.id, triggered_by: TriggeredBy::Manual, scheduled_at: Utc::now() - Duration::days(2), started_at: None, ended_at: Some(Utc::now() - Duration::days(2)), exit_code: Some(0), state: JobRunState::Succeeded }).await.unwrap();
    assert_eq!(store.prune_runs(&job.name, None, Some(Utc::now())).await.unwrap(), vec![run_id]);
    assert!(store.get_run(&job.name, &run_id).await.unwrap().is_none());
    assert_eq!(store.pending_run_log_cleanup(10).await.unwrap()[0].run_id, run_id);
    store.fail_run_log_cleanup(run_id, "permission denied").await.unwrap();
    assert_eq!(store.pending_run_log_cleanup(10).await.unwrap()[0].attempts, 1);
    store.complete_run_log_cleanup(run_id).await.unwrap();
    assert!(store.pending_run_log_cleanup(10).await.unwrap().is_empty());
}

#[tokio::test]
async fn run_filters_are_applied_before_the_limit() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let job = job();
    store.save_job(&job).await.unwrap();
    let base = Utc::now();
    for offset in 0..3 {
        let state = if offset == 2 { JobRunState::Succeeded } else { JobRunState::Failed };
        store.save_run(&JobRun {
            run_id: JobRunId::new(), job_name: job.name.clone(), job_id: job.id,
            triggered_by: TriggeredBy::Manual, scheduled_at: base + Duration::seconds(offset),
            started_at: None, ended_at: Some(base + Duration::seconds(offset)), exit_code: Some(0), state,
        }).await.unwrap();
    }
    let runs = store.list_runs_filtered(&job.name, Some(JobRunState::Failed), None, 2).await.unwrap();
    assert_eq!(runs.len(), 2);
    assert!(runs.iter().all(|run| run.state == JobRunState::Failed));
}

#[tokio::test]
async fn fresh_schema_rejects_orphan_runs_and_stale_run_updates() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let persisted_job = job();
    store.save_job(&persisted_job).await.unwrap();
    let orphan = JobRun {
        run_id: JobRunId::new(),
        job_name: persisted_job.name.clone(),
        job_id: JobId::new(),
        triggered_by: TriggeredBy::Manual,
        scheduled_at: Utc::now(),
        started_at: None,
        ended_at: None,
        exit_code: None,
        state: JobRunState::Pending,
    };
    assert!(matches!(store.save_run(&orphan).await, Err(my_supervisor_core::ports::error::RepoError::Conflict(_))));

    let mut persisted_run = orphan.clone();
    persisted_run.run_id = JobRunId::new();
    persisted_run.job_id = persisted_job.id;
    store.save_run(&persisted_run).await.unwrap();
    persisted_run.job_id = JobId::new();
    persisted_run.state = JobRunState::Cancelled;
    assert!(matches!(store.save_run(&persisted_run).await, Err(my_supervisor_core::ports::error::RepoError::Conflict(_))));
}

#[tokio::test]
async fn runtime_handle_cleanup_preserves_a_replacement_generation() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let spec = my_supervisor_core::domain::ProcessSpec::new("runtime-cleanup", "/bin/true");
    store.save_spec(&spec).await.unwrap();
    let old_handle = my_supervisor_core::domain::ChildHandle {
        process_id: uuid::Uuid::new_v4(), pid: 101, pgid: Some(101),
        generation: Some("macos-libproc:10:1".into()), started_at: Utc::now(),
    };
    let new_handle = my_supervisor_core::domain::ChildHandle {
        process_id: uuid::Uuid::new_v4(), pid: 102, pgid: Some(102),
        generation: Some("macos-libproc:10:2".into()), started_at: Utc::now(),
    };
    store.set_runtime_handle(&spec.name, Some(&old_handle)).await.unwrap();
    store.enqueue_runtime_handle_cleanup(&spec.name, &old_handle, "injected write failure").await.unwrap();
    store.set_runtime_handle(&spec.name, Some(&new_handle)).await.unwrap();
    let cleanup = store.pending_runtime_handle_cleanup(1).await.unwrap().pop().unwrap();
    assert!(!store.clear_runtime_handle_if_matches(&cleanup).await.unwrap());
    assert_eq!(store.get_runtime_handle(&spec.name).await.unwrap(), Some(new_handle));
}
