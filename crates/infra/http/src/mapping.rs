//! Domain ↔ wire DTO mapping. The single home for the contract translation so
//! HTTP routes (and the Tauri invoke adapter in child 03) stay thin. The
//! DTO→domain direction mirrors `config::convert` by design (no shared module
//! without a dependency cycle); both compile against `shared`'s one definition.

use std::path::PathBuf;
use std::time::Duration;

use my_supervisor_application::views::{DaemonInfo, JobView, LogPage, RecoveryDiagnostics};
use my_supervisor_core::domain::{
    ApplyMode, ConfigApplyResult, DependencyFailurePolicy, Job, JobId, JobRun, JobRunState,
    JobTrigger, LifecycleMode, LoadedConfig, LogLine,
    LogRetention, LogStream, ManagementMode, OverlapPolicy, ProcessSpec, ProcessState,
    ProcessStatus, RestartPolicy, ShutdownPolicy, ShutdownSignal, TriggeredBy,
};
use my_supervisor_shared::api::{
    ConfigApplyModeDto, ConfigApplyResultDto, ConfigDiffDto, DaemonStatusDto, JobConfigDto,
    JobDependenciesDto, JobRunDto, JobRunStateDto, JobRunSummaryDto, JobStatusDto, JobTriggerDto,
    LifecycleModeDto, LogLineDto, LogStreamDto, LogsResponseDto,
    ManagementModeDto, OnDependencyFailureDto, OnOverlapDto, ProcessConfigDto, ProcessStateDto,
    ProcessStatusDto, RecoveryDiagnosticDto, RecoveryDiagnosticsDto, RestartPolicyDto,
    ShutdownPolicyDto, ShutdownSignalDto, TriggeredByDto,
};
use my_supervisor_shared::config::FileConfig;

// --- domain -> DTO ----------------------------------------------------------

pub fn process_state_to_dto(state: ProcessState) -> ProcessStateDto {
    match state {
        ProcessState::Starting => ProcessStateDto::Starting,
        ProcessState::Running => ProcessStateDto::Running,
        ProcessState::Stopping => ProcessStateDto::Stopping,
        ProcessState::Crashed => ProcessStateDto::Crashed,
        ProcessState::Stopped => ProcessStateDto::Stopped,
    }
}

pub fn management_mode_to_dto(mode: &ManagementMode) -> ManagementModeDto {
    match mode {
        ManagementMode::Direct => ManagementModeDto::Direct,
        ManagementMode::SystemRegistered { unit_name } => ManagementModeDto::SystemRegistered {
            unit_name: unit_name.clone(),
        },
    }
}

pub fn process_status_to_dto(status: ProcessStatus) -> ProcessStatusDto {
    ProcessStatusDto {
        name: status.name,
        state: process_state_to_dto(status.state),
        management_mode: management_mode_to_dto(&status.management_mode),
        pid: status.pid,
        unit_name: status.unit_name,
        restart_count: status.restart_count,
        started_at: status.started_at,
        cpu_percent: status.cpu_percent,
        memory_bytes: status.memory_bytes,
    }
}

pub fn job_trigger_to_dto(trigger: &JobTrigger) -> JobTriggerDto {
    match trigger {
        JobTrigger::Cron(expr) => JobTriggerDto::Cron { expr: expr.clone() },
        JobTrigger::Interval(d) => JobTriggerDto::Interval {
            every_sec: d.as_secs(),
        },
        JobTrigger::OneShot(at) => JobTriggerDto::OneShot { at: *at },
        JobTrigger::DependsOn(jobs) => JobTriggerDto::DependsOn { jobs: jobs.clone() },
    }
}

pub fn overlap_to_dto(p: OverlapPolicy) -> OnOverlapDto {
    match p {
        OverlapPolicy::Skip => OnOverlapDto::Skip,
        OverlapPolicy::Queue => OnOverlapDto::Queue,
        OverlapPolicy::Parallel => OnOverlapDto::Parallel,
    }
}

pub fn run_state_to_dto(s: JobRunState) -> JobRunStateDto {
    match s {
        JobRunState::Pending => JobRunStateDto::Pending,
        JobRunState::Running => JobRunStateDto::Running,
        JobRunState::Succeeded => JobRunStateDto::Succeeded,
        JobRunState::Failed => JobRunStateDto::Failed,
        JobRunState::TimedOut => JobRunStateDto::TimedOut,
        JobRunState::Cancelled => JobRunStateDto::Cancelled,
        JobRunState::Skipped => JobRunStateDto::Skipped,
    }
}

