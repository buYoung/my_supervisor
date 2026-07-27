//! Daemon-owned bounded telemetry and local-alert coordinator.
//!
//! This module intentionally observes committed/runtime state only.  It does
//! not participate in process, job, or scheduler authority and it exits on
//! the facade's existing shutdown notification.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use my_supervisor_core::domain::{
    AlertEpisode, AlertRule, AlertState, DeliveryCandidate, DeliverySubmission, MetricSample,
};
use my_supervisor_core::ports::AlertDelivery;
use tokio::time::{Instant, MissedTickBehavior};
use uuid::Uuid;

use crate::facade::OperationsFacade;

pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
const DELIVERY_LEASE: chrono::Duration = chrono::Duration::seconds(60);
const DELIVERY_BATCH: usize = 32;

/// Run one sampler/evaluator/delivery owner for a daemon session.  The caller
/// must await this before process teardown so the last completed submissions
/// are persisted; no worker owns or replaces a caller cancellation signal.
pub async fn run(facade: Arc<OperationsFacade>, delivery: Arc<dyn AlertDelivery>) {
    let mut cadence = tokio::time::interval_at(Instant::now() + SAMPLE_INTERVAL, SAMPLE_INTERVAL);
    cadence.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let owner = format!("daemon:{}:{}", std::process::id(), Uuid::new_v4());
    let shutdown = facade.shutdown_signal();
    loop {
        tokio::select! {
            _ = shutdown.notified() => break,
            _ = cadence.tick() => {
                let samples = facade.collect_observability_samples().await;
                evaluate_samples(&facade, &samples).await;
                deliver_claimed(&facade, &delivery, &owner).await;
                if let Err(error) = facade.maintain_observability(Utc::now()).await {
                    tracing::warn!(%error, "observability retention maintenance failed");
                }
            }
        }
    }
    // There is no in-memory aggregation to flush.  Claiming stops before
    // application shutdown reaps children; a persisted lease expires/reclaims
    // after restart if an adapter submission was interrupted.
}

async fn evaluate_samples(facade: &OperationsFacade, samples: &[MetricSample]) {
    let Ok(rules) = facade.list_alert_rules(1_000).await else {
        return;
    };
    for sample in samples {
        for rule in &rules {
            if !rule.enabled || !matches_rule(rule, sample) {
                continue;
            }
            let cause = normalized_cause(&rule.condition);
            let dedupe_key = format!("{}|{}|{}", rule.id, sample.source, cause);
            let episode = AlertEpisode {
                id: Uuid::new_v4(),
                rule_id: rule.id,
                source: sample.source.clone(),
                cause: cause.clone(),
                state: AlertState::Active,
                severity: rule.severity,
                opened_at: sample.occurred_at,
                resolved_at: None,
                acknowledged_at: None,
            };
            let Ok(created) = facade.persist_alert_episode(&episode, &dedupe_key).await else {
                continue;
            };
            if created {
                let payload = match serde_json::to_string(&episode) {
                    Ok(payload) => payload,
                    Err(_) => continue,
                };
                let candidate = DeliveryCandidate {
                    id: Uuid::new_v4(),
                    alert_id: episode.id,
                    kind: "notification".into(),
                    payload,
                    attempt_count: 0,
                    created_at: sample.occurred_at,
                    lease_owner: None,
                    lease_until: None,
                };
                let _ = facade.enqueue_alert_delivery(&candidate).await;
            }
        }
    }
    let by_source = samples
        .iter()
        .map(|sample| (sample.source.as_str(), sample))
        .collect::<HashMap<_, _>>();
    let Ok(episodes) = facade.list_alert_episodes(None, 500).await else {
        return;
    };
    for mut episode in episodes
        .records
        .into_iter()
        .filter(|episode| episode.state != AlertState::Resolved)
    {
        let Some(rule) = rules
            .iter()
            .find(|rule| rule.id == episode.rule_id && rule.enabled)
        else {
            continue;
        };
        let Some(sample) = by_source.get(episode.source.as_str()) else {
            continue;
        };
        if matches_rule(rule, sample) {
            continue;
        }
        episode.state = AlertState::Resolved;
        episode.resolved_at = Some(sample.occurred_at);
        if facade
            .resolve_alert_episode(&episode)
            .await
            .unwrap_or(false)
        {
            if let Ok(payload) = serde_json::to_string(&episode) {
                let candidate = DeliveryCandidate {
                    id: Uuid::new_v4(),
                    alert_id: episode.id,
                    kind: "notification".into(),
                    payload,
                    attempt_count: 0,
                    created_at: sample.occurred_at,
                    lease_owner: None,
                    lease_until: None,
                };
                let _ = facade.enqueue_alert_delivery(&candidate).await;
            }
        }
    }
}

fn matches_rule(rule: &AlertRule, sample: &MetricSample) -> bool {
    let Some((field, threshold)) = rule.condition.split_once('>') else {
        return false;
    };
    let Ok(threshold) = threshold.trim().parse::<f64>() else {
        return false;
    };
    match field.trim() {
        "cpu_percent" => sample.cpu_percent.is_some_and(|value| value > threshold),
        "memory_bytes" => sample
            .memory_bytes
            .is_some_and(|value| (value as f64) > threshold),
        _ => false,
    }
}

fn normalized_cause(condition: &str) -> String {
    condition
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

async fn deliver_claimed(
    facade: &OperationsFacade,
    delivery: &Arc<dyn AlertDelivery>,
    owner: &str,
) {
    let now = Utc::now();
    let Ok(candidates) = facade
        .claim_alert_deliveries(owner, now, now + DELIVERY_LEASE, DELIVERY_BATCH)
        .await
    else {
        return;
    };
    for candidate in candidates {
        let submission = delivery.submit(&candidate).await;
        let _ = facade
            .finish_alert_delivery(&candidate, &submission, Utc::now())
            .await;
    }
}

#[allow(dead_code)]
fn unavailable(detail: impl Into<String>) -> DeliverySubmission {
    DeliverySubmission {
        outcome: "unavailable".into(),
        detail: Some(detail.into()),
    }
}
