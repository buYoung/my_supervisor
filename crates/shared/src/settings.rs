use serde::{Deserialize, Serialize};

/// Presentation preference only. API timestamps and execution instants stay UTC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplaySettingsDto {
    pub time_zone: String,
}
