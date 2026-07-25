//! `msv` — the operations CLI. A thin HTTP/WS client over the daemon's API
//! (`docs/ARCHITECTURE.md` §4.1.2); it embeds no core and forks no wire type.

mod client;

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use chrono::{DateTime, Utc};
use comfy_table::Table;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio_tungstenite::tungstenite::Message;

use client::{CliError, Client};
use my_supervisor_app_daemon::DEFAULT_BASE_URL;
use my_supervisor_shared::api::{
    ConfigApplyModeDto, ConvertRequestDto, ConvertTargetDto, JobRunStateDto, LogLineDto,
    ProcessStateDto,
};
use my_supervisor_shared::config::{ConfigApplyRequestDto, FileConfig};
use my_supervisor_shared::events::EventEnvelope;

const EVENT_DEDUP_CACHE_CAPACITY: usize = 1_024;

#[derive(Parser)]
#[command(name = "msv", version, about = "my-supervisor operations CLI")]
struct Cli {
    /// Operations host base URL (defaults to the local daemon).
    #[arg(long, global = true)]
    url: Option<String>,
    /// Output format.
    #[arg(short = 'o', long, global = true, default_value = "table")]
    output: OutputFormat,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Table,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum ConvertMode {
    Direct,
    SystemRegistered,
}

#[derive(Clone, Copy, ValueEnum)]
enum ConfigMode {
    Merge,
    Replace,
}

#[derive(Subcommand)]
enum Command {
    /// List managed processes.
    Ps,
    /// Show one managed process.
    Show { name: String },
    /// Start a process.
    Start { name: String },
    /// Stop a process.
    Stop {
        name: String,
        #[arg(long)]
        force: bool,
    },
    /// Restart a process.
    Restart { name: String },
    /// Change how a process is managed.
    Convert {
        name: String,
        #[arg(long, value_enum)]
        to: ConvertMode,
        #[arg(long)]
        unit_name: Option<String>,
        #[arg(long)]
        auto_start: bool,
    },
    /// Show recent logs, or follow with -f.
    Logs {
        name: String,
        #[arg(short = 'f', long)]
        follow: bool,
        #[arg(long, default_value_t = 100)]
        tail: usize,
        #[arg(long)]
        since: Option<DateTime<Utc>>,
    },
    /// Register processes/jobs from a TOML config file.
    Add {
        #[arg(short = 'c', long)]
        config: PathBuf,
    },
    /// Unregister a process.
    Remove {
        name: String,
        #[arg(long)]
        force: bool,
    },
    /// Reload the daemon's config file.
    Reload,
    /// Validate or atomically apply a TOML configuration file.
    Config {
        #[command(subcommand)]
        sub: ConfigCmd,
    },
    /// Daemon controls.
    Daemon {
        #[command(subcommand)]
        sub: DaemonCmd,
    },
    /// Job controls.
    Job {
        #[command(subcommand)]
        sub: JobCmd,
    },
}

#[derive(Subcommand)]
enum DaemonCmd {
    /// Show daemon status.
    Status {
        /// Include bounded pending durable recovery diagnostics.
        #[arg(long)]
        recovery: bool,
    },
    /// Follow global events. Terminal events are de-duplicated by stable event_id.
    Events,
    /// Stop the daemon gracefully.
    Shutdown,
}

/// Session-scoped terminal event de-duplication. Durability remains server-side
/// in the SQLite outbox; this bounded cache deliberately does not survive a
/// CLI process restart, so it never claims cross-session exactly-once delivery.
#[derive(Default)]
struct EventDeduper {
    event_ids: HashSet<String>,
    insertion_order: VecDeque<String>,
}

impl EventDeduper {
    fn should_emit(&mut self, event_id: Option<&str>) -> bool {
        let Some(event_id) = event_id else {
            // Older daemons did not send an ID. Preserve compatibility without
            // incorrectly treating a payload match as an exactly-once key.
            return true;
        };
        if !self.event_ids.insert(event_id.to_owned()) {
            return false;
        }
        self.insertion_order.push_back(event_id.to_owned());
        if self.insertion_order.len() > EVENT_DEDUP_CACHE_CAPACITY {
            if let Some(expired_event_id) = self.insertion_order.pop_front() {
                self.event_ids.remove(&expired_event_id);
            }
        }
        true
    }
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Validate a configuration file without changing daemon state.
    Validate {
        #[arg(short = 'f', long)]
        file: PathBuf,
        #[arg(long, value_enum, default_value_t = ConfigMode::Merge)]
        mode: ConfigMode,
    },
    /// Atomically apply a configuration file.
    Apply {
        #[arg(short = 'f', long)]
        file: PathBuf,
        #[arg(long, value_enum, default_value_t = ConfigMode::Merge)]
        mode: ConfigMode,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum JobCmd {
    /// List jobs.
    Ls,
    /// Show one job.
    Show { name: String },
    /// Remove a job.
    Remove {
        name: String,
        #[arg(long)]
        force: bool,
    },
    /// Trigger a job immediately.
    Trigger { name: String },
    /// Request cancellation of a pending or running job run.
    Cancel { name: String, run_id: String },
    /// Show a job's run history.
    Runs {
        name: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show a job run's captured logs.
    Logs {
        name: String,
        run_id: String,
        #[arg(short = 'f', long)]
        follow: bool,
        #[arg(long, default_value_t = 100)]
        tail: usize,
        #[arg(long)]
        since: Option<DateTime<Utc>>,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let client = match Client::new(
        cli.url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
    ) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("error: {}", error.message());
            return ExitCode::from(error.exit_code() as u8);
        }
    };
    match dispatch(&cli, &client).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {}", e.message());
            ExitCode::from(e.exit_code() as u8)
        }
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<(), CliError> {
    let output = serde_json::to_string_pretty(value)
        .map_err(|error| CliError::Failed(format!("serializing output: {error}")))?;
    println!("{output}");
    Ok(())
}

fn print_log_line(line: &LogLineDto, json: bool) -> Result<(), CliError> {
    if json {
        let output = serde_json::to_string(line)
            .map_err(|error| CliError::Failed(format!("serializing output: {error}")))?;
        println!("{output}");
    } else {
        println!(
            "{} [{:?}] {}",
            line.timestamp.to_rfc3339(),
            line.stream,
            line.line
        );
    }
    Ok(())
}

fn print_event(event: &EventEnvelope, json: bool) -> Result<(), CliError> {
    if json {
        // Event follow is JSON Lines: exactly one JSON object for every
        // accepted frame, never a pretty-printed multi-line document.
        let output = serde_json::to_string(event)
            .map_err(|error| CliError::Failed(format!("serializing event: {error}")))?;
        println!("{output}");
    } else {
        println!(
            "{} {} {}",
            event.timestamp.to_rfc3339(),
            event.event_type,
            event.payload
        );
    }
    Ok(())
}

fn process_state_label(state: ProcessStateDto) -> &'static str {
    match state {
        ProcessStateDto::Starting => "starting",
        ProcessStateDto::Running => "running",
        ProcessStateDto::Stopping => "stopping",
        ProcessStateDto::Crashed => "crashed",
        ProcessStateDto::Stopped => "stopped",
    }
}

fn run_state_label(state: JobRunStateDto) -> &'static str {
    match state {
        JobRunStateDto::Pending => "pending",
        JobRunStateDto::Running => "running",
        JobRunStateDto::Succeeded => "succeeded",
        JobRunStateDto::Failed => "failed",
        JobRunStateDto::TimedOut => "timed_out",
        JobRunStateDto::Cancelled => "cancelled",
        JobRunStateDto::Skipped => "skipped",
    }
}

fn format_memory(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

async fn dispatch(cli: &Cli, client: &Client) -> Result<(), CliError> {
    let json = matches!(cli.output, OutputFormat::Json);
    match &cli.command {
        Command::Ps => {
            let list = client.list_processes().await?;
            if json {
                print_json(&list)?;
            } else {
                let mut table = Table::new();
                table.set_header(vec![
                    "NAME", "STATE", "PID", "CPU", "MEMORY", "RESTARTS", "STARTED",
                ]);
                for p in &list.processes {
                    table.add_row(vec![
                        p.name.clone(),
                        process_state_label(p.state).to_string(),
                        p.pid.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                        format!("{:.1}%", p.cpu_percent),
                        format_memory(p.memory_bytes),
                        p.restart_count.to_string(),
                        p.started_at
                            .map(|t| t.to_rfc3339())
                            .unwrap_or_else(|| "-".into()),
                    ]);
                }
                println!("{table}");
            }
        }
        Command::Show { name } => {
            let process = client.get_process(name).await?;
            if json {
                print_json(&process)?;
            } else {
                println!("name:      {}", process.name);
                println!("state:     {}", process_state_label(process.state));
                println!("pid:       {}", process.pid.map_or_else(|| "-".into(), |pid| pid.to_string()));
                println!("restarts:  {}", process.restart_count);
                println!("mode:      {:?}", process.management_mode);
                println!("started:   {}", process.started_at.map_or_else(|| "-".into(), |time| time.to_rfc3339()));
                println!("cpu:       {:.1}%", process.cpu_percent);
                println!("memory:    {}", format_memory(process.memory_bytes));
            }
        }
        Command::Start { name } => {
            client.process_action(name, "start").await?;
            if json {
                print_json(&serde_json::json!({ "name": name, "action": "started" }))?;
            } else {
                println!("started {name}");
            }
        }
        Command::Stop { name, force } => {
            client.stop(name, *force).await?;
            if json {
                print_json(&serde_json::json!({ "name": name, "action": "stopped" }))?;
            } else {
                println!("stopped {name}");
            }
        }
        Command::Restart { name } => {
            let outcome = client.restart(name).await?;
            if json {
                print_json(&serde_json::json!({
                    "name": name,
                    "action": if outcome.is_some() { "noop" } else { "restarted" },
                    "reason": outcome.as_ref().map(|value| value.reason.as_str()),
                }))?;
            } else if let Some(outcome) = outcome {
                println!("restart skipped for {name}: {}", outcome.reason);
            } else {
                println!("restarted {name}");
            }
        }
        Command::Convert {
            name,
            to,
            unit_name,
            auto_start,
        } => {
            let request = ConvertRequestDto {
                to: match to {
                    ConvertMode::Direct => ConvertTargetDto::Direct,
                    ConvertMode::SystemRegistered => ConvertTargetDto::SystemRegistered,
                },
                unit_name: unit_name.clone(),
                auto_start: Some(*auto_start),
            };
            let process = client.convert_process(name, &request).await?;
            if json {
                print_json(&process)?;
            } else {
                println!("converted {name} to {:?}", process.management_mode);
            }
        }
        Command::Logs { name, follow, tail, since } => {
            let page = client.process_logs(name, *tail, *since, None).await?;
            if json && !*follow {
                print_json(&page)?;
            } else {
                for line in &page.lines {
                    print_log_line(line, json)?;
                }
            }
            if *follow {
                follow_process_logs(client, name, json, page.high_watermark).await?;
            }
        }
        Command::Add { config } => {
            add_from_config(client, config, json).await?;
        }
        Command::Remove { name, force } => {
            client.remove(name, *force).await?;
            if json {
                print_json(&serde_json::json!({ "name": name, "action": "removed" }))?;
            } else {
                println!("removed {name}");
            }
        }
        Command::Reload => {
            client.reload().await?;
            if json {
                print_json(&serde_json::json!({ "reloaded": true }))?;
            } else {
                println!("reload requested");
            }
        }
        Command::Config { sub } => match sub {
            ConfigCmd::Validate { file, mode } => {
                let request = config_request(file, *mode, false)?;
                let result = client.validate_config(&request).await?;
                print_config_result(&result, json)?;
            }
            ConfigCmd::Apply { file, mode, dry_run } => {
                let request = config_request(file, *mode, *dry_run)?;
                let result = client.apply_config(&request).await?;
                print_config_result(&result, json)?;
            }
        },
        Command::Daemon { sub } => match sub {
            DaemonCmd::Status { recovery } => {
                if *recovery {
                    let diagnostics = client.recovery_diagnostics().await?;
                    if json {
                        print_json(&diagnostics)?;
                    } else if diagnostics.records.is_empty() {
                        println!("pending recovery: none");
                    } else {
                        let mut table = Table::new();
                        table.set_header(vec!["KIND", "RESOURCE", "STAGE", "ATTEMPTS", "LAST ERROR"]);
                        for record in diagnostics.records {
                            table.add_row(vec![
                                record.kind,
                                record.resource,
                                record.stage,
                                record.attempts.to_string(),
                                record.last_error.unwrap_or_else(|| "-".into()),
                            ]);
                        }
                        println!("{table}");
                    }
                    return Ok(());
                }
                let status = client.daemon_status().await?;
                if json {
                    print_json(&status)?;
                } else {
                    println!("version:    {}", status.version);
                    println!("pid:        {}", status.pid);
                    println!("processes:  {}", status.process_count);
                    println!("started_at: {}", status.started_at.to_rfc3339());
                    println!("config:     {}", status.config_path);
                    println!("log_dir:    {}", status.log_dir);
                }
            }
            DaemonCmd::Events => follow_events(client, json).await?,
            DaemonCmd::Shutdown => {
                client.shutdown().await?;
                if json {
                    print_json(&serde_json::json!({ "shutdown_requested": true }))?;
                } else {
                    println!("shutdown requested");
                }
            }
        },
        Command::Job { sub } => match sub {
            JobCmd::Ls => {
                let list = client.list_jobs().await?;
                if json {
                    print_json(&list)?;
                } else {
                    let mut table = Table::new();
                    table.set_header(vec!["NAME", "TRIGGER", "OVERLAP", "NEXT_RUN"]);
                    for j in &list.jobs {
                        table.add_row(vec![
                            j.name.clone(),
                            format!("{:?}", j.trigger),
                            format!("{:?}", j.on_overlap),
                            j.next_run_at
                                .map(|t| t.to_rfc3339())
                                .unwrap_or_else(|| "-".into()),
                        ]);
                    }
                    println!("{table}");
                }
            }
            JobCmd::Show { name } => {
                let job = client.get_job(name).await?;
                if json {
                    print_json(&job)?;
                } else {
                    println!("name:       {}", job.name);
                    println!("trigger:    {:?}", job.trigger);
                    println!("overlap:    {:?}", job.on_overlap);
                    println!("next_run:   {}", job.next_run_at.map_or_else(|| "-".into(), |time| time.to_rfc3339()));
                    println!("upstream:   {}", job.dependencies.upstream.join(", "));
                    println!("downstream: {}", job.dependencies.downstream.join(", "));
                }
            }
            JobCmd::Remove { name, force } => {
                client.remove_job(name, *force).await?;
                if json {
                    print_json(&serde_json::json!({ "name": name, "action": "removed" }))?;
                } else {
                    println!("removed job {name}");
                }
            }
            JobCmd::Trigger { name } => {
                let run_id = client.trigger_job(name).await?;
                if json {
                    print_json(&serde_json::json!({ "run_id": run_id }))?;
                } else {
                    println!("triggered {name} (run {run_id})");
                }
            }
            JobCmd::Cancel { name, run_id } => {
                client.cancel_run(name, run_id).await?;
                if json {
                    print_json(&serde_json::json!({ "name": name, "run_id": run_id, "cancel_requested": true }))?;
                } else {
                    println!("cancel requested for {name} run {run_id}");
                }
            }
            JobCmd::Runs { name, limit } => {
                let list = client.list_runs(name, *limit).await?;
                if json {
                    print_json(&list)?;
                } else {
                    let mut table = Table::new();
                    table.set_header(vec!["RUN_ID", "STATE", "EXIT", "SCHEDULED", "ENDED"]);
                    for r in &list.runs {
                        table.add_row(vec![
                            r.run_id.clone(),
                            run_state_label(r.state).to_string(),
                            r.exit_code
                                .map(|c| c.to_string())
                                .unwrap_or_else(|| "-".into()),
                            r.scheduled_at.to_rfc3339(),
                            r.ended_at
                                .map(|t| t.to_rfc3339())
                                .unwrap_or_else(|| "-".into()),
                        ]);
                    }
                    println!("{table}");
                }
            }
            JobCmd::Logs { name, run_id, follow, tail, since } => {
                let page = client.run_logs(name, run_id, *tail, *since, None).await?;
                if json && !*follow {
                    print_json(&page)?;
                } else {
                    for line in &page.lines {
                        print_log_line(line, json)?;
                    }
                }
                if *follow {
                    follow_run_logs(client, name, run_id, json, page.high_watermark).await?;
                }
            }
        },
    }
    Ok(())
}

async fn follow_events(client: &Client, json: bool) -> Result<(), CliError> {
    let url = client.events_websocket_url()?;
    let mut backoff_ms = 100_u64;
    let mut deduper = EventDeduper::default();
    loop {
        let connection = tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|error| CliError::Failed(format!("waiting for Ctrl-C: {error}")))?;
                return Ok(());
            }
            result = tokio_tungstenite::connect_async(&url) => result,
        };
        match connection {
            Ok((mut socket, _)) => {
                backoff_ms = 100;
                loop {
                    tokio::select! {
                        signal = tokio::signal::ctrl_c() => {
                            signal.map_err(|error| CliError::Failed(format!("waiting for Ctrl-C: {error}")))?;
                            let _ = socket.send(Message::Close(None)).await;
                            return Ok(());
                        }
                        message = socket.next() => match message {
                            Some(Ok(Message::Text(text))) => {
                                let Ok(event) = serde_json::from_str::<EventEnvelope>(&text) else {
                                    continue;
                                };
                                if deduper.should_emit(event.event_id.as_deref()) {
                                    print_event(&event, json)?;
                                }
                            }
                            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                            _ => {}
                        }
                    }
                }
            }
            Err(_) => {}
        }
        if !wait_for_follow_retry(backoff_ms).await? {
            return Ok(());
        }
        backoff_ms = (backoff_ms * 2).min(2_000);
    }
}