pub fn triggered_by_to_dto(t: &TriggeredBy) -> TriggeredByDto {
    match t {
        TriggeredBy::Schedule => TriggeredByDto::Schedule,
        TriggeredBy::Manual => TriggeredByDto::Manual,
        TriggeredBy::Dependency { upstream_run_id } => TriggeredByDto::Dependency {
            upstream_run_id: upstream_run_id.0.to_string(),
        },
    }
}

pub fn job_run_to_dto(run: &JobRun) -> JobRunDto {
    JobRunDto {
        run_id: run.run_id.0.to_string(),
        job_name: run.job_name.clone(),
        triggered_by: triggered_by_to_dto(&run.triggered_by),
        scheduled_at: run.scheduled_at,
        started_at: run.started_at,
        ended_at: run.ended_at,
        exit_code: run.exit_code,
        state: run_state_to_dto(run.state),
    }
}

fn run_summary_to_dto(run: &JobRun) -> JobRunSummaryDto {
    let duration_sec = match (run.started_at, run.ended_at) {
        (Some(s), Some(e)) => Some((e - s).num_seconds()),
        _ => None,
    };
    JobRunSummaryDto {
        run_id: run.run_id.0.to_string(),
        state: run_state_to_dto(run.state),
        ended_at: run.ended_at,
        duration_sec,
    }
}

pub fn job_view_to_dto(view: JobView) -> JobStatusDto {
    JobStatusDto {
        name: view.job.name.clone(),
        trigger: job_trigger_to_dto(&view.job.trigger),
        on_overlap: overlap_to_dto(view.job.on_overlap),
        last_run: view.last_run.as_ref().map(run_summary_to_dto),
        next_run_at: view.next_run_at,
        success_rate_recent: view.success_rate_recent,
        dependencies: JobDependenciesDto {
            upstream: view.upstream,
            downstream: view.downstream,
        },
    }
}

pub fn log_stream_to_dto(s: LogStream) -> LogStreamDto {
    match s {
        LogStream::Stdout => LogStreamDto::Stdout,
        LogStream::Stderr => LogStreamDto::Stderr,
        LogStream::System => LogStreamDto::System,
    }
}

pub fn log_line_to_dto(line: &LogLine) -> LogLineDto {
    LogLineDto {
        sequence: line.sequence,
        timestamp: line.timestamp,
        stream: log_stream_to_dto(line.stream),
        line: line.line.clone(),
    }
}

pub fn log_page_to_dto(page: LogPage) -> LogsResponseDto {
    LogsResponseDto {
        lines: page.lines.iter().map(log_line_to_dto).collect(),
        truncated: page.truncated,
        dropped_count: page.dropped_count,
        high_watermark: page.high_watermark,
        next_sequence: page.next_sequence,
    }
}

pub fn config_apply_mode_to_domain(mode: ConfigApplyModeDto) -> ApplyMode {
    match mode {
        ConfigApplyModeDto::Merge => ApplyMode::Merge,
        ConfigApplyModeDto::Replace => ApplyMode::Replace,
    }
}

pub fn config_apply_result_to_dto(result: ConfigApplyResult) -> ConfigApplyResultDto {
    let diff = result.diff;
    ConfigApplyResultDto {
        apply_id: result.apply_id.map(|value| value.to_string()),
        mode: match result.mode {
            ApplyMode::Merge => ConfigApplyModeDto::Merge,
            ApplyMode::Replace => ConfigApplyModeDto::Replace,
        },
        diff: ConfigDiffDto {
            added_processes: diff.added_processes,
            updated_processes: diff.updated_processes,
            removed_processes: diff.removed_processes,
            added_jobs: diff.added_jobs,
            updated_jobs: diff.updated_jobs,
            removed_jobs: diff.removed_jobs,
        },
        dry_run: result.dry_run,
    }
}

pub fn file_config_to_loaded(config: FileConfig) -> LoadedConfig {
    LoadedConfig {
        processes: config.processes.into_iter().map(process_config_to_spec).collect(),
        jobs: config.jobs.into_iter().map(job_config_to_job).collect(),
    }
}

pub fn daemon_info_to_dto(info: DaemonInfo) -> DaemonStatusDto {
    DaemonStatusDto {
        version: info.version,
        started_at: info.started_at,
        pid: info.pid,
        process_count: info.process_count,
        config_path: info.config_path,
        log_dir: info.log_dir,
    }
}

