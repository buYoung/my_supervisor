//! `LaunchdAgentProcess` — the macOS `ProcessServiceRegistrar` (child 06).
//! Generates a per-process LaunchAgent plist in `~/Library/LaunchAgents`,
//! bootstraps/boots it out of the GUI domain, and reads status via launchctl.
//! Writes only the user domain (`gui/$(id -u)`) — never the system domain.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use my_supervisor_core::domain::{LogLine, LogStream, ProcessSpec, ProcessState};
use my_supervisor_core::ports::error::RegistrarError;
use my_supervisor_core::ports::ProcessServiceRegistrar;
use serde::Serialize;

pub const SUPERVISOR_LABEL: &str = "com.my-supervisor.daemon";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorServiceState {
    NotInstalled,
    Stopped,
    Starting,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct SupervisorServiceStatus {
    pub state: SupervisorServiceState,
    pub label: String,
    pub plist_path: String,
    pub pid: Option<u32>,
}

/// The supervisor's user LaunchAgent is intentionally separate from
/// `LaunchdAgentProcess`: it owns the one daemon authority, while that adapter
/// owns user-requested managed processes.
pub struct SupervisorLaunchAgent {
    root: PathBuf,
    plist_path: PathBuf,
    binary: PathBuf,
    uid: u32,
    label: String,
}

impl SupervisorLaunchAgent {
    pub fn new(root: PathBuf, binary: PathBuf) -> Result<Self, String> {
        let home = dirs::home_dir()
            .ok_or_else(|| "resolving the current user's home directory".to_string())?;
        let label = SUPERVISOR_LABEL.to_string();
        let plist_path = home
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{SUPERVISOR_LABEL}.plist"));
        #[cfg(debug_assertions)]
        let (label, plist_path) = test_supervisor_identity(label, plist_path);
        if !binary.is_file() {
            return Err(format!(
                "daemon binary is unavailable at {}",
                binary.display()
            ));
        }
        Ok(Self {
            root,
            plist_path,
            binary,
            uid: unsafe { libc::getuid() },
            label,
        })
    }

    pub fn install(&self) -> Result<SupervisorServiceStatus, String> {
        crate::owner::ensure_private_directory(&self.root)
            .map_err(|error| format!("claiming service root: {error}"))?;
        crate::owner::ensure_private_directory(&self.root.join("logs"))
            .map_err(|error| format!("preparing service logs: {error}"))?;
        crate::owner::ensure_private_directory(&self.root.join("run"))
            .map_err(|error| format!("preparing service run directory: {error}"))?;
        let parent = self
            .plist_path
            .parent()
            .ok_or_else(|| "supervisor plist has no parent".to_string())?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("creating LaunchAgents directory: {error}"))?;
        let plist = self.plist();
        write_plist_atomic(&self.plist_path, plist.as_bytes())?;
        // A prior registration may be stale. Bootout is idempotent and the
        // data root is never touched by any lifecycle operation.
        let _ = self.launchctl(&["bootout", &self.target()]);
        self.launchctl_success(&[
            "bootstrap",
            &self.domain(),
            &self.plist_path.display().to_string(),
        ])?;
        self.status()
    }

    pub fn start(&self) -> Result<SupervisorServiceStatus, String> {
        if !self.plist_path.is_file() {
            return self.status();
        }
        let marker = self.intentional_stop_path();
        if marker.exists() {
            std::fs::remove_file(marker)
                .map_err(|error| format!("clearing intentional-stop marker: {error}"))?;
        }
        if self
            .launchctl(&["print", &self.target()])
            .map(|output| output.status.success())
            .unwrap_or(false)
        {
            self.launchctl_success(&["kickstart", "-k", &self.target()])?;
        } else {
            self.launchctl_success(&[
                "bootstrap",
                &self.domain(),
                &self.plist_path.display().to_string(),
            ])?;
        }
        self.status()
    }

    pub fn stop(&self) -> Result<SupervisorServiceStatus, String> {
        if !self.plist_path.is_file() {
            return self.status();
        }
        let marker = self.intentional_stop_path();
        if let Some(parent) = marker.parent() {
            crate::owner::ensure_private_directory(parent)
                .map_err(|error| format!("preparing intentional-stop directory: {error}"))?;
        }
        crate::owner::write_private_file_atomic(&marker, b"intentional-stop\n")
            .map_err(|error| format!("writing intentional-stop marker: {error}"))?;
        let _ = self.launchctl(&["bootout", &self.target()]);
        self.status()
    }

    pub fn uninstall(&self) -> Result<SupervisorServiceStatus, String> {
        let _ = self.launchctl(&["bootout", &self.target()]);
        if self.plist_path.exists() {
            std::fs::remove_file(&self.plist_path)
                .map_err(|error| format!("removing supervisor plist: {error}"))?;
        }
        Ok(SupervisorServiceStatus {
            state: SupervisorServiceState::NotInstalled,
            label: self.label.clone(),
            plist_path: self.plist_path.display().to_string(),
            pid: None,
        })
    }

    pub fn status(&self) -> Result<SupervisorServiceStatus, String> {
        if !self.plist_path.is_file() {
            return Ok(SupervisorServiceStatus {
                state: SupervisorServiceState::NotInstalled,
                label: self.label.clone(),
                plist_path: self.plist_path.display().to_string(),
                pid: None,
            });
        }
        let output = self.launchctl(&["print", &self.target()]);
        match output {
            Ok(output) if output.status.success() => {
                let pid = parse_launchctl_pid(&String::from_utf8_lossy(&output.stdout));
                Ok(SupervisorServiceStatus {
                    state: if pid.is_some() {
                        SupervisorServiceState::Ready
                    } else {
                        SupervisorServiceState::Stopped
                    },
                    label: self.label.clone(),
                    plist_path: self.plist_path.display().to_string(),
                    pid,
                })
            }
            Ok(_) => Ok(SupervisorServiceStatus {
                state: SupervisorServiceState::Stopped,
                label: self.label.clone(),
                plist_path: self.plist_path.display().to_string(),
                pid: None,
            }),
            Err(error) => Err(error),
        }
    }

    fn plist(&self) -> String {
        let marker = self.intentional_stop_path();
        let out = self.root.join("logs").join("daemon.out.log");
        let err = self.root.join("logs").join("daemon.err.log");
        let debug_environment = self.debug_environment();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{}</string>
<key>ProgramArguments</key><array><string>{}</string></array>
{}
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><dict><key>PathState</key><dict><key>{}</key><false/></dict></dict>
<key>ThrottleInterval</key><integer>5</integer>
<key>StandardOutPath</key><string>{}</string>
<key>StandardErrorPath</key><string>{}</string>
</dict></plist>"#,
            xml(&self.label),
            xml(&self.binary.display().to_string()),
            debug_environment,
            xml(&marker.display().to_string()),
            xml(&out.display().to_string()),
            xml(&err.display().to_string())
        )
    }

    fn debug_environment(&self) -> String {
        #[cfg(debug_assertions)]
        {
            let values = [
                (
                    "MSV_DAEMON_TEST_DATA_DIR",
                    std::env::var("MSV_DAEMON_TEST_DATA_DIR").ok(),
                ),
                (
                    "MSV_DAEMON_TEST_CONFIG_PATH",
                    std::env::var("MSV_DAEMON_TEST_CONFIG_PATH").ok(),
                ),
                (
                    "MSV_DAEMON_TEST_BIND_ADDR",
                    std::env::var("MSV_DAEMON_TEST_BIND_ADDR").ok(),
                ),
            ];
            let body: String = values
                .into_iter()
                .filter_map(|(key, value)| {
                    value.map(|value| {
                        format!("<key>{}</key><string>{}</string>", xml(key), xml(&value))
                    })
                })
                .collect();
            if body.is_empty() {
                String::new()
            } else {
                format!("<key>EnvironmentVariables</key><dict>{body}</dict>")
            }
        }
        #[cfg(not(debug_assertions))]
        {
            String::new()
        }
    }

    fn domain(&self) -> String {
        format!("gui/{}", self.uid)
    }
    fn target(&self) -> String {
        format!("{}/{}", self.domain(), self.label)
    }
    fn intentional_stop_path(&self) -> PathBuf {
        self.root.join("run").join("intentional-stop")
    }
    fn launchctl(&self, args: &[&str]) -> Result<std::process::Output, String> {
        Command::new("launchctl")
            .args(args)
            .output()
            .map_err(|error| format!("launchctl: {error}"))
    }
    fn launchctl_success(&self, args: &[&str]) -> Result<(), String> {
        let output = self.launchctl(args)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "launchctl {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

#[cfg(debug_assertions)]
fn test_supervisor_identity(label: String, plist_path: PathBuf) -> (String, PathBuf) {
    let label = std::env::var_os("MSV_DAEMON_TEST_LABEL")
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or(label);
    let plist_path = std::env::var_os("MSV_DAEMON_TEST_PLIST_PATH")
        .map(PathBuf::from)
        .unwrap_or(plist_path);
    (label, plist_path)
}

fn write_plist_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "supervisor plist has no parent".to_string())?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("supervisor"),
        std::process::id()
    ));
    std::fs::write(&temporary, contents)
        .map_err(|error| format!("writing supervisor plist: {error}"))?;
    std::fs::rename(&temporary, path)
        .map_err(|error| format!("installing supervisor plist: {error}"))
}

