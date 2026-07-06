//! `msv` — the operations CLI. A thin HTTP/WS client over the daemon's API
//! (`docs/ARCHITECTURE.md` §4.1.2); it embeds no core and forks no wire type.

mod client;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use comfy_table::Table;
use futures_util::StreamExt;
use serde::Serialize;

use client::{CliError, Client};
use my_supervisor_app_daemon::DEFAULT_BASE_URL;
use my_supervisor_shared::api::{JobRunStateDto, ProcessStateDto};
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

#[derive(Subcommand)]
enum Command {
    /// List managed processes.
    Ps,
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
}

#[derive(Subcommand)]
enum JobCmd {
    /// List jobs.
    Ls,
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
    let client = Client::new(
        cli.url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
    );
    match dispatch(&cli, &client).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {}", e.message());
            ExitCode::from(e.exit_code() as u8)
        }
    }
}

fn print_json<T: Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("error: serializing output: {e}"),
    }
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
                print_json(&list);
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
        Command::Start { name } => {
            client.process_action(name, "start").await?;
            if !json {
                println!("started {name}");
            }
        }
        Command::Stop { name, force } => {
            client.stop(name, *force).await?;
            if !json {
                println!("stopped {name}");
            }
        }
        Command::Restart { name } => {
            client.process_action(name, "restart").await?;
            if !json {
                println!("restarted {name}");
            }
        }
        Command::Logs { name, follow, tail } => {
            let page = client.process_logs(name, *tail).await?;
            if json && !follow {
                print_json(&page);
            } else {
                for line in &page.lines {
                    println!(
                        "{} [{:?}] {}",
                        line.timestamp.to_rfc3339(),
                        line.stream,
                        line.line
                    );
                }
            }
            if *follow {
                follow_logs(client, name).await?;
            }
        }
        Command::Add { config } => {
            add_from_config(client, config).await?;
        }
        Command::Remove { name, force } => {
            client.remove(name, *force).await?;
            if !json {
                println!("removed {name}");
            }
        }
        Command::Reload => {
            client.reload().await?;
            if !json {
                println!("reload requested");
            }
        }
        Command::Daemon { sub } => match sub {
            DaemonCmd::Status => {
                let status = client.daemon_status().await?;
                if json {
                    print_json(&status);
                } else {
                    println!("version:    {}", status.version);
                    println!("pid:        {}", status.pid);
                    println!("processes:  {}", status.process_count);
                    println!("started_at: {}", status.started_at.to_rfc3339());
                    println!("config:     {}", status.config_path);
                    println!("log_dir:    {}", status.log_dir);
                }
            }
        },
        Command::Job { sub } => match sub {
            JobCmd::Ls => {
                let list = client.list_jobs().await?;
                if json {
                    print_json(&list);
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
            JobCmd::Trigger { name } => {
                let run_id = client.trigger_job(name).await?;
                if json {
                    print_json(&serde_json::json!({ "run_id": run_id }));
                } else {
                    println!("triggered {name} (run {run_id})");
                }
            }
            JobCmd::Runs { name, limit } => {
                let list = client.list_runs(name, *limit).await?;
                if json {
                    print_json(&list);
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
async fn follow_logs(client: &Client, name: &str) -> Result<(), CliError> {
    let url = format!("{}/api/v1/processes/{name}/logs", client.ws_base());
    let (mut stream, _) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| CliError::Failed(format!("websocket connect: {e}")))?;
    while let Some(message) = stream.next().await {
        match message {
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                if let Ok(line) =
                    serde_json::from_str::<my_supervisor_shared::api::LogLineDto>(&text)
                {
                    println!(
                        "{} [{:?}] {}",
                        line.timestamp.to_rfc3339(),
                        line.stream,
                        line.line
                    );
                }
            }
            Ok(tokio_tungstenite::tungstenite::Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }
    Ok(())
}

/// Read a TOML config file and register each `[[process]]` / `[[job]]` entry.
async fn add_from_config(client: &Client, path: &PathBuf) -> Result<(), CliError> {
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
        println!("added process {}", process.name);
    }
    for job in &file.jobs {
        client.add_job(job).await?;
        println!("added job {}", job.name);
    }
    Ok(())
}
