//! `OperationsFacade` — the transport-agnostic entry point every host adapter
//! (HTTP route or Tauri invoke) calls. No axum/HTTP types appear in any public
//! signature. Holds the in-memory runtime registry; durable specs/jobs live in
//! the repositories.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use backon::{BackoffBuilder, ExponentialBuilder};
use chrono::{DateTime, Utc};
use tokio::sync::{broadcast, Notify};

use my_supervisor_core::domain::{
    ChildHandle, Job, JobRun, JobRunId, JobRunState, JobTrigger, LifecycleMode, LogLine,
    ManagementMode, ProcessSpec, ProcessState, ProcessStatus, TriggeredBy,
};
use my_supervisor_core::ports::{Aliveness, JobRunner};

use crate::deps::AppDeps;
use crate::error::{AppError, AppResult, ConflictReason, ResourceKind};
use crate::events::DomainEvent;
use crate::runner::ProcessJobRunner;
use crate::views::{ConvertTarget, DaemonInfo, JobView, LogPage, RestartOutcome};

const EVENT_CHANNEL_CAPACITY: usize = 256;
const RECENT_RUNS_WINDOW: usize = 20;
const PROCESS_SUPERVISOR_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone)]
struct RuntimeEntry {
    handle: ChildHandle,
    state: ProcessState,
    /// Tied children are reaped on daemon shutdown; detached ones are left.
    tied: bool,
    restart_count_reset: bool,
    restart_reset_after: Duration,
}

struct QueuedRun {
    job: Job,
    triggered_by: TriggeredBy,
    run_id: JobRunId,
}

enum RunDispatch {
    Started,
    Queued,
    Skipped,
}

/// The single operations entry point shared by all host adapters.
pub struct OperationsFacade {
    deps: AppDeps,
    runner: Arc<dyn JobRunner>,
    runtime: Mutex<HashMap<String, RuntimeEntry>>,
    running_jobs: Arc<Mutex<HashMap<String, usize>>>,
    queued_jobs: Arc<Mutex<HashMap<String, VecDeque<QueuedRun>>>>,
    dependency_signatures: Mutex<HashMap<String, Vec<JobRunId>>>,
    restart_tokens: Mutex<HashMap<String, u64>>,
    pending_restarts: Mutex<HashSet<String>>,
    process_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    events: broadcast::Sender<DomainEvent>,
    shutdown: Arc<Notify>,
    is_shutting_down: AtomicBool,
}

impl OperationsFacade {
    pub fn new(deps: AppDeps) -> Arc<Self> {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let runner = Arc::new(ProcessJobRunner::new(
            deps.lifecycle.clone(),
            deps.job_repo.clone(),
            deps.clock.clone(),
            events.clone(),
        ));
        Arc::new(OperationsFacade {
            deps,
            runner,
            runtime: Mutex::new(HashMap::new()),
            running_jobs: Arc::new(Mutex::new(HashMap::new())),
            queued_jobs: Arc::new(Mutex::new(HashMap::new())),
            dependency_signatures: Mutex::new(HashMap::new()),
            restart_tokens: Mutex::new(HashMap::new()),
            pending_restarts: Mutex::new(HashSet::new()),
            process_locks: Mutex::new(HashMap::new()),
            events,
            shutdown: Arc::new(Notify::new()),
            is_shutting_down: AtomicBool::new(false),
        })
    }

    /// Subscribe to the domain-event stream (`/api/v1/events`).
    pub fn subscribe_events(&self) -> broadcast::Receiver<DomainEvent> {
        self.events.subscribe()
    }

    /// A handle the host awaits to perform a graceful shutdown on request.
    pub fn shutdown_signal(&self) -> Arc<Notify> {
        self.shutdown.clone()
    }

    fn emit(&self, event: DomainEvent) {
        let _ = self.events.send(event);
    }

    // -- Processes ----------------------------------------------------------