pub struct LaunchdAgentProcess {
    agents_dir: PathBuf,
    log_dir: PathBuf,
    uid: u32,
    test_controls: Option<Arc<LaunchdTestControls>>,
}

/// Explicit test-only behavior injected by an integration fixture.  Production
/// assembly uses [`LaunchdAgentProcess::new`] and has no input path to these
/// controls, so ProcessSpec fields and daemon configuration cannot enable it.
#[derive(Default)]
pub struct LaunchdTestControls {
    fail_next_start: AtomicBool,
}

impl LaunchdTestControls {
    pub fn fail_next_start(&self) {
        self.fail_next_start.store(true, Ordering::SeqCst);
    }

    fn take_start_failure(&self) -> bool {
        self.fail_next_start.swap(false, Ordering::SeqCst)
    }
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
            test_controls: None,
        }
    }

    /// Creates a registrar whose failure behavior is controlled exclusively by
    /// the supplied fixture object.  This is intentionally separate from the
    /// production constructor.
    pub fn with_test_controls(log_dir: PathBuf, test_controls: Arc<LaunchdTestControls>) -> Self {
        let mut registrar = Self::new(log_dir);
        registrar.test_controls = Some(test_controls);
        registrar
    }

    fn plist_path(&self, unit_name: &str) -> PathBuf {
        self.agents_dir.join(format!("{unit_name}.plist"))
    }

    fn candidate_plist_path(&self, unit_name: &str) -> PathBuf {
        self.agents_dir
            .join(format!(".{unit_name}.{}.tmp", uuid::Uuid::new_v4()))
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

    async fn bootstrap(&self, unit_name: &str) -> Result<(), RegistrarError> {
        let domain = self.domain();
        let plist_path = self.plist_path(unit_name);
        if !plist_path.exists() {
            return Err(RegistrarError::NotFound(unit_name.to_string()));
        }
        let plist = plist_path.display().to_string();
        let output = self.launchctl(&["bootstrap", &domain, &plist]).await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RegistrarError::RegistrationFailed(format!(
                "launchctl bootstrap failed: {}",
                stderr.trim()
            )));
        }
        Ok(())
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

