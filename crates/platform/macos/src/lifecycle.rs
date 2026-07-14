//! `MacLifecycle` — Direct-mode spawn/probe/reap for macOS (Unix spawn + setsid
//! groups; reconciliation compensates for the absence of a subreaper).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use my_supervisor_core::domain::{ChildHandle, JobRunId, LogLine, LogStream, ProcessSpec};
use my_supervisor_core::ports::lifecycle::{
    Aliveness, LifecycleController, ProbeError, ReapError, SpawnError, TransientOutcome,
};
use my_supervisor_core::ports::LogSink;

use crate::signals::{is_alive, is_process_group_leader, signal_group, SIGKILL, SIGTERM};
use crate::spawn::{attach_pumps, spawn_child, spawn_detached_child, LogTarget};

async fn read_log_tail(
    path: &Path,
    limit: usize,
    stream: LogStream,
) -> Result<Vec<LogLine>, ProbeError> {
    let contents = match tokio::fs::read_to_string(path).await {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(ProbeError::Failed(error.to_string())),
    };
    let lines: Vec<&str> = contents.lines().collect();
    let start = if limit == 0 {
        0
    } else {
        lines.len().saturating_sub(limit)
    };
    Ok(lines[start..]
        .iter()
        .map(|line| LogLine::now(stream, *line))
        .collect())
}

pub struct MacLifecycle {
    log_sink: Arc<dyn LogSink>,
    log_dir: PathBuf,
    children: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
}

impl MacLifecycle {
    pub fn new(log_sink: Arc<dyn LogSink>, log_dir: PathBuf) -> Self {
        MacLifecycle {
            log_sink,
            log_dir,
            children: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn detached_log_paths(&self, process_name: &str) -> (PathBuf, PathBuf) {
        let safe_name: String = process_name
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                    character
                } else {
                    '_'
                }
            })
            .collect();
        (
            self.log_dir.join(format!("direct-{safe_name}.stdout.log")),
            self.log_dir.join(format!("direct-{safe_name}.stderr.log")),
        )
    }

    fn spawn_common(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        let (child, stdout, stderr) = spawn_child(spec, false)?;
        attach_pumps(
            stdout,
            stderr,
            &self.log_sink,
            LogTarget::Process(spec.name.clone()),
        );
        self.track_child(spec, child)
    }

    fn spawn_detached_common(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        let (stdout_path, stderr_path) = self.detached_log_paths(&spec.name);
        let child = spawn_detached_child(spec, &stdout_path, &stderr_path)?;
        self.track_child(spec, child)
    }

    fn track_child(
        &self,
        spec: &ProcessSpec,
        mut child: tokio::process::Child,
    ) -> Result<ChildHandle, SpawnError> {
        let process_id = Uuid::new_v4();
        let pid = child.id().unwrap_or(0);
        let started_at = Utc::now();

        let alive = Arc::new(AtomicBool::new(true));
        self.children
            .lock()
            .unwrap()
            .insert(process_id, alive.clone());

        let children = self.children.clone();
        let sink = self.log_sink.clone();
        let name = spec.name.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            alive.store(false, Ordering::SeqCst);
            children.lock().unwrap().remove(&process_id);
            let note = match status {
                Ok(s) => format!("process exited: {s}"),
                Err(e) => format!("process wait failed: {e}"),
            };
            sink.append(&name, LogLine::now(LogStream::System, note))
                .await;
        });

        Ok(ChildHandle {
            process_id,
            pid,
            started_at,
        })
    }
}

#[async_trait]
impl LifecycleController for MacLifecycle {
    async fn spawn_tied(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        self.spawn_common(spec)
    }

    async fn spawn_detached(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        self.spawn_detached_common(spec)
    }

    async fn probe_alive(&self, handle: &ChildHandle) -> Result<Aliveness, ProbeError> {
        Ok(if is_alive(handle.pid) && is_process_group_leader(handle.pid) {
            Aliveness::Alive
        } else {
            Aliveness::Dead
        })
    }

    async fn tail_detached_logs(
        &self,
        spec: &ProcessSpec,
        lines: usize,
    ) -> Result<Vec<LogLine>, ProbeError> {
        let (stdout_path, stderr_path) = self.detached_log_paths(&spec.name);
        let per_stream = if lines == 0 { 0 } else { lines.div_ceil(2) };
        let mut result = read_log_tail(&stdout_path, per_stream, LogStream::Stdout).await?;
        result.extend(read_log_tail(&stderr_path, per_stream, LogStream::Stderr).await?);
        if lines > 0 && result.len() > lines {
            result = result.split_off(result.len() - lines);
        }
        Ok(result)
    }

    async fn reap_on_shutdown(&self, handles: &[ChildHandle]) -> Result<(), ReapError> {
        for handle in handles {
            signal_group(handle.pid, SIGTERM);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        for handle in handles {
            if is_alive(handle.pid) {
                signal_group(handle.pid, SIGKILL);
            }
        }
        Ok(())
    }

    async fn run_transient(
        &self,
        spec: &ProcessSpec,
        run_id: JobRunId,
    ) -> Result<TransientOutcome, SpawnError> {
        let started_at = Utc::now();
        let (mut child, stdout, stderr) = spawn_child(spec, true)?;
        attach_pumps(stdout, stderr, &self.log_sink, LogTarget::Run(run_id));
        let status = child.wait().await.map_err(|e| SpawnError::Io {
            name: spec.name.clone(),
            message: e.to_string(),
        })?;
        Ok(TransientOutcome {
            started_at,
            ended_at: Utc::now(),
            exit_code: status.code(),
        })
    }
}
