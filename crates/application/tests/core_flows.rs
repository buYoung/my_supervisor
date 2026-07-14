use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use my_supervisor_application::{
    AppDeps, DaemonMeta, NullProcessServiceRegistrar, OperationsFacade,
};
use my_supervisor_core::domain::{
    ChildHandle, Job, JobId, JobRunId, JobTrigger, LifecycleMode, LoadedConfig, LogLine,
    LogRetention, OverlapPolicy, ProcessSpec, TriggeredBy,
};
use my_supervisor_core::ports::{
    Aliveness, ConfigError, ConfigSource, LifecycleController, ProbeError, RealClock, ReapError,
    ShutdownSignaler, SignalError, SpawnError, TransientOutcome,
};
use my_supervisor_infra_logging::InMemoryLogSink;
use my_supervisor_infra_scheduler::TokioScheduler;
use my_supervisor_infra_sqlite::SqliteStore;

struct EmptyConfig;

#[async_trait]
impl ConfigSource for EmptyConfig {
    async fn load(&self) -> Result<LoadedConfig, ConfigError> {
        Ok(LoadedConfig::default())
    }

    fn path(&self) -> PathBuf {
        PathBuf::from("test.toml")
    }
}

struct FakeLifecycle {
    next_pid: AtomicU32,
    alive: Mutex<HashMap<Uuid, bool>>,
    spawn_count: AtomicUsize,
    transient_delay: Duration,
    concurrent_runs: AtomicUsize,
    max_concurrent_runs: AtomicUsize,
}

impl FakeLifecycle {
    fn new(transient_delay: Duration) -> Self {
        Self {
            next_pid: AtomicU32::new(1000),
            alive: Mutex::new(HashMap::new()),
            spawn_count: AtomicUsize::new(0),
            transient_delay,
            concurrent_runs: AtomicUsize::new(0),
            max_concurrent_runs: AtomicUsize::new(0),
        }
    }

    fn spawn(&self) -> ChildHandle {
        let process_id = Uuid::new_v4();
        self.alive.lock().unwrap().insert(process_id, true);
        self.spawn_count.fetch_add(1, Ordering::SeqCst);
        ChildHandle {
            process_id,
            pid: self.next_pid.fetch_add(1, Ordering::SeqCst),
            started_at: Utc::now(),
        }
    }

    fn mark_all_dead(&self) {
        for is_alive in self.alive.lock().unwrap().values_mut() {
            *is_alive = false;
        }
    }

    fn finish(&self, handle: &ChildHandle) {
        self.alive
            .lock()
            .unwrap()
            .insert(handle.process_id, false);
    }
}

#[async_trait]
impl LifecycleController for FakeLifecycle {
    async fn spawn_tied(&self, _spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        Ok(self.spawn())
    }

    async fn spawn_detached(&self, _spec: &ProcessSpec) -> Result<ChildHandle, SpawnError> {
        Ok(self.spawn())
    }

    async fn probe_alive(&self, handle: &ChildHandle) -> Result<Aliveness, ProbeError> {
        Ok(if self
            .alive
            .lock()
            .unwrap()
            .get(&handle.process_id)
            .copied()
            .unwrap_or(false)
        {
            Aliveness::Alive
        } else {
            Aliveness::Dead
        })
    }

    async fn tail_detached_logs(
        &self,
        _spec: &ProcessSpec,
        _lines: usize,
    ) -> Result<Vec<LogLine>, ProbeError> {
        Ok(Vec::new())
    }

    async fn reap_on_shutdown(&self, handles: &[ChildHandle]) -> Result<(), ReapError> {
        for handle in handles {
            self.finish(handle);
        }
        Ok(())
    }

    async fn run_transient(
        &self,
        _spec: &ProcessSpec,
        _run_id: JobRunId,
    ) -> Result<TransientOutcome, SpawnError> {
        let started_at = Utc::now();
        let concurrent = self.concurrent_runs.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_concurrent_runs
            .fetch_max(concurrent, Ordering::SeqCst);
        tokio::time::sleep(self.transient_delay).await;
        self.concurrent_runs.fetch_sub(1, Ordering::SeqCst);
        Ok(TransientOutcome {
            started_at,
            ended_at: Utc::now(),
            exit_code: Some(0),
        })
    }
}

struct FakeShutdown {
    lifecycle: Arc<FakeLifecycle>,
}

#[async_trait]
impl ShutdownSignaler for FakeShutdown {
    async fn request_graceful(
        &self,
        target: &ChildHandle,
        _policy: &my_supervisor_core::domain::ShutdownPolicy,
    ) -> Result<(), SignalError> {
        self.lifecycle.finish(target);
        Ok(())
    }