    pub async fn list_processes(&self) -> AppResult<Vec<ProcessStatus>> {
        let specs = self.deps.state_repo.list_specs().await?;
        let mut out = Vec::with_capacity(specs.len());
        for spec in &specs {
            out.push(self.build_status(spec).await?);
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub async fn get_process(&self, name: &str) -> AppResult<ProcessStatus> {
        let spec = self.require_spec(name).await?;
        self.build_status(&spec).await
    }

    pub async fn add_process(&self, spec: ProcessSpec) -> AppResult<ProcessStatus> {
        validate_process(&spec)?;
        if self.deps.state_repo.get_spec(&spec.name).await?.is_some() {
            return Err(AppError::conflict(ConflictReason::NameConflict));
        }
        if let ManagementMode::SystemRegistered { unit_name } = &spec.management_mode {
            self.deps.registrar.register(unit_name, &spec).await?;
        }

        if let Err(error) = self.deps.state_repo.save_spec(&spec).await {
            if let ManagementMode::SystemRegistered { unit_name } = &spec.management_mode {
                self.deps.registrar.unregister(unit_name).await.ok();
            }
            return Err(error.into());
        }
        if spec.autostart {
            if let Err(error) = self.start_process(&spec.name).await {
                self.deps.state_repo.delete_spec(&spec.name).await.ok();
                if let ManagementMode::SystemRegistered { unit_name } = &spec.management_mode {
                    self.deps.registrar.unregister(unit_name).await.ok();
                }
                return Err(error);
            }
        }
        self.build_status(&spec).await
    }

    pub async fn remove_process(&self, name: &str, force: bool) -> AppResult<()> {
        let spec = self.require_spec(name).await?;
        self.cancel_pending_restart(name);
        let process_lock = self.process_lock(name);
        let _guard = process_lock.lock().await;
        match &spec.management_mode {
            ManagementMode::Direct => {
                let running = self.is_running(name);
                if running && !force {
                    return Err(AppError::conflict(ConflictReason::AlreadyRunning));
                }
                if running {
                    self.stop_process_inner(name, true, &spec).await?;
                }
            }
            ManagementMode::SystemRegistered { unit_name } => {
                self.deps.registrar.unregister(unit_name).await?;
            }
        }
        self.deps.state_repo.delete_spec(name).await?;
        Ok(())
    }

    pub async fn start_process(&self, name: &str) -> AppResult<()> {
        self.cancel_pending_restart(name);
        let process_lock = self.process_lock(name);
        let _guard = process_lock.lock().await;
        self.start_process_inner(name).await
    }

    async fn start_process_inner(&self, name: &str) -> AppResult<()> {
        let spec = self.require_spec(name).await?;
        match &spec.management_mode {
            ManagementMode::Direct => {
                if self.probe_running(name).await {
                    return Err(AppError::conflict(ConflictReason::AlreadyRunning));
                }
                let tied = matches!(spec.lifecycle, LifecycleMode::Tied);
                let handle = match spec.lifecycle {
                    LifecycleMode::Tied => self.deps.lifecycle.spawn_tied(&spec).await?,
                    LifecycleMode::Detached => self.deps.lifecycle.spawn_detached(&spec).await?,
                };
                let persisted_handle = matches!(spec.lifecycle, LifecycleMode::Detached)
                    .then_some(&handle);
                if let Err(error) = self
                    .deps
                    .state_repo
                    .set_runtime_handle(name, persisted_handle)
                    .await
                {
                    self.deps.shutdown.force_kill(&handle).await.ok();
                    return Err(error.into());
                }
                self.runtime.lock().unwrap().insert(
                    name.to_string(),
                    RuntimeEntry {
                        handle,
                        state: ProcessState::Running,
                        tied,
                        restart_count_reset: false,
                        restart_reset_after: spec.restart.reset_after,
                    },
                );
                self.emit(DomainEvent::ProcessStateChanged {
                    name: name.to_string(),
                    from: ProcessState::Stopped,
                    to: ProcessState::Running,
                });
                Ok(())
            }
            ManagementMode::SystemRegistered { unit_name } => {
                self.deps.registrar.start(unit_name).await?;
                Ok(())
            }
        }
    }

    pub async fn stop_process(&self, name: &str, force: bool) -> AppResult<()> {
        let spec = self.require_spec(name).await?;
        let had_pending_restart = self.cancel_pending_restart(name);
        let process_lock = self.process_lock(name);
        let _guard = process_lock.lock().await;
        match self.stop_process_inner(name, force, &spec).await {
            Err(AppError::Conflict {
                reason: ConflictReason::NotRunning,
            }) if had_pending_restart => Ok(()),
            result => result,
        }
    }

    async fn stop_process_inner(
        &self,
        name: &str,
        force: bool,
        spec: &ProcessSpec,
    ) -> AppResult<()> {
        match &spec.management_mode {
            ManagementMode::Direct => {
                let entry = self.runtime.lock().unwrap().remove(name);
                let Some(entry) = entry else {
                    return Err(AppError::conflict(ConflictReason::NotRunning));
                };
                let stop_result = if force {
                    self.deps.shutdown.force_kill(&entry.handle).await
                } else {
                    self.deps
                        .shutdown
                        .request_graceful(&entry.handle, &spec.shutdown)
                        .await
                };
                if let Err(error) = stop_result {
                    self.runtime.lock().unwrap().insert(name.to_string(), entry);
                    return Err(error.into());
                }
                self.deps.state_repo.set_runtime_handle(name, None).await?;
                self.emit(DomainEvent::ProcessStateChanged {
                    name: name.to_string(),
                    from: ProcessState::Running,
                    to: ProcessState::Stopped,
                });
                Ok(())
            }
            ManagementMode::SystemRegistered { unit_name } => {
                self.deps.registrar.stop(unit_name).await?;
                Ok(())
            }
        }
    }

    pub async fn restart_process(&self, name: &str) -> AppResult<RestartOutcome> {
        let spec = self.require_spec(name).await?;
        self.cancel_pending_restart(name);
        let process_lock = self.process_lock(name);
        let _guard = process_lock.lock().await;
        if let ManagementMode::SystemRegistered { .. } = spec.management_mode {
            // DD-025: the OS `Restart=` directive owns restart; in-daemon is a no-op.
            return Ok(RestartOutcome::Noop {
                reason: "managed_by_system".into(),
            });
        }
        // Reset crash-loop counter, then stop if needed and start.
        self.deps.state_repo.set_restart_count(name, 0).await?;
        if self.is_running(name) {
            self.stop_process_inner(name, false, &spec).await?;
        }
        self.start_process_inner(name).await?;
        Ok(RestartOutcome::Accepted)
    }

    /// Convert a process between Direct and SystemRegistered modes (§6.4,
    /// DD-025). Transactional from the user's view: on adapter failure nothing
    /// is persisted and the prior mode is preserved (no residual unit).
    pub async fn convert_process(
        &self,
        name: &str,
        to: ConvertTarget,
        unit_name: Option<String>,
        auto_start: bool,
    ) -> AppResult<ProcessStatus> {
        self.cancel_pending_restart(name);
        let process_lock = self.process_lock(name);
        let _guard = process_lock.lock().await;
        let mut spec = self.require_spec(name).await?;
        let prior_spec = spec.clone();
        let was_running = self.build_status(&prior_spec).await?.state == ProcessState::Running;

        // 1. Stop in the current mode before changing any registration.
        if was_running {
            self.stop_process_inner(name, false, &prior_spec).await?;
        }

        // 2. Tear down the current mode's trace.
        if let ManagementMode::SystemRegistered { unit_name } = &prior_spec.management_mode {
            self.deps.registrar.unregister(unit_name).await?;
        }

        // 3. Establish the new mode (register before persisting so a failed
        //    registration leaves the prior mode untouched).
        match to {
            ConvertTarget::Direct => {
                spec.management_mode = ManagementMode::Direct;
            }
            ConvertTarget::SystemRegistered => {
                let unit = unit_name
                    .filter(|u| !u.trim().is_empty())
                    .unwrap_or_else(|| format!("com.my-supervisor.managed.{name}"));
                spec.management_mode = ManagementMode::SystemRegistered {
                    unit_name: unit.clone(),
                };
                if let Err(error) = self.deps.registrar.register(&unit, &spec).await {
                    self.restore_process_after_failed_convert(&prior_spec, was_running)
                        .await;
                    return Err(error.into());
                }
            }
        }

        // 4. Persist; roll the registration back if persistence fails.
        if let Err(e) = self.deps.state_repo.save_spec(&spec).await {
            if let ManagementMode::SystemRegistered { unit_name } = &spec.management_mode {
                self.deps.registrar.unregister(unit_name).await.ok();
            }
            self.restore_process_after_failed_convert(&prior_spec, was_running)
                .await;
            return Err(e.into());
        }

        // 5. Optionally start in the new mode.
        if auto_start {
            if let Err(e) = self.start_process_inner(name).await {
                if let ManagementMode::SystemRegistered { unit_name } = &spec.management_mode {
                    self.deps.registrar.unregister(unit_name).await.ok();
                }
                self.restore_process_after_failed_convert(&prior_spec, was_running)
                    .await;
                return Err(e);
            }
        }
        self.build_status(&spec).await
    }

    async fn restore_process_after_failed_convert(
        &self,
        prior_spec: &ProcessSpec,
        was_running: bool,
    ) {
        if self.deps.state_repo.save_spec(prior_spec).await.is_err() {
            return;
        }
        if let ManagementMode::SystemRegistered { unit_name } = &prior_spec.management_mode {
            self.deps.registrar.register(unit_name, prior_spec).await.ok();
        }
        if was_running {
            self.start_process_inner(&prior_spec.name).await.ok();
        }
    }

    pub async fn process_logs(
        &self,
        name: &str,
        tail: usize,
        since: Option<DateTime<Utc>>,
    ) -> AppResult<LogPage> {
        let spec = self.require_spec(name).await?;
        match &spec.management_mode {
            ManagementMode::Direct if matches!(spec.lifecycle, LifecycleMode::Detached) => {
                let mut lines = self
                    .deps
                    .lifecycle
                    .tail_detached_logs(&spec, tail)
                    .await
                    .map_err(|error| AppError::Internal(error.to_string()))?;
                if let Some(since) = since {
                    lines.retain(|line| line.timestamp >= since);
                }
                Ok(LogPage {
                    lines,
                    truncated: false,
                    dropped_count: 0,
                })
            }
            ManagementMode::Direct => {
                let result = self.deps.log_sink.tail(name, tail, since).await;
                Ok(LogPage {
                    lines: result.lines,
                    truncated: result.truncated,
                    dropped_count: 0,
                })
            }
            ManagementMode::SystemRegistered { unit_name } => {
                let mut lines = self.deps.registrar.tail_logs(unit_name, tail).await?;
                if let Some(since) = since {
                    lines.retain(|line| line.timestamp >= since);
                }
                Ok(LogPage {
                    lines,
                    truncated: false,
                    dropped_count: 0,
                })
            }
        }
    }

    pub async fn subscribe_process_logs(
        &self,
        name: &str,
    ) -> AppResult<broadcast::Receiver<LogLine>> {
        self.require_spec(name).await?;
        Ok(self.deps.log_sink.subscribe(name))
    }

    /// Live subscription to a job run's logs (`/api/v1/jobs/{name}/runs/{id}/logs`).
    pub fn subscribe_run_logs(&self, run_id: JobRunId) -> broadcast::Receiver<LogLine> {
        self.deps.log_sink.subscribe_run(run_id)
    }

    async fn build_status(&self, spec: &ProcessSpec) -> AppResult<ProcessStatus> {
        let restart_count = self
            .deps
            .state_repo
            .get_restart_count(&spec.name)
            .await
            .unwrap_or(0);
        let unit_name = match &spec.management_mode {
            ManagementMode::SystemRegistered { unit_name } => Some(unit_name.clone()),
            ManagementMode::Direct => None,
        };

        let (state, pid, started_at) = match &spec.management_mode {
            // SystemRegistered status is owned by the OS service manager.
            ManagementMode::SystemRegistered { unit_name } => {
                let state = self
                    .deps
                    .registrar
                    .query_status(unit_name)
                    .await
                    .unwrap_or(ProcessState::Stopped);
                (state, None, None)
            }
            // Direct status comes from the in-memory runtime registry + a probe.
            ManagementMode::Direct => {
                let entry = self.runtime.lock().unwrap().get(&spec.name).cloned();
                match entry {
                    Some(entry) => match self.deps.lifecycle.probe_alive(&entry.handle).await {
                        Ok(Aliveness::Alive) => (
                            entry.state,
                            Some(entry.handle.pid),
                            Some(entry.handle.started_at),
                        ),
                        _ => (ProcessState::Crashed, None, Some(entry.handle.started_at)),
                    },
                    None if self.pending_restarts.lock().unwrap().contains(&spec.name) => {
                        (ProcessState::Crashed, None, None)
                    }
                    None => (ProcessState::Stopped, None, None),
                }
            }
        };

        Ok(ProcessStatus {
            name: spec.name.clone(),
            state,
            management_mode: spec.management_mode.clone(),
            pid,
            unit_name,
            restart_count,
            started_at,
            cpu_percent: 0.0,
            memory_bytes: 0,
        })
    }

    fn is_running(&self, name: &str) -> bool {
        self.runtime.lock().unwrap().contains_key(name)
    }

    fn cancel_pending_restart(&self, name: &str) -> bool {
        let mut tokens = self.restart_tokens.lock().unwrap();
        let token = tokens.entry(name.to_string()).or_default();
        *token = token.wrapping_add(1);
        self.pending_restarts.lock().unwrap().remove(name)
    }

    fn restart_token(&self, name: &str) -> u64 {
        self.restart_tokens
            .lock()
            .unwrap()
            .get(name)
            .copied()
            .unwrap_or(0)
    }

    fn process_lock(&self, name: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.process_locks
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    async fn probe_running(&self, name: &str) -> bool {
        let entry = self.runtime.lock().unwrap().get(name).cloned();
        match entry {
            Some(entry) => {
                matches!(
                    self.deps.lifecycle.probe_alive(&entry.handle).await,
                    Ok(Aliveness::Alive)
                )
            }
            None => false,
        }
    }

    async fn require_spec(&self, name: &str) -> AppResult<ProcessSpec> {
        self.deps
            .state_repo
            .get_spec(name)
            .await?
            .ok_or_else(|| AppError::not_found(ResourceKind::Process, name))
    }

    // -- Jobs ---------------------------------------------------------------

    pub async fn list_jobs(&self) -> AppResult<Vec<JobView>> {
        let jobs = self.deps.job_repo.list_jobs().await?;
        let mut out = Vec::with_capacity(jobs.len());
        for job in &jobs {
            out.push(self.build_job_view(job, &jobs).await?);
        }
        out.sort_by(|a, b| a.job.name.cmp(&b.job.name));
        Ok(out)
    }

    pub async fn get_job(&self, name: &str) -> AppResult<JobView> {
        let all = self.deps.job_repo.list_jobs().await?;
        let job = all
            .iter()
            .find(|j| j.name == name)
            .cloned()
            .ok_or_else(|| AppError::not_found(ResourceKind::Job, name))?;
        self.build_job_view(&job, &all).await
    }

    pub async fn add_job(&self, job: Job) -> AppResult<JobView> {
        let mut all = self.deps.job_repo.list_jobs().await?;
        validate_job(&job, &all, self.deps.clock.now(), false)?;
        if all.iter().any(|j| j.name == job.name) {
            return Err(AppError::conflict(ConflictReason::JobNameConflict));
        }
        if forms_cycle(&job, &all) {
            return Err(AppError::CycleDetected);
        }
        self.deps.job_repo.save_job(&job).await?;
        if let Err(e) = self.deps.scheduler.register(&job.name, &job.trigger).await {
            // Roll back the persisted job if the scheduler rejects the trigger.
            self.deps.job_repo.delete_job(&job.name).await.ok();
            return Err(e.into());
        }
        all.push(job.clone());
        self.build_job_view(&job, &all).await
    }

    pub async fn update_job(&self, name: &str, mut job: Job) -> AppResult<JobView> {
        let all = self.deps.job_repo.list_jobs().await?;
        let existing = all
            .iter()
            .find(|j| j.name == name)
            .ok_or_else(|| AppError::not_found(ResourceKind::Job, name))?;
        // The path name and the existing identity win over the body.
        job.name = name.to_string();
        job.id = existing.id;
        // Cycle-check the replacement against the other jobs.
        let others: Vec<Job> = all.iter().filter(|j| j.name != name).cloned().collect();
        validate_job(&job, &others, self.deps.clock.now(), false)?;
        if forms_cycle(&job, &others) {
            return Err(AppError::CycleDetected);
        }
        self.deps.job_repo.save_job(&job).await?;
        if let Err(e) = self.deps.scheduler.register(&job.name, &job.trigger).await {
            return Err(e.into());
        }
        let mut updated = others;
        updated.push(job.clone());
        self.build_job_view(&job, &updated).await
    }

    pub async fn delete_job(&self, name: &str, force: bool) -> AppResult<()> {
        let all = self.deps.job_repo.list_jobs().await?;
        if !all.iter().any(|j| j.name == name) {
            return Err(AppError::not_found(ResourceKind::Job, name));
        }
        let downstream = downstream_of(name, &all);
        if !downstream.is_empty() && !force {
            return Err(AppError::conflict(ConflictReason::HasDependents));
        }
        self.deps.scheduler.unregister(name).await.ok();
        self.queued_jobs.lock().unwrap().remove(name);
        self.dependency_signatures.lock().unwrap().remove(name);
        self.deps.job_repo.delete_job(name).await?;
        Ok(())
    }

    pub async fn trigger_job(&self, name: &str) -> AppResult<JobRunId> {
        let job = self
            .deps
            .job_repo
            .get_job(name)
            .await?
            .ok_or_else(|| AppError::not_found(ResourceKind::Job, name))?;
        let run_id = JobRunId::new();
        if matches!(
            self.dispatch_run(job, TriggeredBy::Manual, run_id).await?,
            RunDispatch::Skipped
        ) {
            self.record_skipped_run(name, TriggeredBy::Manual, run_id, "overlap_skip")
                .await;
        }
        Ok(run_id)
    }

    pub async fn list_runs(&self, name: &str, limit: usize) -> AppResult<Vec<JobRun>> {
        if self.deps.job_repo.get_job(name).await?.is_none() {
            return Err(AppError::not_found(ResourceKind::Job, name));
        }
        Ok(self.deps.job_repo.list_runs(name, limit).await?)
    }

    pub async fn get_run(&self, name: &str, run_id: &JobRunId) -> AppResult<JobRun> {
        if self.deps.job_repo.get_job(name).await?.is_none() {
            return Err(AppError::not_found(ResourceKind::Job, name));
        }
        self.deps
            .job_repo
            .get_run(name, run_id)
            .await?
            .ok_or_else(|| AppError::not_found(ResourceKind::Run, run_id.0.to_string()))
    }

    async fn dispatch_run(
        &self,
        job: Job,
        triggered_by: TriggeredBy,
        run_id: JobRunId,
    ) -> AppResult<RunDispatch> {
        let name = job.name.clone();
        let pending_run = JobRun {
            run_id,
            job_name: name.clone(),
            triggered_by: triggered_by.clone(),
            scheduled_at: self.deps.clock.now(),
            started_at: None,
            ended_at: None,
            exit_code: None,
            state: JobRunState::Pending,
        };
        self.deps.job_repo.save_run(&pending_run).await?;
        let mut running_jobs = self.running_jobs.lock().unwrap();
        let running_count = running_jobs.get(&name).copied().unwrap_or(0);
        if running_count > 0 {
            match job.on_overlap {
                my_supervisor_core::domain::OverlapPolicy::Skip => {
                    return Ok(RunDispatch::Skipped);
                }
                my_supervisor_core::domain::OverlapPolicy::Queue => {
                    self.queued_jobs
                        .lock()
                        .unwrap()
                        .entry(name.clone())
                        .or_default()
                        .push_back(QueuedRun {
                            job,
                            triggered_by,
                            run_id,
                        });
                    drop(running_jobs);
                    self.emit(DomainEvent::JobRunScheduled { name, run_id });
                    return Ok(RunDispatch::Queued);
                }
                my_supervisor_core::domain::OverlapPolicy::Parallel => {}
            }
        }
        running_jobs.insert(name.clone(), running_count.saturating_add(1));
        drop(running_jobs);

        let runner = self.runner.clone();
        let running_jobs = self.running_jobs.clone();
        let queued_jobs = self.queued_jobs.clone();
        self.emit(DomainEvent::JobRunScheduled {
            name: name.clone(),
            run_id,
        });
        tokio::spawn(async move {
            let mut current = QueuedRun {
                job,
                triggered_by,
                run_id,
            };
            loop {
                let _ = runner
                    .run(&current.job, current.triggered_by, current.run_id)
                    .await;
                let next = {
                    let mut running = running_jobs.lock().unwrap();
                    let remaining = running
                        .get(&name)
                        .copied()
                        .unwrap_or(1)
                        .saturating_sub(1);
                    if remaining > 0 {
                        running.insert(name.clone(), remaining);
                        None
                    } else {
                        running.remove(&name);
                        let next = queued_jobs
                            .lock()
                            .unwrap()
                            .get_mut(&name)
                            .and_then(VecDeque::pop_front);
                        if next.is_some() {
                            running.insert(name.clone(), 1);
                        }
                        next
                    }
                };
                match next {
                    Some(next) => current = next,
                    None => break,
                }
            }
        });
        Ok(RunDispatch::Started)
    }

    async fn record_skipped_run(
        &self,
        job_name: &str,
        triggered_by: TriggeredBy,
        run_id: JobRunId,
        reason: &str,
    ) {
        let now = self.deps.clock.now();
        let run = JobRun {
            run_id,
            job_name: job_name.to_string(),
            triggered_by,
            scheduled_at: now,
            started_at: None,
            ended_at: Some(now),
            exit_code: None,
            state: JobRunState::Skipped,
        };
        self.deps.job_repo.save_run(&run).await.ok();
        self.emit(DomainEvent::JobRunSkipped {
            name: job_name.to_string(),
            run_id,
            reason: reason.into(),
        });
    }

    async fn build_job_view(&self, job: &Job, all: &[Job]) -> AppResult<JobView> {
        let now = self.deps.clock.now();
        let next_run_at = self.deps.scheduler.next_run(&job.trigger, now);
        let runs = self
            .deps
            .job_repo
            .list_runs(&job.name, RECENT_RUNS_WINDOW)
            .await
            .unwrap_or_default();
        let last_run = runs.first().cloned();
        let success_rate_recent = recent_success_rate(&runs);
        let upstream = match &job.trigger {
            JobTrigger::DependsOn(names) => names.clone(),
            _ => Vec::new(),
        };
        let downstream = downstream_of(&job.name, all);
        Ok(JobView {
            job: job.clone(),
            next_run_at,
            last_run,
            success_rate_recent,
            upstream,
            downstream,
        })
    }

    // -- Daemon -------------------------------------------------------------

    pub async fn daemon_status(&self) -> AppResult<DaemonInfo> {
        let process_count = self.deps.state_repo.list_specs().await?.len() as u32;
        Ok(DaemonInfo {
            version: self.deps.meta.version.clone(),
            started_at: self.deps.meta.started_at,
            pid: self.deps.meta.pid,
            process_count,
            config_path: self.deps.meta.config_path.display().to_string(),
            log_dir: self.deps.meta.log_dir.display().to_string(),
        })
    }

    pub async fn reload(&self) -> AppResult<()> {
        let loaded = self.deps.config.load().await?;
        let existing_processes = self.deps.state_repo.list_specs().await?;
        let mut process_names = HashSet::new();
        for spec in &loaded.processes {
            validate_process(spec)?;
            if !process_names.insert(spec.name.clone()) {
                return Err(AppError::InvalidConfig(format!(
                    "duplicate process '{}' in config",
                    spec.name
                )));
            }
            if let Some(existing) = existing_processes
                .iter()
                .find(|existing| existing.name == spec.name)
            {
                if existing.management_mode != spec.management_mode {
                    return Err(AppError::InvalidConfig(format!(
                        "process '{}' changes management mode; use msv convert",
                        spec.name
                    )));
                }
            }
        }

        let existing_jobs = self.deps.job_repo.list_jobs().await?;
        let mut merged_jobs = existing_jobs.clone();
        let mut job_names = HashSet::new();
        for job in &loaded.jobs {
            if !job_names.insert(job.name.clone()) {
                return Err(AppError::InvalidConfig(format!(
                    "duplicate job '{}' in config",
                    job.name
                )));
            }
            merged_jobs.retain(|existing| existing.name != job.name);
            merged_jobs.push(job.clone());
        }
        for job in &loaded.jobs {
            let others: Vec<Job> = merged_jobs
                .iter()
                .filter(|existing| existing.name != job.name)
                .cloned()
                .collect();
            let is_existing = existing_jobs.iter().any(|existing| existing.name == job.name);
            validate_job(job, &others, self.deps.clock.now(), is_existing)?;
            if forms_cycle(job, &others) {
                return Err(AppError::CycleDetected);
            }
        }

        for spec in loaded.processes {
            match existing_processes
                .iter()
                .find(|existing| existing.name == spec.name)
            {
                None => {
                    self.add_process(spec).await?;
                }
                Some(existing) => {
                    if let ManagementMode::SystemRegistered { unit_name } = &spec.management_mode {
                        if let Err(error) = self.deps.registrar.register(unit_name, &spec).await {
                            return Err(error.into());
                        }
                    }
                    if let Err(error) = self.deps.state_repo.save_spec(&spec).await {
                        if let ManagementMode::SystemRegistered { unit_name } =
                            &existing.management_mode
                        {
                            self.deps.registrar.register(unit_name, existing).await.ok();
                        }
                        return Err(error.into());
                    }
                }
            }
        }

        let mut current_job_names: HashSet<String> = existing_jobs
            .iter()
            .map(|job| job.name.clone())
            .collect();
        let mut pending_jobs = loaded.jobs;
        while !pending_jobs.is_empty() {
            let Some(index) = pending_jobs.iter().position(|job| match &job.trigger {
                JobTrigger::DependsOn(names) => {
                    names.iter().all(|name| current_job_names.contains(name))
                }
                _ => true,
            }) else {
                return Err(AppError::InvalidConfig(
                    "job dependencies could not be ordered".into(),
                ));
            };
            let job = pending_jobs.remove(index);
            if current_job_names.contains(&job.name) {
                let existing = self
                    .deps
                    .job_repo
                    .get_job(&job.name)
                    .await?
                    .ok_or_else(|| AppError::not_found(ResourceKind::Job, &job.name))?;
                let mut updated = job;
                updated.id = existing.id;
                self.deps.job_repo.save_job(&updated).await?;
                if let Err(error) = self
                    .deps
                    .scheduler
                    .register(&updated.name, &updated.trigger)
                    .await
                {
                    self.deps.job_repo.save_job(&existing).await.ok();
                    self.deps
                        .scheduler
                        .register(&existing.name, &existing.trigger)
                        .await
                        .ok();
                    return Err(error.into());
                }
            } else {
                self.add_job(job.clone()).await?;
                current_job_names.insert(job.name);
            }
        }
        Ok(())
    }

    pub fn request_shutdown(&self) {
        self.is_shutting_down.store(true, Ordering::SeqCst);
        self.shutdown.notify_waiters();
    }

    /// Reap tied Direct-mode children during daemon shutdown so none are
    /// orphaned. Detached children are intentionally left running.
    pub async fn shutdown_children(&self) {
        self.is_shutting_down.store(true, Ordering::SeqCst);
        let handles: Vec<ChildHandle> = self
            .runtime
            .lock()
            .unwrap()
            .values()
            .filter(|e| e.tied)
            .map(|e| e.handle.clone())
            .collect();
        if !handles.is_empty() {
            if let Err(e) = self.deps.lifecycle.reap_on_shutdown(&handles).await {
                tracing::warn!(error = %e, "reaping children on shutdown failed");
            }
        }
    }

    /// Startup sequence a host runs after assembly: load the config file into
    /// the repositories, arm the scheduler for every known job, and autostart
    /// processes flagged `autostart`.
    pub async fn bootstrap(&self) -> AppResult<()> {
        self.reload().await?;
        for job in self.deps.job_repo.list_jobs().await? {
            for mut run in self.deps.job_repo.list_runs(&job.name, 500).await? {
                if matches!(run.state, JobRunState::Pending | JobRunState::Running) {
                    run.state = JobRunState::Cancelled;
                    run.ended_at = Some(self.deps.clock.now());
                    self.deps.job_repo.save_run(&run).await?;
                }
            }
            self.deps
                .scheduler
                .register(&job.name, &job.trigger)
                .await
                .ok();
        }
        for spec in self.deps.state_repo.list_specs().await? {
            if matches!(spec.management_mode, ManagementMode::Direct)
                && matches!(spec.lifecycle, LifecycleMode::Detached)
                && self.restore_detached_process(&spec).await
            {
                continue;
            }
            if spec.autostart {
                if matches!(
                    self.build_status(&spec).await.map(|status| status.state),
                    Ok(ProcessState::Running)
                ) {
                    continue;
                }
                if let Err(e) = self.start_process(&spec.name).await {
                    tracing::warn!(process = %spec.name, error = %e, "autostart failed");
                }
            }
        }
        Ok(())
    }

    async fn restore_detached_process(&self, spec: &ProcessSpec) -> bool {
        let handle = match self.deps.state_repo.get_runtime_handle(&spec.name).await {
            Ok(Some(handle)) => handle,
            _ => return false,
        };
        if !matches!(self.deps.lifecycle.probe_alive(&handle).await, Ok(Aliveness::Alive)) {
            self.deps
                .state_repo
                .set_runtime_handle(&spec.name, None)
                .await
                .ok();
            return false;
        }
        self.runtime.lock().unwrap().insert(
            spec.name.clone(),
            RuntimeEntry {
                handle,
                state: ProcessState::Running,
                tied: false,
                restart_count_reset: false,
                restart_reset_after: spec.restart.reset_after,
            },
        );
        true
    }

    // -- Scheduler loop -----------------------------------------------------

    /// Drive scheduled runs. Hosts spawn this once after assembly.
    pub async fn run_scheduler_loop(self: Arc<Self>) {
        let mut schedule_events = self.deps.scheduler.subscribe();
        let mut domain_events = self.subscribe_events();
        loop {
            tokio::select! {
                result = schedule_events.recv() => match result {
                    Ok(event) => self.on_schedule_tick(&event.job_name).await,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                result = domain_events.recv() => match result {
                    Ok(DomainEvent::JobRunSucceeded { name, run_id, .. }) => {
                        self.on_dependency_completion(&name, run_id).await;
                    }
                    Ok(DomainEvent::JobRunFailed { name, run_id, .. }) => {
                        self.on_dependency_completion(&name, run_id).await;
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
            }
        }
    }

    /// Detect exited Direct-mode children and restart them with the configured
    /// backoff. SystemRegistered processes remain owned by the OS registrar.
    pub async fn run_process_supervisor_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(PROCESS_SUPERVISOR_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        while !self.is_shutting_down.load(Ordering::SeqCst) {
            interval.tick().await;
            self.reconcile_direct_processes().await;
        }
    }

    async fn reconcile_direct_processes(self: &Arc<Self>) {
        let entries: Vec<(String, RuntimeEntry)> = self
            .runtime
            .lock()
            .unwrap()
            .iter()
            .map(|(name, entry)| (name.clone(), entry.clone()))
            .collect();

        for (name, entry) in entries {
            if matches!(
                self.deps.lifecycle.probe_alive(&entry.handle).await,
                Ok(Aliveness::Alive)
            ) {
                let stable_for = self
                    .deps
                    .clock
                    .now()
                    .signed_duration_since(entry.handle.started_at)
                    .to_std()
                    .unwrap_or_default();
                if !entry.restart_count_reset && stable_for >= entry.restart_reset_after {
                    if self.deps.state_repo.set_restart_count(&name, 0).await.is_ok() {
                        let mut runtime = self.runtime.lock().unwrap();
                        if let Some(current) = runtime.get_mut(&name) {
                            if current.handle.process_id == entry.handle.process_id {
                                current.restart_count_reset = true;
                            }
                        }
                    }
                }
                continue;
            }

            let removed = {
                let mut runtime = self.runtime.lock().unwrap();
                let is_same_process = runtime
                    .get(&name)
                    .map(|current| current.handle.process_id == entry.handle.process_id)
                    .unwrap_or(false);
                if is_same_process {
                    runtime.remove(&name);
                }
                is_same_process
            };
            if !removed {
                continue;
            }
            self.deps.state_repo.set_runtime_handle(&name, None).await.ok();

            self.emit(DomainEvent::ProcessStateChanged {
                name: name.clone(),
                from: ProcessState::Running,
                to: ProcessState::Crashed,
            });

            let spec = match self.deps.state_repo.get_spec(&name).await {
                Ok(Some(spec)) => spec,
                _ => continue,
            };
            if !matches!(spec.management_mode, ManagementMode::Direct) || !spec.restart.enabled {
                continue;
            }

            let restart_count = self
                .deps
                .state_repo
                .get_restart_count(&name)
                .await
                .unwrap_or(0);
            if spec
                .restart
                .max_retries
                .is_some_and(|maximum| restart_count >= maximum)
            {
                continue;
            }

            let next_restart_count = restart_count.saturating_add(1);
            if self
                .deps
                .state_repo
                .set_restart_count(&name, next_restart_count)
                .await
                .is_err()
            {
                continue;
            }

            let Some(delay) = restart_delay(&spec.restart, restart_count) else {
                continue;
            };
            let restart_token = self.restart_token(&name);
            self.pending_restarts
                .lock()
                .unwrap()
                .insert(name.clone());
            let facade = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                if facade.restart_token(&name) == restart_token {
                    facade.pending_restarts.lock().unwrap().remove(&name);
                }
                if facade.is_shutting_down.load(Ordering::SeqCst)
                    || facade.is_running(&name)
                    || facade.restart_token(&name) != restart_token
                {
                    return;
                }
                let process_lock = facade.process_lock(&name);
                let _guard = process_lock.lock().await;
                if facade.is_shutting_down.load(Ordering::SeqCst)
                    || facade.is_running(&name)
                    || facade.restart_token(&name) != restart_token
                {
                    return;
                }
                match facade.start_process_inner(&name).await {
                    Ok(()) if facade.restart_token(&name) != restart_token => {
                        if let Ok(spec) = facade.require_spec(&name).await {
                            facade.stop_process_inner(&name, true, &spec).await.ok();
                        }
                    }
                    Ok(()) => {}
                    Err(error) => {
                        tracing::warn!(process = %name, error = %error, "automatic restart failed");
                    }
                }
            });
        }
    }

    async fn on_schedule_tick(&self, job_name: &str) {
        let job = match self.deps.job_repo.get_job(job_name).await {
            Ok(Some(job)) => job,
            _ => return,
        };
        let run_id = JobRunId::new();
        if matches!(
            self.dispatch_run(job, TriggeredBy::Schedule, run_id).await,
            Ok(RunDispatch::Skipped)
        ) {
            self.record_skipped_run(
                job_name,
                TriggeredBy::Schedule,
                run_id,
                "overlap_skip",
            )
                .await;
        }
    }

    async fn on_dependency_completion(&self, upstream_name: &str, upstream_run_id: JobRunId) {
        let jobs = match self.deps.job_repo.list_jobs().await {
            Ok(jobs) => jobs,
            Err(_) => return,
        };
        for job in jobs {
            let JobTrigger::DependsOn(upstream_names) = &job.trigger else {
                continue;
            };
            if !upstream_names.iter().any(|name| name == upstream_name) {
                continue;
            }

            let mut latest_runs = Vec::with_capacity(upstream_names.len());
            for name in upstream_names {
                let latest = self
                    .deps
                    .job_repo
                    .list_runs(name, 1)
                    .await
                    .ok()
                    .and_then(|runs| runs.into_iter().next());
                let Some(latest) = latest else {
                    latest_runs.clear();
                    break;
                };
                latest_runs.push(latest);
            }
            if latest_runs.len() != upstream_names.len() {
                continue;
            }

            let signature: Vec<JobRunId> = latest_runs.iter().map(|run| run.run_id).collect();
            {
                let mut signatures = self.dependency_signatures.lock().unwrap();
                if signatures.get(&job.name) == Some(&signature) {
                    continue;
                }
                signatures.insert(job.name.clone(), signature);
            }

            let all_succeeded = latest_runs
                .iter()
                .all(|run| run.state == JobRunState::Succeeded);
            let run_id = JobRunId::new();
            let triggered_by = TriggeredBy::Dependency {
                upstream_run_id,
            };
            if all_succeeded
                || job.on_dependency_failure
                    == my_supervisor_core::domain::DependencyFailurePolicy::RunAnyway
            {
                if matches!(
                    self.dispatch_run(job.clone(), triggered_by.clone(), run_id).await,
                    Ok(RunDispatch::Skipped)
                ) {
                    self.record_skipped_run(&job.name, triggered_by, run_id, "overlap_skip")
                        .await;
                }
            } else {
                self.record_skipped_run(&job.name, triggered_by, run_id, "dependency_failure")
                    .await;
            }
        }
    }
}

fn restart_delay(
    policy: &my_supervisor_core::domain::RestartPolicy,
    restart_count: u32,
) -> Option<Duration> {
    let builder = ExponentialBuilder::default()
        .with_factor(policy.backoff_multiplier as f32)
        .with_min_delay(policy.backoff_initial)
        .with_max_delay(policy.backoff_max)
        .without_max_times();
    let mut backoff = if policy.jitter {
        builder.with_jitter().build()
    } else {
        builder.build()
    };
    backoff
        .nth(restart_count as usize)
        .map(|delay| delay.min(policy.backoff_max))
}

fn validate_process(spec: &ProcessSpec) -> AppResult<()> {
    if spec.name.trim().is_empty() {
        return Err(AppError::InvalidConfig("name must not be empty".into()));
    }
    if spec.command.trim().is_empty() {
        return Err(AppError::InvalidConfig("command must not be empty".into()));
    }
    if spec.restart.backoff_multiplier == 0 {
        return Err(AppError::InvalidConfig(
            "restart.backoff_multiplier must be at least 1".into(),
        ));
    }
    if spec.restart.backoff_initial > spec.restart.backoff_max {
        return Err(AppError::InvalidConfig(
            "restart.backoff_initial_ms must not exceed restart.backoff_max_ms".into(),
        ));
    }
    if let ManagementMode::SystemRegistered { unit_name } = &spec.management_mode {
        if unit_name.trim().is_empty() {
            return Err(AppError::InvalidConfig("unit_name must not be empty".into()));
        }
    }
    Ok(())
}

fn validate_job(
    job: &Job,
    existing: &[Job],
    now: DateTime<Utc>,
    allow_past_one_shot: bool,
) -> AppResult<()> {
    if job.name.trim().is_empty() {
        return Err(AppError::InvalidRequest("name must not be empty".into()));
    }
    if job.command.trim().is_empty() {
        return Err(AppError::InvalidRequest("command must not be empty".into()));
    }
    if job.timeout.is_some_and(|timeout| timeout.is_zero()) {
        return Err(AppError::InvalidRequest(
            "timeout_sec must be greater than zero".into(),
        ));
    }
    match &job.trigger {
        JobTrigger::Interval(interval) if interval.is_zero() => {
            return Err(AppError::InvalidRequest(
                "interval must be greater than zero".into(),
            ));
        }
        JobTrigger::OneShot(at) if *at <= now && !allow_past_one_shot => {
            return Err(AppError::InvalidRequest(
                "one-shot time must be in the future".into(),
            ));
        }
        JobTrigger::DependsOn(names) => {
            if names.is_empty() {
                return Err(AppError::InvalidRequest(
                    "depends_on must contain at least one job".into(),
                ));
            }
            let mut unique = HashSet::new();
            for name in names {
                if name == &job.name {
                    return Err(AppError::CycleDetected);
                }
                if !unique.insert(name) {
                    return Err(AppError::InvalidRequest(format!(
                        "depends_on contains duplicate job '{name}'"
                    )));
                }
                if !existing.iter().any(|candidate| candidate.name == *name) {
                    return Err(AppError::InvalidRequest(format!(
                        "dependency job '{name}' is not registered"
                    )));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Detect whether adding `job` introduces a dependency cycle.
fn forms_cycle(job: &Job, existing: &[Job]) -> bool {
    let deps_of = |name: &str| -> Vec<String> {
        existing
            .iter()
            .find(|j| j.name == name)
            .map(|j| match &j.trigger {
                JobTrigger::DependsOn(names) => names.clone(),
                _ => Vec::new(),
            })
            .unwrap_or_default()
    };
    let JobTrigger::DependsOn(direct) = &job.trigger else {
        return false;
    };
    // DFS from each direct dependency; a cycle exists if we reach `job.name`.
    let mut stack: Vec<String> = direct.clone();
    let mut seen = HashSet::new();
    while let Some(current) = stack.pop() {
        if current == job.name {
            return true;
        }
        if !seen.insert(current.clone()) {
            continue;
        }
        stack.extend(deps_of(&current));
    }
    false
}

/// Names of jobs that directly depend on `name`.
fn downstream_of(name: &str, all: &[Job]) -> Vec<String> {
    all.iter()
        .filter(|j| match &j.trigger {
            JobTrigger::DependsOn(names) => names.iter().any(|n| n == name),
            _ => false,
        })
        .map(|j| j.name.clone())
        .collect()
}

fn recent_success_rate(runs: &[JobRun]) -> Option<f32> {
    let terminal: Vec<&JobRun> = runs.iter().filter(|r| r.state.is_terminal()).collect();
    if terminal.is_empty() {
        return None;
    }
    let succeeded = terminal
        .iter()
        .filter(|r| r.state == JobRunState::Succeeded)
        .count();
    Some(succeeded as f32 / terminal.len() as f32)
}

#[cfg(test)]
mod tests {
    use super::restart_delay;
    use my_supervisor_core::domain::RestartPolicy;
    use std::time::Duration;

    #[test]
    fn backoff_grows_and_honors_hard_maximum() {
        let policy = RestartPolicy {
            backoff_initial: Duration::from_millis(100),
            backoff_max: Duration::from_millis(250),
            backoff_multiplier: 2,
            jitter: false,
            ..RestartPolicy::default()
        };
        assert_eq!(restart_delay(&policy, 0), Some(Duration::from_millis(100)));
        let second_delay = restart_delay(&policy, 1).expect("second delay");
        assert!(second_delay >= Duration::from_millis(199));
        assert!(second_delay <= Duration::from_millis(201));
        assert_eq!(restart_delay(&policy, 2), Some(Duration::from_millis(250)));
        assert_eq!(restart_delay(&policy, 20), Some(Duration::from_millis(250)));
    }
}