async fn follow_process_logs(
    client: &Client,
    name: &str,
    json: bool,
    initial_sequence: u64,
) -> Result<(), CliError> {
    follow_logs(client, FollowSource::Process(name), json, initial_sequence).await
}

async fn follow_run_logs(
    client: &Client,
    name: &str,
    run_id: &str,
    json: bool,
    initial_sequence: u64,
) -> Result<(), CliError> {
    follow_logs(client, FollowSource::Run { name, run_id }, json, initial_sequence).await
}

enum FollowSource<'a> {
    Process(&'a str),
    Run { name: &'a str, run_id: &'a str },
}

async fn follow_logs(
    client: &Client,
    source: FollowSource<'_>,
    json: bool,
    mut last_sequence: u64,
) -> Result<(), CliError> {
    let mut backoff_ms = 100_u64;
    loop {
        let gap = tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|error| CliError::Failed(format!("waiting for Ctrl-C: {error}")))?;
                return Ok(());
            }
            result = async {
                match &source {
                    FollowSource::Process(name) => client.process_logs(name, 10_000, None, Some(last_sequence)).await,
                    FollowSource::Run { name, run_id } => client.run_logs(name, run_id, 10_000, None, Some(last_sequence)).await,
                }
            } => result,
        };
        let gap = match gap {
            Ok(gap) => gap,
            Err(_) => {
                if !wait_for_follow_retry(backoff_ms).await? {
                    return Ok(());
                }
                backoff_ms = (backoff_ms * 2).min(2_000);
                continue;
            }
        };
        for line in gap.lines {
            if line.sequence == 0 || line.sequence > last_sequence {
                print_log_line(&line, json)?;
                last_sequence = last_sequence.max(line.sequence);
            }
        }

        let url = match &source {
            FollowSource::Process(name) => client.process_log_websocket_url(name, last_sequence)?,
            FollowSource::Run { name, run_id } => client.run_log_websocket_url(name, run_id, last_sequence)?,
        };
        let connection = tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|error| CliError::Failed(format!("waiting for Ctrl-C: {error}")))?;
                return Ok(());
            }
            result = tokio_tungstenite::connect_async(url) => result,
        };
        match connection {
            Ok((mut socket, _)) => {
                backoff_ms = 100;
                loop {
                    tokio::select! {
                        signal = tokio::signal::ctrl_c() => {
                            signal.map_err(|error| CliError::Failed(format!("waiting for Ctrl-C: {error}")))?;
                            let _ = socket.send(Message::Close(None)).await;
                            return Ok(());
                        }
                        message = socket.next() => match message {
                            Some(Ok(Message::Text(text))) => {
                                let value: serde_json::Value = match serde_json::from_str(&text) {
                                    Ok(value) => value,
                                    Err(_) => continue,
                                };
                                if value.get("type").and_then(|value| value.as_str()) == Some("log.dropped") {
                                    break;
                                }
                                if let Ok(line) = serde_json::from_value::<LogLineDto>(value) {
                                    if line.sequence == 0 || line.sequence > last_sequence {
                                        print_log_line(&line, json)?;
                                        last_sequence = last_sequence.max(line.sequence);
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                            _ => {}
                        }
                    }
                }
            }
            Err(_) => {}
        }
        if !wait_for_follow_retry(backoff_ms).await? {
            return Ok(());
        }
        backoff_ms = (backoff_ms * 2).min(2_000);
    }
}

/// Sleeps between every failed follow phase while allowing Ctrl-C to end the
/// command successfully even when the daemon is unavailable.
async fn wait_for_follow_retry(backoff_ms: u64) -> Result<bool, CliError> {
    tokio::select! {
        signal = tokio::signal::ctrl_c() => {
            signal.map_err(|error| CliError::Failed(format!("waiting for Ctrl-C: {error}")))?;
            Ok(false)
        }
        () = tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)) => Ok(true),
    }
}

