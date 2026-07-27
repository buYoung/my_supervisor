use chrono::Utc;

use my_supervisor_application::views::LogPage;
use my_supervisor_core::domain::{LogLine, LogStream};
use my_supervisor_infra_http::mapping::log_page_to_dto;

#[test]
fn cursor_pages_preserve_repeated_lines_without_content_deduplication() {
    let page = LogPage {
        lines: (1..=10_001)
            .map(|sequence| LogLine {
                sequence,
                timestamp: Utc::now(),
                stream: LogStream::Stdout,
                line: "repeat".into(),
            })
            .collect(),
        truncated: false,
        dropped_count: 0,
        high_watermark: 10_001,
        next_sequence: 10_002,
        earliest_retained_sequence: Some(1),
        cursor_expired: false,
    };

    let dto = log_page_to_dto(page);
    let recovered: Vec<_> = dto
        .lines
        .iter()
        .filter(|line| line.sequence > 10_000)
        .collect();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].sequence, 10_001);
    assert_eq!(recovered[0].line, "repeat");
}
