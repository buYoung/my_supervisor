//! Shared spawn plumbing: build a process in its own session/group and pump its
//! stdout/stderr into the `LogSink`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::{io, os::fd::AsRawFd};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdout};
use tokio::task::JoinHandle;

use my_supervisor_core::domain::{JobRunId, LogLine, LogStream, ProcessSpec};
use my_supervisor_core::ports::lifecycle::SpawnError;
use my_supervisor_core::ports::{LogError, LogSink};

/// Build a tokio command for `spec`, placing the child in its own session so the
/// whole tree can be signalled via the negative pgid without touching the daemon.
pub fn build_command(spec: &ProcessSpec) -> Result<tokio::process::Command, SpawnError> {
    if let Some(cwd) = &spec.cwd {
        if !cwd.exists() {
            return Err(SpawnError::CwdMissing(cwd.display().to_string()));
        }
    }

    let mut std_cmd = std::process::Command::new(&spec.command);
    std_cmd.args(&spec.args);
    std_cmd.envs(&spec.env);
    if let Some(cwd) = &spec.cwd {
        std_cmd.current_dir(cwd);
    }
    std_cmd.stdin(Stdio::null());
    std_cmd.stdout(Stdio::piped());
    std_cmd.stderr(Stdio::piped());

    // Own session/process group: pgid == pid, so `kill(-pid)` reaches the whole
    // tree. SAFETY: setsid is async-signal-safe and the only post-fork action.
    unsafe {
        use std::os::unix::process::CommandExt;
        std_cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    Ok(tokio::process::Command::from(std_cmd))
}

/// Where captured lines are routed.
#[derive(Clone)]
pub enum LogTarget {
    Process(String),
    Run(JobRunId),
}

/// The detached proxy and every independent cleanup owner created alongside
/// it.  Keeping the reaper handles here makes their wait ownership explicit:
/// callers must retain this value until all helpers have exited.
pub struct DetachedChild {
    pub proxy: Child,
    pub reapers: Vec<std::process::Child>,
}

/// Absolute paths for the detached-process helpers chosen by the host during
/// runtime assembly.  The lifecycle never consults the target environment to
/// find these binaries: environment variables belong solely to the target.
#[derive(Clone, Debug)]
pub struct DetachedHelperPaths {
    pub log_proxy: PathBuf,
    pub group_reaper: PathBuf,
}

/// Explicit debug-only fault controls for detached helper integration tests.
///
/// These values are supplied by the test host, never inferred from the
/// target's `ProcessSpec.env`.  Keeping this boundary explicit prevents a
/// target environment variable from changing proxy or reaper ownership.
#[derive(Clone, Debug, Default)]
pub struct DetachedTestControls {
    pub proxy_fail_after_appends: Option<usize>,
    pub reaper_withhold_takeover_ack: bool,
    pub first_reaper_crash_after_start: bool,
}

impl DetachedHelperPaths {
    pub fn new(log_proxy: PathBuf, group_reaper: PathBuf) -> Result<Self, String> {
        let log_proxy = validate_helper_path("msv-log-proxy", log_proxy)?;
        let group_reaper = validate_helper_path("msv-group-reaper", group_reaper)?;
        Ok(Self {
            log_proxy,
            group_reaper,
        })
    }
}

fn validate_helper_path(helper_name: &str, path: PathBuf) -> Result<PathBuf, String> {
    let absolute_path = path
        .canonicalize()
        .map_err(|error| format!("{helper_name} is unavailable at {}: {error}", path.display()))?;
    let metadata = std::fs::metadata(&absolute_path)
        .map_err(|error| format!("reading {helper_name} at {} failed: {error}", absolute_path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{helper_name} at {} is not a regular file", absolute_path.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(format!("{helper_name} at {} is not executable", absolute_path.display()));
        }
    }
    Ok(absolute_path)
}

async fn pump<R>(reader: R, stream: LogStream, sink: Arc<dyn LogSink>, target: LogTarget) -> Result<(), LogError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(text)) = lines.next_line().await {
        let line = LogLine::now(stream, text);
        match &target {
            LogTarget::Process(name) => sink.append(name, line).await,
            LogTarget::Run(run_id) => sink.append_run(*run_id, line).await,
        }?;
    }
    Ok(())
}

/// Attach line-pumps to a freshly-spawned child's stdout/stderr.
pub fn attach_pumps(
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    sink: &Arc<dyn LogSink>,
    target: LogTarget,
) -> Vec<JoinHandle<Result<(), LogError>>> {
    let mut pumps = Vec::with_capacity(2);
    if let Some(out) = stdout {
        pumps.push(tokio::spawn(pump(
            out,
            LogStream::Stdout,
            sink.clone(),
            target.clone(),
        )));
    }
    if let Some(err) = stderr {
        pumps.push(tokio::spawn(pump(err, LogStream::Stderr, sink.clone(), target)));
    }
    pumps
}

/// Spawn the child and return it with its captured pipes detached for pumping.
pub fn spawn_child(
    spec: &ProcessSpec,
    kill_on_drop: bool,
) -> Result<(Child, Option<ChildStdout>, Option<ChildStderr>), SpawnError> {
    let mut cmd = build_command(spec)?;
    cmd.kill_on_drop(kill_on_drop);
    let mut child = cmd.spawn().map_err(|e| SpawnError::Io {
        name: spec.name.clone(),
        message: e.to_string(),
    })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    Ok((child, stdout, stderr))
}

