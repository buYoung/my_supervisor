//! `msv` — the operations CLI. A thin HTTP/WS client over the daemon's API
//! (`docs/ARCHITECTURE.md` §4.1.2); it embeds no core and forks no wire type.

mod client;

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use comfy_table::Table;
use futures_util::StreamExt;
use serde::Serialize;
use tokio_tungstenite::tungstenite::Message;

use client::{CliError, Client};
use my_supervisor_app_daemon::{debug_or_canonical_root, DEFAULT_BASE_URL};
use my_supervisor_platform_macos::{SupervisorLaunchAgent, SupervisorServiceStatus};
use my_supervisor_shared::api::{
    ConfigApplyModeDto, ConvertRequestDto, ConvertTargetDto, JobRunStateDto, LogLineDto,
    LogStreamDto, LogsResponseDto, ProcessStateDto,
};
use my_supervisor_shared::config::{ConfigApplyRequestDto, FileConfig};
use my_supervisor_shared::events::EventEnvelope;

const EVENT_DEDUP_CACHE_CAPACITY: usize = 1_024;
// CLI dispatch has a large established table/json surface.  This process-wide
// selection is set exactly once after clap parsing and leaves legacy `json`
// byte-for-byte behavior untouched.
static JSON_V2_OUTPUT: AtomicBool = AtomicBool::new(false);

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
    JsonV2,
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
    Restart {
        name: String,
        #[arg(long)]
        rolling: bool,
        #[arg(long)]
        operation_id: Option<uuid::Uuid>,
    },
    /// Show independently owned process slots.
    Instances { name: String },
    /// Change the desired Direct-process instance count.
    Scale {
        name: String,
        #[arg(long)]
        instances: u16,
        #[arg(long)]
        operation_id: Option<uuid::Uuid>,
    },
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
    /// User-scoped service lifecycle and authenticated maintenance.
    Service {
        #[command(subcommand)]
        sub: ServiceCmd,
    },
    /// Job controls.
    Job {
        #[command(subcommand)]
        sub: JobCmd,
    },
    /// Query bounded durable observability records.
    Observability {
        #[command(subcommand)]
        sub: ObservabilityCmd,
    },
}

#[derive(Subcommand)]
enum ObservabilityCmd {
    Events {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Metrics {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Alerts {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Deliveries {
        #[arg(long, default_value_t = 100)]
        limit: usize,
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

#[derive(Subcommand)]
enum ServiceCmd {
    /// Install the user-scoped supervisor LaunchAgent without deleting data.
    Install,
    /// Start the installed user service.
    Start,
    /// Intentionally stop the service and suppress KeepAlive until start.
    Stop,
    /// Read local launchd registration state without mutating it.
    Status,
    /// Remove only the user service registration; retain all data.
    Uninstall,
    /// Rotate the native bearer credential and rebootstrap this client.
    RotateToken,
    /// Create one verified owner-serialized backup cut.
    Backup,
    /// Stage a verified snapshot and durable upgrade journal.
    Upgrade,
    /// Restore the last verified upgrade snapshot.
    Rollback,
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
    /// Preview a single job definition from a TOML config without mutation.
    Preview {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        at: DateTime<Utc>,
        #[arg(long, default_value_t = 10)]
        count: u16,
    },
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
    let json_v2 = matches!(cli.output, OutputFormat::JsonV2);
    JSON_V2_OUTPUT.store(json_v2, Ordering::Relaxed);
    if let Command::Service { sub } = &cli.command {
        if matches!(
            sub,
            ServiceCmd::Install
                | ServiceCmd::Start
                | ServiceCmd::Stop
                | ServiceCmd::Status
                | ServiceCmd::Uninstall
        ) {
            return match dispatch_service_offline(
                sub,
                matches!(cli.output, OutputFormat::Json | OutputFormat::JsonV2),
            ) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    print_error(&error, json_v2);
                    ExitCode::from(exit_code(&error, json_v2) as u8)
                }
            };
        }
    }
    let client = match Client::new(
        cli.url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
    ) {
        Ok(client) => client,
        Err(error) => {
            print_error(&error, json_v2);
            return ExitCode::from(exit_code(&error, json_v2) as u8);
        }
    };
    match dispatch(&cli, &client).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            print_error(&e, json_v2);
            ExitCode::from(exit_code(&e, json_v2) as u8)
        }
    }
}

fn dispatch_service_offline(command: &ServiceCmd, json: bool) -> Result<(), CliError> {
    let root = debug_or_canonical_root()
        .map_err(|error| CliError::Failed(format!("resolving service root: {error}")))?;
    let binary = std::env::var_os("MSV_DAEMON_TEST_BINARY")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(|parent| parent.join("msv-daemon")))
        })
        .ok_or_else(|| CliError::Failed("resolving msv-daemon beside the CLI".into()))?;
    let service = SupervisorLaunchAgent::new(root, binary).map_err(CliError::Failed)?;
    let status: SupervisorServiceStatus = match command {
        ServiceCmd::Install => service.install(),
        ServiceCmd::Start => service.start(),
        ServiceCmd::Stop => service.stop(),
        ServiceCmd::Status => service.status(),
        ServiceCmd::Uninstall => service.uninstall(),
        ServiceCmd::RotateToken
        | ServiceCmd::Backup
        | ServiceCmd::Upgrade
        | ServiceCmd::Rollback => unreachable!("online maintenance is dispatched through Client"),
    }
    .map_err(CliError::Failed)?;
    if json {
        print_json(&status)?;
    } else {
        println!("{}: {:?}", status.label, status.state);
    }
    Ok(())
}

