//! Persistence representations of complex domain values (serialized as JSON
//! text columns). Local to this adapter — not the wire format.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use my_supervisor_core::domain::{JobRunId, JobTrigger, ScheduleOccurrence, TriggeredBy};

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerRepr {
    Cron { expr: String },
    Interval { every_sec: u64 },
    OneShot { at: DateTime<Utc> },
    DependsOn { jobs: Vec<String> },
}

impl From<&JobTrigger> for TriggerRepr {
    fn from(t: &JobTrigger) -> Self {
        match t {
            JobTrigger::Cron(expr) => TriggerRepr::Cron { expr: expr.clone() },
            JobTrigger::Interval(d) => TriggerRepr::Interval {
                every_sec: d.as_secs(),
            },
            JobTrigger::OneShot(at) => TriggerRepr::OneShot { at: *at },
            JobTrigger::DependsOn(jobs) => TriggerRepr::DependsOn { jobs: jobs.clone() },
        }
    }
}

impl From<TriggerRepr> for JobTrigger {
    fn from(r: TriggerRepr) -> Self {
        match r {
            TriggerRepr::Cron { expr } => JobTrigger::Cron(expr),
            TriggerRepr::Interval { every_sec } => {
                JobTrigger::Interval(Duration::from_secs(every_sec))
            }
            TriggerRepr::OneShot { at } => JobTrigger::OneShot(at),
            TriggerRepr::DependsOn { jobs } => JobTrigger::DependsOn(jobs),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggeredByRepr {
    Schedule,
    Scheduled {
        trigger_id: String,
        schedule_revision: u64,
        scheduled_at: DateTime<Utc>,
        attempt: u16,
    },
    Manual,
    Dependency {
        upstream_run_id: String,
    },
}

impl From<&TriggeredBy> for TriggeredByRepr {
    fn from(t: &TriggeredBy) -> Self {
        match t {
            TriggeredBy::Schedule => TriggeredByRepr::Schedule,
            TriggeredBy::Scheduled { occurrence } => TriggeredByRepr::Scheduled {
                trigger_id: occurrence.trigger_id.to_string(),
                schedule_revision: occurrence.schedule_revision,
                scheduled_at: occurrence.scheduled_at,
                attempt: occurrence.attempt,
            },
            TriggeredBy::Manual => TriggeredByRepr::Manual,
            TriggeredBy::Dependency { upstream_run_id } => TriggeredByRepr::Dependency {
                upstream_run_id: upstream_run_id.0.to_string(),
            },
        }
    }
}

impl TriggeredByRepr {
    pub fn into_domain(self) -> TriggeredBy {
        match self {
            TriggeredByRepr::Schedule => TriggeredBy::Schedule,
            TriggeredByRepr::Scheduled {
                trigger_id,
                schedule_revision,
                scheduled_at,
                attempt,
            } => TriggeredBy::Scheduled {
                occurrence: ScheduleOccurrence {
                    trigger_id: uuid::Uuid::parse_str(&trigger_id).unwrap_or_default(),
                    schedule_revision,
                    scheduled_at,
                    attempt,
                },
            },
            TriggeredByRepr::Manual => TriggeredBy::Manual,
            TriggeredByRepr::Dependency { upstream_run_id } => {
                let id = uuid::Uuid::parse_str(&upstream_run_id).unwrap_or_default();
                TriggeredBy::Dependency {
                    upstream_run_id: JobRunId(id),
                }
            }
        }
    }
}
