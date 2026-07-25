//! Domain events broadcast to the `/api/v1/events` WS stream. The HTTP adapter
//! maps these onto the `shared::events::EventEnvelope` wire shape.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use my_supervisor_core::domain::{JobRunId, ProcessState};
use tokio::sync::Notify;
use uuid::Uuid;

/// An event delivered to internal subscribers and external transports. Durable
/// terminal events carry a receipt which only an external transport completes
/// after its write succeeds; scheduler subscribers cannot acknowledge it.
#[derive(Debug, Clone)]
pub struct PublishedEvent {
    pub event: DomainEvent,
    pub event_id: Option<Uuid>,
    pub occurred_at: Option<DateTime<Utc>>,
    receipt: Option<Arc<DeliveryReceipt>>,
}

#[derive(Debug)]
struct DeliveryReceipt {
    delivered: std::sync::atomic::AtomicBool,
    notified: Notify,
}

impl PublishedEvent {
    pub fn ordinary(event: DomainEvent) -> Self {
        Self { event, event_id: None, occurred_at: None, receipt: None }
    }

    pub fn durable_terminal(event: DomainEvent, event_id: Uuid, occurred_at: DateTime<Utc>) -> Self {
        Self {
            event,
            event_id: Some(event_id),
            occurred_at: Some(occurred_at),
            receipt: Some(Arc::new(DeliveryReceipt {
                delivered: std::sync::atomic::AtomicBool::new(false),
                notified: Notify::new(),
            })),
        }
    }

    /// Called exclusively by an external transport after its write succeeds.
    pub fn complete_delivery(&self) {
        if let Some(receipt) = &self.receipt {
            receipt.delivered.store(true, std::sync::atomic::Ordering::Release);
            receipt.notified.notify_waiters();
        }
    }

    pub async fn wait_for_external_delivery(&self, timeout: std::time::Duration) -> bool {
        let Some(receipt) = &self.receipt else {
            return false;
        };
        if receipt.delivered.load(std::sync::atomic::Ordering::Acquire) {
            return true;
        }
        let notified = receipt.notified.notified();
        tokio::pin!(notified);
        // Register before the second state check so a write that completes in
        // the send-to-wait race cannot lose its notification.
        notified.as_mut().enable();
        if receipt.delivered.load(std::sync::atomic::Ordering::Acquire) {
            return true;
        }
        tokio::time::timeout(timeout, notified).await.is_ok()
            && receipt.delivered.load(std::sync::atomic::Ordering::Acquire)
    }
}

#[derive(Debug, Clone)]
pub enum DomainEvent {
    ProcessStateChanged {
        name: String,
        from: ProcessState,
        to: ProcessState,
    },
    JobRunScheduled {
        name: String,
        run_id: JobRunId,
    },
    JobRunStarted {
        name: String,
        run_id: JobRunId,
    },
    JobRunSucceeded {
        name: String,
        run_id: JobRunId,
        exit_code: i32,
    },
    JobRunFailed {
        name: String,
        run_id: JobRunId,
        exit_code: Option<i32>,
    },
    JobRunTimedOut {
        name: String,
        run_id: JobRunId,
    },
    JobRunCancelled {
        name: String,
        run_id: JobRunId,
    },
    JobRunSkipped {
        name: String,
        run_id: JobRunId,
        reason: String,
    },
}
