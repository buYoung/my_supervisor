//! `OperationsFacade` — the transport-agnostic entry point every host adapter
//! (HTTP route or Tauri invoke) calls. No axum/HTTP types appear in any public
//! signature. Holds the in-memory runtime registry; durable specs/jobs live in
//! the repositories.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

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

#[derive(Clone)]
struct RuntimeEntry {
    handle: ChildHandle,
    state: ProcessState,
    /// Tied children are reaped on daemon shutdown; detached ones are left.
    tied: bool,
}

/// The single operations entry point shared by all host adapters.
pub struct OperationsFacade {
    deps: AppDeps,
    runner: Arc<dyn JobRunner>,
    runtime: Mutex<HashMap<String, RuntimeEntry>>,
    running_jobs: Arc<Mutex<HashSet<String>>>,
    events: broadcast::Sender<DomainEvent>,
    shutdown: Arc<Notify>,
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
            running_jobs: Arc::new(Mutex::new(HashSet::new())),
            events,
            shutdown: Arc::new(Notify::new()),
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
        if spec.command.trim().is_empty() {
            return Err(AppError::InvalidConfig("command must not be empty".into()));
        }
        if self.deps.state_repo.get_spec(&spec.name).await?.is_some() {
            return Err(AppError::conflict(ConflictReason::NameConflict));
        }
        self.deps.state_repo.save_spec(&spec).await?;
        if spec.autostart {
            self.start_process(&spec.name).await?;
        }
        self.build_status(&spec).await
    }

    pub async fn remove_process(&self, name: &str, force: bool) -> AppResult<()> {
        self.require_spec(name).await?;
        let running = self.is_running(name);
        if running && !force {
            return Err(AppError::conflict(ConflictReason::AlreadyRunning));
        }
        if running {
            self.stop_process(name, true).await.ok();
        }
        self.deps.state_repo.delete_spec(name).await?;
        Ok(())
    }

    pub async fn start_process(&self, name: &str) -> AppResult<()> {
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
                self.runtime.lock().unwrap().insert(
                    name.to_string(),
                    RuntimeEntry {
                        handle,
                        state: ProcessState::Running,
                        tied,
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
        match &spec.management_mode {
            ManagementMode::Direct => {
                let entry = self.runtime.lock().unwrap().get(name).cloned();
                let Some(entry) = entry else {
                    return Err(AppError::conflict(ConflictReason::NotRunning));
                };
                if force {
                    self.deps.shutdown.force_kill(&entry.handle).await?;
                } else {
                    self.deps
                        .shutdown
                        .request_graceful(&entry.handle, &spec.shutdown)
                        .await?;
                }
                self.runtime.lock().unwrap().remove(name);
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
        if let ManagementMode::SystemRegistered { .. } = spec.management_mode {
            // DD-025: the OS `Restart=` directive owns restart; in-daemon is a no-op.
            return Ok(RestartOutcome::Noop {
                reason: "managed_by_system".into(),
            });
        }
        // Reset crash-loop counter, then stop (ignore not-running) and start.
        self.deps.state_repo.set_restart_count(name, 0).await?;
        self.stop_process(name, false).await.ok();
        self.start_process(name).await?;
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
        let mut spec = self.require_spec(name).await?;
        let prior_mode = spec.management_mode.clone();

        // 1. Stop in the current mode (best effort).
        self.stop_process(name, false).await.ok();

        // 2. Tear down the current mode's trace.
        if let ManagementMode::SystemRegistered { unit_name } = &prior_mode {
            self.deps.registrar.unregister(unit_name).await.ok();
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
                self.deps.registrar.register(&unit, &spec).await?;
            }
        }

        // 4. Persist; roll the registration back if persistence fails.
        if let Err(e) = self.deps.state_repo.save_spec(&spec).await {
            if let ManagementMode::SystemRegistered { unit_name } = &spec.management_mode {
                self.deps.registrar.unregister(unit_name).await.ok();
            }
            return Err(e.into());
        }

        // 5. Optionally start in the new mode.
        if auto_start {
            if let Err(e) = self.start_process(name).await {
                tracing::warn!(process = %name, error = %e, "auto_start after convert failed");
            }
        }
        self.build_status(&spec).await
    }

    pub async fn process_logs(
        &self,
        name: &str,
        tail: usize,
        since: Option<DateTime<Utc>>,
    ) -> AppResult<LogPage> {
        self.require_spec(name).await?;
        let result = self.deps.log_sink.tail(name, tail, since).await;
        Ok(LogPage {
            lines: result.lines,
            truncated: result.truncated,
            dropped_count: 0,
        })
    }

    pub async fn subscribe_process_logs(&self, name: &str) -> AppResult<broadcast::Receiver<LogLine>> {
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
                        _ => {
                            self.runtime.lock().unwrap().remove(&spec.name);
                            (ProcessState::Stopped, None, None)
                        }
                    },
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
        if job.command.trim().is_empty() {
            return Err(AppError::InvalidRequest("command must not be empty".into()));
        }
        let mut all = self.deps.job_repo.list_jobs().await?;
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
        if job.command.trim().is_empty() {
            return Err(AppError::InvalidRequest("command must not be empty".into()));
        }
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
        {
            let running = self.running_jobs.lock().unwrap();
            if running.contains(name) {
                return Err(AppError::conflict(ConflictReason::AlreadyRunning));
            }
        }
        let run_id = JobRunId::new();
        self.spawn_run(job, TriggeredBy::Manual, run_id);
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

    /// Spawn a background run, tracking it in the overlap guard for its lifetime.
    fn spawn_run(&self, job: Job, triggered_by: TriggeredBy, run_id: JobRunId) {
        self.running_jobs.lock().unwrap().insert(job.name.clone());
        let runner = self.runner.clone();
        let running_jobs = self.running_jobs.clone();
        self.emit(DomainEvent::JobRunScheduled {
            name: job.name.clone(),
            run_id,
        });
        tokio::spawn(async move {
            let name = job.name.clone();
            let _ = runner.run(&job, triggered_by, run_id).await;
            running_jobs.lock().unwrap().remove(&name);
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
        for spec in loaded.processes {
            self.deps.state_repo.save_spec(&spec).await?;
        }
        for job in loaded.jobs {
            if self.deps.job_repo.get_job(&job.name).await?.is_none() {
                self.deps.job_repo.save_job(&job).await?;
                self.deps.scheduler.register(&job.name, &job.trigger).await.ok();
            }
        }
        Ok(())
    }

    pub fn request_shutdown(&self) {
        self.shutdown.notify_waiters();
    }

    /// Reap tied Direct-mode children during daemon shutdown so none are
    /// orphaned. Detached children are intentionally left running.
    pub async fn shutdown_children(&self) {
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
            self.deps
                .scheduler
                .register(&job.name, &job.trigger)
                .await
                .ok();
        }
        for spec in self.deps.state_repo.list_specs().await? {
            if spec.autostart {
                if let Err(e) = self.start_process(&spec.name).await {
                    tracing::warn!(process = %spec.name, error = %e, "autostart failed");
                }
            }
        }
        Ok(())
    }

    // -- Scheduler loop -----------------------------------------------------

    /// Drive scheduled runs. Hosts spawn this once after assembly.
    pub async fn run_scheduler_loop(self: Arc<Self>) {
        let mut rx = self.deps.scheduler.subscribe();
        loop {
            match rx.recv().await {
                Ok(event) => self.on_schedule_tick(&event.job_name).await,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn on_schedule_tick(&self, job_name: &str) {
        let job = match self.deps.job_repo.get_job(job_name).await {
            Ok(Some(job)) => job,
            _ => return,
        };
        let already_running = self.running_jobs.lock().unwrap().contains(job_name);
        if already_running && job.on_overlap == my_supervisor_core::domain::OverlapPolicy::Skip {
            let run = JobRun {
                run_id: JobRunId::new(),
                job_name: job_name.to_string(),
                triggered_by: TriggeredBy::Schedule,
                scheduled_at: self.deps.clock.now(),
                started_at: None,
                ended_at: Some(self.deps.clock.now()),
                exit_code: None,
                state: JobRunState::Skipped,
            };
            self.deps.job_repo.save_run(&run).await.ok();
            self.emit(DomainEvent::JobRunSkipped {
                name: job_name.to_string(),
                run_id: run.run_id,
                reason: "overlap_skip".into(),
            });
            return;
        }
        self.spawn_run(job, TriggeredBy::Schedule, JobRunId::new());
    }
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
