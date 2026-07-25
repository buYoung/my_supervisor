#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use my_supervisor_app_daemon::build_deps_with_paths;
use my_supervisor_infra_http::assemble;
use tokio::io::AsyncReadExt;
use tokio::process::Child;
use tokio::sync::oneshot;

struct EphemeralDaemon {
    facade: Arc<my_supervisor_application::OperationsFacade>,
    port: u16,
    router: axum::Router,
    shutdown: Option<oneshot::Sender<()>>,
    server: Option<tokio::task::JoinHandle<()>>,
    scheduler: tokio::task::JoinHandle<()>,
    supervisor: tokio::task::JoinHandle<()>,
}

impl EphemeralDaemon {
    async fn start(root: &Path) -> Self {
        let assembled = assemble(
            build_deps_with_paths(root.join("daemon-data"), root.join("daemon-config.toml"))
                .await
                .expect("daemon dependencies assemble"),
        );
        assembled.facade.bootstrap().await.expect("daemon bootstrap");
        let facade = assembled.facade;
        let scheduler = tokio::spawn(facade.clone().run_scheduler_loop());
        let supervisor = tokio::spawn(facade.clone().run_process_supervisor_loop());
        let router = assembled.router;
        let mut daemon = Self {
            facade,
            port: 0,
            router,
            shutdown: None,
            server: None,
            scheduler,
            supervisor,
        };
        daemon.start_transport().await;
        daemon
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    async fn start_transport(&mut self) {
        let bind_port = self.port;
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, bind_port))
            .await
            .expect("ephemeral daemon binds");
        self.port = listener.local_addr().expect("listener address").port();
        let (shutdown, receiver) = oneshot::channel();
        let router = self.router.clone();
        self.server = Some(tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async { let _ = receiver.await; })
                .await
                .expect("ephemeral daemon serves");
        }));
        self.shutdown = Some(shutdown);
    }

    async fn stop_transport(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(server) = self.server.take() {
            server.await.expect("daemon server joins");
        }
    }

    async fn stop(mut self) {
        self.stop_transport().await;
        self.facade.shutdown_all().await.expect("daemon drains work");
        self.scheduler.await.expect("scheduler joins");
        self.supervisor.await.expect("supervisor joins");
    }
}

fn temporary_directory() -> PathBuf {
    std::env::temp_dir().join(format!("my-supervisor-cli-e2e-{}", uuid::Uuid::new_v4()))
}

async fn write_config(path: &Path, contents: &str) {
    tokio::fs::write(path, contents).await.expect("configuration writes");
}

async fn cli(url: &str, args: &[&str]) -> std::process::Output {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_msv"))
        .args(["--url", url, "--output", "json"])
        .args(args)
        .output()
        .await
        .expect("msv starts")
}

struct FollowChild {
    child: Child,
    stdout: tokio::task::JoinHandle<Vec<u8>>,
    stderr: tokio::task::JoinHandle<Vec<u8>>,
}

async fn start_follow(url: &str, args: &[&str]) -> FollowChild {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_msv"))
        .args(["--url", url, "--output", "json"])
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("msv follow starts");
    let mut stdout = child.stdout.take().expect("follow stdout captures");
    let mut stderr = child.stderr.take().expect("follow stderr captures");
    FollowChild {
        child,
        stdout: tokio::spawn(async move {
            let mut output = Vec::new();
            stdout.read_to_end(&mut output).await.expect("follow stdout reads");
            output
        }),
        stderr: tokio::spawn(async move {
            let mut output = Vec::new();
            stderr.read_to_end(&mut output).await.expect("follow stderr reads");
            output
        }),
    }
}

async fn interrupt_follow(mut follow: FollowChild) -> std::process::Output {
    let pid = follow.child.id().expect("follow child has PID") as i32;
    // SAFETY: PID belongs to the child process just spawned by this test.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGINT) }, 0, "SIGINT reaches msv follow");
    let status = tokio::time::timeout(Duration::from_secs(10), follow.child.wait())
        .await
        .expect("msv follow exits after SIGINT")
        .expect("msv follow wait succeeds");
    std::process::Output {
        status,
        stdout: follow.stdout.await.expect("follow stdout joins"),
        stderr: follow.stderr.await.expect("follow stderr joins"),
    }
}

