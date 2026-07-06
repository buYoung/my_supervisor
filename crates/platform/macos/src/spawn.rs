//! Shared spawn plumbing: build a process in its own session/group and pump its
//! stdout/stderr into the `LogSink`.

use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdout};

use my_supervisor_core::domain::{JobRunId, LogLine, LogStream, ProcessSpec};
use my_supervisor_core::ports::lifecycle::SpawnError;
use my_supervisor_core::ports::LogSink;

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
            libc::setsid();
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

async fn pump<R>(reader: R, stream: LogStream, sink: Arc<dyn LogSink>, target: LogTarget)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(text)) = lines.next_line().await {
        let line = LogLine::now(stream, text);
        match &target {
            LogTarget::Process(name) => sink.append(name, line).await,
            LogTarget::Run(run_id) => sink.append_run(*run_id, line).await,
        }
    }
}

/// Attach line-pumps to a freshly-spawned child's stdout/stderr.
pub fn attach_pumps(
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    sink: &Arc<dyn LogSink>,
    target: LogTarget,
) {
    if let Some(out) = stdout {
        tokio::spawn(pump(out, LogStream::Stdout, sink.clone(), target.clone()));
    }
    if let Some(err) = stderr {
        tokio::spawn(pump(err, LogStream::Stderr, sink.clone(), target));
    }
}

/// Spawn the child and return it with its captured pipes detached for pumping.
pub fn spawn_child(
    spec: &ProcessSpec,
) -> Result<(Child, Option<ChildStdout>, Option<ChildStderr>), SpawnError> {
    let mut cmd = build_command(spec)?;
    cmd.kill_on_drop(false);
    let mut child = cmd.spawn().map_err(|e| SpawnError::Io {
        name: spec.name.clone(),
        message: e.to_string(),
    })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    Ok((child, stdout, stderr))
}
