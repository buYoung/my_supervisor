use std::sync::Arc;
use std::time::Duration;

use my_supervisor_core::domain::{LifecycleMode, ProcessSpec};
use my_supervisor_core::ports::{Aliveness, LifecycleController, LogSink};
use my_supervisor_infra_logging::InMemoryLogSink;
use my_supervisor_platform_macos::{DetachedHelperPaths, DetachedTestControls, MacLifecycle};

fn log_dir() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "my-supervisor-detached-journal-{}",
        uuid::Uuid::new_v4()
    ))
}

async fn wait_for_lines(
    lifecycle: &MacLifecycle,
    spec: &ProcessSpec,
    expected: usize,
) -> my_supervisor_core::ports::LogTail {
    for _ in 0..80 {
        let page = lifecycle
            .tail_detached_logs(spec, 0, None, None, std::slice::from_ref(&spec.name))
            .await
            .unwrap();
        if page.lines.len() >= expected {
            return page;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let page = lifecycle
        .tail_detached_logs(spec, 0, None, None, std::slice::from_ref(&spec.name))
        .await
        .unwrap();
    panic!(
        "detached journal did not receive {expected} lines; received {} with high-watermark {}",
        page.lines.len(),
        page.high_watermark
    )
}

async fn remove_test_directory(directory: &std::path::Path) {
    for attempt in 0..20 {
        match tokio::fs::remove_dir_all(directory).await {
            Ok(()) => return,
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty && attempt < 19 => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(error) => panic!("removing {}: {error}", directory.display()),
        }
    }
    unreachable!("the final directory removal either returns or panics");
}

fn detached_helper_paths() -> DetachedHelperPaths {
    DetachedHelperPaths::new(
        std::path::PathBuf::from(env!("CARGO_BIN_EXE_msv-log-proxy")),
        std::path::PathBuf::from(env!("CARGO_BIN_EXE_msv-group-reaper")),
    )
    .expect("test helper binaries are executable")
}

fn detached_lifecycle(log_sink: Arc<dyn LogSink>, directory: std::path::PathBuf) -> MacLifecycle {
    MacLifecycle::with_detached_helpers(log_sink, directory, detached_helper_paths())
}

fn detached_lifecycle_with_controls(
    log_sink: Arc<dyn LogSink>,
    directory: std::path::PathBuf,
    test_controls: DetachedTestControls,
) -> MacLifecycle {
    MacLifecycle::with_detached_helpers_and_test_controls(
        log_sink,
        directory,
        detached_helper_paths(),
        test_controls,
    )
}

fn assert_pid_absent(pid: i32) {
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "PID {pid} remained alive"
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

async fn assert_group_absent(pgid: u32) {
    for _ in 0..80 {
        // SAFETY: signal 0 only checks whether the dedicated group remains.
        if unsafe { libc::kill(-(pgid as i32), 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("process group {pgid} remained after detached cleanup");
}

#[tokio::test]
async fn detached_proxy_preserves_interleaved_sequence_cursor_and_terminal_line() {
    let directory = log_dir();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(directory.clone()));
    let lifecycle = detached_lifecycle(sink, directory.clone());
    let mut spec = ProcessSpec::new("한글 name / detached", "/bin/sh");
    spec.lifecycle = LifecycleMode::Detached;
    spec.args = vec!["-c".into(), "printf 'stdout-first\\n'; i=1; while [ $i -le 100 ]; do printf 'stderr-%s\\n' $i >&2; i=$((i + 1)); done; sleep 1".into()];

    lifecycle.spawn_detached(&spec).await.unwrap();
    let page = wait_for_lines(&lifecycle, &spec, 102).await;
    assert_eq!(page.high_watermark, 102);
    assert_eq!(page.lines.len(), 102);
    assert_eq!(page.lines.first().unwrap().sequence, 1);
    assert_eq!(page.lines.last().unwrap().sequence, 102);
    assert!(page
        .lines
        .windows(2)
        .all(|pair| pair[0].sequence + 1 == pair[1].sequence));
    assert!(page.lines.iter().any(|line| line.line == "stdout-first"));
    assert_eq!(
        page.lines
            .iter()
            .filter(|line| line.line.starts_with("stderr-"))
            .count(),
        100
    );
    assert!(page
        .lines
        .last()
        .unwrap()
        .line
        .starts_with("target exited:"));

    let tail = lifecycle
        .tail_detached_logs(&spec, 10, None, None, &[spec.name.clone()])
        .await
        .unwrap();
    assert!(tail.truncated);
    assert_eq!(tail.lines.len(), 10);
    let resumed = lifecycle
        .tail_detached_logs(&spec, 0, None, Some(100), &[spec.name.clone()])
        .await
        .unwrap();
    assert_eq!(
        resumed
            .lines
            .iter()
            .map(|line| line.sequence)
            .collect::<Vec<_>>(),
        vec![101, 102]
    );

    let restarted = MacLifecycle::new(
        Arc::new(InMemoryLogSink::with_log_dir(directory.clone())),
        directory.clone(),
    );
    let restarted_page = restarted
        .tail_detached_logs(&spec, 0, None, Some(100), &[spec.name.clone()])
        .await
        .unwrap();
    assert_eq!(
        restarted_page
            .lines
            .iter()
            .map(|line| line.sequence)
            .collect::<Vec<_>>(),
        vec![101, 102]
    );
    remove_test_directory(&directory).await;
}

#[tokio::test]
async fn detached_proxy_reaps_target_when_journal_append_fails() {
    let directory = log_dir();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(directory.clone()));
    let lifecycle = detached_lifecycle_with_controls(
        sink,
        directory.clone(),
        DetachedTestControls {
            proxy_fail_after_appends: Some(0),
            ..Default::default()
        },
    );
    let mut spec = ProcessSpec::new("journal failure target", "/bin/sh");
    spec.lifecycle = LifecycleMode::Detached;
    // The target writes one line and then outlives its pipes. The proxy must
    // not panic or leave the dedicated group alive when the append fails.
    spec.args = vec![
        "-c".into(),
        "printf 'trigger failure\\n'; exec sleep 60".into(),
    ];

    let handle = lifecycle.spawn_detached(&spec).await.unwrap();
    for _ in 0..80 {
        if matches!(lifecycle.probe_alive(&handle).await, Ok(Aliveness::Dead)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(matches!(
        lifecycle.probe_alive(&handle).await,
        Ok(Aliveness::Dead)
    ));
    let pgid = handle.pgid.expect("detached proxy owns a process group");
    assert_group_absent(pgid).await;

    remove_test_directory(&directory).await;
}

#[tokio::test]
async fn detached_proxy_reaper_removes_shell_and_grandchild_after_journal_failure() {
    let directory = log_dir();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(directory.clone()));
    let lifecycle = detached_lifecycle_with_controls(
        sink,
        directory.clone(),
        DetachedTestControls {
            proxy_fail_after_appends: Some(1),
            ..Default::default()
        },
    );
    let target_pids = directory.join("target-pids");
    let mut spec = ProcessSpec::new("journal failure shell tree", "/bin/sh");
    spec.lifecycle = LifecycleMode::Detached;
    spec.args = vec![
        "-c".into(),
        "printf 'ready\\n'; sleep 60 & grandchild=$!; printf '%s %s\\n' \"$$\" \"$grandchild\" > \"$1\"; printf 'trigger failure\\n'; wait".into(),
        "sh".into(),
        target_pids.display().to_string(),
    ];

    let handle = lifecycle.spawn_detached(&spec).await.unwrap();
    let recorded_pids = for_read_target_pids(&target_pids).await;
    for _ in 0..120 {
        if matches!(lifecycle.probe_alive(&handle).await, Ok(Aliveness::Dead)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(matches!(
        lifecycle.probe_alive(&handle).await,
        Ok(Aliveness::Dead)
    ));
    let pgid = handle.pgid.expect("detached proxy owns a process group");
    assert_group_absent(pgid).await;
    for pid in recorded_pids {
        assert_pid_absent(pid);
    }
    assert_no_detached_journal(&directory).await;

    remove_test_directory(&directory).await;
}

#[tokio::test]
async fn detached_proxy_reaper_cleans_target_when_takeover_ack_is_withheld() {
    let directory = log_dir();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(directory.clone()));
    let lifecycle = detached_lifecycle_with_controls(
        sink,
        directory.clone(),
        DetachedTestControls {
            proxy_fail_after_appends: Some(0),
            reaper_withhold_takeover_ack: true,
            ..Default::default()
        },
    );
    let mut spec = ProcessSpec::new("journal handshake failure", "/bin/sh");
    spec.lifecycle = LifecycleMode::Detached;
    spec.args = vec![
        "-c".into(),
        "printf 'trigger failure\\n'; exec sleep 60".into(),
    ];

    let handle = lifecycle.spawn_detached(&spec).await.unwrap();
    for _ in 0..120 {
        if matches!(lifecycle.probe_alive(&handle).await, Ok(Aliveness::Dead)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(matches!(
        lifecycle.probe_alive(&handle).await,
        Ok(Aliveness::Dead)
    ));
    let pgid = handle.pgid.expect("detached proxy owns a process group");
    assert_group_absent(pgid).await;

    remove_test_directory(&directory).await;
}

#[tokio::test]
async fn detached_replacement_owner_reaps_after_the_first_reaper_crashes() {
    let directory = log_dir();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(directory.clone()));
    let lifecycle = detached_lifecycle_with_controls(
        sink,
        directory.clone(),
        DetachedTestControls {
            proxy_fail_after_appends: Some(1),
            first_reaper_crash_after_start: true,
            ..Default::default()
        },
    );
    let target_pids = directory.join("target-pids");
    let mut spec = ProcessSpec::new("replacement cleanup owner", "/bin/sh");
    spec.lifecycle = LifecycleMode::Detached;
    spec.args = vec![
        "-c".into(),
        "printf 'ready\\n'; sleep 60 & grandchild=$!; printf '%s %s\\n' \"$$\" \"$grandchild\" > \"$1\"; printf 'trigger failure\\n'; wait".into(),
        "sh".into(),
        target_pids.display().to_string(),
    ];

    let handle = lifecycle.spawn_detached(&spec).await.unwrap();
    let recorded_pids = for_read_target_pids(&target_pids).await;
    for _ in 0..120 {
        if matches!(lifecycle.probe_alive(&handle).await, Ok(Aliveness::Dead)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(matches!(
        lifecycle.probe_alive(&handle).await,
        Ok(Aliveness::Dead)
    ));
    let pgid = handle.pgid.expect("detached proxy owns a process group");
    assert_group_absent(pgid).await;
    for pid in recorded_pids {
        assert_pid_absent(pid);
    }
    assert_no_detached_journal(&directory).await;
    remove_test_directory(&directory).await;
}

#[tokio::test]
async fn detached_reapers_clean_the_anchor_target_and_journal_after_proxy_sigkill() {
    let directory = log_dir();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(directory.clone()));
    let lifecycle = detached_lifecycle(sink, directory.clone());
    let target_pids = directory.join("target-pids");
    let mut spec = ProcessSpec::new("proxy crash cleanup", "/bin/sh");
    spec.lifecycle = LifecycleMode::Detached;
    spec.args = vec![
        "-c".into(),
        "sleep 60 & grandchild=$!; printf '%s %s\\n' \"$$\" \"$grandchild\" > \"$1\"; printf 'ready\\n'; wait".into(),
        "sh".into(),
        target_pids.display().to_string(),
    ];

    let handle = lifecycle.spawn_detached(&spec).await.unwrap();
    let recorded_pids = for_read_target_pids(&target_pids).await;
    assert_eq!(unsafe { libc::kill(handle.pid as i32, libc::SIGKILL) }, 0);
    for _ in 0..120 {
        if matches!(lifecycle.probe_alive(&handle).await, Ok(Aliveness::Dead)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(matches!(
        lifecycle.probe_alive(&handle).await,
        Ok(Aliveness::Dead)
    ));
    let pgid = handle.pgid.expect("detached proxy owns a process group");
    assert_group_absent(pgid).await;
    for pid in recorded_pids {
        assert_pid_absent(pid);
    }
    assert_no_detached_journal(&directory).await;
    remove_test_directory(&directory).await;
}

#[tokio::test]
async fn target_fault_named_environment_does_not_control_production_helpers() {
    let directory = log_dir();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let sink: Arc<dyn LogSink> = Arc::new(InMemoryLogSink::with_log_dir(directory.clone()));
    let lifecycle = detached_lifecycle(sink, directory.clone());
    let mut spec = ProcessSpec::new("target environment isolation", "/bin/sh");
    spec.lifecycle = LifecycleMode::Detached;
    spec.env.insert(
        "MSV_DETACHED_TEST_PROXY_FAIL_AFTER_APPENDS".into(),
        "0".into(),
    );
    spec.env.insert(
        "MSV_DETACHED_TEST_REAPER_WITHHOLD_TAKEOVER_ACK".into(),
        "1".into(),
    );
    spec.args = vec![
        "-c".into(),
        "printf 'target-env=%s\\n' \"$MSV_DETACHED_TEST_PROXY_FAIL_AFTER_APPENDS\"".into(),
    ];

    lifecycle.spawn_detached(&spec).await.unwrap();
    let page = wait_for_lines(&lifecycle, &spec, 2).await;
    assert!(page.lines.iter().any(|line| line.line == "target-env=0"));
    assert!(page
        .lines
        .last()
        .unwrap()
        .line
        .starts_with("target exited:"));
    remove_test_directory(&directory).await;
}

async fn for_read_target_pids(path: &std::path::Path) -> Vec<i32> {
    for _ in 0..80 {
        if let Ok(contents) = tokio::fs::read_to_string(path).await {
            let pids = contents
                .split_whitespace()
                .map(|pid| pid.parse::<i32>().expect("target PID is numeric"))
                .collect::<Vec<_>>();
            if pids.len() == 2 {
                return pids;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("shell target did not record its leader and grandchild PIDs");
}

async fn assert_no_detached_journal(directory: &std::path::Path) {
    let mut entries = tokio::fs::read_dir(directory).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let file_name = entry.file_name();
        let is_detached_journal = file_name
            .to_str()
            .is_some_and(|name| name.starts_with("direct-") && name.ends_with(".jsonl"));
        assert!(
            !is_detached_journal,
            "failed detached journal remained at {}",
            entry.path().display()
        );
    }
}