async fn wait_for_final_log_line(url: &str, args: &[&str], final_line: &str) {
    for _ in 0..600 {
        let output = output_text(&cli(url, args).await);
        if output.contains(final_line) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{final_line} was not written to the durable log cursor");
}

async fn wait_for_process_group(pid_file: &Path) -> u32 {
    for _ in 0..100 {
        if let Ok(contents) = tokio::fs::read_to_string(pid_file).await {
            let process_group = contents
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .expect("term-tree records its dedicated process group");
            return process_group;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("term-tree did not record its process group");
}

async fn wait_for_process_group_exit(process_group: u32) {
    for _ in 0..100 {
        // SAFETY: this test owns the Direct process group recorded by its leader.
        if unsafe { libc::kill(-(process_group as i32), 0) } != 0 {
            assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("Direct process group {process_group} remained after stop/config replace");
}

fn output_text(output: &std::process::Output) -> String {
    assert!(
        output.status.success(),
        "msv failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "successful msv command wrote stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).expect("msv emits UTF-8")
}

fn json_lines(output: &str, expected_count: usize, first_line: &str, last_line: &str) {
    let lines = output
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .unwrap_or_else(|error| panic!("follow emits one JSON object per line; invalid row {line:?}: {error}"))
        })
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), expected_count, "follow emits every expected JSONL row");
    let sequences = lines
        .iter()
        .map(|line| line["sequence"].as_u64().expect("log row has a numeric sequence"))
        .collect::<Vec<_>>();
    assert!(sequences.iter().all(|sequence| *sequence > 0), "log sequence never uses compatibility zero");
    assert!(sequences.windows(2).all(|pair| pair[0] < pair[1]), "log sequence is unique and strictly increasing");
    assert_eq!(lines.first().and_then(|line| line["line"].as_str()), Some(first_line));
    assert_eq!(lines.last().and_then(|line| line["line"].as_str()), Some(last_line));
}

fn assert_failed_command(output: &std::process::Output, expected_code: &str) {
    assert_eq!(output.status.code(), Some(1), "command keeps the documented general failure exit code");
    assert!(output.stdout.is_empty(), "failed command writes no stdout");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(expected_code), "stderr contains the stable API error code: {stderr}");
}

#[test]
fn cli_exposes_cancel_config_and_follow_contracts() {
    let output = Command::new(env!("CARGO_BIN_EXE_msv"))
        .arg("--help")
        .output()
        .expect("msv help starts");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(help.contains("config"));
    assert!(help.contains("job"));
    assert!(help.contains("logs"));
}