pub fn recovery_diagnostics_to_dto(diagnostics: RecoveryDiagnostics) -> RecoveryDiagnosticsDto {
    RecoveryDiagnosticsDto {
        records: diagnostics
            .records
            .into_iter()
            .map(|record| RecoveryDiagnosticDto {
                kind: record.kind,
                id: record.id,
                resource: record.resource,
                stage: record.stage,
                attempts: record.attempts,
                last_error: record.last_error,
            })
            .collect(),
    }
}

// --- DTO -> domain (request bodies) ----------------------------------------

fn restart_policy(dto: Option<RestartPolicyDto>) -> RestartPolicy {
    let defaults = RestartPolicy::default();
    let Some(dto) = dto else {
        return defaults;
    };
    RestartPolicy {
        enabled: dto.enabled.unwrap_or(defaults.enabled),
        max_retries: dto.max_retries,
        backoff_initial: Duration::from_millis(
            dto.backoff_initial_ms
                .unwrap_or(defaults.backoff_initial.as_millis() as u64),
        ),
        backoff_max: Duration::from_millis(
            dto.backoff_max_ms
                .unwrap_or(defaults.backoff_max.as_millis() as u64),
        ),
        backoff_multiplier: dto
            .backoff_multiplier
            .unwrap_or(defaults.backoff_multiplier),
        jitter: dto.jitter.unwrap_or(defaults.jitter),
        reset_after: Duration::from_millis(
            dto.reset_after_ms
                .unwrap_or(defaults.reset_after.as_millis() as u64),
        ),
    }
}

fn shutdown_policy(dto: Option<ShutdownPolicyDto>) -> ShutdownPolicy {
    let defaults = ShutdownPolicy::default();
    let Some(dto) = dto else {
        return defaults;
    };
    ShutdownPolicy {
        signal: match dto.signal {
            Some(ShutdownSignalDto::Int) => ShutdownSignal::Int,
            Some(ShutdownSignalDto::Kill) => ShutdownSignal::Kill,
            _ => ShutdownSignal::Term,
        },
        grace_period: Duration::from_millis(
            dto.grace_period_ms
                .unwrap_or(defaults.grace_period.as_millis() as u64),
        ),
    }
}

pub fn process_config_to_spec(dto: ProcessConfigDto) -> ProcessSpec {
    let management_mode = match dto.management_mode {
        Some(ManagementModeDto::SystemRegistered { unit_name }) => {
            ManagementMode::SystemRegistered { unit_name }
        }
        _ => ManagementMode::Direct,
    };
    let lifecycle = match dto.lifecycle {
        Some(LifecycleModeDto::Detached) => LifecycleMode::Detached,
        _ => LifecycleMode::Tied,
    };
    ProcessSpec {
        name: dto.name,
        command: dto.command,
        args: dto.args,
        cwd: dto.cwd.map(PathBuf::from),
        env: dto.env,
        management_mode,
        lifecycle,
        autostart: dto.autostart.unwrap_or(false),
        restart: restart_policy(dto.restart),
        shutdown: shutdown_policy(dto.shutdown),
    }
}

pub fn job_trigger_to_domain(dto: JobTriggerDto) -> JobTrigger {
    match dto {
        JobTriggerDto::Cron { expr } => JobTrigger::Cron(expr),
        JobTriggerDto::Interval { every_sec } => {
            JobTrigger::Interval(Duration::from_secs(every_sec))
        }
        JobTriggerDto::OneShot { at } => JobTrigger::OneShot(at),
        JobTriggerDto::DependsOn { jobs } => JobTrigger::DependsOn(jobs),
    }
}

pub fn job_config_to_job(dto: JobConfigDto) -> Job {
    Job {
        id: JobId::new(),
        name: dto.name,
        command: dto.command,
        args: dto.args,
        cwd: dto.cwd.map(PathBuf::from),
        env: dto.env,
        trigger: job_trigger_to_domain(dto.trigger),
        on_overlap: match dto.on_overlap {
            Some(OnOverlapDto::Queue) => OverlapPolicy::Queue,
            Some(OnOverlapDto::Parallel) => OverlapPolicy::Parallel,
            _ => OverlapPolicy::Skip,
        },
        on_dependency_failure: match dto.on_dependency_failure {
            Some(OnDependencyFailureDto::RunAnyway) => DependencyFailurePolicy::RunAnyway,
            _ => DependencyFailurePolicy::Skip,
        },
        timeout: dto.timeout_sec.map(Duration::from_secs),
        log_retention: dto
            .log_retention
            .map(|retention| LogRetention {
                max_runs: retention.max_runs,
                max_age_days: retention.max_age_days,
            })
            .unwrap_or_default(),
    }
}
