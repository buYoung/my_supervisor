//! Historical log exploration shared by native, HTTP and command-line clients.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use my_supervisor_core::domain::{JobRunId, LogLine, LogStream, ManagementMode};

use crate::{AppDeps, AppError, AppResult};

const SCAN_PAGE_SIZE: usize = 2_000;
const MAX_SCANNED_LINES: usize = 200_000;
const MAX_RUNS_PER_JOB: usize = 2_000;

#[derive(Debug, Clone)]
pub struct LogQuery {
    pub time_zone: Option<String>,
    pub kind: String,
    pub source: Option<String>,
    pub run_id: Option<JobRunId>,
    pub level: String,
    pub stream: Option<String>,
    pub search: Option<String>,
    pub message: Option<String>,
    pub instance: Option<String>,
    pub step: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub grouping: String,
    pub ascending: bool,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub run_id: Option<JobRunId>,
    pub instance: Option<String>,
    pub step: Option<String>,
    pub level: String,
    pub message: String,
    pub raw: LogLine,
}

#[derive(Debug, Clone)]
pub struct LogGroup {
    pub entry: LogEntry,
    pub count: usize,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub preview: Vec<LogEntry>,
}

#[derive(Debug, Clone)]
pub struct LogQueryResult {
    pub records: Vec<LogGroup>,
    pub total_lines: usize,
    pub total_groups: usize,
    pub scanned_lines: usize,
    pub next_offset: Option<usize>,
    pub snapshot_at: DateTime<Utc>,
    pub level_counts: BTreeMap<String, usize>,
    pub daily_counts: BTreeMap<String, usize>,
    /// Every incomplete source is reported; counts never imply complete history.
    pub partial_sources: Vec<String>,
}

struct Source {
    name: String,
    run_id: Option<JobRunId>,
    system_unit: Option<String>,
}

pub async fn query(deps: &AppDeps, request: LogQuery) -> AppResult<LogQueryResult> {
    validate(&request)?;
    let time_zone = request
        .time_zone
        .as_deref()
        .unwrap_or("UTC")
        .parse::<chrono_tz::Tz>()
        .map_err(|_| AppError::InvalidRequest("invalid time_zone".into()))?;
    let snapshot_at = request.until.unwrap_or_else(|| deps.clock.now());
    let mut result = LogQueryResult {
        records: Vec::new(),
        total_lines: 0,
        total_groups: 0,
        scanned_lines: 0,
        next_offset: None,
        snapshot_at,
        level_counts: BTreeMap::new(),
        daily_counts: BTreeMap::new(),
        partial_sources: Vec::new(),
    };
    let sources = sources(deps, &request, &mut result.partial_sources).await?;
    let mut groups = HashMap::<String, LogGroup>::new();
    let search = request.search.as_deref().unwrap_or("").to_lowercase();
    for source in sources {
        let source_label = source.run_id.map_or_else(
            || source.name.clone(),
            |run_id| format!("{}/{}", source.name, run_id.0),
        );
        let mut cursor = 0;
        loop {
            if result.scanned_lines >= MAX_SCANNED_LINES {
                result.partial_sources.push(source_label.clone());
                break;
            }
            let limit = SCAN_PAGE_SIZE.min(MAX_SCANNED_LINES - result.scanned_lines);
            let page = if let Some(run_id) = source.run_id {
                deps.log_sink
                    .scan_run(run_id, limit, request.since, snapshot_at, cursor)
                    .await
            } else if let Some(unit) = &source.system_unit {
                // The OS service manager only provides a bounded tail. Expose
                // that limitation rather than merging it into complete counts.
                result.partial_sources.push(source_label.clone());
                let lines = deps.registrar.tail_logs(unit, limit).await?;
                Ok(my_supervisor_core::ports::LogTail {
                    lines,
                    ..Default::default()
                })
            } else {
                deps.log_sink
                    .scan(&source.name, limit, request.since, snapshot_at, cursor)
                    .await
            };
            let page = match page {
                Ok(page) => page,
                Err(error) => {
                    tracing::warn!(source = %source_label, %error, "log search source unavailable");
                    result.partial_sources.push(source_label.clone());
                    break;
                }
            };
            let next_cursor = page.next_sequence;
            for raw in page.lines {
                result.scanned_lines += 1;
                if raw.timestamp > snapshot_at
                    || request.since.is_some_and(|since| raw.timestamp < since)
                {
                    continue;
                }
                let entry = classify(&request.kind, &source.name, source.run_id, raw);
                if request
                    .stream
                    .as_deref()
                    .is_some_and(|value| value != stream_name(entry.raw.stream))
                    || (!search.is_empty() && !entry.raw.line.to_lowercase().contains(&search))
                    || request
                        .message
                        .as_ref()
                        .is_some_and(|value| value != &entry.message)
                    || request
                        .instance
                        .as_ref()
                        .is_some_and(|value| entry.instance.as_ref() != Some(value))
                    || request
                        .step
                        .as_ref()
                        .is_some_and(|value| entry.step.as_ref() != Some(value))
                {
                    continue;
                }
                *result.level_counts.entry(entry.level.clone()).or_default() += 1;
                *result
                    .daily_counts
                    .entry(
                        entry
                            .raw
                            .timestamp
                            .with_timezone(&time_zone)
                            .format("%Y-%m-%d")
                            .to_string(),
                    )
                    .or_default() += 1;
                if !matches_level(&request.level, &entry.level) {
                    continue;
                }
                result.total_lines += 1;
                let key = if request.grouping == "repeat" {
                    serde_json::to_string(&(
                        &entry.kind,
                        &entry.source,
                        &entry.run_id.map(|id| id.0.to_string()),
                        &entry.instance,
                        &entry.step,
                        &entry.level,
                        stream_name(entry.raw.stream),
                        &entry.message,
                    ))
                    .expect("serializable log key")
                } else {
                    entry.id.clone()
                };
                let timestamp = entry.raw.timestamp;
                let group = groups.entry(key).or_insert_with(|| LogGroup {
                    entry: entry.clone(),
                    count: 0,
                    first_seen: timestamp,
                    last_seen: timestamp,
                    preview: Vec::new(),
                });
                group.count += 1;
                group.first_seen = group.first_seen.min(timestamp);
                if timestamp >= group.last_seen {
                    group.last_seen = timestamp;
                    group.entry = entry.clone();
                }
                if group.preview.len() < 3 {
                    group.preview.push(entry);
                }
            }
            if !page.truncated || next_cursor <= cursor || source.system_unit.is_some() {
                break;
            }
            cursor = next_cursor;
        }
    }
    let mut groups: Vec<_> = groups.into_values().collect();
    groups.sort_by(|left, right| {
        let order = if request.ascending {
            left.first_seen.cmp(&right.first_seen)
        } else {
            right.last_seen.cmp(&left.last_seen)
        };
        order.then_with(|| left.entry.id.cmp(&right.entry.id))
    });
    result.total_groups = groups.len();
    result.next_offset = (request.offset.saturating_add(request.limit) < groups.len())
        .then_some(request.offset.saturating_add(request.limit));
    result.records = groups
        .into_iter()
        .skip(request.offset)
        .take(request.limit)
        .collect();
    result.partial_sources.sort();
    result.partial_sources.dedup();
    Ok(result)
}