    async fn force_kill(&self, target: &ChildHandle) -> Result<(), SignalError> {
        self.lifecycle.finish(target);
        Ok(())
    }
}

async fn facade(transient_delay: Duration) -> (Arc<OperationsFacade>, Arc<FakeLifecycle>) {
    let lifecycle = Arc::new(FakeLifecycle::new(transient_delay));
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let log_sink = Arc::new(InMemoryLogSink::new());
    let dependencies = AppDeps {
        lifecycle: lifecycle.clone(),
        shutdown: Arc::new(FakeShutdown {
            lifecycle: lifecycle.clone(),
        }),
        registrar: Arc::new(NullProcessServiceRegistrar),
        state_repo: store.clone(),
        job_repo: store,
        scheduler: Arc::new(TokioScheduler::new()),
        log_sink,
        clock: Arc::new(RealClock),
        config: Arc::new(EmptyConfig),
        meta: DaemonMeta::new(PathBuf::from("test.toml"), PathBuf::from("logs")),
    };
    (OperationsFacade::new(dependencies), lifecycle)
}

fn job(name: &str, overlap: OverlapPolicy) -> Job {
    Job {
        id: JobId::new(),
        name: name.to_string(),
        command: "fake".to_string(),
        args: Vec::new(),
        cwd: None,
        env: BTreeMap::new(),
        trigger: JobTrigger::Interval(Duration::from_secs(3600)),
        on_overlap: overlap,
        on_dependency_failure: Default::default(),
        timeout: None,
        log_retention: LogRetention::default(),
    }
}

#[tokio::test]
async fn queued_runs_are_serial_and_parallel_runs_overlap() {
    let (queue_facade, queue_lifecycle) = facade(Duration::from_millis(50)).await;
    queue_facade
        .add_job(job("queue", OverlapPolicy::Queue))
        .await
        .unwrap();
    for _ in 0..3 {
        queue_facade.trigger_job("queue").await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(220)).await;
    assert_eq!(queue_lifecycle.max_concurrent_runs.load(Ordering::SeqCst), 1);
    assert!(queue_facade
        .list_runs("queue", 3)
        .await
        .unwrap()
        .iter()
        .all(|run| run.state.is_terminal()));

    let (parallel_facade, parallel_lifecycle) = facade(Duration::from_millis(50)).await;
    parallel_facade
        .add_job(job("parallel", OverlapPolicy::Parallel))
        .await
        .unwrap();
    for _ in 0..3 {
        parallel_facade.trigger_job("parallel").await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        parallel_lifecycle.max_concurrent_runs.load(Ordering::SeqCst),
        3
    );
}

#[tokio::test]
async fn stopping_during_backoff_cancels_restart() {
    let (facade, lifecycle) = facade(Duration::ZERO).await;
    let mut spec = ProcessSpec::new("worker", "fake");
    spec.autostart = true;
    spec.lifecycle = LifecycleMode::Tied;
    spec.restart.backoff_initial = Duration::from_millis(150);
    spec.restart.backoff_max = Duration::from_millis(150);
    spec.restart.jitter = false;
    facade.add_process(spec).await.unwrap();

    let supervisor = tokio::spawn(facade.clone().run_process_supervisor_loop());
    lifecycle.mark_all_dead();
    tokio::time::sleep(Duration::from_millis(30)).await;
    facade.stop_process("worker", false).await.unwrap();
    tokio::time::sleep(Duration::from_millis(220)).await;

    assert_eq!(lifecycle.spawn_count.load(Ordering::SeqCst), 1);
    supervisor.abort();
}

#[tokio::test]
async fn dependency_success_runs_downstream() {
    let (facade, _) = facade(Duration::from_millis(20)).await;
    facade
        .add_job(job("upstream", OverlapPolicy::Skip))
        .await
        .unwrap();
    let mut downstream = job("downstream", OverlapPolicy::Skip);
    downstream.trigger = JobTrigger::DependsOn(vec!["upstream".to_string()]);
    facade.add_job(downstream).await.unwrap();

    let scheduler_loop = tokio::spawn(facade.clone().run_scheduler_loop());
    facade.trigger_job("upstream").await.unwrap();
    tokio::time::sleep(Duration::from_millis(120)).await;

    let runs = facade.list_runs("downstream", 1).await.unwrap();
    assert_eq!(runs.len(), 1);
    assert!(matches!(
        runs[0].triggered_by,
        TriggeredBy::Dependency { .. }
    ));
    scheduler_loop.abort();
}
