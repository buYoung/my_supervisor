use chrono::Utc;

use my_supervisor_application::views::LogPage;
use my_supervisor_core::domain::{
    ApplyMode, ConfigApplyResult, ConfigDiff, LogLine, LogStream, MisfirePolicy,
};
use my_supervisor_infra_http::mapping::{
    config_apply_result_to_dto, file_config_to_loaded, log_page_to_dto,
};
use my_supervisor_shared::api::{JobConfigDto, JobTriggerDto, ProcessConfigDto};
use my_supervisor_shared::config::FileConfig;

#[test]
fn log_response_exposes_durable_cursor_fields() {
    let response = log_page_to_dto(LogPage {
        lines: vec![LogLine {
            sequence: 7,
            timestamp: Utc::now(),
            stream: LogStream::Stdout,
            line: "same line".into(),
        }],
        truncated: true,
        dropped_count: 3,
        high_watermark: 7,
        next_sequence: 8,
        earliest_retained_sequence: Some(1),
        cursor_expired: false,
    });

    assert_eq!(response.lines[0].sequence, 7);
    assert!(response.truncated);
    assert_eq!(response.high_watermark, 7);
    assert_eq!(response.next_sequence, 8);
}

#[test]
fn config_request_maps_the_shared_file_schema_without_transport_state() {
    let loaded = file_config_to_loaded(FileConfig {
        processes: vec![ProcessConfigDto {
            definition_id: None,
            name: "worker".into(),
            command: "/bin/true".into(),
            args: Vec::new(),
            cwd: None,
            env: Default::default(),
            management_mode: None,
            lifecycle: None,
            autostart: None,
            restart: None,
            shutdown: None,
            instances: None,
            watch: None,
            memory: None,
            liveness: None,
            readiness: None,
            rolling: None,
        }],
        jobs: vec![JobConfigDto {
            name: "job".into(),
            command: "/bin/true".into(),
            args: Vec::new(),
            cwd: None,
            env: Default::default(),
            trigger: JobTriggerDto::Interval { every_sec: 60 },
            on_overlap: None,
            on_dependency_failure: None,
            timeout_sec: None,
            log_retention: None,
            timezone: None,
            schedule_revision: None,
            trigger_id: None,
            misfire_policy: None,
            retry_policy: None,
            admission: None,
        }],
    });

    assert_eq!(loaded.processes[0].name, "worker");
    assert_eq!(loaded.jobs[0].name, "job");
}

#[test]
fn omitted_job_fields_use_current_macos_defaults() {
    let loaded = file_config_to_loaded(FileConfig {
        processes: Vec::new(),
        jobs: vec![JobConfigDto {
            name: "local-defaults".into(),
            command: "/bin/true".into(),
            args: Vec::new(),
            cwd: None,
            env: Default::default(),
            trigger: JobTriggerDto::Cron {
                expr: "0 * * * *".into(),
            },
            on_overlap: None,
            on_dependency_failure: None,
            timeout_sec: None,
            log_retention: None,
            timezone: None,
            schedule_revision: None,
            trigger_id: None,
            misfire_policy: None,
            retry_policy: None,
            admission: None,
        }],
    });
    let job = &loaded.jobs[0];

    assert_eq!(
        job.timezone,
        iana_time_zone::get_timezone().expect("test host resolves an IANA timezone")
    );
    assert_eq!(job.misfire_policy, MisfirePolicy::RunOnce);
    assert!(!job.trigger_id.is_nil());
}

#[test]
fn config_result_preserves_dry_run_and_diff() {
    let response = config_apply_result_to_dto(ConfigApplyResult {
        apply_id: None,
        mode: ApplyMode::Replace,
        diff: ConfigDiff {
            added_jobs: vec!["job".into()],
            ..ConfigDiff::default()
        },
        dry_run: true,
    });

    assert!(response.dry_run);
    assert_eq!(response.diff.added_jobs, ["job"]);
}