fn print_json<T: Serialize>(value: &T) -> Result<(), CliError> {
    let output = if JSON_V2_OUTPUT.load(Ordering::Relaxed) {
        serde_json::to_string(
            &serde_json::json!({ "ok": true, "data": value, "error": null, "partial": null }),
        )
    } else {
        serde_json::to_string_pretty(value)
    }
    .map_err(|error| CliError::Failed(format!("serializing output: {error}")))?;
    println!("{output}");
    Ok(())
}

fn print_error(error: &CliError, json_v2: bool) {
    if json_v2 && matches!(error, CliError::Partial(_)) {
        // The preceding successful operation envelope already carries the
        // required partial payload.
        return;
    }
    if json_v2 {
        let _ = serde_json::to_string(&serde_json::json!({
            "ok": false,
            "data": null,
            "error": { "message": error.message() },
            "partial": match error { CliError::Partial(message) => Some(message), _ => None },
        }))
        .map(|value| println!("{value}"));
    } else {
        eprintln!("error: {}", error.message());
    }
}

fn exit_code(error: &CliError, json_v2: bool) -> i32 {
    if !json_v2 {
        return error.exit_code();
    }
    match error {
        CliError::Partial(_) => 3,
        CliError::DaemonDown => 4,
        CliError::NotFound(_) | CliError::Failed(_) => 1,
    }
}