fn parse_launchctl_pid(output: &str) -> Option<u32> {
    output.lines().find_map(|line| {
        if !line.contains("\"PID\"") && !line.trim_start().starts_with("pid =") {
            return None;
        }
        line.split_once('=')?
            .1
            .trim()
            .trim_end_matches(';')
            .parse()
            .ok()
    })
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

        let candidate_path = self.candidate_plist_path(unit_name);
        let plist = self.build_plist(unit_name, spec);
        tokio::fs::write(&candidate_path, plist)
            .await
            .map_err(|e| RegistrarError::RegistrationFailed(e.to_string()))?;
        if let Err(error) = tokio::fs::rename(&candidate_path, &plist_path).await {
            let _ = tokio::fs::remove_file(&candidate_path).await;
            return Err(RegistrarError::RegistrationFailed(error.to_string()));
        }

        // Registration is a reversible filesystem-only prepare step. In
        // particular, do not boot out an already-running same-label unit here:
        // later target preparation may still fail and compensation must retain
        // the old live service and PID.
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
        if self
            .test_controls
            .as_ref()
            .is_some_and(|controls| controls.take_start_failure())
        {
            return Err(RegistrarError::RegistrationFailed(
                "injected launchd start failure".to_string(),
            ));
        }
        // `start` is the live replacement boundary. The application persists
        // ForwardRecovery before it calls this for a changed same-label spec.
        let target = self.service_target(unit_name);
        self.launchctl(&["bootout", &target]).await.ok();
        self.bootstrap(unit_name).await?;
        Ok(())
    }

    async fn stop(&self, unit_name: &str) -> Result<(), RegistrarError> {
        let target = self.service_target(unit_name);
        let output = self.launchctl(&["bootout", &target]).await?;
        if output.status.success() || !self.is_registered(unit_name).await {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(RegistrarError::RegistrationFailed(format!(
            "launchctl bootout failed: {}",
            stderr.trim()
        )))
    }

    async fn query_status(&self, unit_name: &str) -> Result<ProcessState, RegistrarError> {
        let output = self.launchctl(&["list", unit_name]).await?;
        if !output.status.success() {
            return Err(RegistrarError::NotFound(unit_name.to_string()));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        // launchctl prints a `"PID" = <n>;` line only while the job is running.
        Ok(if parse_launchctl_pid(&text).is_some() {
            ProcessState::Running
        } else {
            ProcessState::Stopped
        })
    }

    async fn query_pid(&self, unit_name: &str) -> Result<Option<u32>, RegistrarError> {
        let output = self.launchctl(&["list", unit_name]).await?;
        if !output.status.success() {
            return Err(RegistrarError::NotFound(unit_name.to_string()));
        }
        Ok(parse_launchctl_pid(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    async fn tail_logs(
        &self,
        unit_name: &str,
        lines: usize,
    ) -> Result<Vec<LogLine>, RegistrarError> {
        // launchd owns these raw files.  The supervisor neither rotates nor
        // assigns their file offsets public cursor meaning; a bounded `tail`
        // read keeps external ingestion from loading unbounded history.
        let per_stream = if lines == 0 {
            10_000
        } else {
            lines.div_ceil(2)
        };
        let stdout = bounded_tail(self.out_log(unit_name), per_stream).await;
        let stderr = bounded_tail(self.err_log(unit_name), per_stream).await;
        let stdout_lines: Vec<&str> = stdout.lines().collect();
        let stderr_lines: Vec<&str> = stderr.lines().collect();
        let stdout_start = if lines == 0 {
            0
        } else {
            stdout_lines.len().saturating_sub(lines.div_ceil(2))
        };
        let stderr_start = if lines == 0 {
            0
        } else {
            stderr_lines.len().saturating_sub(lines / 2)
        };
        let timestamp = Utc::now();
        let collected = stdout_lines[stdout_start..]
            .iter()
            .enumerate()
            .map(|(offset, line)| LogLine {
                sequence: (offset + 1) as u64,
                timestamp,
                stream: LogStream::Stdout,
                line: (*line).to_string(),
            })
            .chain(
                stderr_lines[stderr_start..]
                    .iter()
                    .enumerate()
                    .map(|(offset, line)| LogLine {
                        sequence: (stdout_lines.len() - stdout_start + offset + 1) as u64,
                        timestamp,
                        stream: LogStream::Stderr,
                        line: (*line).to_string(),
                    }),
            )
            .collect();
        Ok(collected)
    }
}

async fn bounded_tail(path: PathBuf, lines: usize) -> String {
    let count = lines.to_string();
    let output = tokio::process::Command::new("tail")
        .args(["-n", &count])
        .arg(path)
        .output()
        .await;
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_launchctl_pid, LaunchdAgentProcess};
    use my_supervisor_core::domain::ProcessSpec;
    use std::path::PathBuf;

    #[test]
    fn parses_running_pid_and_rejects_stopped_output() {
        assert_eq!(parse_launchctl_pid("{\n  \"PID\" = 4321;\n}"), Some(4321));
        assert_eq!(parse_launchctl_pid("{\n  \"LastExitStatus\" = 0;\n}"), None);
    }

    #[test]
    fn generated_plist_preserves_working_directory_and_escapes_values() {
        let registrar = LaunchdAgentProcess {
            agents_dir: PathBuf::from("/tmp/agents"),
            log_dir: PathBuf::from("/tmp/logs"),
            uid: 501,
            test_controls: None,
        };
        let mut spec = ProcessSpec::new("service", "/bin/echo");
        spec.args = vec!["a&b".to_string()];
        spec.cwd = Some(PathBuf::from("/tmp/work dir"));
        spec.autostart = true;
        let plist = registrar.build_plist("com.example.service", &spec);
        assert!(plist.contains("<string>a&amp;b</string>"));
        assert!(plist.contains("<key>WorkingDirectory</key><string>/tmp/work dir</string>"));
        assert!(plist.contains("<key>RunAtLoad</key><true/>"));
    }
}
