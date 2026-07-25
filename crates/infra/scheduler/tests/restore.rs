use chrono::Utc;
use my_supervisor_core::domain::JobTrigger;
use my_supervisor_core::ports::{Scheduler, ScheduledJob, SchedulerSnapshot};
use my_supervisor_infra_scheduler::TokioScheduler;

#[tokio::test]
async fn restore_replaces_armed_triggers_with_snapshot() {
    let scheduler = TokioScheduler::new();
    let original = SchedulerSnapshot { entries: vec![ScheduledJob {
        name: "hourly".into(), trigger: JobTrigger::Interval(std::time::Duration::from_secs(3600)),
    }]};
    scheduler.restore(&original).await.unwrap();
    scheduler.register("temporary", &JobTrigger::Interval(std::time::Duration::from_secs(1))).await.unwrap();
    scheduler.restore(&original).await.unwrap();
    assert_eq!(scheduler.snapshot().await.unwrap(), original);
    assert_eq!(scheduler.next_run(&original.entries[0].trigger, Utc::now()).is_some(), true);
}