fn print_operation(
    operation: &my_supervisor_shared::api::ProcessOperationDto,
    json: bool,
) -> Result<(), CliError> {
    if json {
        if JSON_V2_OUTPUT.load(Ordering::Relaxed) {
            let partial = operation.outcomes.iter().any(|outcome| {
                !matches!(
                    outcome.state,
                    my_supervisor_shared::api::ProcessOperationInstanceStateDto::Completed
                )
            });
            let output = serde_json::to_string(&serde_json::json!({
                "ok": true,
                "data": operation,
                "error": null,
                "partial": if partial { serde_json::json!({ "outcomes": operation.outcomes }) } else { serde_json::Value::Null },
            })).map_err(|error| CliError::Failed(format!("serializing output: {error}")))?;
            println!("{output}");
            return Ok(());
        }
        return print_json(operation);
    }
    println!("{} {}: {}", operation.kind, operation.name, operation.phase);
    for outcome in &operation.outcomes {
        let failed_stage = outcome
            .failed_stage
            .as_deref()
            .map(|stage| format!(": {stage}"))
            .unwrap_or_default();
        println!(
            "  [{}] {} ({:?}){failed_stage}",
            outcome.ordinal, outcome.instance_id, outcome.state
        );
    }
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
    let json = matches!(cli.output, OutputFormat::Json | OutputFormat::JsonV2);
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
                println!(
                    "pid:       {}",
                    process
                        .pid
                        .map_or_else(|| "-".into(), |pid| pid.to_string())
                );
                println!("restarts:  {}", process.restart_count);
                println!("mode:      {:?}", process.management_mode);
                println!(
                    "started:   {}",
                    process
                        .started_at
                        .map_or_else(|| "-".into(), |time| time.to_rfc3339())
                );
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
        Command::Restart {
            name,
            rolling,
            operation_id,
        } => {
            if *rolling {
                let operation = client.rolling_restart_process(name, *operation_id).await?;
                print_operation(&operation, json)?;
                if operation.outcomes.iter().any(|outcome| {
                    !matches!(
                        outcome.state,
                        my_supervisor_shared::api::ProcessOperationInstanceStateDto::Completed
                    )
                }) {
                    return Err(CliError::Partial(
                        "rolling restart completed with partial per-instance outcomes".into(),
                    ));
                }
                return Ok(());
            }
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
        Command::Instances { name } => {
            let response = client.process_instances(name).await?;
            if json {
                print_json(&response)?;
            } else {
                let mut table = Table::new();
                table.set_header(vec!["ORDINAL", "INSTANCE", "STATE", "PID", "RESTARTS"]);
                for instance in response.instances {
                    table.add_row(vec![
                        instance.ordinal.to_string(),
                        instance.instance_id.to_string(),
                        process_state_label(instance.state).into(),
                        instance
                            .pid
                            .map(|pid| pid.to_string())
                            .unwrap_or_else(|| "-".into()),
                        instance.restart_count.to_string(),
                    ]);
                }
                println!("{table}");
            }
        }
        Command::Scale {
            name,
            instances,
            operation_id,
        } => {
            let operation = client
                .scale_process(name, *instances, *operation_id)
                .await?;
            print_operation(&operation, json)?;
            if operation.outcomes.iter().any(|outcome| {
                !matches!(
                    outcome.state,
                    my_supervisor_shared::api::ProcessOperationInstanceStateDto::Completed
                )
            }) {
                return Err(CliError::Partial(
                    "scale completed with partial per-instance outcomes".into(),
                ));
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
        Command::Logs {
            name,
            follow,
            tail,
            since,
        } => {
            if *follow {
                follow_process_logs(client, name, json, *tail, *since).await?;
            } else {
                let page = client.process_logs(name, *tail, *since, None).await?;
                if json {
                    print_json(&page)?;
                } else {
                    for line in &page.lines {
                        print_log_line(line, json)?;
                    }
                }
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
            ConfigCmd::Apply {
                file,
                mode,
                dry_run,
            } => {
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
                        table.set_header(vec![
                            "KIND",
                            "RESOURCE",
                            "STAGE",
                            "ATTEMPTS",
                            "LAST ERROR",
                        ]);
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
        Command::Service { sub } => match sub {
            ServiceCmd::Install
            | ServiceCmd::Start
            | ServiceCmd::Stop
            | ServiceCmd::Status
            | ServiceCmd::Uninstall => {
                unreachable!("offline service lifecycle returns before native client construction")
            }
            ServiceCmd::RotateToken => print_json(&client.rotate_token().await?)?,
            ServiceCmd::Backup => print_json(&client.backup().await?)?,
            ServiceCmd::Upgrade => print_json(&client.upgrade().await?)?,
            ServiceCmd::Rollback => print_json(&client.rollback().await?)?,
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
                    println!(
                        "next_run:   {}",
                        job.next_run_at
                            .map_or_else(|| "-".into(), |time| time.to_rfc3339())
                    );
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
            JobCmd::Preview { config, at, count } => {
                let config = read_config(config)?;
                let job = config.jobs.into_iter().next().ok_or_else(|| {
                    CliError::Failed("preview config must contain one [[job]] entry".into())
                })?;
                let preview = client
                    .preview_job(&my_supervisor_shared::api::JobPreviewRequestDto {
                        config: job,
                        at: *at,
                        count: *count,
                    })
                    .await?;
                if json {
                    print_json(&preview)?;
                } else {
                    for occurrence in preview.occurrences {
                        println!(
                            "{} {}",
                            occurrence.scheduled_at.to_rfc3339(),
                            occurrence.local_time
                        );
                    }
                }
            }
            JobCmd::Cancel { name, run_id } => {
                client.cancel_run(name, run_id).await?;
                if json {
                    print_json(
                        &serde_json::json!({ "name": name, "run_id": run_id, "cancel_requested": true }),
                    )?;
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
            JobCmd::Logs {
                name,
                run_id,
                follow,
                tail,
                since,
            } => {
                if *follow {
                    follow_run_logs(client, name, run_id, json, *tail, *since).await?;
                } else {
                    let page = client.run_logs(name, run_id, *tail, *since, None).await?;
                    if json {
                        print_json(&page)?;
                    } else {
                        for line in &page.lines {
                            print_log_line(line, json)?;
                        }
                    }
                }
            }
        },
        Command::Observability { sub } => match sub {
            ObservabilityCmd::Events { limit } => {
                print_json(&client.observability_events(None, *limit).await?)?
            }
            ObservabilityCmd::Metrics { limit } => {
                print_json(&client.observability_metrics(None, *limit).await?)?
            }
            ObservabilityCmd::Alerts { limit } => {
                print_json(&client.observability_alerts(None, *limit).await?)?
            }
            ObservabilityCmd::Deliveries { limit } => {
                print_json(&client.observability_deliveries(None, *limit).await?)?
            }
        },
    }
    Ok(())
}

#[allow(clippy::single_match)]
async fn follow_events(client: &Client, json: bool) -> Result<(), CliError> {
    let url = client.events_websocket_url()?;
    let mut backoff_ms = 100_u64;
    let mut deduper = EventDeduper::default();
    let mut interrupt = Box::pin(tokio::signal::ctrl_c());
    loop {
        let connection = tokio::select! {
            signal = &mut interrupt => {
                signal.map_err(|error| CliError::Failed(format!("waiting for Ctrl-C: {error}")))?;
                return Ok(());
            }
            result = tokio_tungstenite::connect_async(client.websocket_request(&url)?) => result,
        };
        match connection {
            Ok((mut socket, _)) => {
                backoff_ms = 100;
                loop {
                    tokio::select! {
                        signal = &mut interrupt => {
                            signal.map_err(|error| CliError::Failed(format!("waiting for Ctrl-C: {error}")))?;
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
                        },
                        // The transport can be restarted without an explicit
                        // Close frame for an upgraded idle socket. Refresh the
                        // live-only event connection so durable outbox retries
                        // reach the current listener; stable IDs de-duplicate
                        // any overlap between connections.
                        () = tokio::time::sleep(std::time::Duration::from_millis(500)) => break,
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
    tail: usize,
    since: Option<DateTime<Utc>>,
) -> Result<(), CliError> {
    follow_logs(client, FollowSource::Process(name), json, tail, since).await
}

async fn follow_run_logs(
    client: &Client,
    name: &str,
    run_id: &str,
    json: bool,
    tail: usize,
    since: Option<DateTime<Utc>>,
) -> Result<(), CliError> {
    follow_logs(
        client,
        FollowSource::Run { name, run_id },
        json,
        tail,
        since,
    )
    .await
}

enum FollowSource<'a> {
    Process(&'a str),
    Run { name: &'a str, run_id: &'a str },
}

#[allow(clippy::single_match, clippy::collapsible_match)]
async fn follow_logs(
    client: &Client,
    source: FollowSource<'_>,
    json: bool,
    tail: usize,
    since: Option<DateTime<Utc>>,
) -> Result<(), CliError> {
    let mut backoff_ms = 100_u64;
    let mut last_sequence = 0_u64;
    let mut interrupt = Box::pin(tokio::signal::ctrl_c());
    let mut is_initial_snapshot = true;
    loop {
        // The public API caps a bounded tail at 10,000 rows. For a larger
        // initial tail, request the retained snapshot once and apply the
        // caller's bound locally before handing its high-watermark to WS.
        // Reconnect recovery is always cursor-based and therefore unbounded.
        let snapshot_tail = if is_initial_snapshot && tail <= 10_000 {
            tail
        } else {
            0
        };
        let gap = tokio::select! {
            signal = &mut interrupt => {
                signal.map_err(|error| CliError::Failed(format!("waiting for Ctrl-C: {error}")))?;
                connect_and_drain_follow_websocket(client, &source, json, &mut last_sequence).await?;
                drain_follow_log_page(client, &source, json, since, &mut last_sequence).await?;
                return Ok(());
            }
            result = follow_log_page(client, &source, snapshot_tail, since, last_sequence) => result,
        };
        let gap = match gap {
            Ok(gap) => gap,
            Err(_) => {
                tokio::select! {
                    signal = &mut interrupt => {
                        signal.map_err(|error| CliError::Failed(format!("waiting for Ctrl-C: {error}")))?;
                        connect_and_drain_follow_websocket(client, &source, json, &mut last_sequence).await?;
                        drain_follow_log_page(client, &source, json, since, &mut last_sequence).await?;
                        return Ok(());
                    }
                    () = tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)) => {}
                }
                backoff_ms = (backoff_ms * 2).min(2_000);
                continue;
            }
        };

        // Truncation is normal for the initial bounded tail. Cursor recovery
        // uses tail=0, so truncation there means the gap contract was broken.
        if !is_initial_snapshot && gap.truncated {
            return Err(CliError::Failed(
                "follow log gap remained truncated after an unbounded cursor recovery request"
                    .into(),
            ));
        }

        let initial_output_count = gap
            .lines
            .iter()
            .filter(|line| !matches!(line.stream, LogStreamDto::System))
            .count();
        let mut initial_output_to_skip = if is_initial_snapshot && tail > 10_000 {
            initial_output_count.saturating_sub(tail)
        } else {
            0
        };
        for line in gap.lines {
            if line.sequence == 0 || line.sequence > last_sequence {
                let is_initial_system_metadata =
                    is_initial_snapshot && matches!(line.stream, LogStreamDto::System);
                if is_initial_system_metadata {
                    last_sequence = last_sequence.max(line.sequence);
                    continue;
                }
                if initial_output_to_skip > 0 {
                    initial_output_to_skip = initial_output_to_skip.saturating_sub(1);
                    last_sequence = last_sequence.max(line.sequence);
                    continue;
                }
                print_log_line(&line, json)?;
                last_sequence = last_sequence.max(line.sequence);
            }
        }
        is_initial_snapshot = false;

        let url = match &source {
            FollowSource::Process(name) => client.process_log_websocket_url(name, last_sequence)?,
            FollowSource::Run { name, run_id } => {
                client.run_log_websocket_url(name, run_id, last_sequence)?
            }
        };
        let connection = tokio::select! {
            signal = &mut interrupt => {
                signal.map_err(|error| CliError::Failed(format!("waiting for Ctrl-C: {error}")))?;
                connect_and_drain_follow_websocket(client, &source, json, &mut last_sequence).await?;
                drain_follow_log_page(client, &source, json, since, &mut last_sequence).await?;
                return Ok(());
            }
            result = tokio_tungstenite::connect_async(client.websocket_request(&url)?) => result,
        };
        let mut should_recover_immediately = false;
        match connection {
            Ok((mut socket, _)) => {
                backoff_ms = 100;
                loop {
                    tokio::select! {
                        signal = &mut interrupt => {
                            signal.map_err(|error| CliError::Failed(format!("waiting for Ctrl-C: {error}")))?;
                            while let Ok(Some(message)) = tokio::time::timeout(
                                std::time::Duration::from_secs(2),
                                socket.next(),
                            ).await {
                                match message {
                                    Ok(Message::Text(text)) => {
                                        if consume_follow_text(&text, json, &mut last_sequence)? {
                                            break;
                                        }
                                    }
                                    Ok(Message::Close(_)) | Err(_) => break,
                                    _ => {}
                                }
                            }
                            drain_follow_log_page(client, &source, json, since, &mut last_sequence).await?;
                            return Ok(());
                        }
                        message = socket.next() => match message {
                            Some(Ok(Message::Text(text))) => {
                                if consume_follow_text(&text, json, &mut last_sequence)? {
                                    should_recover_immediately = true;
                                    break;
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
        if should_recover_immediately {
            continue;
        }
        tokio::select! {
            signal = &mut interrupt => {
                signal.map_err(|error| CliError::Failed(format!("waiting for Ctrl-C: {error}")))?;
                connect_and_drain_follow_websocket(client, &source, json, &mut last_sequence).await?;
                drain_follow_log_page(client, &source, json, since, &mut last_sequence).await?;
                return Ok(());
            }
            () = tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)) => {}
        }
        backoff_ms = (backoff_ms * 2).min(2_000);
    }
}

async fn follow_log_page(
    client: &Client,
    source: &FollowSource<'_>,
    tail: usize,
    since: Option<DateTime<Utc>>,
    after_sequence: u64,
) -> Result<LogsResponseDto, CliError> {
    match source {
        FollowSource::Process(name) => {
            client
                .process_logs(name, tail, since, Some(after_sequence))
                .await
        }
        FollowSource::Run { name, run_id } => {
            client
                .run_logs(name, run_id, tail, since, Some(after_sequence))
                .await
        }
    }
}

/// Best-effort final cursor drain. A follower may receive SIGINT immediately
/// after another client observes the durable final row, while its WebSocket is
/// still delivering or recovering from `log.dropped`. Reading once from the
/// last consumed sequence closes that race without changing unavailable-daemon
/// Ctrl-C into an error.
async fn drain_follow_log_page(
    client: &Client,
    source: &FollowSource<'_>,
    json: bool,
    since: Option<DateTime<Utc>>,
    last_sequence: &mut u64,
) -> Result<(), CliError> {
    let Ok(gap) = follow_log_page(client, source, 0, since, *last_sequence).await else {
        return Ok(());
    };
    for line in gap.lines {
        if line.sequence == 0 || line.sequence > *last_sequence {
            print_log_line(&line, json)?;
            *last_sequence = (*last_sequence).max(line.sequence);
        }
    }
    Ok(())
}

/// Consume one log WebSocket text frame. `true` requests immediate cursor
/// recovery after the server reports a broadcast gap.
fn consume_follow_text(text: &str, json: bool, last_sequence: &mut u64) -> Result<bool, CliError> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Ok(false);
    };
    if value.get("type").and_then(|value| value.as_str()) == Some("log.dropped") {
        return Ok(true);
    }
    if let Ok(line) = serde_json::from_value::<LogLineDto>(value) {
        if line.sequence == 0 || line.sequence > *last_sequence {
            print_log_line(&line, json)?;
            *last_sequence = (*last_sequence).max(line.sequence);
        }
    }
    Ok(false)
}

/// Complete the initial REST-to-WebSocket handoff even when SIGINT wins the
/// connection race. The server snapshot starts at the last consumed sequence,
/// so buffered rows remain ordered and de-duplicated by `consume_follow_text`.
#[allow(clippy::collapsible_match)]
async fn connect_and_drain_follow_websocket(
    client: &Client,
    source: &FollowSource<'_>,
    json: bool,
    last_sequence: &mut u64,
) -> Result<(), CliError> {
    let url = match source {
        FollowSource::Process(name) => client.process_log_websocket_url(name, *last_sequence)?,
        FollowSource::Run { name, run_id } => {
            client.run_log_websocket_url(name, run_id, *last_sequence)?
        }
    };
    let Ok(Ok((mut socket, _))) = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio_tungstenite::connect_async(client.websocket_request(&url)?),
    )
    .await
    else {
        return Ok(());
    };
    while let Ok(Some(message)) =
        tokio::time::timeout(std::time::Duration::from_secs(2), socket.next()).await
    {
        match message {
            Ok(Message::Text(text)) => {
                if consume_follow_text(&text, json, last_sequence)? {
                    break;
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }
    Ok(())
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
    let result = client
        .apply_config(&ConfigApplyRequestDto {
            mode: ConfigApplyModeDto::Merge,
            dry_run: false,
            config: file,
        })
        .await?;
    print_config_result(&result, json)?;
    Ok(())
}

fn read_config(path: &PathBuf) -> Result<FileConfig, CliError> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| CliError::Failed(format!("reading {}: {error}", path.display())))?;
    toml::from_str(&contents).map_err(|error| CliError::Failed(format!("parsing config: {error}")))
}

fn config_request(
    path: &PathBuf,
    mode: ConfigMode,
    dry_run: bool,
) -> Result<ConfigApplyRequestDto, CliError> {
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
    println!(
        "config {} ({:?})",
        if result.dry_run {
            "validated"
        } else {
            "applied"
        },
        result.mode
    );
    println!(
        "processes: +{} ~{} -{}",
        diff.added_processes.len(),
        diff.updated_processes.len(),
        diff.removed_processes.len()
    );
    println!(
        "jobs:      +{} ~{} -{}",
        diff.added_jobs.len(),
        diff.updated_jobs.len(),
        diff.removed_jobs.len()
    );
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