async fn sources(
    deps: &AppDeps,
    query: &LogQuery,
    partial: &mut Vec<String>,
) -> AppResult<Vec<Source>> {
    let mut sources = Vec::new();
    if query.kind == "process" {
        let processes = deps.state_repo.list_specs().await?;
        if let Some(name) = &query.source {
            if !processes.iter().any(|process| &process.name == name) {
                return Err(AppError::not_found(crate::ResourceKind::Process, name));
            }
        }
        for process in processes.into_iter().filter(|process| {
            query
                .source
                .as_ref()
                .is_none_or(|name| name == &process.name)
        }) {
            sources.push(Source {
                name: process.name,
                run_id: None,
                system_unit: match process.management_mode {
                    ManagementMode::SystemRegistered { unit_name } => Some(unit_name),
                    ManagementMode::Direct => None,
                },
            });
        }
    } else {
        let jobs = deps.job_repo.list_jobs().await?;
        if let Some(name) = &query.source {
            if !jobs.iter().any(|job| &job.name == name) {
                return Err(AppError::not_found(crate::ResourceKind::Job, name));
            }
        }
        for job in jobs
            .into_iter()
            .filter(|job| query.source.as_ref().is_none_or(|name| name == &job.name))
        {
            if let Some(run_id) = query.run_id {
                let run = deps
                    .job_repo
                    .get_run(&job.name, &run_id)
                    .await?
                    .filter(|run| run.job_name == job.name)
                    .ok_or_else(|| {
                        AppError::InvalidRequest("run does not belong to this job".into())
                    })?;
                sources.push(Source {
                    name: run.job_name,
                    run_id: Some(run_id),
                    system_unit: None,
                });
            } else {
                let mut runs = deps
                    .job_repo
                    .list_runs(&job.name, MAX_RUNS_PER_JOB + 1)
                    .await?;
                if runs.len() > MAX_RUNS_PER_JOB {
                    partial.push(job.name.clone());
                    runs.truncate(MAX_RUNS_PER_JOB);
                }
                for run in runs {
                    if query
                        .since
                        .is_some_and(|since| run.ended_at.is_some_and(|end| end < since))
                    {
                        continue;
                    }
                    sources.push(Source {
                        name: job.name.clone(),
                        run_id: Some(run.run_id),
                        system_unit: None,
                    });
                }
            }
        }
    }
    sources.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.run_id.map(|id| id.0).cmp(&right.run_id.map(|id| id.0)))
    });
    Ok(sources)
}

