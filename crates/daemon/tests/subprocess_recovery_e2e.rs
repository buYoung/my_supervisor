#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use my_supervisor_core::ports::JobRepository;
use my_supervisor_infra_sqlite::SqliteStore;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, UnixStream};
use tokio::process::{Child, Command};
use tokio::sync::Notify;

fn test_root() -> PathBuf {
    // macOS limits Unix-domain socket paths to 104 bytes.  The sandbox's
    // per-user temp directory can itself exceed that, so keep the private
    // control socket directly below `/tmp` while retaining a unique prefix.
    PathBuf::from("/tmp").join(format!(
        "my-supervisor-daemon-recovery-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ))
}

fn unused_port() -> u16 {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn daemon(root: &PathBuf, port: u16) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_msv-daemon"));
    command
        .env("MSV_DAEMON_TEST_DATA_DIR", root)
        .env("MSV_DAEMON_TEST_BIND_ADDR", format!("127.0.0.1:{port}"))
        .env("MSV_DAEMON_TEST_CONTROL_SOCKET", root.join("private-control.sock"))
        .kill_on_drop(true)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command
}

async fn wait_for_health(port: u16) {
    for _ in 0..100 {
        if request(port, "GET", "/api/v1/health", None).await.starts_with("HTTP/1.1 200") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("daemon did not become healthy");
}

async fn request(port: u16, method: &str, path: &str, body: Option<&str>) -> String {
    let body = body.unwrap_or("");
    let mut stream = match TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await {
        Ok(stream) => stream,
        Err(_) => return String::new(),
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

async fn arm(root: &PathBuf, command: &str) {
    let socket = root.join("private-control.sock");
    for _ in 0..100 {
        if let Ok(mut stream) = UnixStream::connect(&socket).await {
            stream.write_all(command.as_bytes()).await.unwrap();
            let mut response = [0_u8; 32];
            let count = stream.read(&mut response).await.unwrap();
            assert_eq!(std::str::from_utf8(&response[..count]).unwrap(), "ok\n");
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("private daemon test control did not become available");
}

async fn kill(child: &mut Child) {
    child.start_kill().unwrap();
    child.wait().await.unwrap();
}

async fn cli_binary() -> PathBuf {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_msv-daemon"))
        .parent()
        .expect("daemon binary has a target directory")
        .join("msv");
    if binary.is_file() {
        return binary;
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("daemon crate belongs to the workspace")
        .to_path_buf();
    let status = Command::new("cargo")
        .current_dir(workspace)
        .args(["build", "-p", "my-supervisor-app-cli", "--bin", "msv"])
        .status()
        .await
        .expect("CLI build starts");
    assert!(status.success(), "CLI build succeeds for daemon/CLI E2E");
    assert!(binary.is_file(), "CLI binary exists after build");
    binary
}

struct EventFollow {
    child: Child,
    stdout: tokio::task::JoinHandle<()>,
    stderr: tokio::task::JoinHandle<Vec<u8>>,
    lines: Arc<Mutex<Vec<String>>>,
    line_notify: Arc<Notify>,
}

async fn follow_events(port: u16) -> EventFollow {
    let mut child = Command::new(cli_binary().await)
        .args(["--url", &format!("http://127.0.0.1:{port}"), "--output", "json", "daemon", "events"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("msv daemon events starts");
    let mut stdout = child.stdout.take().expect("CLI stdout captures");
    let mut stderr = child.stderr.take().expect("CLI stderr captures");
    let lines = Arc::new(Mutex::new(Vec::new()));
    let line_notify = Arc::new(Notify::new());
    let captured_lines = lines.clone();
    let captured_line_notify = line_notify.clone();
    EventFollow {
        child,
        stdout: tokio::spawn(async move {
            let mut reader = BufReader::new(&mut stdout).lines();
            while let Some(line) = reader.next_line().await.expect("CLI stdout line reads") {
                captured_lines.lock().unwrap().push(line);
                captured_line_notify.notify_waiters();
            }
        }),
        stderr: tokio::spawn(async move {
            let mut output = Vec::new();
            stderr.read_to_end(&mut output).await.expect("CLI stderr reads");
            output
        }),
        lines,
        line_notify,
    }
}

async fn wait_for_terminal_event_line(follow: &EventFollow) -> serde_json::Value {
    for _ in 0..120 {
        if let Some(event) = follow.lines.lock().unwrap().iter().find_map(|line| {
            serde_json::from_str::<serde_json::Value>(line).ok().filter(|event| {
                event["type"] == "job.run_succeeded"
                    && event["event_id"].as_str().is_some_and(|event_id| !event_id.is_empty())
            })
        }) {
            return event;
        }
        let _ = tokio::time::timeout(Duration::from_millis(100), follow.line_notify.notified()).await;
    }
    panic!("CLI did not record the first durable terminal event before timeout");
}

async fn wait_for_pending_terminal_event(store: &SqliteStore) -> my_supervisor_core::ports::TransientTerminalEvent {
    for _ in 0..120 {
        if let Some(event) = store.pending_transient_terminal_events(10).await.unwrap().into_iter().next() {
            return event;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("acknowledgement failure did not retain the terminal outbox record");
}

async fn stop_follow(mut follow: EventFollow) -> (std::process::Output, Vec<String>) {
    let pid = follow.child.id().expect("CLI has PID") as i32;
    // SAFETY: this test owns the spawned CLI process.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGINT) }, 0, "SIGINT reaches CLI follow");
    let status = tokio::time::timeout(Duration::from_secs(10), follow.child.wait())
        .await
        .expect("CLI follow exits")
        .expect("CLI follow wait succeeds");
    follow.stdout.await.expect("CLI stdout joins");
    let lines = follow.lines.lock().unwrap().clone();
    (std::process::Output {
        status,
        stdout: lines.join("\n").into_bytes(),
        stderr: follow.stderr.await.expect("CLI stderr joins"),
    }, lines)
}

fn process_is_gone(pid: i32) -> bool {
    // SAFETY: `kill(pid, 0)` only queries the two child PIDs captured by this test.
    (unsafe { libc::kill(pid, 0) }) == -1
        && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

#[tokio::test]
async fn sigkill_restart_replays_terminal_outbox_to_one_cli_json_line_and_converges() {
    let root = test_root();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(
        root.join("config.toml"),
        "[[job]]\nname = \"terminal-replay\"\ncommand = \"/bin/sh\"\nargs = [\"-c\", \"exit 0\"]\n[job.trigger]\ntype = \"interval\"\nevery_sec = 3600\n",
    )
    .await
    .unwrap();
    let port = unused_port();
    let mut first = daemon(&root, port).spawn().unwrap();
    let first_pid = first.id().expect("first daemon has PID") as i32;
    wait_for_health(port).await;
    let follow = follow_events(port).await;
    // Keep every pre-crash acknowledgement attempt failing.  This prevents a
    // fast retry from erasing the durable evidence between the CLI line being
    // observed and the test's DB assertion.
    arm(&root, "terminal_ack 100\n").await;

    assert!(request(port, "POST", "/api/v1/jobs/terminal-replay/trigger", None).await.starts_with("HTTP/1.1 202"));
    let first_cli_event = wait_for_terminal_event_line(&follow).await;
    let first_event_id = first_cli_event["event_id"].as_str().unwrap().to_owned();

    let store = SqliteStore::connect(root.join("state.db")).await.unwrap();
    assert_eq!(store.list_runs("terminal-replay", 10).await.unwrap().len(), 1);
    let pending_event = wait_for_pending_terminal_event(&store).await;
    assert_eq!(pending_event.event_id.to_string(), first_event_id, "CLI first frame and durable outbox share the stable event ID");
    // The normal completion path commits Run+outbox atomically and therefore
    // has no transient-cleanup ticket; that ticket only exists for unreaped
    // lifecycle recovery.
    assert!(store.pending_transient_cleanup(10).await.unwrap().is_empty());
    drop(store);
    // The injected acknowledgement failure leaves the terminal row immutable
    // and the durable outbox present when the daemon is SIGKILLed.
    kill(&mut first).await;

    let mut second = daemon(&root, port).spawn().unwrap();
    let second_pid = second.id().expect("second daemon has PID") as i32;
    wait_for_health(port).await;
    tokio::time::sleep(Duration::from_secs(6)).await;

    let store = SqliteStore::connect(root.join("state.db")).await.unwrap();
    for _ in 0..100 {
        if store.pending_transient_terminal_events(10).await.unwrap().is_empty()
            && store.pending_transient_cleanup(10).await.unwrap().is_empty()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(store.pending_transient_terminal_events(10).await.unwrap().is_empty());
    assert!(store.pending_transient_cleanup(10).await.unwrap().is_empty());
    let runs = store.list_runs("terminal-replay", 10).await.unwrap();
    assert_eq!(runs.len(), 1);
    assert!(runs[0].state.is_terminal());
    drop(store);

    kill(&mut second).await;
    let (output, recorded_lines) = stop_follow(follow).await;
    assert!(output.status.success(), "CLI event follow exits successfully: {}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stderr.is_empty(), "CLI event follow keeps stderr empty");
    let matching_events = recorded_lines
        .iter()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("CLI event line is JSON"))
        .filter(|event| event["event_id"] == first_event_id && event["type"] == "job.run_succeeded")
        .collect::<Vec<_>>();
    assert_eq!(matching_events.len(), 1, "CLI session dedupe prints replayed terminal event once");
    tokio::fs::remove_dir_all(&root).await.unwrap();
    assert!(!root.exists(), "temporary daemon directory is removed");
    assert!(process_is_gone(first_pid), "first daemon process is reaped");
    assert!(process_is_gone(second_pid), "second daemon process is reaped");
}

#[tokio::test]
async fn sigkill_restart_completes_pre_cancellation_delete_rollback_without_losing_queued_runs() {
    let root = test_root();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(
        root.join("config.toml"),
        "[[job]]\nname = \"delete-rollback\"\ncommand = \"/bin/sh\"\nargs = [\"-c\", \"sleep 2\"]\non_overlap = \"queue\"\n[job.trigger]\ntype = \"interval\"\nevery_sec = 3600\n",
    )
    .await
    .unwrap();
    let port = unused_port();
    let mut first = daemon(&root, port).spawn().unwrap();
    wait_for_health(port).await;
    assert!(request(port, "POST", "/api/v1/jobs/delete-rollback/trigger", None).await.starts_with("HTTP/1.1 202"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(request(port, "POST", "/api/v1/jobs/delete-rollback/trigger", None).await.starts_with("HTTP/1.1 202"));
    arm(&root, "deletion_cancellation 1\n").await;
    arm(&root, "deletion_clear 1\n").await;
    let deletion = request(port, "DELETE", "/api/v1/jobs/delete-rollback?force=true", None).await;
    assert!(deletion.starts_with("HTTP/1.1 500"), "expected injected rollback clear failure: {deletion}");

    kill(&mut first).await;
    // The active shell was intentionally short-lived.  Wait for it before
    // reopening so the test can prove restart recovery does not retain a child
    // or queued-run registry entry.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let store = SqliteStore::connect(root.join("state.db")).await.unwrap();
    assert_eq!(store.list_incomplete_job_deletions().await.unwrap().len(), 1);
    assert_eq!(store.list_runs("delete-rollback", 10).await.unwrap().len(), 2);
    drop(store);

    let mut second = daemon(&root, port).spawn().unwrap();
    wait_for_health(port).await;
    let store = SqliteStore::connect(root.join("state.db")).await.unwrap();
    for _ in 0..100 {
        if store.list_incomplete_job_deletions().await.unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(store.get_job("delete-rollback").await.unwrap().is_some());
    assert_eq!(store.list_runs("delete-rollback", 10).await.unwrap().len(), 2);
    assert!(store.list_incomplete_job_deletions().await.unwrap().is_empty());
    let terminal_events = store.pending_transient_terminal_events(10).await.unwrap();
    assert_eq!(terminal_events.len(), 1, "restart cancellation commits its terminal outbox event");
    assert_eq!(terminal_events[0].state, my_supervisor_core::domain::JobRunState::Cancelled);
    assert!(store.pending_transient_cleanup(10).await.unwrap().is_empty());
    drop(store);

    kill(&mut second).await;
    tokio::fs::remove_dir_all(&root).await.unwrap();
}

#[tokio::test]
async fn sigkill_restart_forward_recovers_direct_config_snapshot_after_commit_failure() {
    let root = test_root();
    tokio::fs::create_dir_all(&root).await.unwrap();
    let config_path = root.join("config.toml");
    tokio::fs::write(
        &config_path,
        "[[job]]\nname = \"config-previous\"\ncommand = \"/bin/true\"\n[job.trigger]\ntype = \"interval\"\nevery_sec = 3600\n",
    )
    .await
    .unwrap();
    let port = unused_port();
    let mut first = daemon(&root, port).spawn().unwrap();
    wait_for_health(port).await;
    tokio::fs::write(
        &config_path,
        "[[job]]\nname = \"config-target\"\ncommand = \"/bin/true\"\n[job.trigger]\ntype = \"interval\"\nevery_sec = 3600\n",
    )
    .await
    .unwrap();
    arm(&root, "config_snapshot 1\n").await;
    let reload = request(port, "POST", "/api/v1/daemon/reload", None).await;
    assert!(!reload.starts_with("HTTP/1.1 200"), "expected injected config commit failure: {reload}");
    let store = SqliteStore::connect(root.join("state.db")).await.unwrap();
    assert_eq!(store.list_incomplete_config_applies().await.unwrap().len(), 1);
    drop(store);

    kill(&mut first).await;
    let mut second = daemon(&root, port).spawn().unwrap();
    wait_for_health(port).await;
    let store = SqliteStore::connect(root.join("state.db")).await.unwrap();
    for _ in 0..100 {
        if store.list_incomplete_config_applies().await.unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(store.get_job("config-previous").await.unwrap().is_none());
    assert!(store.get_job("config-target").await.unwrap().is_some());
    assert!(store.list_incomplete_config_applies().await.unwrap().is_empty());
    assert!(store.list_incomplete_job_deletions().await.unwrap().is_empty());
    assert!(store.pending_transient_terminal_events(10).await.unwrap().is_empty());
    assert!(store.pending_transient_cleanup(10).await.unwrap().is_empty());
    drop(store);

    kill(&mut second).await;
    tokio::fs::remove_dir_all(&root).await.unwrap();
}