/// Spawn the detached log proxy as the dedicated process-group leader.  The
/// target inherits that group, while the proxy owns the durable interleaved
/// journal after the daemon exits.
pub fn spawn_detached_child(
    spec: &ProcessSpec,
    journal_path: &Path,
    helpers: &DetachedHelperPaths,
    test_controls: &DetachedTestControls,
) -> Result<DetachedChild, SpawnError> {
    if let Some(cwd) = &spec.cwd {
        if !cwd.exists() {
            return Err(SpawnError::CwdMissing(cwd.display().to_string()));
        }
    }
    // Two independent helpers are prepared before the target can start.  If
    // one helper disappears after the protocol has armed the target group,
    // the other remains a verified cleanup owner instead of leaving a group
    // whose leader proxy may already be gone.
    let mut proxy_controls = Vec::with_capacity(2);
    let mut reaper_controls = Vec::with_capacity(2);
    let mut reapers = Vec::with_capacity(2);
    for reaper_index in 0..2 {
        let (proxy_control, reaper_control) = std::os::unix::net::UnixStream::pair().map_err(|error| SpawnError::Io {
            name: spec.name.clone(),
            message: format!("creating detached reaper control channel failed: {error}"),
        })?;
        set_close_on_exec(&proxy_control, true).map_err(|error| control_channel_error(spec, error))?;
        set_close_on_exec(&reaper_control, false).map_err(|error| control_channel_error(spec, error))?;

        let mut reaper_command = std::process::Command::new(&helpers.group_reaper);
        reaper_command
            .arg("--control-fd")
            .arg(reaper_control.as_raw_fd().to_string())
            .args(detached_reaper_test_arguments(test_controls, reaper_index))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            use std::os::unix::process::CommandExt;
            reaper_command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut reaper = match reaper_command.spawn() {
            Ok(reaper) => reaper,
            Err(error) => {
                reap_helpers(&mut reapers);
                return Err(SpawnError::Io {
                    name: spec.name.clone(),
                    message: format!("spawning detached group reaper failed: {error}"),
                });
            }
        };
        if let Err(error) = set_close_on_exec(&reaper_control, true) {
            let _ = reaper.kill();
            let _ = reaper.wait();
            reap_helpers(&mut reapers);
            return Err(control_channel_error(spec, error));
        }
        proxy_controls.push(proxy_control);
        reaper_controls.push(reaper_control);
        reapers.push(reaper);
    }
    for proxy_control in &proxy_controls {
        if let Err(error) = set_close_on_exec(proxy_control, false) {
            reap_helpers(&mut reapers);
            return Err(control_channel_error(spec, error));
        }
    }

    let mut command = std::process::Command::new(&helpers.log_proxy);
    command
        .args(detached_proxy_test_arguments(test_controls))
        .arg("--journal")
        .arg(journal_path)
        .args(proxy_controls.iter().flat_map(|control| ["--control-fd".into(), control.as_raw_fd().to_string()]))
        .arg("--")
        .arg(&spec.command)
        .args(&spec.args)
        // The proxy forwards this environment unchanged to its target. Helper
        // control itself is carried only by private descriptors and debug-only
        // test arguments, so a target variable can never enable a helper fault.
        .envs(&spec.env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    // SAFETY: `setsid` is async-signal-safe and is the sole post-fork action.
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(false);
    let proxy = command.spawn().map_err(|error| SpawnError::Io {
        name: spec.name.clone(),
        message: error.to_string(),
    });
    for proxy_control in &proxy_controls {
        let _ = set_close_on_exec(proxy_control, true);
    }
    drop(proxy_controls);
    drop(reaper_controls);
    match proxy {
        Ok(proxy) => Ok(DetachedChild { proxy, reapers }),
        Err(error) => {
            reap_helpers(&mut reapers);
            Err(error)
        }
    }
}

fn reap_helpers(reapers: &mut [std::process::Child]) {
    for reaper in reapers {
        let _ = reaper.kill();
        let _ = reaper.wait();
    }
}

fn detached_proxy_test_arguments(test_controls: &DetachedTestControls) -> Vec<std::ffi::OsString> {
    #[cfg(debug_assertions)]
    {
        return test_controls.proxy_fail_after_appends
            .map(|value| vec!["--test-fail-after-appends".into(), value.to_string().into()])
            .unwrap_or_default();
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = test_controls;
        Vec::new()
    }
}

fn detached_reaper_test_arguments(
    test_controls: &DetachedTestControls,
    reaper_index: usize,
) -> Vec<std::ffi::OsString> {
    #[cfg(debug_assertions)]
    {
        let mut arguments = Vec::new();
        if test_controls.reaper_withhold_takeover_ack {
            arguments.push("--test-withhold-takeover-ack".into());
        }
        if reaper_index == 0 && test_controls.first_reaper_crash_after_start {
            arguments.push("--test-crash-after-start".into());
        }
        return arguments;
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = test_controls;
        let _ = reaper_index;
        Vec::new()
    }
}

fn control_channel_error(spec: &ProcessSpec, error: io::Error) -> SpawnError {
    SpawnError::Io {
        name: spec.name.clone(),
        message: format!("configuring detached reaper control channel failed: {error}"),
    }
}

fn set_close_on_exec(stream: &std::os::unix::net::UnixStream, enabled: bool) -> io::Result<()> {
    let descriptor = stream.as_raw_fd();
    let current = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
    if current == -1 {
        return Err(io::Error::last_os_error());
    }
    let flags = if enabled {
        current | libc::FD_CLOEXEC
    } else {
        current & !libc::FD_CLOEXEC
    };
    if unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
