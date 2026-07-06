//! `LaunchdAgentProcess` — the macOS `ProcessServiceRegistrar` (child 06).
//! Generates a per-process LaunchAgent plist in `~/Library/LaunchAgents`,
//! bootstraps/boots it out of the GUI domain, and reads status via launchctl.
//! Writes only the user domain (`gui/$(id -u)`) — never the system domain.

use std::path::PathBuf;

use async_trait::async_trait;
use chrono::Utc;

use my_supervisor_core::domain::{LogLine, LogStream, ProcessSpec, ProcessState};
use my_supervisor_core::ports::error::RegistrarError;
use my_supervisor_core::ports::ProcessServiceRegistrar;

pub struct LaunchdAgentProcess {
    agents_dir: PathBuf,
    log_dir: PathBuf,
    uid: u32,
}

impl LaunchdAgentProcess {
    pub fn new(log_dir: PathBuf) -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        // SAFETY: getuid is always safe.
        let uid = unsafe { libc::getuid() };
        LaunchdAgentProcess {
            agents_dir: PathBuf::from(home).join("Library").join("LaunchAgents"),
            log_dir,
            uid,
        }
    }

    fn plist_path(&self, unit_name: &str) -> PathBuf {
        self.agents_dir.join(format!("{unit_name}.plist"))
    }

    fn domain(&self) -> String {
        format!("gui/{}", self.uid)
    }

    fn service_target(&self, unit_name: &str) -> String {
        format!("gui/{}/{unit_name}", self.uid)
    }

    fn out_log(&self, unit_name: &str) -> PathBuf {
        self.log_dir.join(format!("{unit_name}.out.log"))
    }

    fn err_log(&self, unit_name: &str) -> PathBuf {
        self.log_dir.join(format!("{unit_name}.err.log"))
    }

    fn build_plist(&self, unit_name: &str, spec: &ProcessSpec) -> String {
        let mut args = String::new();
        args.push_str(&format!("    <string>{}</string>\n", xml(&spec.command)));
        for arg in &spec.args {
            args.push_str(&format!("    <string>{}</string>\n", xml(arg)));
        }

        let mut env = String::new();
        if !spec.env.is_empty() {
            env.push_str("  <key>EnvironmentVariables</key>\n  <dict>\n");
            for (k, v) in &spec.env {
                env.push_str(&format!(
                    "    <key>{}</key><string>{}</string>\n",
                    xml(k),
                    xml(v)
                ));
            }
            env.push_str("  </dict>\n");
        }

        let cwd = spec
            .cwd
            .as_ref()
            .map(|p| {
                format!(
                    "  <key>WorkingDirectory</key><string>{}</string>\n",
                    xml(&p.display().to_string())
                )
            })
            .unwrap_or_default();

        let keep_alive = if spec.restart.enabled {
            "true"
        } else {
            "false"
        };

        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array>
{args}  </array>
  <key>RunAtLoad</key><{run_at_load}/>
  <key>KeepAlive</key><{keep_alive}/>
{cwd}{env}  <key>StandardOutPath</key><string>{out}</string>
  <key>StandardErrorPath</key><string>{err}</string>
</dict>
</plist>
"#,
            label = xml(unit_name),
            args = args,
            run_at_load = if spec.autostart { "true" } else { "false" },
            keep_alive = keep_alive,
            cwd = cwd,
            env = env,
            out = xml(&self.out_log(unit_name).display().to_string()),
            err = xml(&self.err_log(unit_name).display().to_string()),
        )
    }

    async fn launchctl(&self, args: &[&str]) -> Result<std::process::Output, RegistrarError> {
        tokio::process::Command::new("launchctl")
            .args(args)
            .output()
            .await
            .map_err(|e| RegistrarError::RegistrationFailed(format!("launchctl: {e}")))
    }

    async fn is_registered(&self, unit_name: &str) -> bool {
        self.launchctl(&["list", unit_name])
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// Minimal XML text escaping for plist string values.
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[async_trait]
impl ProcessServiceRegistrar for LaunchdAgentProcess {
    async fn register(&self, unit_name: &str, spec: &ProcessSpec) -> Result<(), RegistrarError> {
        let plist_path = self.plist_path(unit_name);
        // Idempotency vs conflict: a plist we already own is fine to overwrite;
        // a label already live in the domain without our plist is a conflict.
        if self.is_registered(unit_name).await && !plist_path.exists() {
            return Err(RegistrarError::UnitNameConflict(unit_name.to_string()));
        }

        tokio::fs::create_dir_all(&self.agents_dir)
            .await
            .map_err(|e| RegistrarError::RegistrationFailed(e.to_string()))?;
        tokio::fs::create_dir_all(&self.log_dir).await.ok();

        let plist = self.build_plist(unit_name, spec);
        tokio::fs::write(&plist_path, plist)
            .await
            .map_err(|e| RegistrarError::RegistrationFailed(e.to_string()))?;

        // Replace any prior instance, then bootstrap the fresh plist.
        let target = self.service_target(unit_name);
        self.launchctl(&["bootout", &target]).await.ok();
        let domain = self.domain();
        let plist_str = plist_path.display().to_string();
        let output = self.launchctl(&["bootstrap", &domain, &plist_str]).await?;
        if !output.status.success() {
            tokio::fs::remove_file(&plist_path).await.ok();
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RegistrarError::RegistrationFailed(format!(
                "launchctl bootstrap failed: {}",
                stderr.trim()
            )));
        }
        Ok(())
    }

    async fn unregister(&self, unit_name: &str) -> Result<(), RegistrarError> {
        let target = self.service_target(unit_name);
        self.launchctl(&["bootout", &target]).await.ok();
        tokio::fs::remove_file(self.plist_path(unit_name))
            .await
            .ok();
        Ok(())
    }

    async fn start(&self, unit_name: &str) -> Result<(), RegistrarError> {
        let target = self.service_target(unit_name);
        let output = self.launchctl(&["kickstart", "-k", &target]).await?;
        if !output.status.success() {
            return Err(RegistrarError::NotFound(unit_name.to_string()));
        }
        Ok(())
    }

    async fn stop(&self, unit_name: &str) -> Result<(), RegistrarError> {
        let target = self.service_target(unit_name);
        self.launchctl(&["kill", "SIGTERM", &target]).await.ok();
        Ok(())
    }

    async fn query_status(&self, unit_name: &str) -> Result<ProcessState, RegistrarError> {
        let output = self.launchctl(&["list", unit_name]).await?;
        if !output.status.success() {
            return Err(RegistrarError::NotFound(unit_name.to_string()));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        // launchctl prints a `"PID" = <n>;` line only while the job is running.
        Ok(if text.contains("\"PID\"") {
            ProcessState::Running
        } else {
            ProcessState::Stopped
        })
    }

    async fn tail_logs(
        &self,
        unit_name: &str,
        lines: usize,
    ) -> Result<Vec<LogLine>, RegistrarError> {
        let path = self.out_log(unit_name);
        let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        let collected: Vec<&str> = content.lines().collect();
        let start = collected.len().saturating_sub(lines);
        Ok(collected[start..]
            .iter()
            .map(|l| LogLine {
                timestamp: Utc::now(),
                stream: LogStream::Stdout,
                line: l.to_string(),
            })
            .collect())
    }
}
