use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::LogLineDto;

fn process_kind() -> String {
    "process".into()
}
fn attention_level() -> String {
    "attention".into()
}
fn repeat_grouping() -> String {
    "repeat".into()
}
fn page_limit() -> usize {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogQueryDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    #[serde(default = "process_kind")]
    pub kind: String,
    pub source: Option<String>,
    pub run_id: Option<Uuid>,
    #[serde(default = "attention_level")]
    pub level: String,
    pub stream: Option<String>,
    pub search: Option<String>,
    pub message: Option<String>,
    pub instance: Option<String>,
    pub step: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    #[serde(default = "repeat_grouping")]
    pub grouping: String,
    #[serde(default)]
    pub ascending: bool,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "page_limit")]
    pub limit: usize,
}

impl Default for LogQueryDto {
    fn default() -> Self {
        Self {
            time_zone: None,
            kind: process_kind(),
            source: None,
            run_id: None,
            level: attention_level(),
            stream: None,
            search: None,
            message: None,
            instance: None,
            step: None,
            since: None,
            until: None,
            grouping: repeat_grouping(),
            ascending: false,
            offset: 0,
            limit: page_limit(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntryDto {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub run_id: Option<Uuid>,
    pub instance: Option<String>,
    pub step: Option<String>,
    pub level: String,
    pub message: String,
    pub raw: LogLineDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogGroupDto {
    pub entry: LogEntryDto,
    pub count: usize,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub preview: Vec<LogEntryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogQueryResultDto {
    pub records: Vec<LogGroupDto>,
    pub total_lines: usize,
    pub total_groups: usize,
    pub scanned_lines: usize,
    pub next_offset: Option<usize>,
    pub snapshot_at: DateTime<Utc>,
    pub level_counts: BTreeMap<String, usize>,
    pub daily_counts: BTreeMap<String, usize>,
    pub partial_sources: Vec<String>,
}
