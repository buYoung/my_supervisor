//! `my-supervisor-infra-scheduler` — evaluates cron / interval / one-shot
//! triggers with `tokio::time` timers and queues fire events. `DependsOn`
//! triggers are not timed here; they are propagated by run-completion observers
//! (DD-028).

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::Cron;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use my_supervisor_core::domain::{Job, JobTrigger, ScheduleOccurrence};
use my_supervisor_core::ports::error::SchedulerError;
use my_supervisor_core::ports::scheduler::{
    ScheduleEvent, ScheduledJob, Scheduler, SchedulerSnapshot,
};

/// Cap a single sleep so far-future one-shots stay responsive to unregister.
const MAX_SLEEP: StdDuration = StdDuration::from_secs(3600);

pub struct TokioScheduler {
    event_sender: mpsc::UnboundedSender<ScheduleEvent>,
    event_receiver: Mutex<mpsc::UnboundedReceiver<ScheduleEvent>>,
    timers: StdMutex<HashMap<String, JoinHandle<()>>>,
    triggers: StdMutex<HashMap<String, JobTrigger>>,
}

impl Default for TokioScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl TokioScheduler {
    pub fn new() -> Self {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        TokioScheduler {
            event_sender,
            event_receiver: Mutex::new(event_receiver),
            timers: StdMutex::new(HashMap::new()),
            triggers: StdMutex::new(HashMap::new()),
        }
    }

    fn abort(&self, job_name: &str) {
        if let Some(handle) = self.timers.lock().unwrap().remove(job_name) {
            handle.abort();
        }
    }
}

/// Parse a 5-field cron expression.
fn parse_cron(expr: &str) -> Result<Cron, SchedulerError> {
    Cron::new(expr)
        .parse()
        .map_err(|e| SchedulerError::InvalidCron(format!("{e}")))
}