fn validate(query: &LogQuery) -> AppResult<()> {
    let error = |message: &str| AppError::InvalidRequest(message.into());
    if !matches!(query.kind.as_str(), "process" | "job") {
        return Err(error("kind must be process or job"));
    }
    if !matches!(query.grouping.as_str(), "repeat" | "none") {
        return Err(error("grouping must be repeat or none"));
    }
    if query.limit == 0 || query.limit > 5_000 || query.offset > MAX_SCANNED_LINES {
        return Err(error("invalid log page bounds"));
    }
    if query
        .since
        .zip(query.until)
        .is_some_and(|(since, until)| since > until)
    {
        return Err(error("since must not be later than until"));
    }
    if query.run_id.is_some() && (query.kind != "job" || query.source.is_none()) {
        return Err(error("run_id requires a job source"));
    }
    if query
        .stream
        .as_deref()
        .is_some_and(|value| !matches!(value, "stdout" | "stderr" | "system"))
    {
        return Err(error("unknown log stream"));
    }
    if !matches!(query.level.as_str(), "attention" | "all" | "without_info")
        && !query.level.split(',').all(|value| {
            matches!(
                value,
                "trace" | "debug" | "info" | "warn" | "error" | "fatal" | "unknown"
            )
        })
    {
        return Err(error("unknown log level"));
    }
    Ok(())
}

fn matches_level(filter: &str, level: &str) -> bool {
    match filter {
        "all" => true,
        "attention" => matches!(level, "warn" | "error" | "fatal" | "unknown"),
        "without_info" => level != "info",
        _ => filter.split(',').any(|value| value == level),
    }
}

pub fn stream_name(stream: LogStream) -> &'static str {
    match stream {
        LogStream::Stdout => "stdout",
        LogStream::Stderr => "stderr",
        LogStream::System => "system",
    }
}

fn normalize_level(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "trace" | "10" => Some("trace"),
        "debug" | "20" => Some("debug"),
        "info" | "information" | "30" => Some("info"),
        "warn" | "warning" | "40" => Some("warn"),
        "error" | "err" | "50" => Some("error"),
        "fatal" | "critical" | "panic" | "60" => Some("fatal"),
        _ => None,
    }
}

fn classify(kind: &str, source: &str, run_id: Option<JobRunId>, raw: LogLine) -> LogEntry {
    let json = serde_json::from_str::<serde_json::Value>(&raw.line).ok();
    let level = json
        .as_ref()
        .and_then(|value| {
            ["level", "severity", "levelname", "log.level"]
                .iter()
                .filter_map(|key| value.get(key))
                .find_map(|value| {
                    normalize_level(
                        value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string())
                            .as_str(),
                    )
                })
                .or_else(|| {
                    value
                        .pointer("/log/level")
                        .and_then(|value| value.as_str())
                        .and_then(normalize_level)
                })
        })
        .or_else(|| {
            if json.is_some() {
                return None;
            }
            // Only the prefix is considered; a sentence containing "error" later
            // is not automatically an error-level record.
            raw.line
                .split_whitespace()
                .take(4)
                .enumerate()
                .find_map(|(index, value)| {
                    let token = value.trim_matches(|ch: char| !ch.is_alphanumeric());
                    if !token.chars().all(|ch| ch.is_ascii_alphabetic()) {
                        return None;
                    }
                    if index != 0 && !value.starts_with('[') && token != token.to_ascii_uppercase()
                    {
                        return None;
                    }
                    normalize_level(token)
                })
        })
        .unwrap_or("unknown")
        .to_string();
    let message = json
        .as_ref()
        .and_then(|value| value.get("message").or_else(|| value.get("msg")))
        .and_then(|value| value.as_str())
        .unwrap_or(&raw.line)
        .to_string();
    let instance = json
        .as_ref()
        .and_then(|value| value.get("instance_id").or_else(|| value.get("pid")))
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string())
        });
    let id = format!(
        "{kind}:{source}:{}:{}:{}",
        run_id.map(|id| id.0.to_string()).unwrap_or_default(),
        raw.sequence,
        raw.timestamp.timestamp_nanos_opt().unwrap_or(0)
    );
    LogEntry {
        id,
        kind: kind.into(),
        source: source.into(),
        run_id,
        instance,
        step: json
            .as_ref()
            .and_then(|value| value.get("step_id"))
            .and_then(|value| value.as_str())
            .map(str::to_owned),
        level,
        message,
        raw,
    }
}
