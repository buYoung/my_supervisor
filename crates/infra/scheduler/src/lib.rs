//! `my-supervisor-infra-scheduler` — evaluates cron / interval / one-shot
//! triggers with `tokio::time` timers and broadcasts fire events. `DependsOn`
//! triggers are not timed here; they are propagated by run-completion observers
//! (DD-028).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use croner::Cron;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use my_supervisor_core::domain::JobTrigger;
use my_supervisor_core::ports::error::SchedulerError;
use my_supervisor_core::ports::scheduler::{ScheduleEvent, Scheduler};

const EVENT_CAPACITY: usize = 256;
/// Cap a single sleep so far-future one-shots stay responsive to unregister.
const MAX_SLEEP: StdDuration = StdDuration::from_secs(3600);

pub struct TokioScheduler {
    tx: broadcast::Sender<ScheduleEvent>,
    timers: Mutex<HashMap<String, JoinHandle<()>>>,
}

impl Default for TokioScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl TokioScheduler {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(EVENT_CAPACITY);
        TokioScheduler {
            tx,
            timers: Mutex::new(HashMap::new()),
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

fn next_cron(cron: &Cron, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    cron.find_next_occurrence(&after, false).ok()
}

/// Compute the next fire time for any trigger (pure).
fn next_for(trigger: &JobTrigger, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match trigger {
        JobTrigger::Cron(expr) => parse_cron(expr).ok().and_then(|c| next_cron(&c, after)),
        JobTrigger::Interval(dur) => {
            chrono::Duration::from_std(*dur).ok().map(|d| after + d)
        }
        JobTrigger::OneShot(at) => (*at > after).then_some(*at),
        JobTrigger::DependsOn(_) => None,
    }
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

        if matches!(trigger, JobTrigger::DependsOn(_)) {
            return Ok(());
        }

        let tx = self.tx.clone();
        let name = job_name.to_string();
        let trigger = trigger.clone();
        let handle = tokio::spawn(async move {
            loop {
                let Some(next) = next_for(&trigger, Utc::now()) else {
                    break;
                };
                sleep_until(next).await;
                let _ = tx.send(ScheduleEvent {
                    job_name: name.clone(),
                    scheduled_at: next,
                });
                if matches!(trigger, JobTrigger::OneShot(_)) {
                    break;
                }
            }
        });
        self.timers.lock().unwrap().insert(job_name.to_string(), handle);
        Ok(())
    }

    async fn unregister(&self, job_name: &str) -> Result<(), SchedulerError> {
        self.abort(job_name);
        Ok(())
    }

    fn next_run(&self, trigger: &JobTrigger, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        next_for(trigger, after)
    }

    fn subscribe(&self) -> broadcast::Receiver<ScheduleEvent> {
        self.tx.subscribe()
    }
}
