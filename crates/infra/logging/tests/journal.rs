use my_supervisor_core::domain::{JobRunId, LogLine, LogStream};
use my_supervisor_core::ports::LogSink;
use my_supervisor_infra_logging::InMemoryLogSink;

#[tokio::test]
async fn durable_sequences_continue_after_restart_and_seal_rejects_late_append() {
    let directory = std::env::temp_dir().join(format!("my-supervisor-journal-e2e-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let run_id = JobRunId::new();
    let sink = InMemoryLogSink::with_log_dir(directory.clone());
    for _ in 0..10_001 { sink.append_run(run_id, LogLine::now(LogStream::Stdout, "repeat")).await.unwrap(); }
    let page = sink.tail_run(run_id, 10_001, None, None).await;
    assert_eq!((page.lines.len(), page.lines[0].sequence, page.high_watermark), (10_001, 1, 10_001));
    sink.seal_run(run_id).await.unwrap();
    assert!(sink.append_run(run_id, LogLine::now(LogStream::Stdout, "late")).await.is_err());
    drop(sink);
    let sink = InMemoryLogSink::with_log_dir(directory.clone());
    assert_eq!(sink.tail_run(run_id, 0, None, None).await.high_watermark, 10_001);
    tokio::fs::remove_dir_all(directory).await.unwrap();
}

#[tokio::test]
async fn run_cursor_recovers_past_the_memory_window_before_applying_limit() {
    let directory = std::env::temp_dir().join(format!("my-supervisor-run-cursor-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let run_id = JobRunId::new();
    let sink = InMemoryLogSink::with_log_dir(directory.clone());

    for sequence in 1..=10_001 {
        sink.append_run(run_id, LogLine::now(LogStream::Stdout, format!("line-{sequence}"))).await.unwrap();
    }

    let page = sink.tail_run(run_id, 3, None, Some(0)).await;
    assert_eq!(page.lines.iter().map(|line| line.sequence).collect::<Vec<_>>(), vec![9_999, 10_000, 10_001]);
    assert!(page.truncated);
    assert_eq!((page.high_watermark, page.next_sequence), (10_001, 10_002));

    tokio::fs::remove_dir_all(directory).await.unwrap();
}

#[tokio::test]
async fn legacy_collision_is_quarantined_and_singleton_history_migrates_once() {
    let directory = std::env::temp_dir().join(format!("my-supervisor-legacy-journal-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let legacy = directory.join("process-a_b.jsonl");
    tokio::fs::write(&legacy, "{\"sequence\":1,\"timestamp\":\"2026-01-01T00:00:00Z\",\"stream\":\"stdout\",\"line\":\"historic\"}\n").await.unwrap();

    let sink = InMemoryLogSink::with_log_dir(directory.clone());
    sink.register_process_names(&["a/b".into(), "a?b".into()]);
    assert!(sink.tail("a/b", 0, None, None).await.lines.is_empty());

    let singleton = InMemoryLogSink::with_log_dir(directory.clone());
    singleton.register_process_names(&["a/b".into()]);
    let page = singleton.tail("a/b", 0, None, None).await;
    assert_eq!(page.lines.iter().map(|line| line.line.as_str()).collect::<Vec<_>>(), vec!["historic"]);
    singleton.append("a/b", LogLine::now(LogStream::Stdout, "new")).await.unwrap();
    drop(singleton);
    let restarted = InMemoryLogSink::with_log_dir(directory.clone());
    assert_eq!(restarted.tail("a/b", 0, None, None).await.lines.len(), 2);
    tokio::fs::remove_dir_all(directory).await.unwrap();
}
