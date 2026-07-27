use async_trait::async_trait;
use std::collections::HashMap;

use crate::{backend, dt_to_str, SqliteStore};
use chrono::{DateTime, Timelike, Utc};
use my_supervisor_core::domain::{
    AlertEpisode, AlertRule, AlertState, DeliveryAttempt, DeliveryCandidate, DeliverySubmission,
    MetricSample, ObservabilityPage, OperatorEvent,
};
use my_supervisor_core::ports::{ObservabilityRepository, RepoError};
use sqlx::{Row, Sqlite, Transaction};

const MAX_PAGE: usize = 500;
const RAW_METRIC_RETENTION: chrono::Duration = chrono::Duration::hours(24);
const MINUTE_METRIC_RETENTION: chrono::Duration = chrono::Duration::days(30);
const COMPLETED_RECORD_RETENTION: chrono::Duration = chrono::Duration::days(30);
const COMPLETED_RECORD_LIMIT: i64 = 10_000;

fn page_limit(limit: usize) -> usize {
    limit.clamp(1, MAX_PAGE)
}
fn cursor(cursor: Option<&str>) -> Result<Option<(String, String)>, RepoError> {
    cursor
        .map(|value| {
            value
                .split_once('|')
                .map(|(at, id)| (at.to_owned(), id.to_owned()))
                .ok_or_else(|| {
                    RepoError::Backend("observability cursor is invalid or expired".into())
                })
        })
        .transpose()
}
fn page<T>(
    records: Vec<T>,
    keys: Vec<(String, String)>,
    earliest: Option<DateTime<Utc>>,
) -> ObservabilityPage<T> {
    let high_watermark = keys.last().map(|(at, id)| format!("{at}|{id}"));
    ObservabilityPage {
        records,
        next_cursor: high_watermark.clone(),
        high_watermark,
        earliest_retained_at: earliest,
    }
}
fn parse_datetime(value: String) -> Result<DateTime<Utc>, RepoError> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(backend)
}

pub(crate) async fn insert_operator_event(
    transaction: &mut Transaction<'_, Sqlite>,
    occurred_at: DateTime<Utc>,
    source: String,
    kind: String,
    severity: my_supervisor_core::domain::AlertSeverity,
    message: String,
    transition_key: String,
) -> Result<(), RepoError> {
    let event = OperatorEvent {
        id: uuid::Uuid::new_v4(),
        occurred_at,
        source,
        kind,
        severity,
        message,
        transition_key,
    };
    sqlx::query("INSERT INTO observability_events(id, occurred_at, transition_key, payload) VALUES(?, ?, ?, ?) ON CONFLICT(transition_key) DO NOTHING")
        .bind(event.id.to_string()).bind(dt_to_str(&event.occurred_at)).bind(&event.transition_key)
        .bind(serde_json::to_string(&event).map_err(backend)?).execute(&mut **transaction).await.map_err(backend)?;
    Ok(())
}

#[derive(Default)]
struct MinuteMetric {
    cpu_sum: f64,
    cpu_count: u64,
    memory_sum: u128,
    memory_count: u64,
    partial_bucket: bool,
}

impl MinuteMetric {
    fn add(&mut self, sample: &MetricSample) {
        if let Some(cpu) = sample.cpu_percent {
            self.cpu_sum += cpu;
            self.cpu_count += 1;
        }
        if let Some(memory) = sample.memory_bytes {
            self.memory_sum = self.memory_sum.saturating_add(u128::from(memory));
            self.memory_count += 1;
        }
        self.partial_bucket |=
            sample.partial_bucket || sample.cpu_percent.is_none() || sample.memory_bytes.is_none();
    }
}

