use chrono::{TimeZone, Utc};
use my_supervisor_core::domain::{Job, JobId, JobTrigger};
use my_supervisor_core::ports::error::SchedulerError;
use my_supervisor_core::ports::Scheduler;
use my_supervisor_infra_scheduler::TokioScheduler;

fn job_with_trigger(trigger: JobTrigger) -> Job {
    Job {
        id: JobId::new(),
        name: "preview-fixture".into(),
        command: "unused".into(),
        args: Vec::new(),
        cwd: None,
        env: Default::default(),
        trigger,
        on_overlap: Default::default(),
        on_dependency_failure: Default::default(),
        timeout: None,
        log_retention: Default::default(),
        timezone: "Asia/Seoul".into(),
        schedule_revision: 0,
        trigger_id: uuid::Uuid::new_v4(),
        misfire_policy: Default::default(),
        retry_policy: Default::default(),
        admission: Default::default(),
    }
}

#[tokio::test]
async fn preview_rejects_invalid_cron_without_arming_a_timer() {
    let scheduler = TokioScheduler::new();
    let before = scheduler.snapshot().await.unwrap();
    let error = scheduler
        .preview(&job_with_trigger(JobTrigger::Cron("not cron".into())), Utc::now(), 3)
        .unwrap_err();
    assert!(matches!(error, SchedulerError::InvalidCron(_)));
    assert_eq!(scheduler.snapshot().await.unwrap(), before);
}

#[tokio::test]
async fn preview_keeps_timezone_and_valid_empty_results_distinct_from_errors() {
    let scheduler = TokioScheduler::new();
    let after = Utc.with_ymd_and_hms(2026, 10, 26, 0, 0, 0).unwrap();
    let dates = scheduler
        .preview(&job_with_trigger(JobTrigger::Cron("0 9 * * *".into())), after, 3)
        .unwrap();
    assert_eq!(dates.len(), 3);
    assert_eq!(dates[0], Utc.with_ymd_and_hms(2026, 10, 27, 0, 0, 0).unwrap());
    assert!(scheduler
        .preview(&job_with_trigger(JobTrigger::DependsOn(Vec::new())), after, 3)
        .unwrap()
        .is_empty());
    assert!(scheduler.snapshot().await.unwrap().entries.is_empty());
}
