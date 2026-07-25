#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use my_supervisor_application::{AppDeps, DaemonMeta, NullProcessServiceRegistrar, OperationsFacade};
use my_supervisor_config::TomlConfigSource;
use my_supervisor_core::domain::{
    DependencyFailurePolicy, Job, JobId, JobRunState, JobTrigger, LogRetention, OverlapPolicy,
};
use my_supervisor_core::ports::{LogSink, RealClock};
use my_supervisor_infra_http::build_router;
use my_supervisor_infra_logging::InMemoryLogSink;
use my_supervisor_infra_scheduler::TokioScheduler;
use my_supervisor_infra_sqlite::SqliteStore;
use my_supervisor_platform_macos::{MacLifecycle, UnixShutdown};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn long_running_job(name: &str, overlap: OverlapPolicy) -> Job {
    Job {
        id: JobId::new(),
        name: name.into(),
        command: "/bin/sh".into(),
        args: vec!["-c".into(), "trap 'exit 0' TERM; sleep 30 & wait".into()],
        cwd: None,
        env: BTreeMap::new(),
        trigger: JobTrigger::Interval(Duration::from_secs(60)),
        on_overlap: overlap,
        on_dependency_failure: DependencyFailurePolicy::Skip,
        timeout: None,
        log_retention: LogRetention::default(),
    }
}

async fn facade() -> (Arc<OperationsFacade>, PathBuf) {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("my-supervisor-api-ownership-{}-{nonce}", std::process::id()));
    let log_dir = root.join("logs");
    tokio::fs::create_dir_all(&log_dir).await.unwrap();
    let store = Arc::new(SqliteStore::connect(root.join("state.db")).await.unwrap());
    let log_sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(log_dir.clone()));
    let dependencies = AppDeps {
        lifecycle: Arc::new(MacLifecycle::new(log_sink.clone(), log_dir.clone())),
        shutdown: Arc::new(UnixShutdown::new()),
        registrar: Arc::new(NullProcessServiceRegistrar),
        state_repo: store.clone(),
        job_repo: store,
        scheduler: Arc::new(TokioScheduler::new()),
        log_sink,
        clock: Arc::new(RealClock),
        config: Arc::new(TomlConfigSource::new(root.join("config.toml"))),
        meta: DaemonMeta::new(root.join("config.toml"), log_dir),
    };
    (OperationsFacade::new(dependencies), root)
}

async fn post_status(port: u16, path: &str) -> u16 {
    let mut stream = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await.unwrap();
    let request = format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[tokio::test]
async fn cancel_route_does_not_mutate_a_run_owned_by_another_job() {
    let (facade, root) = facade().await;
    facade.add_job(long_running_job("job-a", OverlapPolicy::Skip)).await.unwrap();
    facade.add_job(long_running_job("job-b", OverlapPolicy::Queue)).await.unwrap();
    let active = facade.trigger_job("job-b").await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let queued = facade.trigger_job("job-b").await.unwrap();

    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel::<()>();
    let server_facade = facade.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, build_router(server_facade))
            .with_graceful_shutdown(async { let _ = shutdown_receiver.await; })
            .await
            .unwrap();
    });

    assert_eq!(post_status(port, &format!("/api/v1/jobs/job-a/runs/{}/cancel", active.0)).await, 404);
    assert_eq!(post_status(port, &format!("/api/v1/jobs/job-a/runs/{}/cancel", queued.0)).await, 404);
    assert_eq!(facade.get_run("job-b", &active).await.unwrap().state, JobRunState::Running);
    assert_eq!(facade.get_run("job-b", &queued).await.unwrap().state, JobRunState::Pending);

    facade.cancel_run("job-b", active).await.unwrap();
    let _ = shutdown_sender.send(());
    server.await.unwrap();
    facade.shutdown_all().await.unwrap();
    tokio::fs::remove_dir_all(root).await.unwrap();
}