#[async_trait]
impl ObservabilityRepository for SqliteStore {
    async fn upsert_alert_rule(&self, rule: &AlertRule) -> Result<(), RepoError> {
        sqlx::query("INSERT INTO observability_rules(id, payload, deleted_at) VALUES(?, ?, NULL) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload, deleted_at=NULL")
            .bind(rule.id.to_string()).bind(serde_json::to_string(rule).map_err(backend)?).execute(&self.pool).await.map_err(backend)?;
        Ok(())
    }
    async fn delete_alert_rule(
        &self,
        id: uuid::Uuid,
        deleted_at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        sqlx::query("UPDATE observability_rules SET deleted_at=? WHERE id=?")
            .bind(dt_to_str(&deleted_at))
            .bind(id.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        let rows = sqlx::query("SELECT id,payload FROM observability_alerts")
            .fetch_all(&mut *transaction)
            .await
            .map_err(backend)?;
        for row in rows {
            let episode: AlertEpisode =
                serde_json::from_str(&row.try_get::<String, _>("payload").map_err(backend)?)
                    .map_err(backend)?;
            if episode.rule_id != id {
                continue;
            }
            let attempt = DeliveryAttempt {
                id: uuid::Uuid::new_v4(),
                alert_id: episode.id,
                occurred_at: deleted_at,
                kind: "notification".into(),
                outcome: "cancelled".into(),
                detail: Some("alert rule was deleted".into()),
                lease_until: None,
            };
            sqlx::query("INSERT INTO observability_delivery_attempts(id,alert_id,occurred_at,payload) VALUES(?,?,?,?)").bind(attempt.id.to_string()).bind(attempt.alert_id.to_string()).bind(dt_to_str(&attempt.occurred_at)).bind(serde_json::to_string(&attempt).map_err(backend)?).execute(&mut *transaction).await.map_err(backend)?;
            sqlx::query("DELETE FROM observability_delivery_candidates WHERE alert_id=?")
                .bind(episode.id.to_string())
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
        }
        transaction.commit().await.map_err(backend)?;
        Ok(())
    }
    async fn list_alert_rules(&self, limit: usize) -> Result<Vec<AlertRule>, RepoError> {
        sqlx::query(
            "SELECT payload FROM observability_rules WHERE deleted_at IS NULL ORDER BY id LIMIT ?",
        )
        .bind(page_limit(limit) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?
        .into_iter()
        .map(|r| {
            serde_json::from_str(&r.try_get::<String, _>("payload").map_err(backend)?)
                .map_err(backend)
        })
        .collect()
    }
    async fn record_event(&self, event: &OperatorEvent) -> Result<(), RepoError> {
        sqlx::query("INSERT INTO observability_events(id, occurred_at, transition_key, payload) VALUES(?, ?, ?, ?) ON CONFLICT(transition_key) DO NOTHING").bind(event.id.to_string()).bind(dt_to_str(&event.occurred_at)).bind(&event.transition_key).bind(serde_json::to_string(event).map_err(backend)?).execute(&self.pool).await.map_err(backend)?;
        Ok(())
    }
    async fn list_events(
        &self,
        after: Option<&str>,
        limit: usize,
    ) -> Result<ObservabilityPage<OperatorEvent>, RepoError> {
        let after = cursor(after)?;
        let rows = if let Some((at,id)) = after { sqlx::query("SELECT occurred_at,id,payload FROM observability_events WHERE (occurred_at > ? OR (occurred_at = ? AND id > ?)) ORDER BY occurred_at,id LIMIT ?").bind(&at).bind(&at).bind(id).bind(page_limit(limit) as i64).fetch_all(&self.pool).await } else { sqlx::query("SELECT occurred_at,id,payload FROM observability_events ORDER BY occurred_at,id LIMIT ?").bind(page_limit(limit) as i64).fetch_all(&self.pool).await }.map_err(backend)?;
        let keys = rows
            .iter()
            .map(|r| {
                Ok((
                    r.try_get("occurred_at").map_err(backend)?,
                    r.try_get("id").map_err(backend)?,
                ))
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        let records = rows
            .into_iter()
            .map(|r| {
                let payload: String = r.try_get("payload").map_err(backend)?;
                serde_json::from_str(&payload).map_err(backend)
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        page_earliest(self, "observability_events", "occurred_at", records, keys).await
    }
    async fn record_metric(&self, sample: &MetricSample) -> Result<(), RepoError> {
        sqlx::query("INSERT OR IGNORE INTO observability_metrics(id,occurred_at,source,payload) VALUES(?,?,?,?)").bind(sample.id.to_string()).bind(dt_to_str(&sample.occurred_at)).bind(&sample.source).bind(serde_json::to_string(sample).map_err(backend)?).execute(&self.pool).await.map_err(backend)?;
        Ok(())
    }
    async fn maintain(&self, now: DateTime<Utc>) -> Result<(), RepoError> {
        let raw_cutoff = now - RAW_METRIC_RETENTION;
        let sealed_cutoff = raw_cutoff
            .with_second(0)
            .and_then(|value| value.with_nanosecond(0))
            .ok_or_else(|| {
                RepoError::Backend("could not align observability retention cutoff".into())
            })?;
        let minute_cutoff = now - MINUTE_METRIC_RETENTION;
        let completed_cutoff = now - COMPLETED_RECORD_RETENTION;
        let mut transaction = self.pool.begin().await.map_err(backend)?;

        // A bucket is inserted only after its UTC minute is sealed.  Raw rows
        // remain until the next minute boundary, so a partial cutoff never
        // overwrites or loses a previously sealed aggregate.
        let rows = sqlx::query(
            "SELECT occurred_at,source,payload FROM observability_metrics WHERE occurred_at < ?",
        )
        .bind(dt_to_str(&sealed_cutoff))
        .fetch_all(&mut *transaction)
        .await
        .map_err(backend)?;
        let mut minutes = HashMap::<(String, DateTime<Utc>), MinuteMetric>::new();
        for row in rows {
            let occurred_at = parse_datetime(row.try_get("occurred_at").map_err(backend)?)?;
            let bucket_start = occurred_at
                .with_second(0)
                .and_then(|value| value.with_nanosecond(0))
                .ok_or_else(|| RepoError::Backend("could not align metric bucket".into()))?;
            let sample: MetricSample =
                serde_json::from_str(&row.try_get::<String, _>("payload").map_err(backend)?)
                    .map_err(backend)?;
            minutes
                .entry((row.try_get("source").map_err(backend)?, bucket_start))
                .or_default()
                .add(&sample);
        }
        for ((source, bucket_start), metric) in minutes {
            let aggregate = MetricSample {
                id: uuid::Uuid::new_v4(),
                occurred_at: bucket_start,
                source: source.clone(),
                cpu_percent: (metric.cpu_count > 0)
                    .then_some(metric.cpu_sum / metric.cpu_count as f64),
                memory_bytes: (metric.memory_count > 0)
                    .then_some((metric.memory_sum / u128::from(metric.memory_count)) as u64),
                partial_bucket: metric.partial_bucket,
            };
            sqlx::query("INSERT INTO observability_metric_minutes(bucket_start,source,payload) VALUES(?,?,?) ON CONFLICT(bucket_start,source) DO NOTHING")
                .bind(dt_to_str(&bucket_start)).bind(source).bind(serde_json::to_string(&aggregate).map_err(backend)?)
                .execute(&mut *transaction).await.map_err(backend)?;
        }
        sqlx::query("DELETE FROM observability_metrics WHERE occurred_at < ?")
            .bind(dt_to_str(&sealed_cutoff))
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        sqlx::query("DELETE FROM observability_metric_minutes WHERE bucket_start < ?")
            .bind(dt_to_str(&minute_cutoff))
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;

        sqlx::query("DELETE FROM observability_events WHERE occurred_at < ?")
            .bind(dt_to_str(&completed_cutoff))
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        sqlx::query("DELETE FROM observability_events WHERE id IN (SELECT id FROM observability_events ORDER BY occurred_at DESC,id DESC LIMIT -1 OFFSET ?)").bind(COMPLETED_RECORD_LIMIT).execute(&mut *transaction).await.map_err(backend)?;
        sqlx::query("DELETE FROM observability_delivery_attempts WHERE occurred_at < ?")
            .bind(dt_to_str(&completed_cutoff))
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        sqlx::query("DELETE FROM observability_delivery_attempts WHERE id IN (SELECT id FROM observability_delivery_attempts ORDER BY occurred_at DESC,id DESC LIMIT -1 OFFSET ?)").bind(COMPLETED_RECORD_LIMIT).execute(&mut *transaction).await.map_err(backend)?;
        sqlx::query("DELETE FROM observability_alerts WHERE payload LIKE '%\"state\":\"Resolved\"%' AND opened_at < ?").bind(dt_to_str(&completed_cutoff)).execute(&mut *transaction).await.map_err(backend)?;
        sqlx::query("DELETE FROM observability_alerts WHERE id IN (SELECT id FROM observability_alerts WHERE payload LIKE '%\"state\":\"Resolved\"%' ORDER BY opened_at DESC,id DESC LIMIT -1 OFFSET ?)").bind(COMPLETED_RECORD_LIMIT).execute(&mut *transaction).await.map_err(backend)?;
        sqlx::query("DELETE FROM observability_episode_keys WHERE alert_id NOT IN (SELECT id FROM observability_alerts)").execute(&mut *transaction).await.map_err(backend)?;

        transaction.commit().await.map_err(backend)?;
        Ok(())
    }
    async fn upsert_alert_episode(
        &self,
        episode: &AlertEpisode,
        dedupe_key: &str,
    ) -> Result<bool, RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let inserted = sqlx::query("INSERT INTO observability_episode_keys(dedupe_key,alert_id) VALUES(?,?) ON CONFLICT(dedupe_key) DO NOTHING")
            .bind(dedupe_key).bind(episode.id.to_string()).execute(&mut *transaction).await.map_err(backend)?.rows_affected() == 1;
        if inserted {
            sqlx::query("INSERT INTO observability_alerts(id,opened_at,payload) VALUES(?,?,?)")
                .bind(episode.id.to_string())
                .bind(dt_to_str(&episode.opened_at))
                .bind(serde_json::to_string(episode).map_err(backend)?)
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
        }
        transaction.commit().await.map_err(backend)?;
        Ok(inserted)
    }
    async fn resolve_alert_episode(&self, episode: &AlertEpisode) -> Result<bool, RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let updated = sqlx::query("UPDATE observability_alerts SET payload=? WHERE id=?")
            .bind(serde_json::to_string(episode).map_err(backend)?)
            .bind(episode.id.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(backend)?
            .rows_affected()
            == 1;
        if updated {
            sqlx::query("DELETE FROM observability_episode_keys WHERE alert_id=?")
                .bind(episode.id.to_string())
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
        }
        transaction.commit().await.map_err(backend)?;
        Ok(updated)
    }
    async fn enqueue_delivery_candidate(
        &self,
        candidate: &DeliveryCandidate,
    ) -> Result<(), RepoError> {
        sqlx::query("INSERT INTO observability_delivery_candidates(id,alert_id,kind,payload,attempt_count,created_at,lease_owner,lease_until) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING")
            .bind(candidate.id.to_string()).bind(candidate.alert_id.to_string()).bind(&candidate.kind).bind(&candidate.payload).bind(candidate.attempt_count as i64).bind(dt_to_str(&candidate.created_at)).bind(candidate.lease_owner.as_deref()).bind(candidate.lease_until.as_ref().map(dt_to_str))
            .execute(&self.pool).await.map_err(backend)?;
        Ok(())
    }
    async fn claim_delivery_candidates(
        &self,
        owner: &str,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<DeliveryCandidate>, RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let rows = sqlx::query("SELECT id,alert_id,kind,payload,attempt_count,created_at FROM observability_delivery_candidates WHERE lease_until IS NULL OR lease_until <= ? ORDER BY created_at,id LIMIT ?")
            .bind(dt_to_str(&now)).bind(page_limit(limit.min(10_000)) as i64).fetch_all(&mut *transaction).await.map_err(backend)?;
        let mut candidates = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("id").map_err(backend)?;
            let alert_id: String = row.try_get("alert_id").map_err(backend)?;
            let changed = sqlx::query("UPDATE observability_delivery_candidates SET lease_owner=?,lease_until=?,attempt_count=attempt_count+1 WHERE id=? AND (lease_until IS NULL OR lease_until <= ?)")
                .bind(owner).bind(dt_to_str(&lease_until)).bind(&id).bind(dt_to_str(&now)).execute(&mut *transaction).await.map_err(backend)?.rows_affected();
            if changed == 1 {
                candidates.push(DeliveryCandidate {
                    id: uuid::Uuid::parse_str(&id).map_err(backend)?,
                    alert_id: uuid::Uuid::parse_str(&alert_id).map_err(backend)?,
                    kind: row.try_get("kind").map_err(backend)?,
                    payload: row.try_get("payload").map_err(backend)?,
                    attempt_count: row
                        .try_get::<i64, _>("attempt_count")
                        .map_err(backend)?
                        .saturating_add(1) as u8,
                    created_at: parse_datetime(row.try_get("created_at").map_err(backend)?)?,
                    lease_owner: Some(owner.to_owned()),
                    lease_until: Some(lease_until),
                });
            }
        }
        transaction.commit().await.map_err(backend)?;
        Ok(candidates)
    }
    async fn finish_delivery_candidate(
        &self,
        candidate: &DeliveryCandidate,
        submission: &DeliverySubmission,
        occurred_at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        let attempt = DeliveryAttempt {
            id: uuid::Uuid::new_v4(),
            alert_id: candidate.alert_id,
            occurred_at,
            kind: candidate.kind.clone(),
            outcome: submission.outcome.clone(),
            detail: submission.detail.clone(),
            lease_until: candidate.lease_until,
        };
        sqlx::query("INSERT INTO observability_delivery_attempts(id,alert_id,occurred_at,payload) VALUES(?,?,?,?)")
            .bind(attempt.id.to_string()).bind(attempt.alert_id.to_string()).bind(dt_to_str(&attempt.occurred_at)).bind(serde_json::to_string(&attempt).map_err(backend)?)
            .execute(&mut *transaction).await.map_err(backend)?;
        if submission.outcome == "failed" && candidate.attempt_count < 3 {
            let retry_at = occurred_at
                + chrono::Duration::seconds(5_i64.saturating_pow(candidate.attempt_count as u32));
            sqlx::query("UPDATE observability_delivery_candidates SET lease_owner=NULL,lease_until=? WHERE id=?")
                .bind(dt_to_str(&retry_at)).bind(candidate.id.to_string()).execute(&mut *transaction).await.map_err(backend)?;
        } else {
            sqlx::query("DELETE FROM observability_delivery_candidates WHERE id=?")
                .bind(candidate.id.to_string())
                .execute(&mut *transaction)
                .await
                .map_err(backend)?;
        }
        transaction.commit().await.map_err(backend)?;
        Ok(())
    }
    async fn cancel_delivery_candidates_for_alert(
        &self,
        alert_id: uuid::Uuid,
        occurred_at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        let rows =
            sqlx::query("SELECT id,kind FROM observability_delivery_candidates WHERE alert_id=?")
                .bind(alert_id.to_string())
                .fetch_all(&self.pool)
                .await
                .map_err(backend)?;
        for row in rows {
            let attempt = DeliveryAttempt {
                id: uuid::Uuid::new_v4(),
                alert_id,
                occurred_at,
                kind: row.try_get("kind").map_err(backend)?,
                outcome: "cancelled".into(),
                detail: Some("alert rule was deleted".into()),
                lease_until: None,
            };
            sqlx::query("INSERT INTO observability_delivery_attempts(id,alert_id,occurred_at,payload) VALUES(?,?,?,?)").bind(attempt.id.to_string()).bind(alert_id.to_string()).bind(dt_to_str(&occurred_at)).bind(serde_json::to_string(&attempt).map_err(backend)?).execute(&self.pool).await.map_err(backend)?;
        }
        sqlx::query("DELETE FROM observability_delivery_candidates WHERE alert_id=?")
            .bind(alert_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }
    async fn list_metrics(
        &self,
        source: Option<&str>,
        after: Option<&str>,
        limit: usize,
    ) -> Result<ObservabilityPage<MetricSample>, RepoError> {
        let after = cursor(after)?;
        let rows = match (source, after) {
            (Some(source), Some((at, id))) => sqlx::query("SELECT occurred_at,id,payload FROM observability_metrics WHERE source=? AND (occurred_at>? OR (occurred_at=? AND id>?)) ORDER BY occurred_at,id LIMIT ?").bind(source).bind(&at).bind(&at).bind(id).bind(page_limit(limit) as i64).fetch_all(&self.pool).await,
            (Some(source), None) => sqlx::query("SELECT occurred_at,id,payload FROM observability_metrics WHERE source=? ORDER BY occurred_at,id LIMIT ?").bind(source).bind(page_limit(limit) as i64).fetch_all(&self.pool).await,
            (None, Some((at, id))) => sqlx::query("SELECT occurred_at,id,payload FROM observability_metrics WHERE occurred_at>? OR (occurred_at=? AND id>?) ORDER BY occurred_at,id LIMIT ?").bind(&at).bind(&at).bind(id).bind(page_limit(limit) as i64).fetch_all(&self.pool).await,
            (None, None) => sqlx::query("SELECT occurred_at,id,payload FROM observability_metrics ORDER BY occurred_at,id LIMIT ?").bind(page_limit(limit) as i64).fetch_all(&self.pool).await,
        }.map_err(backend)?;
        let keys = rows
            .iter()
            .map(|row| {
                Ok((
                    row.try_get("occurred_at").map_err(backend)?,
                    row.try_get("id").map_err(backend)?,
                ))
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        let records = rows
            .into_iter()
            .map(|row| {
                serde_json::from_str(&row.try_get::<String, _>("payload").map_err(backend)?)
                    .map_err(backend)
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        page_earliest(self, "observability_metrics", "occurred_at", records, keys).await
    }
    async fn list_alerts(
        &self,
        after: Option<&str>,
        limit: usize,
    ) -> Result<ObservabilityPage<AlertEpisode>, RepoError> {
        let after = cursor(after)?;
        let rows = if let Some((at, id)) = after { sqlx::query("SELECT opened_at,id,payload FROM observability_alerts WHERE opened_at>? OR (opened_at=? AND id>?) ORDER BY opened_at,id LIMIT ?").bind(&at).bind(&at).bind(id).bind(page_limit(limit) as i64).fetch_all(&self.pool).await } else { sqlx::query("SELECT opened_at,id,payload FROM observability_alerts ORDER BY opened_at,id LIMIT ?").bind(page_limit(limit) as i64).fetch_all(&self.pool).await }.map_err(backend)?;
        let keys = rows
            .iter()
            .map(|row| {
                Ok((
                    row.try_get("opened_at").map_err(backend)?,
                    row.try_get("id").map_err(backend)?,
                ))
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        let records = rows
            .into_iter()
            .map(|row| {
                serde_json::from_str(&row.try_get::<String, _>("payload").map_err(backend)?)
                    .map_err(backend)
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        page_earliest(self, "observability_alerts", "opened_at", records, keys).await
    }
    async fn acknowledge_alert(
        &self,
        id: uuid::Uuid,
        at: DateTime<Utc>,
    ) -> Result<bool, RepoError> {
        let row = sqlx::query("SELECT payload FROM observability_alerts WHERE id=?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        let Some(row) = row else {
            return Ok(false);
        };
        let mut episode: AlertEpisode =
            serde_json::from_str(&row.try_get::<String, _>("payload").map_err(backend)?)
                .map_err(backend)?;
        if episode.state == AlertState::Resolved {
            return Ok(false);
        }
        episode.state = AlertState::AcknowledgedActive;
        episode.acknowledged_at = Some(at);
        sqlx::query("UPDATE observability_alerts SET payload=? WHERE id=?")
            .bind(serde_json::to_string(&episode).map_err(backend)?)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(true)
    }
    async fn list_delivery_attempts(
        &self,
        alert_id: Option<uuid::Uuid>,
        after: Option<&str>,
        limit: usize,
    ) -> Result<ObservabilityPage<DeliveryAttempt>, RepoError> {
        let after = cursor(after)?;
        let rows = match (alert_id, after) {
            (Some(alert_id), Some((at, id))) => sqlx::query("SELECT occurred_at,id,payload FROM observability_delivery_attempts WHERE alert_id=? AND (occurred_at>? OR (occurred_at=? AND id>?)) ORDER BY occurred_at,id LIMIT ?").bind(alert_id.to_string()).bind(&at).bind(&at).bind(id).bind(page_limit(limit) as i64).fetch_all(&self.pool).await,
            (Some(alert_id), None) => sqlx::query("SELECT occurred_at,id,payload FROM observability_delivery_attempts WHERE alert_id=? ORDER BY occurred_at,id LIMIT ?").bind(alert_id.to_string()).bind(page_limit(limit) as i64).fetch_all(&self.pool).await,
            (None, Some((at, id))) => sqlx::query("SELECT occurred_at,id,payload FROM observability_delivery_attempts WHERE occurred_at>? OR (occurred_at=? AND id>?) ORDER BY occurred_at,id LIMIT ?").bind(&at).bind(&at).bind(id).bind(page_limit(limit) as i64).fetch_all(&self.pool).await,
            (None, None) => sqlx::query("SELECT occurred_at,id,payload FROM observability_delivery_attempts ORDER BY occurred_at,id LIMIT ?").bind(page_limit(limit) as i64).fetch_all(&self.pool).await,
        }.map_err(backend)?;
        let keys = rows
            .iter()
            .map(|row| {
                Ok((
                    row.try_get("occurred_at").map_err(backend)?,
                    row.try_get("id").map_err(backend)?,
                ))
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        let records = rows
            .into_iter()
            .map(|row| {
                serde_json::from_str(&row.try_get::<String, _>("payload").map_err(backend)?)
                    .map_err(backend)
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        page_earliest(
            self,
            "observability_delivery_attempts",
            "occurred_at",
            records,
            keys,
        )
        .await
    }
}

async fn page_earliest<T>(
    store: &SqliteStore,
    table: &str,
    column: &str,
    records: Vec<T>,
    keys: Vec<(String, String)>,
) -> Result<ObservabilityPage<T>, RepoError> {
    let sql = format!("SELECT MIN({column}) AS earliest FROM {table}");
    let earliest = sqlx::query(&sql)
        .fetch_one(&store.pool)
        .await
        .map_err(backend)?
        .try_get::<Option<String>, _>("earliest")
        .map_err(backend)?
        .map(|v| {
            DateTime::parse_from_rfc3339(&v)
                .map(|d| d.with_timezone(&Utc))
                .map_err(backend)
        })
        .transpose()?;
    Ok(page(records, keys, earliest))
}
