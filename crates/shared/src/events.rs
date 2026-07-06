//! WebSocket event envelope (`/api/v1/events`). The `type` string is a stable
//! key per `docs/API.md` §3.1; payloads are event-specific JSON.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventEnvelope {
    #[serde(rename = "type")]
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub payload: serde_json::Value,
}

impl EventEnvelope {
    pub fn new(event_type: impl Into<String>, payload: serde_json::Value) -> Self {
        EventEnvelope {
            event_type: event_type.into(),
            timestamp: Utc::now(),
            payload,
        }
    }
}

/// Control frame inserted into a log WS stream when lines are dropped (DD-012).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogDroppedFrame {
    #[serde(rename = "type")]
    pub frame_type: LogDroppedType,
    pub payload: LogDroppedPayload,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogDroppedType {
    #[serde(rename = "log.dropped")]
    LogDropped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogDroppedPayload {
    pub count: u64,
}