fn next_cron(cron: &Cron, after: DateTime<Utc>, timezone: &str) -> Option<DateTime<Utc>> {
    let timezone: Tz = timezone.parse().ok()?;
    cron.find_next_occurrence(&after.with_timezone(&timezone), false)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

/// Compute the next fire time for any trigger (pure).
fn next_for_in_timezone(
    trigger: &JobTrigger,
    after: DateTime<Utc>,
    timezone: &str,
) -> Option<DateTime<Utc>> {
    match trigger {
        JobTrigger::Cron(expr) => parse_cron(expr)
            .ok()
            .and_then(|c| next_cron(&c, after, timezone)),
        JobTrigger::Interval(dur) => chrono::Duration::from_std(*dur).ok().map(|d| after + d),
        JobTrigger::OneShot(at) => (*at > after).then_some(*at),
        JobTrigger::DependsOn(_) => None,
    }
}

fn next_for(trigger: &JobTrigger, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    next_for_in_timezone(trigger, after, "UTC")
}

/// Sleep until `target`, but no longer than `MAX_SLEEP` per hop so the timer
/// reacts to abort promptly. Returns once `now >= target`.
async fn sleep_until(target: DateTime<Utc>) {
    loop {
        let now = Utc::now();
        if now >= target {
            return;
        }
        let remaining = (target - now)
            .to_std()
            .unwrap_or(StdDuration::ZERO)
            .min(MAX_SLEEP);
        tokio::time::sleep(remaining).await;
    }
}

#[async_trait]
impl Scheduler for TokioScheduler {
    async fn register(&self, job_name: &str, trigger: &JobTrigger) -> Result<(), SchedulerError> {
        // Validate cron up front so registration fails fast on a bad expression.
        if let JobTrigger::Cron(expr) = trigger {
            parse_cron(expr)?;
        }
        self.abort(job_name);
        self.triggers
            .lock()
            .unwrap()
            .insert(job_name.to_string(), trigger.clone());

        if matches!(trigger, JobTrigger::DependsOn(_)) {
            return Ok(());
        }

        let event_sender = self.event_sender.clone();
        let name = job_name.to_string();
        let trigger = trigger.clone();
        let handle = tokio::spawn(async move {
            while let Some(next) = next_for(&trigger, Utc::now()) {
                sleep_until(next).await;
                if event_sender
                    .send(ScheduleEvent {
                        job_name: name.clone(),
                        scheduled_at: next,
                        occurrence: ScheduleOccurrence {
                            trigger_id: uuid::Uuid::nil(),
                            schedule_revision: 0,
                            scheduled_at: next,
                            attempt: 1,
                        },
                    })
                    .is_err()
                {
                    break;
                }
                if matches!(trigger, JobTrigger::OneShot(_)) {
                    break;
                }
            }
        });
        self.timers
            .lock()
            .unwrap()
            .insert(job_name.to_string(), handle);
        Ok(())
    }

    async fn register_job(&self, job: &Job) -> Result<(), SchedulerError> {
        if let JobTrigger::Cron(expr) = &job.trigger {
            parse_cron(expr)?;
            if job.timezone.parse::<Tz>().is_err() {
                return Err(SchedulerError::InvalidTimezone(job.timezone.clone()));
            }
        }
        self.abort(&job.name);
        self.triggers
            .lock()
            .unwrap()
            .insert(job.name.clone(), job.trigger.clone());
        if matches!(job.trigger, JobTrigger::DependsOn(_)) {
            return Ok(());
        }
        let event_sender = self.event_sender.clone();
        let name = job.name.clone();
        let trigger = job.trigger.clone();
        let timezone = job.timezone.clone();
        let trigger_id = job.trigger_id;
        let schedule_revision = job.schedule_revision;
        let handle = tokio::spawn(async move {
            while let Some(next) = next_for_in_timezone(&trigger, Utc::now(), &timezone) {
                sleep_until(next).await;
                let occurrence = ScheduleOccurrence {
                    trigger_id,
                    schedule_revision,
                    scheduled_at: next,
                    attempt: 1,
                };
                if event_sender
                    .send(ScheduleEvent {
                        job_name: name.clone(),
                        scheduled_at: next,
                        occurrence,
                    })
                    .is_err()
                {
                    break;
                }
                if matches!(trigger, JobTrigger::OneShot(_)) {
                    break;
                }
            }
        });
        self.timers.lock().unwrap().insert(job.name.clone(), handle);
        Ok(())
    }

    async fn unregister(&self, job_name: &str) -> Result<(), SchedulerError> {
        self.abort(job_name);
        self.triggers.lock().unwrap().remove(job_name);
        Ok(())
    }

    async fn snapshot(&self) -> Result<SchedulerSnapshot, SchedulerError> {
        let mut entries: Vec<ScheduledJob> = self
            .triggers
            .lock()
            .unwrap()
            .iter()
            .map(|(name, trigger)| ScheduledJob {
                name: name.clone(),
                trigger: trigger.clone(),
            })
            .collect();
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(SchedulerSnapshot { entries })
    }

    async fn restore(&self, snapshot: &SchedulerSnapshot) -> Result<(), SchedulerError> {
        let current: Vec<String> = self.triggers.lock().unwrap().keys().cloned().collect();
        for name in current {
            self.unregister(&name).await?;
        }
        for entry in &snapshot.entries {
            self.register(&entry.name, &entry.trigger).await?;
        }
        Ok(())
    }

    fn next_run(&self, trigger: &JobTrigger, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        next_for(trigger, after)
    }

    fn preview(
        &self,
        job: &Job,
        after: DateTime<Utc>,
        count: u16,
    ) -> Result<Vec<DateTime<Utc>>, SchedulerError> {
        if count > 100 {
            return Err(SchedulerError::PreviewBounded("count exceeds 100".into()));
        }
        if job.timezone.parse::<Tz>().is_err() {
            return Err(SchedulerError::InvalidTimezone(job.timezone.clone()));
        }
        let mut results = Vec::with_capacity(count as usize);
        let mut cursor = after;
        for _ in 0..count {
            let Some(next) = next_for_in_timezone(&job.trigger, cursor, &job.timezone) else {
                break;
            };
            if next - after > chrono::Duration::days(365 * 5) {
                return Err(SchedulerError::PreviewBounded(
                    "search horizon exceeds 5 years".into(),
                ));
            }
            results.push(next);
            cursor = next;
        }
        Ok(results)
    }

    async fn next_event(&self) -> Option<ScheduleEvent> {
        self.event_receiver.lock().await.recv().await
    }
}
