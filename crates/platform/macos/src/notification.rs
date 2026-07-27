//! macOS Notification Center submission adapter.

//! `osascript` receives a fixed program and literal argv; alert payload is not
//! interpolated into script source.  A successful process exit only means the
//! submission was accepted, never that Notification Center displayed it.

use async_trait::async_trait;
use my_supervisor_core::domain::{DeliveryCandidate, DeliverySubmission};
use my_supervisor_core::ports::AlertDelivery;

#[derive(Debug, Clone, Default)]
pub struct NotificationCenterDelivery;

impl NotificationCenterDelivery {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AlertDelivery for NotificationCenterDelivery {
    async fn submit(&self, candidate: &DeliveryCandidate) -> DeliverySubmission {
        if candidate.kind != "notification" {
            return DeliverySubmission {
                outcome: "unavailable".into(),
                detail: Some("delivery kind is not Notification Center".into()),
            };
        }
        let script = "on run argv\n display notification item 1 of argv with title \"my-supervisor\"\nend run";
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::process::Command::new("/usr/bin/osascript")
                .args(["-e", script, "--", candidate.payload.as_str()])
                .output(),
        )
        .await;
        match result {
            Ok(Ok(output)) if output.status.success() => DeliverySubmission {
                outcome: "submitted".into(),
                detail: None,
            },
            Ok(Ok(output)) => DeliverySubmission {
                outcome: "denied".into(),
                detail: Some(
                    String::from_utf8_lossy(&output.stderr)
                        .chars()
                        .take(1024)
                        .collect(),
                ),
            },
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => DeliverySubmission {
                outcome: "unavailable".into(),
                detail: Some(error.to_string()),
            },
            Ok(Err(error)) => DeliverySubmission {
                outcome: "failed".into(),
                detail: Some(error.to_string()),
            },
            Err(_) => DeliverySubmission {
                outcome: "failed".into(),
                detail: Some("Notification Center submission timed out after 30 seconds".into()),
            },
        }
    }
}
