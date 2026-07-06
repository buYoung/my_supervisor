//! `MacLifecycle` — Direct-mode spawn/probe/reap for macOS (Unix spawn + setsid
//! groups; reconciliation compensates for the absence of a subreaper).

use std::collections::HashMap;
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

use crate::signals::{is_alive, signal_group, SIGKILL, SIGTERM};
use crate::spawn::{attach_pumps, spawn_child, LogTarget};

pub struct MacLifecycle {
    log_sink: Arc<dyn LogSink>,
    children: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
}

impl MacLifecycle {
    pub fn new(log_sink: Arc<dyn LogSink>) -> Self {
        MacLifecycle {
            log_sink,
            children: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn spawn_common(&self, spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        let (mut child, stdout, stderr) = spawn_child(spec)?;
        let process_id = Uuid::new_v4();
        let pid = child.id().unwrap_or(0);
        let started_at = Utc::now();

        attach_pumps(
            stdout,
            stderr,
            &self.log_sink,
            LogTarget::Process(spec.name.clone()),
        );

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
        self.spawn_common(spec)
    }

    async fn probe_alive(&self, handle: &ChildHandle) -> Result<Aliveness, ProbeError> {
        let tracked = self
            .children
            .lock()
            .unwrap()
            .get(&handle.process_id)
            .map(|a| a.load(Ordering::SeqCst))
            .unwrap_or(false);
        Ok(if tracked && is_alive(handle.pid) {
            Aliveness::Alive
        } else {
            Aliveness::Dead
        })
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
        let (mut child, stdout, stderr) = spawn_child(spec)?;
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
