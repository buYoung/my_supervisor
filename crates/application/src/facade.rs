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
use tokio::sync::{broadcast, oneshot, watch, Notify};
use tokio::task::JoinHandle;

use my_supervisor_core::domain::{
    ApplyMode, ChildHandle, ConfigApplyJournal, ConfigApplyResult, ConfigApplyStage, ConfigSnapshot,
    DependencySignature, Job, JobDeletionJournal, JobDeletionStage, JobId, JobRun, JobRunId, JobRunState, JobTrigger, LifecycleMode,
    LogLine, ManagementMode, ProcessResourceUsage, ProcessSpec, ProcessState, ProcessStatus,
    TriggeredBy,
};
use my_supervisor_core::ports::{
    Aliveness, CleanupTicket, JobRepository, JobRunner, LogSink, RunExecutionControl,
    TransientCleanupStage, TransientCompletion, TransientTerminalEvent,
};
use my_supervisor_core::ports::error::RunnerError;

use crate::deps::AppDeps;
use crate::config_apply::{diff as config_diff, target_snapshot};
use crate::error::{AppError, AppResult, ConflictReason, ResourceKind};
use crate::events::{DomainEvent, PublishedEvent};
use crate::runner::ProcessJobRunner;
use crate::views::{
    ConvertTarget, DaemonInfo, JobView, LogPage, RecoveryDiagnostic, RecoveryDiagnostics,
    RestartOutcome,
};

const EVENT_CHANNEL_CAPACITY: usize = 256;
const RECENT_RUNS_WINDOW: usize = 20;
const PROCESS_SUPERVISOR_INTERVAL: Duration = Duration::from_millis(250);
const LOG_RETENTION_CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);
const RUNTIME_HANDLE_CLEANUP_INTERVAL: Duration = Duration::from_secs(30);
const TRANSIENT_CLEANUP_INTERVAL: Duration = Duration::from_secs(5);
const JOB_DELETION_RECOVERY_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct RuntimeEntry {
    handle: ChildHandle,
    state: ProcessState,
    /// Tied children are reaped on daemon shutdown; detached ones are left.
    tied: bool,
    restart_count_reset: bool,
    restart_reset_after: Duration,
}

