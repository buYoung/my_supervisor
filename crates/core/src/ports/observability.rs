use crate::domain::{
    AlertEpisode, AlertRule, DeliveryAttempt, DeliveryCandidate, DeliverySubmission, MetricSample,
    ObservabilityPage, OperatorEvent,
};
use crate::ports::error::RepoError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Bounded observability storage. These records are downstream evidence and
/// never grant authority to mutate process, job, occurrence, or run state.
#[async_trait]
pub trait ObservabilityRepository: Send + Sync {
    async fn upsert_alert_rule(&self, rule: &AlertRule) -> Result<(), RepoError>;
    async fn delete_alert_rule(
        &self,
        id: uuid::Uuid,
        deleted_at: DateTime<Utc>,
    ) -> Result<(), RepoError>;
    async fn list_alert_rules(&self, limit: usize) -> Result<Vec<AlertRule>, RepoError>;
    async fn record_event(&self, event: &OperatorEvent) -> Result<(), RepoError>;
    async fn list_events(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ObservabilityPage<OperatorEvent>, RepoError>;
    async fn record_metric(&self, sample: &MetricSample) -> Result<(), RepoError>;
    async fn maintain(&self, now: DateTime<Utc>) -> Result<(), RepoError>;
    async fn upsert_alert_episode(
        &self,
        episode: &AlertEpisode,
        dedupe_key: &str,
    ) -> Result<bool, RepoError>;
    async fn resolve_alert_episode(&self, episode: &AlertEpisode) -> Result<bool, RepoError>;
    async fn enqueue_delivery_candidate(
        &self,
        candidate: &DeliveryCandidate,
    ) -> Result<(), RepoError>;
    async fn claim_delivery_candidates(
        &self,
        owner: &str,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<DeliveryCandidate>, RepoError>;
    async fn finish_delivery_candidate(
        &self,
        candidate: &DeliveryCandidate,
        submission: &DeliverySubmission,
        occurred_at: DateTime<Utc>,
    ) -> Result<(), RepoError>;
    async fn cancel_delivery_candidates_for_alert(
        &self,
        alert_id: uuid::Uuid,
        occurred_at: DateTime<Utc>,
    ) -> Result<(), RepoError>;
    async fn list_metrics(
        &self,
        source: Option<&str>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ObservabilityPage<MetricSample>, RepoError>;
    async fn list_alerts(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ObservabilityPage<AlertEpisode>, RepoError>;
    async fn acknowledge_alert(&self, id: uuid::Uuid, at: DateTime<Utc>)
        -> Result<bool, RepoError>;
    async fn list_delivery_attempts(
        &self,
        alert_id: Option<uuid::Uuid>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ObservabilityPage<DeliveryAttempt>, RepoError>;
}

/// Local delivery is downstream of persisted observability evidence.  A
/// returned submission records adapter capability only; it never confirms
/// that an operating system displayed a notification.
#[async_trait]
pub trait AlertDelivery: Send + Sync {
    async fn submit(&self, candidate: &DeliveryCandidate) -> DeliverySubmission;
}
