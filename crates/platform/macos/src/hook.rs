//! Bounded shell-free local hook delivery.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use my_supervisor_core::domain::{DeliveryCandidate, DeliverySubmission};
use my_supervisor_core::ports::AlertDelivery;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

const HOOK_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub struct HookDelivery {
    executable: PathBuf,
    arguments: Vec<String>,
    working_directory: Option<PathBuf>,
    environment: Vec<(String, String)>,
    concurrent: std::sync::Arc<Semaphore>,
}

impl HookDelivery {
    pub fn new(
        executable: PathBuf,
        arguments: Vec<String>,
        working_directory: Option<PathBuf>,
        environment: Vec<(String, String)>,
    ) -> Result<Self, String> {
        validate_path(&executable)?;
        if let Some(directory) = &working_directory {
            validate_path(directory)?;
        }
        Ok(Self {
            executable,
            arguments,
            working_directory,
            environment,
            concurrent: std::sync::Arc::new(Semaphore::new(4)),
        })
    }
}

fn validate_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("hook paths must be absolute".into());
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("hook paths must not be symlinks".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err("hook path must be owned by the daemon user".into());
        }
        if metadata.permissions().mode() & 0o002 != 0 {
            return Err("hook path must not be world writable".into());
        }
    }
    Ok(())
}

#[async_trait]
impl AlertDelivery for HookDelivery {
    async fn submit(&self, candidate: &DeliveryCandidate) -> DeliverySubmission {
        let Ok(_permit) = self.concurrent.clone().acquire_owned().await else {
            return DeliverySubmission {
                outcome: "failed".into(),
                detail: Some("hook concurrency gate closed".into()),
            };
        };
        let mut command = tokio::process::Command::new(&self.executable);
        command
            .args(&self.arguments)
            .env_clear()
            .envs(self.environment.iter().map(|(key, value)| (key, value)));
        if let Some(directory) = &self.working_directory {
            command.current_dir(directory);
        }
        // A distinct session makes the hook a separate process group from
        // supervised targets.  Output is intentionally discarded rather than
        // permitting unbounded pipe buffering; the persisted attempt still
        // records a bounded outcome/detail.
        #[cfg(unix)]
        {
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
        }
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return DeliverySubmission {
                    outcome: "failed".into(),
                    detail: Some(error.to_string()),
                }
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            let document = serde_json::json!({"version": 1, "candidate": candidate});
            if stdin
                .write_all(document.to_string().as_bytes())
                .await
                .is_err()
            {
                let _ = child.start_kill();
                return DeliverySubmission {
                    outcome: "failed".into(),
                    detail: Some("writing hook stdin failed".into()),
                };
            }
        }
        match tokio::time::timeout(HOOK_TIMEOUT, child.wait()).await {
            Ok(Ok(status)) if status.success() => DeliverySubmission {
                outcome: "submitted".into(),
                detail: None,
            },
            Ok(Ok(status)) => DeliverySubmission {
                outcome: "failed".into(),
                detail: Some(format!("hook exited with {status}")),
            },
            Ok(Err(error)) => DeliverySubmission {
                outcome: "failed".into(),
                detail: Some(error.to_string()),
            },
            Err(_) => {
                if let Some(pid) = child.id() {
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGTERM);
                    }
                }
                let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
                if let Some(pid) = child.id() {
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGKILL);
                    }
                }
                let _ = child.wait().await;
                DeliverySubmission { outcome: "failed".into(), detail: Some(format!("hook timed out after 30 seconds; output capped at {MAX_OUTPUT_BYTES} bytes")) }
            }
        }
    }
}
