use crate::client::{CliError, Client};
use clap::Subcommand;
use reqwest::Method;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum ProcessCommand {
    /// Read the complete process configuration.
    Config { name: String },
    /// Apply configuration JSON, including name, command, arguments and policies.
    Update {
        name: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        expected: Option<PathBuf>,
    },
}
#[derive(Subcommand)]
pub enum RuntimeCommand {
    /// Discover every matching executable on PATH, including installed versions.
    List { name: String },
    /// Fetch supported releases from the runtime's official distributor.
    Versions {
        name: String,
        #[arg(long)]
        all: bool,
    },
    /// Install a verified runtime archive into an empty directory.
    Install {
        name: String,
        #[arg(long)]
        version: String,
        #[arg(long)]
        directory: PathBuf,
    },
    /// Read and cache command or script help. Pass script arguments with --arg.
    Help {
        program: String,
        #[arg(long = "arg", allow_hyphen_values = true)]
        args: Vec<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
}
#[derive(Subcommand)]
pub enum WorkflowCommand {
    /// List recent executions with their recorded flow version and step path.
    History {
        name: String,
    },
    /// Read a saved flow or one of its immutable versions.
    Get {
        name: String,
        #[arg(long)]
        revision: Option<u64>,
    },
    /// Save a flow using an optimistic version check.
    Save {
        name: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        expected_revision: u64,
    },
    Versions {
        name: String,
    },
    /// Compare steps, connections and their settings between two versions.
    Diff {
        name: String,
        #[arg(long)]
        from: u64,
        #[arg(long)]
        to: u64,
    },
    /// Run a saved flow or draft. Only this job runs unless --with-dependents is set.
    Run {
        name: String,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        step: Option<String>,
        #[arg(long)]
        with_dependents: bool,
        #[arg(long)]
        wait: bool,
    },
    /// Inspect the exact flow, step states, results and logs of a run.
    Result {
        name: String,
        run_id: String,
    },
    ResetCircuit {
        name: String,
        step: String,
    },
    /// Export a standalone Bash/Zsh script. Environment values are excluded by default.
    Export {
        name: String,
        #[arg(long,default_value="bash",value_parser=["bash","zsh"])]
        shell: String,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        include_environment: bool,
    },
}
fn encode(value: &str) -> String {
    let mut url = reqwest::Url::parse("http://localhost").unwrap();
    url.path_segments_mut().unwrap().push(value);
    url.path().trim_start_matches('/').to_string()
}
fn read(path: &Path) -> Result<Value, CliError> {
    serde_json::from_slice(&std::fs::read(path).map_err(|e| CliError::Failed(e.to_string()))?)
        .map_err(|e| CliError::Failed(e.to_string()))
}
pub async fn process(client: &Client, command: &ProcessCommand) -> Result<Value, CliError> {
    match command {
        ProcessCommand::Config{name}=>client.workspace_request(Method::GET,&format!("/api/v1/processes/{}/config",encode(name)),None).await,
        ProcessCommand::Update{name,file,expected}=>client.workspace_request(Method::PATCH,&format!("/api/v1/processes/{}/config",encode(name)),Some(json!({"config":read(file)?,"expected":expected.as_deref().map(read).transpose()?}))).await
    }
}
pub async fn runtime(client: &Client, command: &RuntimeCommand) -> Result<Value, CliError> {
    let request = match command {
        RuntimeCommand::List { name } => json!({"operation":"executables","name":name}),
        RuntimeCommand::Versions { name, all } => {
            json!({"operation":"versions","name":name,"all":all})
        }
        RuntimeCommand::Install {
            name,
            version,
            directory,
        } => json!({"operation":"install","name":name,"version":version,"directory":directory}),
        RuntimeCommand::Help { program, args, cwd } => {
            json!({"operation":"help","program":program,"args":args,"cwd":cwd})
        }
    };
    client
        .workspace_request(Method::POST, "/api/v1/tools", Some(request))
        .await
}
pub async fn workflow(client: &Client, command: &WorkflowCommand) -> Result<Value, CliError> {
    let name = match command {
        WorkflowCommand::Get { name, .. }
        | WorkflowCommand::History { name }
        | WorkflowCommand::Save { name, .. }
        | WorkflowCommand::Versions { name }
        | WorkflowCommand::Diff { name, .. }
        | WorkflowCommand::Run { name, .. }
        | WorkflowCommand::Result { name, .. }
        | WorkflowCommand::ResetCircuit { name, .. }
        | WorkflowCommand::Export { name, .. } => name,
    };
    let base = format!("/api/v1/jobs/{}/workflow", encode(name));
    match command {
        WorkflowCommand::Get { revision, .. } => {
            client
                .workspace_request(
                    Method::GET,
                    &format!(
                        "{base}{}",
                        revision
                            .map(|v| format!("?revision={v}"))
                            .unwrap_or_default()
                    ),
                    None,
                )
                .await
        }
        WorkflowCommand::Save {
            file,
            expected_revision,
            ..
        } => {
            let value = read(file)?;
            let definition = value.get("definition").cloned().unwrap_or(value);
            client
                .workspace_request(
                    Method::PUT,
                    &base,
                    Some(json!({"definition":definition,"expected_revision":expected_revision})),
                )
                .await
        }
        WorkflowCommand::Versions { .. } => {
            client
                .workspace_request(Method::GET, &format!("{base}/versions"), None)
                .await
        }
        WorkflowCommand::History { .. } => {
            client
                .workspace_request(Method::GET, &format!("{base}/history"), None)
                .await
        }
        WorkflowCommand::Diff { from, to, .. } => {
            let a = client
                .workspace_request(Method::GET, &format!("{base}?revision={from}"), None)
                .await?;
            let b = client
                .workspace_request(Method::GET, &format!("{base}?revision={to}"), None)
                .await?;
            Ok(
                json!({"from":from,"to":to,"steps":diff(&a["definition"]["nodes"],&b["definition"]["nodes"]),"connections":diff(&a["definition"]["edges"],&b["definition"]["edges"])}),
            )
        }
        WorkflowCommand::Result { run_id, .. } => {
            client
                .workspace_request(
                    Method::GET,
                    &format!("{base}/runs/{}", encode(run_id)),
                    None,
                )
                .await
        }
        WorkflowCommand::ResetCircuit { step, .. } => {
            client
                .workspace_request(
                    Method::POST,
                    &format!("{base}/circuits/{}/reset", encode(step)),
                    None,
                )
                .await
        }
        WorkflowCommand::Export {
            shell,
            file,
            include_environment,
            ..
        } => {
            let value = client
                .workspace_request(
                    Method::POST,
                    &format!("{base}/export"),
                    Some(json!({"shell":shell,"include_environment":include_environment})),
                )
                .await?;
            if let Some(path) = file {
                let mut output = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map_err(|e| CliError::Failed(e.to_string()))?;
                std::io::Write::write_all(
                    &mut output,
                    value["script"].as_str().unwrap_or("").as_bytes(),
                )
                .map_err(|e| CliError::Failed(e.to_string()))?;
                Ok(json!({"path":path,"shell":shell}))
            } else {
                Ok(value)
            }
        }
        WorkflowCommand::Run {
            file,
            step,
            with_dependents,
            wait,
            ..
        } => {
            let value = file.as_deref().map(read).transpose()?;
            let definition = value.map(|v| v.get("definition").cloned().unwrap_or(v));
            let started=client.workspace_request(Method::POST,&format!("{base}/run"),Some(json!({"definition":definition,"only_node":step,"single_job":!with_dependents}))).await?;
            if !wait {
                return Ok(started);
            }
            let id = started["run_id"]
                .as_str()
                .ok_or_else(|| CliError::Failed("missing run ID".into()))?;
            loop {
                let result = client
                    .workspace_request(Method::GET, &format!("{base}/runs/{id}"), None)
                    .await?;
                let state = result["state"].as_str().unwrap_or("pending");
                if !["pending", "running"].contains(&state) {
                    if state != "succeeded" {
                        return Err(CliError::Failed(format!(
                            "workflow {id} ended as {state}: {}",
                            result["summary"]
                        )));
                    }
                    return Ok(result);
                }
                tokio::select! {_=tokio::time::sleep(std::time::Duration::from_millis(500))=>{},_=tokio::signal::ctrl_c()=>{client.cancel_run(name,id).await?;return Err(CliError::Failed(format!("cancellation requested for {id}")));}}
            }
        }
    }
}
fn diff(before: &Value, after: &Value) -> Value {
    let a = before.as_array().cloned().unwrap_or_default();
    let b = after.as_array().cloned().unwrap_or_default();
    let mut result = Vec::new();
    for old in &a {
        let next = b.iter().find(|v| v["id"] == old["id"]);
        if next != Some(old) {
            result.push(json!({"id":old["id"],"kind":if next.is_some(){"modified"}else{"removed"},"before":old,"after":next}));
        }
    }
    for next in &b {
        if !a.iter().any(|v| v["id"] == next["id"]) {
            result.push(json!({"id":next["id"],"kind":"added","after":next}));
        }
    }
    json!(result)
}