struct ActiveRun {
    job: Job,
    triggered_by: TriggeredBy,
    run_id: JobRunId,
    cancellation: watch::Sender<bool>,
    child: Mutex<Option<ChildHandle>>,
    runner: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActiveRunPhase {
    Queued,
    Starting,
    Running,
    Tombstoned,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobDeletionResumeIntent {
    Foreground,
    Recovery,
}

fn is_pre_cancellation_deletion_stage(stage: JobDeletionStage) -> bool {
    matches!(
        stage,
        JobDeletionStage::Prepared
            | JobDeletionStage::DispatchFrozen
            | JobDeletionStage::SchedulerUnregistered
            | JobDeletionStage::RollbackRequired
    )
}

struct ActiveRunRecord {
    run: Arc<ActiveRun>,
    phase: ActiveRunPhase,
}

#[derive(Default)]
struct ActiveRunState {
    runs: HashMap<JobRunId, ActiveRunRecord>,
    runs_by_job: HashMap<JobId, HashSet<JobRunId>>,
    queued_by_job: HashMap<JobId, VecDeque<JobRunId>>,
    // Durable cleanup is owned by the repository and the lifecycle adapter.
    // Keep only the Job/Run relationship needed to block destructive actions
    // in this daemon instance; never retain a completed runner JoinHandle.
    cleanup_by_job: HashMap<JobId, HashSet<JobRunId>>,
    tombstoned_jobs: HashSet<JobId>,
    frozen_jobs: HashSet<JobId>,
}

#[derive(Default)]
struct ActiveRunRegistry {
    state: Mutex<ActiveRunState>,
}

enum CancelledRun {
    Queued(Arc<ActiveRun>),
    Active,
    Missing,
}

struct DrainedRuns {
    queued: Vec<Arc<ActiveRun>>,
    runners: Vec<JoinHandle<()>>,
}

impl ActiveRunRegistry {
    fn register(&self, job: Job, triggered_by: TriggeredBy, run_id: JobRunId, phase: ActiveRunPhase) -> Arc<ActiveRun> {
        let (cancellation, _) = watch::channel(false);
        let run = Arc::new(ActiveRun {
            job,
            triggered_by,
            run_id,
            cancellation,
            child: Mutex::new(None),
            runner: Mutex::new(None),
        });
        let mut state = self.state.lock().unwrap();
        state
            .runs_by_job
            .entry(run.job.id)
            .or_default()
            .insert(run_id);
        state.runs.insert(run_id, ActiveRunRecord { run: run.clone(), phase });
        run
    }

    fn running_count(&self, job_id: JobId) -> usize {
        let state = self.state.lock().unwrap();
        state
            .runs_by_job
            .get(&job_id)
            .into_iter()
            .flatten()
            .filter_map(|run_id| state.runs.get(run_id))
            .filter(|record| matches!(record.phase, ActiveRunPhase::Starting | ActiveRunPhase::Running))
            .count()
    }

    fn has_runs(&self, job_id: JobId) -> bool {
        let state = self.state.lock().unwrap();
        state.runs_by_job.get(&job_id).is_some_and(|run_ids| !run_ids.is_empty())
            || state.cleanup_by_job.get(&job_id).is_some_and(|run_ids| !run_ids.is_empty())
    }

    fn freeze_dispatch(&self, job_id: JobId) {
        self.state.lock().unwrap().frozen_jobs.insert(job_id);
    }

    fn thaw_dispatch(&self, job_id: JobId) {
        self.state.lock().unwrap().frozen_jobs.remove(&job_id);
    }

    fn is_dispatch_frozen(&self, job_id: JobId) -> bool {
        self.state.lock().unwrap().frozen_jobs.contains(&job_id)
    }

    fn queued_runs(&self, job_id: JobId) -> Vec<Arc<ActiveRun>> {
        let state = self.state.lock().unwrap();
        state.queued_by_job.get(&job_id).into_iter().flatten()
            .filter_map(|run_id| state.runs.get(run_id))
            .map(|record| record.run.clone())
            .collect()
    }

    fn enqueue(&self, run: &ActiveRun) {
        self.state
            .lock()
            .unwrap()
            .queued_by_job
            .entry(run.job.id)
            .or_default()
            .push_back(run.run_id);
    }

    fn set_runner(&self, run_id: JobRunId, runner: JoinHandle<()>) {
        if let Some(record) = self.state.lock().unwrap().runs.get(&run_id) {
            *record.run.runner.lock().unwrap() = Some(runner);
        }
    }

    fn publish_child(&self, run_id: JobRunId, child: ChildHandle) {
        let mut state = self.state.lock().unwrap();
        if let Some(record) = state.runs.get_mut(&run_id) {
            *record.run.child.lock().unwrap() = Some(child);
            if record.phase != ActiveRunPhase::Tombstoned {
                record.phase = ActiveRunPhase::Running;
            }
        }
    }

    fn cancellation(&self, run_id: JobRunId) -> Option<watch::Receiver<bool>> {
        self.state
            .lock()
            .unwrap()
            .runs
            .get(&run_id)
            .map(|record| record.run.cancellation.subscribe())
    }

    fn should_persist_terminal(&self, run_id: JobRunId) -> bool {
        let state = self.state.lock().unwrap();
        state
            .runs
            .get(&run_id)
            .is_some_and(|record| {
                record.phase != ActiveRunPhase::Tombstoned
                    && !state.tombstoned_jobs.contains(&record.run.job.id)
            })
    }

    /// Cancel a Run only when it is still owned by the Job selected by the
    /// caller.  Ownership comparison and every mutable registry operation are
    /// deliberately kept under this one lock: checking ownership after
    /// sending the cancellation signal lets `/jobs/A/...` cancel Job B.
    fn cancel_owned(&self, job_id: JobId, run_id: JobRunId) -> CancelledRun {
        let mut state = self.state.lock().unwrap();
        let Some(record) = state.runs.get(&run_id) else {
            return CancelledRun::Missing;
        };
        if record.run.job.id != job_id {
            return CancelledRun::Missing;
        }
        let run = record.run.clone();
        let phase = record.phase;
        run.cancellation.send_replace(true);
        if phase != ActiveRunPhase::Queued {
            return CancelledRun::Active;
        }
        state
            .queued_by_job
            .entry(run.job.id)
            .or_default()
            .retain(|queued_run_id| *queued_run_id != run_id);
        state.runs.remove(&run_id);
        if let Some(run_ids) = state.runs_by_job.get_mut(&run.job.id) {
            run_ids.remove(&run_id);
            if run_ids.is_empty() {
                state.runs_by_job.remove(&run.job.id);
            }
        }
        CancelledRun::Queued(run)
    }

    /// Stop accepting work for one job and take ownership of all currently
    /// spawned runner tasks.  Awaiting the returned handles is the drain
    /// barrier: a caller must not remove durable state before it completes.
    fn begin_drain(&self, job_id: JobId, tombstone: bool) -> DrainedRuns {
        let mut state = self.state.lock().unwrap();
        if tombstone {
            state.tombstoned_jobs.insert(job_id);
        }
        let run_ids: Vec<JobRunId> = state
            .runs_by_job
            .get(&job_id)
            .into_iter()
            .flatten()
            .copied()
            .collect();
        let mut queued = Vec::new();
        let mut runners = Vec::new();
        for run_id in run_ids {
            let Some(record) = state.runs.get_mut(&run_id) else {
                continue;
            };
            record.run.cancellation.send_replace(true);
            if record.phase == ActiveRunPhase::Queued {
                queued.push(record.run.clone());
                continue;
            }
            if tombstone {
                record.phase = ActiveRunPhase::Tombstoned;
            }
            if let Some(runner) = record.run.runner.lock().unwrap().take() {
                runners.push(runner);
            }
        }
        for queued_run in &queued {
            state.runs.remove(&queued_run.run_id);
            if let Some(run_ids) = state.runs_by_job.get_mut(&job_id) {
                run_ids.remove(&queued_run.run_id);
            }
        }
        state.queued_by_job.remove(&job_id);
        if state
            .runs_by_job
            .get(&job_id)
            .is_some_and(HashSet::is_empty)
        {
            state.runs_by_job.remove(&job_id);
        }
        DrainedRuns { queued, runners }
    }

    fn finish(&self, run_id: JobRunId) -> Option<Arc<ActiveRun>> {
        let mut state = self.state.lock().unwrap();
        let record = state.runs.remove(&run_id)?;
        *record.run.child.lock().unwrap() = None;
        let job_id = record.run.job.id;
        if let Some(run_ids) = state.runs_by_job.get_mut(&record.run.job.id) {
            run_ids.remove(&run_id);
        }
        if state
            .runs_by_job
            .get(&job_id)
            .is_some_and(HashSet::is_empty)
        {
            state.runs_by_job.remove(&job_id);
        }
        if state
            .runs_by_job
            .get(&job_id)
            .into_iter()
            .flatten()
            .filter_map(|other_run_id| state.runs.get(other_run_id))
            .any(|other| matches!(other.phase, ActiveRunPhase::Starting | ActiveRunPhase::Running))
        {
            return None;
        }
        let next_run_id = state
            .queued_by_job
            .get_mut(&job_id)
            .and_then(VecDeque::pop_front)?;
        let next = state.runs.get_mut(&next_run_id)?;
        next.phase = ActiveRunPhase::Starting;
        Some(next.run.clone())
    }

    fn finish_unreaped(&self, run_id: JobRunId) -> Option<Arc<ActiveRun>> {
        let mut state = self.state.lock().unwrap();
        let record = state.runs.remove(&run_id)?;
        let job_id = record.run.job.id;
        state.cleanup_by_job.entry(job_id).or_default().insert(run_id);
        if let Some(run_ids) = state.runs_by_job.get_mut(&job_id) {
            run_ids.remove(&run_id);
        }
        if state.runs_by_job.get(&job_id).is_some_and(HashSet::is_empty) {
            state.runs_by_job.remove(&job_id);
        }
        let next_run_id = state.queued_by_job.get_mut(&job_id).and_then(VecDeque::pop_front)?;
        let next = state.runs.get_mut(&next_run_id)?;
        next.phase = ActiveRunPhase::Starting;
        Some(next.run.clone())
    }

    fn finish_cleanup(&self, job_id: JobId, run_id: JobRunId) {
        let mut state = self.state.lock().unwrap();
        if let Some(run_ids) = state.cleanup_by_job.get_mut(&job_id) {
            run_ids.remove(&run_id);
            if run_ids.is_empty() {
                state.cleanup_by_job.remove(&job_id);
            }
        }
    }
}

struct ActiveRunControl {
    registry: Arc<ActiveRunRegistry>,
    run_id: JobRunId,
}

#[async_trait::async_trait]
impl RunExecutionControl for ActiveRunControl {
    async fn publish_child(&self, child: ChildHandle) {
        self.registry.publish_child(self.run_id, child);
    }

    fn child(&self) -> Option<ChildHandle> {
        let state = self.registry.state.lock().unwrap();
        state
            .runs
            .get(&self.run_id)
            .and_then(|record| record.run.child.lock().unwrap().clone())
    }

    fn cancellation(&self) -> watch::Receiver<bool> {
        self.registry
            .cancellation(self.run_id)
            .unwrap_or_else(|| {
                let (sender, receiver) = watch::channel(true);
                drop(sender);
                receiver
            })
    }

    fn should_persist_terminal(&self) -> bool {
        self.registry.should_persist_terminal(self.run_id)
    }
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
    active_runs: Arc<ActiveRunRegistry>,
    restart_tokens: Mutex<HashMap<String, u64>>,
    pending_restarts: Mutex<HashSet<String>>,
    process_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    job_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    events: broadcast::Sender<PublishedEvent>,
    internal_events: broadcast::Sender<PublishedEvent>,
    terminal_events_in_flight: Mutex<HashSet<uuid::Uuid>>,
    shutdown: Arc<Notify>,
    is_shutting_down: AtomicBool,
    is_applying_config: AtomicBool,
}

impl OperationsFacade {
    pub fn new(deps: AppDeps) -> Arc<Self> {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let (internal_events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let runner = Arc::new(ProcessJobRunner::new(
            deps.lifecycle.clone(),
            deps.job_repo.clone(),
            deps.log_sink.clone(),
            deps.clock.clone(),
            events.clone(),
            internal_events.clone(),
        ));
        Arc::new(OperationsFacade {
            deps,
            runner,
            runtime: Mutex::new(HashMap::new()),
            active_runs: Arc::new(ActiveRunRegistry::default()),
            restart_tokens: Mutex::new(HashMap::new()),
            pending_restarts: Mutex::new(HashSet::new()),
            process_locks: Mutex::new(HashMap::new()),
            job_locks: Mutex::new(HashMap::new()),
            events,
            internal_events,
            terminal_events_in_flight: Mutex::new(HashSet::new()),
            shutdown: Arc::new(Notify::new()),
            is_shutting_down: AtomicBool::new(false),
            is_applying_config: AtomicBool::new(false),
        })
    }

    /// Subscribe to the domain-event stream (`/api/v1/events`).
    pub fn subscribe_events(&self) -> broadcast::Receiver<PublishedEvent> {
        self.events.subscribe()
    }

    /// A handle the host awaits to perform a graceful shutdown on request.
    pub fn shutdown_signal(&self) -> Arc<Notify> {
        self.shutdown.clone()
    }

    fn emit(&self, event: DomainEvent) {
        let _ = self.events.send(PublishedEvent::ordinary(event));
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
                // A stale map entry must not make removal require `--force` or
                // cause a signal to be sent to a recycled PID.
                let running = self.probe_running(name).await?;
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
                if self.probe_running(name).await? {
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
                let entry = self.runtime.lock().unwrap().get(name).cloned();
                let Some(entry) = entry else {
                    return Err(AppError::conflict(ConflictReason::NotRunning));
                };
                self.runtime
                    .lock()
                    .unwrap()
                    .entry(name.to_string())
                    .and_modify(|current| current.state = ProcessState::Stopping);
                let stop_result = if force {
                    self.deps.shutdown.force_kill(&entry.handle).await
                } else {
                    self.deps
                        .shutdown
                        .request_graceful(&entry.handle, &spec.shutdown)
                        .await
                };
                if let Err(error) = stop_result {
                    self.runtime
                        .lock()
                        .unwrap()
                        .entry(name.to_string())
                        .and_modify(|current| current.state = ProcessState::Running);
                    return Err(error.into());
                }
                // OS termination is authoritative.  Never restore a dead
                // handle to the in-memory running map just because durable
                // cleanup is temporarily unavailable.
                self.runtime.lock().unwrap().remove(name);
                self.emit(DomainEvent::ProcessStateChanged {
                    name: name.to_string(),
                    from: ProcessState::Running,
                    to: ProcessState::Stopped,
                });
                if let Err(error) = self.deps.state_repo.set_runtime_handle(name, None).await {
                    if let Err(queue_error) = self
                        .deps
                        .state_repo
                        .enqueue_runtime_handle_cleanup(name, &entry.handle, &error.to_string())
                        .await
                    {
                        tracing::error!(process = %name, %queue_error, "failed to record runtime-handle cleanup");
                    }
                    return Err(error.into());
                }
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
        self.process_logs_with_cursor(name, tail, since, None).await
    }

    pub async fn process_logs_with_cursor(
        &self,
        name: &str,
        tail: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
    ) -> AppResult<LogPage> {
        let spec = self.require_spec(name).await?;
        let known_process_names = self
            .deps
            .state_repo
            .list_specs()
            .await?
            .into_iter()
            .map(|process| process.name)
            .collect::<Vec<_>>();
        self.deps.log_sink.register_process_names(&known_process_names);
        match &spec.management_mode {
            ManagementMode::Direct if matches!(spec.lifecycle, LifecycleMode::Detached) => {
                let result = self
                    .deps
                    .lifecycle
                    .tail_detached_logs(&spec, tail, since, after_sequence, &known_process_names)
                    .await
                    .map_err(|error| AppError::Internal(error.to_string()))?;
                Ok(LogPage {
                    lines: result.lines,
                    truncated: result.truncated,
                    dropped_count: 0,
                    high_watermark: result.high_watermark,
                    next_sequence: result.next_sequence,
                })
            }
            ManagementMode::Direct => {
                let result = self.deps.log_sink.tail(name, tail, since, after_sequence).await;
                Ok(LogPage {
                    lines: result.lines,
                    truncated: result.truncated,
                    dropped_count: 0,
                    high_watermark: result.high_watermark,
                    next_sequence: result.next_sequence,
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
                    high_watermark: 0,
                    next_sequence: 1,
                })
            }
        }
    }

    pub async fn subscribe_process_logs(
        &self,
        name: &str,
    ) -> AppResult<broadcast::Receiver<LogLine>> {
        let spec = self.require_spec(name).await?;
        if matches!(spec.management_mode, ManagementMode::Direct)
            && matches!(spec.lifecycle, LifecycleMode::Detached)
        {
            return self
                .deps
                .lifecycle
                .subscribe_detached_logs(&spec)
                .await
                .map_err(|error| AppError::Internal(error.to_string()));
        }
        Ok(self.deps.log_sink.subscribe(name))
    }

    /// Live subscription to a job run's logs (`/api/v1/jobs/{name}/runs/{id}/logs`).
    pub fn subscribe_run_logs(&self, run_id: JobRunId) -> broadcast::Receiver<LogLine> {
        self.deps.log_sink.subscribe_run(run_id)
    }

    /// Returns a Run journal page using the same filter-before-limit cursor
    /// contract as process journals.  Keeping the predicates at the sink
    /// boundary lets a reconnect recover entries that have already left the
    /// in-memory ring.
    pub async fn run_logs(
        &self,
        name: &str,
        run_id: JobRunId,
        tail: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
    ) -> AppResult<LogPage> {
        self.get_run(name, &run_id).await?;
        let result = self
            .deps
            .log_sink
            .tail_run(run_id, tail, since, after_sequence)
            .await;
        Ok(LogPage { lines: result.lines, truncated: result.truncated, dropped_count: 0, high_watermark: result.high_watermark, next_sequence: result.next_sequence })
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
                let pid = if state == ProcessState::Running {
                    self.deps.registrar.query_pid(unit_name).await.unwrap_or(None)
                } else {
                    None
                };
                (state, pid, None)
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

        let runtime_handle = {
            self.runtime
                .lock()
                .unwrap()
                .get(&spec.name)
                .map(|entry| entry.handle.clone())
        };
        // System-registered adapters have no durable child identity.  They may
        // explicitly decline this synthetic, non-signalable handle; this keeps
        // Direct-mode sampling PID-safe while preserving adapter-provided
        // service metrics where available.
        let usage_handle = runtime_handle.or_else(|| pid.map(|pid| ChildHandle {
            process_id: uuid::Uuid::nil(),
            pid,
            pgid: None,
            generation: None,
            started_at: self.deps.clock.now(),
        }));
        let usage = match usage_handle {
            Some(handle) => self
                .deps
                .lifecycle
                .resource_usage(&handle)
                .await
                .unwrap_or_default(),
            None => ProcessResourceUsage::default(),
        };

        Ok(ProcessStatus {
            name: spec.name.clone(),
            state,
            management_mode: spec.management_mode.clone(),
            pid,
            unit_name,
            restart_count,
            started_at,
            cpu_percent: usage.cpu_percent,
            memory_bytes: usage.memory_bytes,
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

    fn job_lock(&self, name: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.job_locks
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// A probe error is not evidence that a process exited.  Propagating it
    /// keeps callers from deleting a durable handle or spawning a duplicate.
    async fn probe_running(&self, name: &str) -> AppResult<bool> {
        let entry = self.runtime.lock().unwrap().get(name).cloned();
        match entry {
            Some(entry) => self
                .deps
                .lifecycle
                .probe_alive(&entry.handle)
                .await
                .map(|aliveness| matches!(aliveness, Aliveness::Alive))
                .map_err(|error| AppError::Internal(format!("process_state_unverified: {error}"))),
            None => Ok(false),
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
            // A scheduler rejection must not leave the durable definition
            // different from the trigger that is still armed in memory.
            let original_error = e.to_string();
            let restore_store = self.deps.job_repo.save_job(existing).await;
            let restore_scheduler = self
                .deps
                .scheduler
                .register(&existing.name, &existing.trigger)
                .await;
            if let Err(restore_error) = restore_store {
                return Err(AppError::Internal(format!(
                    "scheduler update failed: {original_error}; restoring job definition failed: {restore_error}"
                )));
            }
            if let Err(restore_error) = restore_scheduler {
                return Err(AppError::Internal(format!(
                    "scheduler update failed: {original_error}; restoring previous schedule failed: {restore_error}"
                )));
            }
            return Err(e.into());
        }
        let mut updated = others;
        updated.push(job.clone());
        self.build_job_view(&job, &updated).await
    }

    pub async fn delete_job(&self, name: &str, force: bool) -> AppResult<()> {
        let job_lock = self.job_lock(name);
        let _guard = job_lock.lock().await;
        if let Some(journal) = self.deps.job_repo.get_job_deletion_journal(name).await? {
            return self.resume_job_deletion(journal, JobDeletionResumeIntent::Foreground).await;
        }
        let all = self.deps.job_repo.list_jobs().await?;
        let job = all
            .iter()
            .find(|job| job.name == name)
            .ok_or_else(|| AppError::not_found(ResourceKind::Job, name))?;
        let downstream = downstream_of(name, &all);
        // `force` authorizes cancellation of this job's own work; it never
        // silently rewrites a dependency graph.  A dependent must be changed
        // or removed in the same future batch operation.
        if !downstream.is_empty() {
            return Err(AppError::conflict(ConflictReason::HasDependents));
        }
        if !force && self.active_runs.has_runs(job.id) {
            return Err(AppError::conflict(ConflictReason::JobHasActiveRuns));
        }
        let journal = JobDeletionJournal {
            deletion_id: uuid::Uuid::new_v4(),
            job: job.clone(),
            stage: JobDeletionStage::Prepared,
            run_ids: Vec::new(),
            last_error: None,
        };
        self.deps.job_repo.create_job_deletion_journal(&journal).await?;
        let journal = self.deps.job_repo.get_job_deletion_journal(name).await?
            .ok_or_else(|| AppError::Internal("job deletion journal disappeared after prepare".into()))?;
        self.resume_job_deletion(journal, JobDeletionResumeIntent::Foreground).await
    }

    pub async fn trigger_job(&self, name: &str) -> AppResult<JobRunId> {
        let job = self
            .deps
            .job_repo
            .get_job(name)
            .await?
            .ok_or_else(|| AppError::not_found(ResourceKind::Job, name))?;
        if self.active_runs.is_dispatch_frozen(job.id) {
            return Err(AppError::JobDeletionRecoveryRequired(format!("{name} deletion is in progress")));
        }
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

    pub async fn list_runs_filtered(
        &self,
        name: &str,
        state: Option<JobRunState>,
        since: Option<DateTime<Utc>>,
        limit: usize,
    ) -> AppResult<Vec<JobRun>> {
        if self.deps.job_repo.get_job(name).await?.is_none() {
            return Err(AppError::not_found(ResourceKind::Job, name));
        }
        Ok(self.deps.job_repo.list_runs_filtered(name, state, since, limit).await?)
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

    /// Request cancellation for a single active run. The request is accepted
    /// once its facade-owned cancellation sender is updated; the terminal state
    /// is persisted only after the lifecycle adapter has reaped the child and
    /// joined both output pumps.
    pub async fn cancel_run(&self, name: &str, run_id: JobRunId) -> AppResult<()> {
        let job = self
            .deps
            .job_repo
            .get_job(name)
            .await?
            .ok_or_else(|| AppError::not_found(ResourceKind::Job, name))?;
        match self.active_runs.cancel_owned(job.id, run_id) {
            CancelledRun::Active => Ok(()),
            CancelledRun::Queued(queued) => {
                if queued.job.id != job.id {
                    return Err(AppError::not_found(ResourceKind::Run, run_id.0.to_string()));
                }
                let mut run = self
                    .deps
                    .job_repo
                    .get_run(name, &run_id)
                    .await?
                    .ok_or_else(|| AppError::not_found(ResourceKind::Run, run_id.0.to_string()))?;
                run.state = JobRunState::Cancelled;
                run.ended_at = Some(self.deps.clock.now());
                self.commit_terminal_cancellation(&run).await?;
                Ok(())
            }
            CancelledRun::Missing => {
                let run = self
                    .deps
                    .job_repo
                    .get_run(name, &run_id)
                    .await?
                    .ok_or_else(|| AppError::not_found(ResourceKind::Run, run_id.0.to_string()))?;
                if run.state.is_terminal() {
                    Err(AppError::conflict(ConflictReason::RunAlreadyFinished))
                } else {
                    Err(AppError::not_found(ResourceKind::Run, run_id.0.to_string()))
                }
            }
        }
    }

    async fn dispatch_run(
        &self,
        job: Job,
        triggered_by: TriggeredBy,
        run_id: JobRunId,
    ) -> AppResult<RunDispatch> {
        if self.is_shutting_down.load(Ordering::SeqCst) {
            return Ok(RunDispatch::Skipped);
        }
        if self.active_runs.is_dispatch_frozen(job.id) {
            return Ok(RunDispatch::Skipped);
        }
        let name = job.name.clone();
        let pending_run = JobRun {
            run_id,
            job_name: name.clone(),
            job_id: job.id,
            triggered_by: triggered_by.clone(),
            scheduled_at: self.deps.clock.now(),
            started_at: None,
            ended_at: None,
            exit_code: None,
            state: JobRunState::Pending,
        };
        self.deps.job_repo.save_run(&pending_run).await?;
        let running_count = self.active_runs.running_count(job.id);
        if running_count > 0 {
            match job.on_overlap {
                my_supervisor_core::domain::OverlapPolicy::Skip => {
                    return Ok(RunDispatch::Skipped);
                }
                my_supervisor_core::domain::OverlapPolicy::Queue => {
                    let queued = self.active_runs.register(
                        job,
                        triggered_by,
                        run_id,
                        ActiveRunPhase::Queued,
                    );
                    self.active_runs.enqueue(&queued);
                    self.emit(DomainEvent::JobRunScheduled { name, run_id });
                    return Ok(RunDispatch::Queued);
                }
                my_supervisor_core::domain::OverlapPolicy::Parallel => {}
            }
        }
        let active_run = self.active_runs.register(
            job,
            triggered_by,
            run_id,
            ActiveRunPhase::Starting,
        );
        self.emit(DomainEvent::JobRunScheduled {
            name: name.clone(),
            run_id,
        });
        spawn_active_run(
            active_run,
            self.active_runs.clone(),
            self.runner.clone(),
            self.deps.job_repo.clone(),
            self.deps.log_sink.clone(),
            self.deps.clock.clone(),
        );
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
            job_id: self
                .deps
                .job_repo
                .get_job(job_name)
                .await
                .ok()
                .flatten()
                .map(|job| job.id)
                .unwrap_or_default(),
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
        if let Ok(Some(job)) = self.deps.job_repo.get_job(job_name).await {
            enforce_log_retention(
                &job,
                &self.deps.job_repo,
                &self.deps.log_sink,
                self.deps.clock.now(),
            )
            .await;
        }
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

    /// List pending durable recovery work without exposing target commands,
    /// environments, PIDs, or native identity tokens.
    pub async fn recovery_diagnostics(&self) -> AppResult<RecoveryDiagnostics> {
        const RECOVERY_DIAGNOSTIC_LIMIT: usize = 100;
        let mut records = Vec::new();

        for cleanup in self.deps.state_repo.pending_runtime_handle_cleanup(RECOVERY_DIAGNOSTIC_LIMIT).await? {
            records.push(RecoveryDiagnostic {
                kind: "runtime_handle_cleanup".into(),
                id: cleanup.process_id.to_string(),
                resource: cleanup.name,
                stage: "clear_runtime_handle".into(),
                attempts: cleanup.attempts,
                last_error: cleanup.last_error,
            });
        }
        for cleanup in self.deps.job_repo.pending_transient_cleanup(RECOVERY_DIAGNOSTIC_LIMIT).await? {
            records.push(RecoveryDiagnostic {
                kind: "transient_cleanup".into(),
                id: cleanup.cleanup_id.to_string(),
                resource: format!("{}/{}", cleanup.job_name, cleanup.run_id.0),
                stage: format!("{:?}", cleanup.stage).to_lowercase(),
                attempts: cleanup.attempts,
                last_error: cleanup.last_error,
            });
        }
        for cleanup in self.deps.job_repo.pending_run_log_cleanup(RECOVERY_DIAGNOSTIC_LIMIT).await? {
            records.push(RecoveryDiagnostic {
                kind: "run_log_cleanup".into(),
                id: cleanup.run_id.0.to_string(),
                resource: cleanup.run_id.0.to_string(),
                stage: "remove_run_logs".into(),
                attempts: cleanup.attempts,
                last_error: cleanup.last_error,
            });
        }
        for journal in self.deps.job_repo.list_incomplete_job_deletions().await? {
            records.push(RecoveryDiagnostic {
                kind: "job_deletion".into(),
                id: journal.deletion_id.to_string(),
                resource: journal.job.name,
                stage: format!("{:?}", journal.stage).to_lowercase(),
                attempts: 0,
                last_error: journal.last_error,
            });
        }
        for journal in self.deps.job_repo.list_incomplete_config_applies().await? {
            records.push(RecoveryDiagnostic {
                kind: "config_apply".into(),
                id: journal.apply_id.to_string(),
                resource: "daemon_config".into(),
                stage: format!("{:?}", journal.stage).to_lowercase(),
                attempts: 0,
                last_error: journal.compensation_error,
            });
        }
        Ok(RecoveryDiagnostics { records })
    }

    pub async fn validate_config(
        &self,
        loaded: &my_supervisor_core::domain::LoadedConfig,
        mode: ApplyMode,
    ) -> AppResult<ConfigApplyResult> {
        let current = self.config_snapshot().await?;
        let target = target_snapshot(&current, loaded, mode);
        validate_config_snapshot(&current, &target, self.deps.clock.now())?;
        Ok(ConfigApplyResult { apply_id: None, mode, diff: config_diff(&current, &target), dry_run: true })
    }

    pub async fn apply_config(
        &self,
        loaded: my_supervisor_core::domain::LoadedConfig,
        mode: ApplyMode,
        dry_run: bool,
    ) -> AppResult<ConfigApplyResult> {
        if self.is_applying_config.swap(true, Ordering::SeqCst) {
            return Err(AppError::ConfigRecoveryRequired("another config apply is in progress".into()));
        }
        let result = self.apply_config_inner(loaded, mode, dry_run).await;
        self.is_applying_config.store(false, Ordering::SeqCst);
        result
    }

    async fn apply_config_inner(
        &self,
        loaded: my_supervisor_core::domain::LoadedConfig,
        mode: ApplyMode,
        dry_run: bool,
    ) -> AppResult<ConfigApplyResult> {
        if !self.deps.job_repo.list_incomplete_config_applies().await?.is_empty() {
            return Err(AppError::ConfigRecoveryRequired("an incomplete config apply must be recovered first".into()));
        }
        let previous = self.config_snapshot().await?;
        let target = target_snapshot(&previous, &loaded, mode);
        validate_config_snapshot(&previous, &target, self.deps.clock.now())?;
        let diff = config_diff(&previous, &target);
        if dry_run {
            return Ok(ConfigApplyResult { apply_id: None, mode, diff, dry_run: true });
        }
        let apply_id = uuid::Uuid::new_v4();
        let journal = ConfigApplyJournal {
            apply_id,
            previous: previous.clone(),
            target: target.clone(),
            diff: diff.clone(),
            stage: ConfigApplyStage::Prepared,
            compensation_error: None,
            target_direct_starts: Vec::new(),
        };
        self.deps.job_repo.create_config_apply_journal(&journal).await?;
        let scheduler_snapshot = match self.deps.scheduler.snapshot().await {
            Ok(snapshot) => snapshot,
            Err(error) => return self.config_apply_failed(apply_id, error.to_string()).await,
        };

        let apply_result: AppResult<Vec<String>> = async {
            self.stage_scheduler_and_target_registrar(&previous, &target).await?;
            self.deps.job_repo.set_config_apply_stage(apply_id, ConfigApplyStage::SchedulerAndRegistrarStaged, None).await?;
            // Removing a Job starts cancellation, and removing an old launchd
            // unit destroys its previous native registration.  Both are
            // irreversible boundaries: persist the target-only recovery
            // direction before either side effect runs.
            if self.has_irreversible_config_removal(&previous, &target) {
                self.deps.job_repo.set_config_apply_stage(apply_id, ConfigApplyStage::ForwardRecovery, None).await?;
            }
            self.remove_obsolete_system_registered_units(&previous, &target).await?;
            for job in &previous.jobs {
                if !target.jobs.iter().any(|candidate| candidate.name == job.name) {
                    self.drain_job(job, true).await?;
                }
            }
            self.stop_changed_direct_processes(&previous, &target).await?;
            self.deps.job_repo.set_config_apply_stage(apply_id, ConfigApplyStage::DirectProcessesStopped, None).await?;
            self.deps.job_repo.apply_config_snapshot(&target).await?;
            self.deps.job_repo.set_config_apply_stage(apply_id, ConfigApplyStage::DatabaseCommitted, None).await?;
            let started = self.start_target_direct_processes(
                apply_id,
                &previous,
                &target,
                &[],
            ).await?;
            self.start_target_system_registered_processes(&target).await?;
            self.deps.job_repo.set_config_apply_stage(apply_id, ConfigApplyStage::NewProcessesStarted, None).await?;
            Ok(started)
        }.await;
        match apply_result {
            Ok(_) => {
                self.deps.job_repo.clear_config_apply_journal(apply_id).await?;
                Ok(ConfigApplyResult { apply_id: Some(apply_id), mode, diff, dry_run: false })
            }
            Err(error) => {
                let stage = self
                    .deps
                    .job_repo
                    .list_incomplete_config_applies()
                    .await?
                    .into_iter()
                    .find(|journal| journal.apply_id == apply_id)
                    .map(|journal| journal.stage)
                    .unwrap_or(ConfigApplyStage::Prepared);
                if matches!(stage, ConfigApplyStage::ForwardRecovery | ConfigApplyStage::DirectProcessesStopped | ConfigApplyStage::DatabaseCommitted | ConfigApplyStage::NewProcessesStarted) {
                    self.deps.job_repo.set_config_apply_stage(apply_id, ConfigApplyStage::ForwardRecovery, Some(&error.to_string())).await?;
                    return Err(AppError::ConfigRecoveryRequired(format!(
                        "apply {apply_id} crossed the removal boundary and is recovering toward its target: {error}"
                    )));
                }
                let recovery = self.compensate_config_apply(apply_id, &previous, &target, &scheduler_snapshot).await;
                match recovery {
                    Ok(()) => Err(error),
                    Err(recovery_error) => {
                        self.deps.job_repo.set_config_apply_stage(apply_id, ConfigApplyStage::CompensationFailed, Some(&recovery_error.to_string())).await?;
                        Err(AppError::ConfigRecoveryRequired(format!("{error}; compensation failed: {recovery_error}")))
                    }
                }
            }
        }
    }

    pub async fn reload(&self) -> AppResult<()> {
        let loaded = self.deps.config.load().await?;
        self.apply_config(loaded, ApplyMode::Replace, false).await.map(|_| ())
    }

    pub async fn recover_incomplete_config_apply(&self) -> AppResult<()> {
        for journal in self.deps.job_repo.list_incomplete_config_applies().await? {
            if matches!(
                journal.stage,
                ConfigApplyStage::ForwardRecovery
                    | ConfigApplyStage::DirectProcessesStopped
                    | ConfigApplyStage::DatabaseCommitted
                    | ConfigApplyStage::NewProcessesStarted
            ) {
                self.forward_recover_config_apply(&journal).await?;
                continue;
            }
            let scheduler_snapshot = my_supervisor_core::ports::SchedulerSnapshot {
                entries: journal.previous.jobs.iter().map(|job| my_supervisor_core::ports::ScheduledJob {
                    name: job.name.clone(),
                    trigger: job.trigger.clone(),
                }).collect(),
            };
            self.compensate_config_apply(journal.apply_id, &journal.previous, &journal.target, &scheduler_snapshot).await?;
        }
        Ok(())
    }

    /// Resume a journal that passed the Replace cancellation boundary. This is
    /// intentionally idempotent: every replay repeats the target staging and
    /// durable target snapshot rather than reviving cancelled old Runs.
    async fn forward_recover_config_apply(&self, journal: &ConfigApplyJournal) -> AppResult<()> {
        self.stage_scheduler_and_target_registrar(&journal.previous, &journal.target).await?;
        self.remove_obsolete_system_registered_units(&journal.previous, &journal.target).await?;
        for job in &journal.previous.jobs {
            if !journal.target.jobs.iter().any(|candidate| candidate.name == job.name)
                && self.deps.job_repo.get_job(&job.name).await?.is_some()
            {
                self.drain_job(job, true).await?;
            }
        }
        self.stop_changed_direct_processes(&journal.previous, &journal.target).await?;
        self.deps.job_repo.apply_config_snapshot(&journal.target).await?;
        self.deps
            .job_repo
            .set_config_apply_stage(journal.apply_id, ConfigApplyStage::NewProcessesStarted, None)
            .await?;
        self.start_target_direct_processes(
            journal.apply_id,
            &journal.previous,
            &journal.target,
            &journal.target_direct_starts,
        ).await?;
        self.start_target_system_registered_processes(&journal.target).await?;
        self.deps.job_repo.clear_config_apply_journal(journal.apply_id).await?;
        Ok(())
    }

    async fn config_snapshot(&self) -> AppResult<ConfigSnapshot> {
        let processes = self.deps.state_repo.list_specs().await?;
        let jobs = self.deps.job_repo.list_jobs().await?;
        let running_direct_processes = self.runtime.lock().unwrap().keys().cloned().collect();
        Ok(ConfigSnapshot { processes, jobs, running_direct_processes })
    }

    /// Prepare target scheduling and launchd definitions while every old unit
    /// remains intact.  The caller persists `ForwardRecovery` before calling
    /// [`Self::remove_obsolete_system_registered_units`].
    async fn stage_scheduler_and_target_registrar(&self, previous: &ConfigSnapshot, target: &ConfigSnapshot) -> AppResult<()> {
        for job in &target.jobs { self.deps.scheduler.register(&job.name, &job.trigger).await?; }
        for job in &previous.jobs {
            if !target.jobs.iter().any(|candidate| candidate.name == job.name) { self.deps.scheduler.unregister(&job.name).await?; }
        }
        for spec in &target.processes {
            if let ManagementMode::SystemRegistered { unit_name } = &spec.management_mode { self.deps.registrar.register(unit_name, spec).await?; }
        }
        Ok(())
    }

    fn has_irreversible_config_removal(&self, previous: &ConfigSnapshot, target: &ConfigSnapshot) -> bool {
        previous.jobs.iter().any(|job| !target.jobs.iter().any(|candidate| candidate.name == job.name))
            || previous.processes.iter().any(|previous_spec| {
                Self::obsolete_system_registered_unit(previous_spec, target)
                    || Self::replaces_system_registered_unit(previous_spec, target)
            })
    }

    fn replaces_system_registered_unit(previous_spec: &ProcessSpec, target: &ConfigSnapshot) -> bool {
        let ManagementMode::SystemRegistered { unit_name } = &previous_spec.management_mode else {
            return false;
        };
        target.processes.iter().any(|target_spec| {
            target_spec.name == previous_spec.name
                && matches!(
                    &target_spec.management_mode,
                    ManagementMode::SystemRegistered { unit_name: target_unit_name }
                        if target_unit_name == unit_name
                )
                && target_spec != previous_spec
        })
    }

    fn obsolete_system_registered_unit(previous_spec: &ProcessSpec, target: &ConfigSnapshot) -> bool {
        let ManagementMode::SystemRegistered { unit_name } = &previous_spec.management_mode else {
            return false;
        };
        !target.processes.iter().any(|target_spec| {
            target_spec.name == previous_spec.name
                && matches!(
                    &target_spec.management_mode,
                    ManagementMode::SystemRegistered { unit_name: target_unit_name }
                        if target_unit_name == unit_name
                )
        })
    }

    async fn remove_obsolete_system_registered_units(&self, previous: &ConfigSnapshot, target: &ConfigSnapshot) -> AppResult<()> {
        for spec in &previous.processes {
            if Self::obsolete_system_registered_unit(spec, target) {
                if let ManagementMode::SystemRegistered { unit_name } = &spec.management_mode {
                    self.deps.registrar.unregister(unit_name).await?;
                }
            }
        }
        Ok(())
    }

    async fn resume_job_deletion(
        &self,
        mut journal: JobDeletionJournal,
        intent: JobDeletionResumeIntent,
    ) -> AppResult<()> {
        let job = &journal.job;
        if journal.stage == JobDeletionStage::RollbackRequired
            || (intent == JobDeletionResumeIntent::Recovery
                && is_pre_cancellation_deletion_stage(journal.stage))
        {
            return self.complete_job_deletion_rollback(&journal).await;
        }
        if journal.stage == JobDeletionStage::Prepared {
            self.active_runs.freeze_dispatch(job.id);
            if let Err(error) = self.deps.job_repo.update_job_deletion_journal(
                journal.deletion_id, JobDeletionStage::DispatchFrozen, None, None,
            ).await {
                self.active_runs.thaw_dispatch(job.id);
                return Err(error.into());
            }
            journal.stage = JobDeletionStage::DispatchFrozen;
        } else {
            self.active_runs.freeze_dispatch(job.id);
        }

        if journal.stage == JobDeletionStage::DispatchFrozen {
            if let Err(error) = self.deps.scheduler.unregister(&job.name).await {
                return match self.rollback_pre_cancellation(&journal, &error.to_string()).await {
                    Ok(()) => Err(error.into()),
                    Err(rollback_error) => Err(rollback_error),
                };
            }
            self.deps.job_repo.update_job_deletion_journal(
                journal.deletion_id, JobDeletionStage::SchedulerUnregistered, None, None,
            ).await?;
            journal.stage = JobDeletionStage::SchedulerUnregistered;
        }

        if journal.stage == JobDeletionStage::SchedulerUnregistered {
            let queued = self.active_runs.queued_runs(job.id);
            let cancelled_at = self.deps.clock.now();
            let terminal_events = queued
                .iter()
                .map(|run| self.cancelled_terminal_event(&job.name, run.run_id, cancelled_at))
                .collect::<Vec<_>>();
            // The database transition is the cancellation boundary.  A failed
            // transaction leaves every queued Run, terminal outbox event, and
            // journal untouched, which makes rollback safe to retry after restart.
            if let Err(error) = self.deps.job_repo.cancel_queued_runs_for_job_deletion(
                journal.deletion_id,
                &job.name,
                &terminal_events,
            ).await {
                return match self.rollback_pre_cancellation(&journal, &error.to_string()).await {
                    Ok(()) => Err(error.into()),
                    Err(rollback_error) => Err(rollback_error),
                };
            }
            journal.stage = JobDeletionStage::CancellationStarted;
            for queued_run in queued {
                self.publish_internal_cancellation(&job.name, queued_run.run_id);
            }
        }

        if matches!(journal.stage, JobDeletionStage::CancellationStarted | JobDeletionStage::RunsDraining) {
            self.deps.job_repo.update_job_deletion_journal(
                journal.deletion_id, JobDeletionStage::RunsDraining, None, None,
            ).await?;
            journal.stage = JobDeletionStage::RunsDraining;
            let drained = self.active_runs.begin_drain(job.id, true);
            // Queued runs were durably marked before cancellation.  The normal
            // drain path would save and emit them again, so deletion owns this
            // finalization boundary directly.
            for runner in drained.runners {
                if let Err(error) = runner.await {
                    self.record_job_deletion_failure(&journal, &format!("job drain join failed: {error}")).await;
                    return Err(AppError::JobDeletionRecoveryRequired(format!("{} ({})", job.name, journal.deletion_id)));
                }
            }
            self.reconcile_transient_cleanup().await;
            if self.active_runs.has_runs(job.id) {
                self.record_job_deletion_failure(&journal, "job drain did not release every active run").await;
                return Err(AppError::JobDeletionRecoveryRequired(format!("{} ({})", job.name, journal.deletion_id)));
            }
        }

        if journal.stage == JobDeletionStage::RunsDraining {
            let run_ids = match self.deps.job_repo.commit_job_deletion_rows(journal.deletion_id, &job.name).await {
                Ok(run_ids) => run_ids,
                Err(error) => {
                    self.record_job_deletion_failure(&journal, &error.to_string()).await;
                    return Err(AppError::JobDeletionRecoveryRequired(format!("{} ({})", job.name, journal.deletion_id)));
                }
            };
            journal.stage = JobDeletionStage::RowsDeleted;
            journal.run_ids = run_ids;
        }

        if matches!(journal.stage, JobDeletionStage::RowsDeleted | JobDeletionStage::LogsCleaning) {
            self.deps.job_repo.update_job_deletion_journal(
                journal.deletion_id, JobDeletionStage::LogsCleaning, None, None,
            ).await?;
            journal.stage = JobDeletionStage::LogsCleaning;
            for run_id in &journal.run_ids {
                if let Err(error) = self.deps.log_sink.remove_run(*run_id).await {
                    let _ = self.deps.job_repo.fail_run_log_cleanup(*run_id, &error.to_string()).await;
                    self.record_job_deletion_failure(&journal, &error.to_string()).await;
                    return Err(AppError::JobDeletionRecoveryRequired(format!("{} ({})", job.name, journal.deletion_id)));
                }
                self.deps.job_repo.complete_run_log_cleanup(*run_id).await?;
            }
            self.deps.job_repo.update_job_deletion_journal(
                journal.deletion_id, JobDeletionStage::Completed, None, None,
            ).await?;
        }
        self.active_runs.thaw_dispatch(job.id);
        self.deps.job_repo.clear_job_deletion_journal(journal.deletion_id).await?;
        Ok(())
    }

    async fn rollback_pre_cancellation(&self, journal: &JobDeletionJournal, error: &str) -> AppResult<()> {
        let rollback_direction_result = self.deps.job_repo.update_job_deletion_journal(
            journal.deletion_id, JobDeletionStage::RollbackRequired, None, Some(error),
        ).await;
        let rollback_result = self.complete_job_deletion_rollback(journal).await;
        if let Err(rollback_error) = rollback_result {
            return Err(rollback_error);
        }
        if let Err(rollback_direction_error) = rollback_direction_result {
            tracing::warn!(%rollback_direction_error, deletion_id = %journal.deletion_id, "job deletion rollback direction was not persisted; recovery remains rollback-only");
        }
        Ok(())
    }

    /// `RollbackRequired` is deliberately a terminal direction: no recovery
    /// path from this state may re-enter cancellation or row deletion.
    async fn complete_job_deletion_rollback(&self, journal: &JobDeletionJournal) -> AppResult<()> {
        let register_result = self.deps.scheduler.register(&journal.job.name, &journal.job.trigger).await;
        self.active_runs.thaw_dispatch(journal.job.id);
        register_result?;
        self.deps.job_repo.clear_job_deletion_journal(journal.deletion_id).await?;
        Ok(())
    }

    async fn record_job_deletion_failure(&self, journal: &JobDeletionJournal, error: &str) {
        let _ = self.deps.job_repo.update_job_deletion_journal(
            journal.deletion_id, journal.stage, None, Some(error),
        ).await;
    }

    /// Freeze dispatch, cancel queued/active work, and wait for every runner
    /// before a job definition can disappear.  A failed scheduler unregister
    /// intentionally leaves the job untouched so callers never report a
    /// successful delete while it can still be dispatched.
    async fn drain_job(&self, job: &Job, tombstone: bool) -> AppResult<()> {
        self.deps.scheduler.unregister(&job.name).await?;
        let drained = self.active_runs.begin_drain(job.id, tombstone);
        for queued in drained.queued {
            let mut run = self
                .deps
                .job_repo
                .get_run(&job.name, &queued.run_id)
                .await?
                .ok_or_else(|| AppError::not_found(ResourceKind::Run, queued.run_id.0.to_string()))?;
            run.state = JobRunState::Cancelled;
            run.ended_at = Some(self.deps.clock.now());
            self.commit_terminal_cancellation(&run).await?;
        }
        for runner in drained.runners {
            runner
                .await
                .map_err(|error| AppError::Internal(format!("job drain join failed: {error}")))?;
        }
        self.reconcile_transient_cleanup().await;
        if self.active_runs.has_runs(job.id) {
            return Err(AppError::Internal("job drain did not release every active run".into()));
        }
        Ok(())
    }

    /// Cancellations without a native cleanup ticket still commit the terminal
    /// Run and durable external event before dependency scheduling receives a
    /// separate post-commit internal completion notification.
    async fn commit_terminal_cancellation(&self, run: &JobRun) -> AppResult<()> {
        let terminal_event = self.cancelled_terminal_event(
            &run.job_name,
            run.run_id,
            run.ended_at.unwrap_or_else(|| self.deps.clock.now()),
        );
        self.deps
            .job_repo
            .commit_terminal_run_with_event(run, &terminal_event)
            .await?;
        self.publish_internal_cancellation(&run.job_name, run.run_id);
        Ok(())
    }

    fn cancelled_terminal_event(
        &self,
        job_name: &str,
        run_id: JobRunId,
        occurred_at: DateTime<Utc>,
    ) -> TransientTerminalEvent {
        TransientTerminalEvent {
            cleanup_id: uuid::Uuid::new_v4(),
            event_id: uuid::Uuid::new_v4(),
            occurred_at,
            job_name: job_name.to_string(),
            run_id,
            state: JobRunState::Cancelled,
            exit_code: None,
        }
    }

    fn publish_internal_cancellation(&self, job_name: &str, run_id: JobRunId) {
        let _ = self.internal_events.send(PublishedEvent::ordinary(DomainEvent::JobRunCancelled {
            name: job_name.to_string(),
            run_id,
        }));
    }

    async fn stop_changed_direct_processes(&self, previous: &ConfigSnapshot, target: &ConfigSnapshot) -> AppResult<()> {
        for old in &previous.processes {
            let changed_or_removed = target.processes.iter().find(|new| new.name == old.name).is_none_or(|new| new != old);
            if changed_or_removed && matches!(old.management_mode, ManagementMode::Direct) {
                self.stop_direct_process_for_config(old).await?;
            }
        }
        Ok(())
    }

    /// Stop a changed Direct process even when recovery is running in a fresh
    /// facade whose in-memory runtime map is empty.  Detached handles are
    /// checked through the lifecycle port first, so an unverified identity is
    /// never replaced by a duplicate target spawn.
    async fn stop_direct_process_for_config(&self, spec: &ProcessSpec) -> AppResult<()> {
        if self.is_running(&spec.name) {
            // Configuration replacement has the same owned-group completion
            // boundary as an explicit stop.  The graceful path verifies the
            // native handle, then escalates to KILL only if its dedicated
            // process group remains after the configured grace period.
            self.stop_process(&spec.name, false).await?;
            return Ok(());
        }
        let Some(handle) = self.deps.state_repo.get_runtime_handle(&spec.name).await? else {
            return Ok(());
        };
        match self.deps.lifecycle.probe_alive(&handle).await {
            Ok(Aliveness::Alive) => self.deps.shutdown.force_kill(&handle).await?,
            Ok(Aliveness::Dead) => {}
            Err(error) => {
                return Err(AppError::Internal(format!(
                    "cannot verify previous Direct process {} during config recovery: {error}",
                    spec.name
                )));
            }
        }
        if let Err(error) = self.deps.state_repo.set_runtime_handle(&spec.name, None).await {
            self.deps
                .state_repo
                .enqueue_runtime_handle_cleanup(&spec.name, &handle, &error.to_string())
                .await
                .map_err(|queue_error| AppError::Internal(format!(
                    "clearing previous Direct process {} failed: {error}; cleanup queue failed: {queue_error}",
                    spec.name
                )))?;
            return Err(error.into());
        }
        Ok(())
    }

    async fn start_target_direct_processes(
        &self,
        apply_id: uuid::Uuid,
        previous: &ConfigSnapshot,
        target: &ConfigSnapshot,
        persisted_starts: &[my_supervisor_core::domain::ConfigTargetDirectStart],
    ) -> AppResult<Vec<String>> {
        let mut started = Vec::new();
        let desired = crate::config_apply::desired_running_target(previous, target);
        for spec in &target.processes {
            if matches!(spec.management_mode, ManagementMode::Direct)
                && desired.iter().any(|name| name == &spec.name)
            {
                let persisted_start = persisted_starts.iter().find(|start| start.name == spec.name);
                if let Some(existing) = persisted_start {
                    if existing.spec != *spec {
                        return Err(AppError::Internal(format!(
                            "config recovery target identity changed for {}",
                            spec.name
                        )));
                    }
                }
                if !self.is_running(&spec.name) && matches!(spec.lifecycle, LifecycleMode::Detached) {
                    self.restore_detached_process(spec).await;
                }
                if self.is_running(&spec.name) {
                    let handle = self.runtime.lock().unwrap().get(&spec.name).map(|entry| entry.handle.clone())
                        .ok_or_else(|| AppError::Internal(format!("missing runtime handle for {}", spec.name)))?;
                    if let Some(expected_generation) = persisted_start.and_then(|start| start.expected_generation.as_deref()) {
                        if handle.generation.as_deref() != Some(expected_generation) {
                            return Err(AppError::Internal(format!(
                                "config recovery native generation mismatch for {}",
                                spec.name
                            )));
                        }
                    }
                    self.deps.job_repo.record_config_target_direct_start(
                        apply_id,
                        &my_supervisor_core::domain::ConfigTargetDirectStart {
                            name: spec.name.clone(),
                            spec: spec.clone(),
                            expected_generation: handle.generation,
                        },
                    ).await?;
                    continue;
                }
                self.deps.job_repo.record_config_target_direct_start(
                    apply_id,
                    &my_supervisor_core::domain::ConfigTargetDirectStart {
                        name: spec.name.clone(),
                        spec: spec.clone(),
                        expected_generation: None,
                    },
                ).await?;
                self.start_process(&spec.name).await?;
                let handle = self.runtime.lock().unwrap().get(&spec.name).map(|entry| entry.handle.clone())
                    .ok_or_else(|| AppError::Internal(format!("started Direct process {} has no runtime handle", spec.name)))?;
                self.deps.job_repo.record_config_target_direct_start(
                    apply_id,
                    &my_supervisor_core::domain::ConfigTargetDirectStart {
                        name: spec.name.clone(),
                        spec: spec.clone(),
                        expected_generation: handle.generation,
                    },
                ).await?;
                started.push(spec.name.clone());
            }
        }
        Ok(started)
    }

    /// SystemRegistered units are prepared before the old-unit boundary, but
    /// only started after the target snapshot is durable.  A failure therefore
    /// leaves a ForwardRecovery journal that can safely retry the target.
    async fn start_target_system_registered_processes(&self, target: &ConfigSnapshot) -> AppResult<()> {
        for spec in &target.processes {
            if spec.autostart {
                if let ManagementMode::SystemRegistered { unit_name } = &spec.management_mode {
                    self.deps.registrar.start(unit_name).await?;
                }
            }
        }
        Ok(())
    }

    async fn compensate_config_apply(&self, apply_id: uuid::Uuid, previous: &ConfigSnapshot, target: &ConfigSnapshot, scheduler_snapshot: &my_supervisor_core::ports::SchedulerSnapshot) -> AppResult<()> {
        for spec in &target.processes {
            if matches!(spec.management_mode, ManagementMode::Direct) && self.is_running(&spec.name) && !previous.running_direct_processes.contains(&spec.name) {
                self.stop_process(&spec.name, true).await?;
            }
        }
        self.deps.job_repo.restore_config_apply_snapshot(apply_id).await?;
        self.deps.scheduler.restore(scheduler_snapshot).await?;
        for spec in &target.processes {
            if let ManagementMode::SystemRegistered { unit_name } = &spec.management_mode {
                let reuses_previous_unit = previous.processes.iter().any(|previous_spec| {
                    matches!(
                        &previous_spec.management_mode,
                        ManagementMode::SystemRegistered { unit_name: previous_unit_name }
                            if previous_unit_name == unit_name
                    )
                });
                if !reuses_previous_unit {
                    self.deps.registrar.unregister(unit_name).await?;
                }
            }
        }
        for spec in &previous.processes {
            if let ManagementMode::SystemRegistered { unit_name } = &spec.management_mode { self.deps.registrar.register(unit_name, spec).await?; }
        }
        for name in &previous.running_direct_processes {
            if !self.is_running(name) { self.start_process(name).await?; }
        }
        self.deps.job_repo.clear_config_apply_journal(apply_id).await?;
        Ok(())
    }

    async fn config_apply_failed<T>(&self, apply_id: uuid::Uuid, message: String) -> AppResult<T> {
        self.deps.job_repo.set_config_apply_stage(apply_id, ConfigApplyStage::CompensationFailed, Some(&message)).await?;
        Err(AppError::ConfigRecoveryRequired(message))
    }

    pub fn request_shutdown(&self) {
        self.is_shutting_down.store(true, Ordering::SeqCst);
        self.shutdown.notify_waiters();
    }

    /// Drain every Job and then reap tied Direct children.  The caller is
    /// responsible for joining scheduler/supervisor tasks after this succeeds.
    pub async fn shutdown_all(&self) -> AppResult<()> {
        self.is_shutting_down.store(true, Ordering::SeqCst);
        self.shutdown.notify_waiters();
        let jobs = self.deps.job_repo.list_jobs().await?;
        for job in &jobs {
            self.drain_job(job, false).await?;
        }
        let handles: Vec<ChildHandle> = self
            .runtime
            .lock()
            .unwrap()
            .values()
            .filter(|entry| entry.tied)
            .map(|entry| entry.handle.clone())
            .collect();
        if !handles.is_empty() {
            self.deps
                .lifecycle
                .reap_on_shutdown(&handles)
                .await
                .map_err(|error| AppError::Internal(format!("tied process reap failed: {error}")))?;
            self.runtime.lock().unwrap().retain(|_, entry| !entry.tied);
        }
        Ok(())
    }

    /// Reap tied Direct-mode children during daemon shutdown so none are
    /// orphaned. Detached children are intentionally left running.
    pub async fn shutdown_children(&self) {
        if let Err(error) = self.shutdown_all().await {
            tracing::error!(%error, "daemon shutdown drain failed");
        }
    }

    /// Startup sequence a host runs after assembly: load the config file into
    /// the repositories, arm the scheduler for every known job, and autostart
    /// processes flagged `autostart`.
    pub async fn bootstrap(&self) -> AppResult<()> {
        // Preserve queued rows that belong to a deletion rollback.  They were
        // never committed through the cancellation boundary, so startup must
        // not accidentally make the failed deletion irreversible.
        let rollback_required_job_ids = self.deps.job_repo.list_incomplete_job_deletions().await?
            .into_iter()
            .filter(|journal| is_pre_cancellation_deletion_stage(journal.stage))
            .map(|journal| journal.job.id)
            .collect::<HashSet<_>>();
        self.reconcile_job_deletions().await;
        self.recover_incomplete_config_apply().await?;
        self.reload().await?;
        // Resolve durable cleanup before blanket startup cancellation turns a
        // recoverable Running row into an event-less terminal state.
        self.reconcile_transient_cleanup().await;
        self.reconcile_runtime_handle_cleanup().await;
        self.reconcile_run_log_cleanup().await;
        for job in self.deps.job_repo.list_jobs().await? {
            for mut run in self.deps.job_repo.list_runs(&job.name, 500).await? {
                if run.state == JobRunState::Pending && rollback_required_job_ids.contains(&job.id) {
                    continue;
                }
                if matches!(run.state, JobRunState::Pending | JobRunState::Running) {
                    run.state = JobRunState::Cancelled;
                    run.ended_at = Some(self.deps.clock.now());
                    self.commit_terminal_cancellation(&run).await?;
                }
            }
            self.deps
                .scheduler
                .register(&job.name, &job.trigger)
                .await
                .ok();
            enforce_log_retention(
                &job,
                &self.deps.job_repo,
                &self.deps.log_sink,
                self.deps.clock.now(),
            )
            .await;
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
        match self.deps.lifecycle.probe_alive(&handle).await {
            Ok(Aliveness::Alive) => {}
            Ok(Aliveness::Dead) => {
                if let Err(error) = self.deps.state_repo.set_runtime_handle(&spec.name, None).await {
                    if let Err(queue_error) = self
                        .deps
                        .state_repo
                        .enqueue_runtime_handle_cleanup(&spec.name, &handle, &error.to_string())
                        .await
                    {
                        tracing::error!(process = %spec.name, %queue_error, "failed to record restored runtime-handle cleanup");
                    }
                }
                return false;
            }
            Err(error) => {
                tracing::warn!(process = %spec.name, %error, "detached process state is unverified; leaving durable handle untouched");
                // Returning true prevents bootstrap from creating a second
                // process while the old identity cannot be verified.
                return true;
            }
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
        let mut domain_events = self.internal_events.subscribe();
        let mut retention_cleanup = tokio::time::interval(LOG_RETENTION_CLEANUP_INTERVAL);
        retention_cleanup.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut runtime_handle_cleanup = tokio::time::interval(RUNTIME_HANDLE_CLEANUP_INTERVAL);
        runtime_handle_cleanup.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut transient_cleanup = tokio::time::interval(TRANSIENT_CLEANUP_INTERVAL);
        transient_cleanup.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut job_deletion_recovery = tokio::time::interval(JOB_DELETION_RECOVERY_INTERVAL);
        job_deletion_recovery.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            if self.is_shutting_down.load(Ordering::SeqCst) {
                break;
            }
            tokio::select! {
                _ = self.shutdown.notified() => break,
                _ = tokio::time::sleep(Duration::from_millis(100)) => continue,
                _ = runtime_handle_cleanup.tick() => self.reconcile_runtime_handle_cleanup().await,
                _ = transient_cleanup.tick() => self.reconcile_transient_cleanup().await,
                _ = job_deletion_recovery.tick() => self.reconcile_job_deletions().await,
                result = schedule_events.recv() => match result {
                    Ok(event) => self.on_schedule_tick(&event.job_name).await,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                result = domain_events.recv() => match result {
                    Ok(PublishedEvent { event: DomainEvent::JobRunSucceeded { name, run_id, .. }, .. }) => {
                        self.on_dependency_completion(&name, run_id).await;
                    }
                    Ok(PublishedEvent { event: DomainEvent::JobRunFailed { name, run_id, .. }, .. }) => {
                        self.on_dependency_completion(&name, run_id).await;
                    }
                    Ok(PublishedEvent { event: DomainEvent::JobRunTimedOut { name, run_id }, .. })
                    | Ok(PublishedEvent { event: DomainEvent::JobRunCancelled { name, run_id }, .. }) => {
                        self.on_dependency_completion(&name, run_id).await;
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                _ = retention_cleanup.tick() => self.prune_all_job_logs().await,
            }
        }
    }

    async fn prune_all_job_logs(&self) {
        let Ok(jobs) = self.deps.job_repo.list_jobs().await else {
            return;
        };
        let now = self.deps.clock.now();
        for job in jobs {
            enforce_log_retention(&job, &self.deps.job_repo, &self.deps.log_sink, now).await;
        }
        self.reconcile_run_log_cleanup().await;
    }

    async fn try_remove_run_log(&self, run_id: JobRunId) {
        match self.deps.log_sink.remove_run(run_id).await {
            Ok(()) => { let _ = self.deps.job_repo.complete_run_log_cleanup(run_id).await; }
            Err(error) => { let _ = self.deps.job_repo.fail_run_log_cleanup(run_id, &error.to_string()).await; }
        }
    }

    async fn reconcile_run_log_cleanup(&self) {
        let Ok(pending) = self.deps.job_repo.pending_run_log_cleanup(1_000).await else { return; };
        for cleanup in pending { self.try_remove_run_log(cleanup.run_id).await; }
        let Ok(on_disk) = self.deps.log_sink.persisted_run_ids().await else { return; };
        for run_id in on_disk {
            let mut exists = false;
            if let Ok(jobs) = self.deps.job_repo.list_jobs().await {
                for job in jobs {
                    if self.deps.job_repo.get_run(&job.name, &run_id).await.ok().flatten().is_some() { exists = true; break; }
                }
            }
            if !exists && self.deps.job_repo.enqueue_run_log_cleanup(run_id).await.is_ok() { self.try_remove_run_log(run_id).await; }
        }
    }

    async fn reconcile_job_deletions(&self) {
        let Ok(journals) = self.deps.job_repo.list_incomplete_job_deletions().await else {
            return;
        };
        for journal in journals {
            let job_lock = self.job_lock(&journal.job.name);
            let Ok(_guard) = job_lock.try_lock() else {
                continue;
            };
            if let Err(error) = self.resume_job_deletion(journal, JobDeletionResumeIntent::Recovery).await {
                tracing::warn!(%error, "job deletion recovery remains pending");
            }
        }
    }

    /// Finish durable cleanup left behind after an already-exited Direct
    /// process could not clear its persisted handle.  The repository performs
    /// the identity comparison in the same statement as the clear, so a new
    /// process for this name is never removed by an older retry.
    async fn reconcile_runtime_handle_cleanup(&self) {
        let Ok(pending) = self.deps.state_repo.pending_runtime_handle_cleanup(1_000).await else {
            return;
        };
        for cleanup in pending {
            match self.deps.state_repo.clear_runtime_handle_if_matches(&cleanup).await {
                Ok(_) => {
                    let _ = self
                        .deps
                        .state_repo
                        .complete_runtime_handle_cleanup(&cleanup.name)
                        .await;
                }
                Err(error) => {
                    tracing::warn!(process = %cleanup.name, %error, "runtime-handle cleanup retry failed");
                }
            }
        }
    }

    /// Resume every durable transient-cleanup ticket.  The platform owns live
    /// child/pump tasks; after restart it uses the verified native handle in
    /// the ticket.  Terminal persistence is intentionally last and is made
    /// idempotent by checking the existing Run state before emitting an event.
    async fn reconcile_transient_cleanup(&self) {
        self.deliver_transient_terminal_events().await;
        let Ok(pending) = self.deps.job_repo.pending_transient_cleanup(1_000).await else {
            return;
        };
        for ticket in pending {
            match self.deps.lifecycle.resume_transient_cleanup(&ticket).await {
                Ok(TransientCompletion::Exited(_))
                | Ok(TransientCompletion::TimedOut(_))
                | Ok(TransientCompletion::Cancelled(_)) => {
                    if let Err(error) = self.finish_transient_cleanup(&ticket).await {
                        let _ = self.deps.job_repo.update_transient_cleanup(
                            &ticket,
                            TransientCleanupStage::PersistTerminal,
                            Some(&error.to_string()),
                        ).await;
                    }
                }
                Ok(TransientCompletion::CleanupPending { cause, stage, .. }) => {
                    let _ = self.deps.job_repo.update_transient_cleanup(&ticket, stage, Some(&cause)).await;
                }
                Err(error) => {
                    let _ = self.deps.job_repo.update_transient_cleanup(
                        &ticket,
                        ticket.stage,
                        Some(&error.to_string()),
                    ).await;
                }
            }
        }
        self.deliver_transient_terminal_events().await;
    }

    /// The terminal row and outbox are committed together by the repository.
    /// Delivery is retried before any further cleanup work and only then removes
    /// the ticket, keeping restart recovery on the durable side of the handoff.
    async fn deliver_transient_terminal_events(&self) {
        let Ok(events) = self.deps.job_repo.pending_transient_terminal_events(1_000).await else {
            return;
        };
        let cleanup_by_id = self
            .deps
            .job_repo
            .pending_transient_cleanup(1_000)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|ticket| (ticket.cleanup_id, ticket))
            .collect::<HashMap<_, _>>();
        for event in events {
            if !self
                .terminal_events_in_flight
                .lock()
                .unwrap()
                .insert(event.event_id)
            {
                continue;
            }
            let domain_event = match event.state {
                JobRunState::Succeeded => DomainEvent::JobRunSucceeded {
                    name: event.job_name.clone(), run_id: event.run_id, exit_code: event.exit_code.unwrap_or(0),
                },
                JobRunState::TimedOut => DomainEvent::JobRunTimedOut {
                    name: event.job_name.clone(), run_id: event.run_id,
                },
                JobRunState::Cancelled => DomainEvent::JobRunCancelled {
                    name: event.job_name.clone(), run_id: event.run_id,
                },
                _ => DomainEvent::JobRunFailed {
                    name: event.job_name.clone(), run_id: event.run_id, exit_code: event.exit_code,
                },
            };
            let published = PublishedEvent::durable_terminal(domain_event, event.event_id, event.occurred_at);
            if self.events.send(published.clone()).is_ok()
                && published
                    .wait_for_external_delivery(std::time::Duration::from_millis(200))
                    .await
                && self
                    .deps
                    .job_repo
                    .acknowledge_transient_terminal_event(event.event_id, event.cleanup_id)
                    .await
                    .is_ok()
            {
                if let Some(ticket) = cleanup_by_id.get(&event.cleanup_id) {
                    self.active_runs.finish_cleanup(ticket.job_id, ticket.run_id);
                }
            }
            self.terminal_events_in_flight.lock().unwrap().remove(&event.event_id);
        }
    }

    async fn finish_transient_cleanup(&self, ticket: &CleanupTicket) -> AppResult<()> {
        self.deps.job_repo.update_transient_cleanup(
            ticket,
            TransientCleanupStage::SealLog,
            None,
        ).await?;
        self.deps
            .log_sink
            .seal_run(ticket.run_id)
            .await
            .map_err(|error| AppError::Internal(format!("transient cleanup log seal failed: {error}")))?;
        self.deps.job_repo.update_transient_cleanup(
            ticket,
            TransientCleanupStage::PersistTerminal,
            None,
        ).await?;

        let Some(mut run) = self.deps.job_repo.get_run(&ticket.job_name, &ticket.run_id).await? else {
            // A later deletion saga owns an absent Run.  It is already no
            // longer possible to attach this completion to a replacement job.
            self.deps.job_repo.complete_transient_cleanup(ticket.cleanup_id).await?;
            self.active_runs.finish_cleanup(ticket.job_id, ticket.run_id);
            return Ok(());
        };
        if !run.state.is_terminal() {
            run.state = ticket.intended_terminal_state;
            run.started_at = Some(ticket.outcome.started_at);
            run.ended_at = Some(ticket.outcome.ended_at);
            run.exit_code = ticket.outcome.exit_code;
            self.deps.job_repo.commit_transient_cleanup_terminal(ticket, &run).await?;
        } else {
            // An injected crash may have committed the terminal Run before
            // outbox delivery.  Preserve the existing terminal row and create
            // the missing idempotent outbox record through the same boundary.
            self.deps.job_repo.commit_transient_cleanup_terminal(ticket, &run).await?;
        }
        self.deliver_transient_terminal_events().await;
        Ok(())
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
            // `stop_process_inner` owns a controlled TERM→KILL transition and
            // removes the runtime entry only after the whole process group is
            // gone.  Treating that in-flight state as a crash races the stop
            // path and can schedule an unwanted automatic restart.
            if entry.state == ProcessState::Stopping {
                continue;
            }
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
            if let Err(error) = self.deps.state_repo.set_runtime_handle(&name, None).await {
                if let Err(queue_error) = self
                    .deps
                    .state_repo
                    .enqueue_runtime_handle_cleanup(&name, &entry.handle, &error.to_string())
                    .await
                {
                    tracing::error!(process = %name, %queue_error, "failed to record crashed runtime-handle cleanup");
                }
            }

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
        if self.is_applying_config.load(Ordering::SeqCst) {
            return;
        }
        let job = match self.deps.job_repo.get_job(job_name).await {
            Ok(Some(job)) => job,
            _ => return,
        };
        if self.active_runs.is_dispatch_frozen(job.id) {
            return;
        }
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

            let all_succeeded = latest_runs
                .iter()
                .all(|run| run.state == JobRunState::Succeeded);
            let run_id = JobRunId::new();
            let triggered_by = TriggeredBy::Dependency {
                upstream_run_id,
            };
            let pending_run = JobRun {
                run_id,
                job_name: job.name.clone(),
                job_id: job.id,
                triggered_by: triggered_by.clone(),
                scheduled_at: self.deps.clock.now(),
                started_at: None,
                ended_at: None,
                exit_code: None,
                state: JobRunState::Pending,
            };
            let signature = DependencySignature {
                run_ids: latest_runs.iter().map(|run| run.run_id).collect(),
            };
            let claimed = match self
                .deps
                .job_repo
                .claim_dependency_run(&job.name, &signature, &pending_run)
                .await
            {
                Ok(claimed) => claimed,
                Err(_) => continue,
            };
            if !claimed {
                continue;
            }
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

fn spawn_active_run(
    active_run: Arc<ActiveRun>,
    registry: Arc<ActiveRunRegistry>,
    runner: Arc<dyn JobRunner>,
    job_repo: Arc<dyn JobRepository>,
    log_sink: Arc<dyn LogSink>,
    clock: Arc<dyn my_supervisor_core::ports::SystemClock>,
) {
    let job = active_run.job.clone();
    let triggered_by = active_run.triggered_by.clone();
    let run_id = active_run.run_id;
    let (start_sender, start_receiver) = oneshot::channel();
    let task_registry = registry.clone();
    let join = tokio::spawn(async move {
        // The facade stores this join handle before allowing runner execution,
        // so a cancellation/drain operation never loses an in-flight task.
        if start_receiver.await.is_err() {
            return;
        }
        let control = Arc::new(ActiveRunControl {
            registry: task_registry.clone(),
            run_id,
        });
        match runner.run(&job, triggered_by, run_id, control).await {
            Err(RunnerError::Unreaped(cause)) => {
                // Preserve cleanup ownership for delete/shutdown, but release
                // the overlap slot so a queued Run is not held behind a pump
                // that has already detached from execution completion.
                tracing::warn!(job = %job.name, run_id = %run_id.0, cause = %cause, "job cleanup left an unreaped child");
                if let Some(next) = task_registry.finish_unreaped(run_id) {
                    spawn_active_run(
                        next,
                        task_registry.clone(),
                        runner.clone(),
                        job_repo.clone(),
                        log_sink.clone(),
                        clock.clone(),
                    );
                }
            }
            result => {
                if let Err(error) = result {
                    tracing::warn!(job = %job.name, run_id = %run_id.0, error = %error, "job runner failed");
                }
                if task_registry.should_persist_terminal(run_id) {
                    enforce_log_retention(&job, &job_repo, &log_sink, clock.now()).await;
                }
                if let Some(next) = task_registry.finish(run_id) {
                    spawn_active_run(
                        next,
                        task_registry.clone(),
                        runner.clone(),
                        job_repo.clone(),
                        log_sink.clone(),
                        clock.clone(),
                    );
                }
            }
        }
    });
    registry.set_runner(run_id, join);
    let _ = start_sender.send(());
}

async fn enforce_log_retention(
    job: &Job,
    job_repo: &Arc<dyn JobRepository>,
    log_sink: &Arc<dyn LogSink>,
    now: DateTime<Utc>,
) {
    let retention = job.log_retention;
    let older_than = retention
        .max_age_days
        .map(|days| now - chrono::Duration::days(i64::from(days)));
    match job_repo
        .prune_runs(&job.name, retention.max_runs, older_than)
        .await
    {
        Ok(removed_run_ids) => {
            for run_id in removed_run_ids {
                match log_sink.remove_run(run_id).await {
                    Ok(()) => { let _ = job_repo.complete_run_log_cleanup(run_id).await; }
                    Err(error) => { let _ = job_repo.fail_run_log_cleanup(run_id, &error.to_string()).await; }
                }
            }
        }
        Err(error) => {
            tracing::warn!(job = %job.name, %error, "pruning retained run logs failed");
        }
    }
}

fn validate_config_snapshot(
    previous: &ConfigSnapshot,
    target: &ConfigSnapshot,
    now: DateTime<Utc>,
) -> AppResult<()> {
    let mut process_names = HashSet::new();
    for spec in &target.processes {
        validate_process(spec)?;
        if !process_names.insert(spec.name.clone()) {
            return Err(AppError::InvalidConfig(format!("duplicate process '{}' in config", spec.name)));
        }
        if let Some(existing) = previous.processes.iter().find(|existing| existing.name == spec.name) {
            if existing.management_mode != spec.management_mode {
                return Err(AppError::InvalidConfig(format!("process '{}' changes management mode; use msv convert", spec.name)));
            }
        }
    }
    let mut job_names = HashSet::new();
    for job in &target.jobs {
        if !job_names.insert(job.name.clone()) {
            return Err(AppError::InvalidConfig(format!("duplicate job '{}' in config", job.name)));
        }
        let others: Vec<Job> = target.jobs.iter().filter(|other| other.name != job.name).cloned().collect();
        let is_existing = previous.jobs.iter().any(|existing| existing.name == job.name);
        validate_job(job, &others, now, is_existing)?;
        if forms_cycle(job, &others) { return Err(AppError::CycleDetected); }
    }
    Ok(())
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
    if job.log_retention.max_runs == Some(0) {
        return Err(AppError::InvalidRequest(
            "log_retention.max_runs must be greater than zero".into(),
        ));
    }
    if job.log_retention.max_age_days == Some(0) {
        return Err(AppError::InvalidRequest(
            "log_retention.max_age_days must be greater than zero".into(),
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
