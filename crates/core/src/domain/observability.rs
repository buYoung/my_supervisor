//! Transport-independent, bounded observability records.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertState {
    Active,
    AcknowledgedActive,
    Resolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: Uuid,
    pub name: String,
    pub condition: String,
    pub severity: AlertSeverity,
    pub cooldown_seconds: u64,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorEvent {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub source: String,
    pub kind: String,
    pub severity: AlertSeverity,
    pub message: String,
    /// Stable source transition identity. Replays with the same key are one event.
    pub transition_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub source: String,
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub partial_bucket: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertEpisode {
    pub id: Uuid,
    pub rule_id: Uuid,
    pub source: String,
    pub cause: String,
    pub state: AlertState,
    pub severity: AlertSeverity,
    pub opened_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryAttempt {
    pub id: Uuid,
    pub alert_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub kind: String,
    pub outcome: String,
    pub detail: Option<String>,
    pub lease_until: Option<DateTime<Utc>>,
}

/// A daemon-owned, retryable delivery request.  It is intentionally separate
/// from an alert episode: episode creation is exactly-once, while a delivery
/// attempt is at-least-once across a daemon restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryCandidate {
    pub id: Uuid,
    pub alert_id: Uuid,
    pub kind: String,
    pub payload: String,
    pub attempt_count: u8,
    pub created_at: DateTime<Utc>,
    pub lease_owner: Option<String>,
    pub lease_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliverySubmission {
    pub outcome: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityPage<T> {
    pub records: Vec<T>,
    pub next_cursor: Option<String>,
    pub high_watermark: Option<String>,
    pub earliest_retained_at: Option<DateTime<Utc>>,
}
