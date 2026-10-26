use crate::{AppError, AppResult, OperationsFacade};
use serde::{Deserialize, Serialize};

const DISPLAY_SETTINGS_KEY: &str = "settings/display";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplaySettings {
    pub time_zone: String,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            time_zone: "UTC".into(),
        }
    }
}

impl OperationsFacade {
    pub async fn display_settings(&self) -> AppResult<DisplaySettings> {
        let Some(value) = self
            .workspace_deps()
            .state_repo
            .workspace_state(DISPLAY_SETTINGS_KEY)
            .await?
        else {
            return Ok(DisplaySettings::default());
        };
        serde_json::from_str(&value)
            .map_err(|error| AppError::Internal(format!("invalid display settings: {error}")))
    }

    pub async fn set_display_time_zone(&self, time_zone: &str) -> AppResult<DisplaySettings> {
        let zone = time_zone.trim().parse::<chrono_tz::Tz>().map_err(|_| {
            AppError::InvalidRequest(
                "time_zone must be an IANA time zone such as UTC or Asia/Seoul".into(),
            )
        })?;
        let settings = DisplaySettings {
            time_zone: zone.to_string(),
        };
        let value = serde_json::to_string(&settings)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        self.workspace_deps()
            .state_repo
            .save_workspace_state(DISPLAY_SETTINGS_KEY, &value)
            .await?;
        Ok(settings)
    }
}
