//! Versioned editor documents and durable execution snapshots.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDocument {
    pub revision: u64,
    pub saved_at: DateTime<Utc>,
    pub value: String,
}
