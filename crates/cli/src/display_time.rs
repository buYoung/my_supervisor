use crate::client::{CliError, Client};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use std::sync::OnceLock;

static TIME_ZONE: OnceLock<Tz> = OnceLock::new();

pub async fn configure(client: &Client, override_zone: Option<&str>) -> Result<(), CliError> {
    let zone = if let Some(zone) = override_zone {
        zone.to_owned()
    } else {
        client
            .display_settings()
            .await
            .map(|settings| settings.time_zone)
            .unwrap_or_else(|_| "UTC".into())
    };
    let zone = zone
        .parse::<Tz>()
        .map_err(|_| CliError::Failed("invalid display time zone".into()))?;
    let _ = TIME_ZONE.set(zone);
    Ok(())
}

pub fn time_zone() -> Tz {
    *TIME_ZONE.get().unwrap_or(&chrono_tz::UTC)
}

pub fn format(value: &DateTime<Utc>) -> String {
    value.with_timezone(&time_zone()).to_rfc3339()
}
