//! `msv` — the operations CLI. A thin HTTP/WS client over the daemon's API
//! (`docs/ARCHITECTURE.md` §4.1.2); it embeds no core and forks no wire type.

mod client;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use comfy_table::Table;
use serde::Serialize;

use client::{CliError, Client};
use my_supervisor_app_daemon::DEFAULT_BASE_URL;
use my_supervisor_shared::api::{
    ConvertRequestDto, ConvertTargetDto, JobRunStateDto, LogLineDto, ProcessStateDto,
};
use my_supervisor_shared::config::FileConfig;

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
    Status,
    /// Stop the daemon gracefully.
    Shutdown,
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
    /// Show a job's run history.
    Runs {
        name: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
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
        JobRunStateDto::Cancelled => "cancelled",
        JobRunStateDto::Skipped => "skipped",
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
                table.set_header(vec!["NAME", "STATE", "PID", "RESTARTS", "STARTED"]);
                for p in &list.processes {
                    table.add_row(vec![
                        p.name.clone(),
                        process_state_label(p.state).to_string(),
                        p.pid.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
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
        Command::Logs { name, follow, tail } => {
            let page = client.process_logs(name, *tail).await?;
            if json && !follow {
                print_json(&page)?;
            } else {
                for line in &page.lines {
                    print_log_line(line, json)?;
                }
            }
            if *follow {
                follow_logs(client, name, json).await?;
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
        Command::Daemon { sub } => match sub {
            DaemonCmd::Status => {
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
        },
    }
    Ok(())
}

/// Follow a process's logs over WS until interrupted.
async fn follow_logs(client: &Client, name: &str, json: bool) -> Result<(), CliError> {
    const FOLLOW_WINDOW: usize = 10_000;
    let initial = client.process_logs(name, FOLLOW_WINDOW).await?;
    let mut seen_counts = HashMap::<(String, String), usize>::new();
    for line in initial.lines {
        *seen_counts
            .entry((format!("{:?}", line.stream), line.line))
            .or_default() += 1;
    }
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let page = client.process_logs(name, FOLLOW_WINDOW).await?;
        let mut observed_counts = HashMap::<(String, String), usize>::new();
        for line in page.lines {
            let key = (format!("{:?}", line.stream), line.line.clone());
            let observed = observed_counts.entry(key.clone()).or_default();
            *observed += 1;
            if *observed > seen_counts.get(&key).copied().unwrap_or(0) {
                print_log_line(&line, json)?;
            }
        }
        seen_counts = observed_counts;
    }
}

/// Read a TOML config file and register each `[[process]]` / `[[job]]` entry.
async fn add_from_config(client: &Client, path: &PathBuf, json: bool) -> Result<(), CliError> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| CliError::Failed(format!("reading {}: {e}", path.display())))?;
    let file: FileConfig =
        toml::from_str(&contents).map_err(|e| CliError::Failed(format!("parsing config: {e}")))?;
    if file.processes.is_empty() && file.jobs.is_empty() {
        return Err(CliError::Failed(
            "config has no [[process]] or [[job]] entries".into(),
        ));
    }
    for process in &file.processes {
        client.add_process(process).await?;
        if !json {
            println!("added process {}", process.name);
        }
    }
    for job in &file.jobs {
        client.add_job(job).await?;
        if !json {
            println!("added job {}", job.name);
        }
    }
    if json {
        print_json(&serde_json::json!({
            "processes": file.processes.iter().map(|process| &process.name).collect::<Vec<_>>(),
            "jobs": file.jobs.iter().map(|job| &job.name).collect::<Vec<_>>(),
        }))?;
    }
    Ok(())
}