#[tokio::test]
async fn actual_cli_and_ephemeral_daemon_cover_config_cancel_follow_reconnect_and_exit_codes() {
    let root = temporary_directory();
    tokio::fs::create_dir_all(&root).await.expect("temporary directory creates");
    let config = root.join("valid.toml");
    let replacement = root.join("replacement.toml");
    let invalid = root.join("invalid.toml");
    let base_config_contents = r#"
[[process]]
name = "lots-process"
command = "/bin/sh"
args = ["-c", "sleep 1; i=1; while [ $i -le 10001 ]; do echo process-$i; i=$((i+1)); done; sleep 30"]
autostart = false

[[process]]
name = "follow-process"
command = "/bin/sh"
args = ["-c", "echo before-disconnect; sleep 2; echo after-reconnect; sleep 30"]
autostart = false

[[job]]
name = "cancel-job"
command = "/bin/sh"
args = ["-c", "trap 'exit 0' TERM; sleep 30 & wait"]
trigger = { type = "interval", every_sec = 3600 }

[[job]]
name = "lots-job"
command = "/bin/sh"
args = ["-c", "sleep 1; i=1; while [ $i -le 10001 ]; do echo run-$i; i=$((i+1)); done; sleep 30"]
trigger = { type = "interval", every_sec = 3600 }

[[job]]
name = "event-job"
command = "/bin/true"
trigger = { type = "interval", every_sec = 3600 }
"#;
    let term_tree_pids = root.join("term-tree-pids");
    let term_tree = format!(
        r#"

[[process]]
name = "term-tree"
command = "/bin/sh"
args = ["-c", "trap 'exit 0' TERM; (trap '' TERM; while :; do sleep 1; done) & printf '%s %s' \"$$\" \"$!\" > '{}'; wait"]
autostart = false
shutdown = {{ grace_period_ms = 100 }}
"#,
        term_tree_pids.display()
    );
    let replacement_tree = r#"

[[process]]
name = "term-tree"
command = "/bin/sh"
args = ["-c", "sleep 30"]
autostart = false
"#;
    let config_contents = format!("{base_config_contents}{term_tree}");
    let replacement_contents = format!("{base_config_contents}{replacement_tree}");
    write_config(&config, &config_contents).await;
    write_config(&replacement, &replacement_contents).await;
    write_config(
        &invalid,
        r#"
[[process]]
name = "invalid-command"
command = ""
"#,
    )
    .await;

    let mut daemon = EphemeralDaemon::start(&root).await;
    let url = daemon.url();
    let result = async {
        output_text(&cli(&url, &["config", "validate", "--file", config.to_str().unwrap(), "--mode", "replace"]).await);
        let invalid_validate = cli(&url, &["config", "validate", "--file", invalid.to_str().unwrap()]).await;
        assert_failed_command(&invalid_validate, "invalid_config");
        let invalid_apply = cli(&url, &["config", "apply", "--file", invalid.to_str().unwrap()]).await;
        assert_failed_command(&invalid_apply, "invalid_config");
        output_text(&cli(&url, &["config", "apply", "--file", config.to_str().unwrap(), "--mode", "replace"]).await);

        output_text(&cli(&url, &["start", "lots-process"]).await);
        let process_follow = start_follow(&url, &["logs", "lots-process", "--follow", "--tail", "10001"]).await;
        wait_for_final_log_line(&url, &["logs", "lots-process", "--tail", "1"], "process-10001").await;
        let process_follow_output = output_text(&interrupt_follow(process_follow).await);
        json_lines(&process_follow_output, 10_001, "process-1", "process-10001");

        let cancel_run = output_text(&cli(&url, &["job", "trigger", "cancel-job"]).await);
        let cancel_run_id = serde_json::from_str::<serde_json::Value>(&cancel_run)
            .expect("trigger result is JSON")["run_id"]
            .as_str()
            .expect("trigger returns run id")
            .to_owned();
        output_text(&cli(&url, &["job", "cancel", "cancel-job", &cancel_run_id]).await);
        let mut cancelled = false;
        for _ in 0..40 {
            let runs = output_text(&cli(&url, &["job", "runs", "cancel-job", "--limit", "20"]).await);
            cancelled = serde_json::from_str::<serde_json::Value>(&runs)
                .expect("runs result is JSON")["runs"]
                .as_array()
                .expect("runs list")
                .iter()
                .any(|run| run["run_id"] == cancel_run_id && run["state"] == "cancelled");
            if cancelled { break; }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(cancelled, "job cancel reaches terminal cancelled state");

        let lots_run = output_text(&cli(&url, &["job", "trigger", "lots-job"]).await);
        let lots_run_id = serde_json::from_str::<serde_json::Value>(&lots_run)
            .expect("trigger result is JSON")["run_id"]
            .as_str()
            .expect("trigger returns run id")
            .to_owned();
        let run_follow = start_follow(&url, &["job", "logs", "lots-job", &lots_run_id, "--follow", "--tail", "10001"]).await;
        wait_for_final_log_line(&url, &["job", "logs", "lots-job", &lots_run_id, "--tail", "1"], "run-10001").await;
        let run_follow_output = output_text(&interrupt_follow(run_follow).await);
        json_lines(&run_follow_output, 10_001, "run-1", "run-10001");
        output_text(&cli(&url, &["job", "cancel", "lots-job", &lots_run_id]).await);

        output_text(&cli(&url, &["start", "follow-process"]).await);
        tokio::time::sleep(Duration::from_millis(150)).await;
        let reconnect_follow = start_follow(&url, &["logs", "follow-process", "--follow", "--tail", "10001"]).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        daemon.stop_transport().await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        daemon.start_transport().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let reconnect_output = output_text(&interrupt_follow(reconnect_follow).await);
        let reconnect_lines = reconnect_output
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("reconnect follow emits JSON Lines"))
            .collect::<Vec<_>>();
        let reconnect_sequences = reconnect_lines
            .iter()
            .map(|line| line["sequence"].as_u64().expect("reconnect row has sequence"))
            .collect::<Vec<_>>();
        assert!(reconnect_sequences.windows(2).all(|pair| pair[0] < pair[1]), "reconnect keeps cursor order unique");
        assert_eq!(reconnect_lines.first().and_then(|line| line["line"].as_str()), Some("before-disconnect"));
        assert_eq!(reconnect_lines.last().and_then(|line| line["line"].as_str()), Some("after-reconnect"));

        let event_follow = start_follow(&url, &["daemon", "events"]).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        daemon.stop_transport().await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        daemon.start_transport().await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let event_run = output_text(&cli(&url, &["job", "trigger", "event-job"]).await);
        let event_run_id = serde_json::from_str::<serde_json::Value>(&event_run)
            .expect("event trigger result is JSON")["run_id"]
            .as_str()
            .expect("event trigger returns run id")
            .to_owned();
        let mut event_reached_terminal = false;
        let mut latest_event_runs = String::new();
        for _ in 0..70 {
            let runs = output_text(&cli(&url, &["job", "runs", "event-job", "--limit", "20"]).await);
            latest_event_runs = runs.clone();
            event_reached_terminal = serde_json::from_str::<serde_json::Value>(&runs)
                .expect("event runs result is JSON")["runs"]
                .as_array()
                .expect("event runs list")
                .iter()
                .any(|run| run["run_id"] == event_run_id && run["state"] == "failed");
            if event_reached_terminal {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(event_reached_terminal, "event job reaches durable terminal state: {latest_event_runs}");
        // The durable outbox reconcile interval is five seconds. Keep the
        // follower alive across a complete retry window after the terminal
        // row is observed.
        tokio::time::sleep(Duration::from_secs(6)).await;
        let event_output = interrupt_follow(event_follow).await;
        assert!(event_output.status.success(), "event follow exits cleanly: {}", String::from_utf8_lossy(&event_output.stderr));
        assert!(event_output.stderr.is_empty(), "successful event follow keeps stderr empty");
        let terminal_events = String::from_utf8(event_output.stdout)
            .expect("event follow emits UTF-8 JSON Lines")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("each event line is a JSON object"))
            .filter(|event| {
                event["payload"]["run_id"] == event_run_id
                    && event["type"] == "job.run_failed"
                    && event["event_id"].as_str().is_some_and(|id| !id.is_empty())
            })
            .collect::<Vec<_>>();
        assert_eq!(terminal_events.len(), 1, "reconnect emits the terminal ID once");
        assert!(terminal_events[0]["timestamp"].as_str().is_some());

        let term_tree_start = cli(&url, &["start", "term-tree"]).await;
        assert!(term_tree_start.status.success(), "CLI starts TERM-tree: {}", String::from_utf8_lossy(&term_tree_start.stderr));
        let stopped_group = wait_for_process_group(&term_tree_pids).await;
        let term_tree_stop = cli(&url, &["stop", "term-tree"]).await;
        assert!(term_tree_stop.status.success(), "CLI stops TERM-tree: {}", String::from_utf8_lossy(&term_tree_stop.stderr));
        wait_for_process_group_exit(stopped_group).await;

        tokio::fs::remove_file(&term_tree_pids).await.expect("old term-tree PID record removes");
        let term_tree_restart = cli(&url, &["start", "term-tree"]).await;
        assert!(term_tree_restart.status.success(), "CLI restarts TERM-tree: {}", String::from_utf8_lossy(&term_tree_restart.stderr));
        let replaced_group = wait_for_process_group(&term_tree_pids).await;
        let replacement_apply = cli(&url, &["config", "apply", "--file", replacement.to_str().unwrap(), "--mode", "replace"]).await;
        assert!(replacement_apply.status.success(), "CLI replaces running TERM-tree: {}", String::from_utf8_lossy(&replacement_apply.stderr));
        wait_for_process_group_exit(replaced_group).await;

        Ok::<(), String>(())
    }
    .await;

    daemon.stop().await;
    tokio::fs::remove_dir_all(&root)
        .await
        .expect("temporary E2E directory removes");
    result.expect("actual CLI release-gate scenario succeeds");
}