/// Read a TOML config file and register each `[[process]]` / `[[job]]` entry.
async fn add_from_config(client: &Client, path: &PathBuf, json: bool) -> Result<(), CliError> {
    let file = read_config(path)?;
    if file.processes.is_empty() && file.jobs.is_empty() {
        return Err(CliError::Failed(
            "config has no [[process]] or [[job]] entries".into(),
        ));
    }
    let result = client.apply_config(&ConfigApplyRequestDto {
        mode: ConfigApplyModeDto::Merge,
        dry_run: false,
        config: file,
    }).await?;
    print_config_result(&result, json)?;
    Ok(())
}

fn read_config(path: &PathBuf) -> Result<FileConfig, CliError> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| CliError::Failed(format!("reading {}: {error}", path.display())))?;
    toml::from_str(&contents).map_err(|error| CliError::Failed(format!("parsing config: {error}")))
}

fn config_request(path: &PathBuf, mode: ConfigMode, dry_run: bool) -> Result<ConfigApplyRequestDto, CliError> {
    Ok(ConfigApplyRequestDto {
        mode: match mode {
            ConfigMode::Merge => ConfigApplyModeDto::Merge,
            ConfigMode::Replace => ConfigApplyModeDto::Replace,
        },
        dry_run,
        config: read_config(path)?,
    })
}

fn print_config_result(
    result: &my_supervisor_shared::api::ConfigApplyResultDto,
    json: bool,
) -> Result<(), CliError> {
    if json {
        return print_json(result);
    }
    let diff = &result.diff;
    println!("config {} ({:?})", if result.dry_run { "validated" } else { "applied" }, result.mode);
    println!("processes: +{} ~{} -{}", diff.added_processes.len(), diff.updated_processes.len(), diff.removed_processes.len());
    println!("jobs:      +{} ~{} -{}", diff.added_jobs.len(), diff.updated_jobs.len(), diff.removed_jobs.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::EventDeduper;

    #[test]
    fn terminal_event_deduper_preserves_first_seen_order_and_accepts_legacy_frames() {
        let mut deduper = EventDeduper::default();
        let accepted = [Some("A"), Some("A"), Some("B"), None, None]
            .into_iter()
            .filter(|event_id| deduper.should_emit(*event_id))
            .collect::<Vec<_>>();

        assert_eq!(accepted, vec![Some("A"), Some("B"), None, None]);
    }
}
