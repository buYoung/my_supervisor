//! Durable workflow definitions. Execution uses the same lifecycle ownership
//! and cancellation port as ordinary scheduled jobs.
use crate::{AppError, AppResult, OperationsFacade};
use my_supervisor_core::domain::{Job, WorkspaceDocument};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    #[serde(default = "version")]
    pub version: u32,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub group: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Value>,
}
fn version() -> u32 {
    1
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(default = "position")]
    pub position: Value,
    pub data: Value,
}
fn position() -> Value {
    json!({"x":100,"y":100})
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(rename = "sourceHandle", default = "out")]
    pub source_handle: String,
}
fn out() -> String {
    "out".into()
}
pub fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
pub fn number(value: &Value, key: &str, default: f64) -> f64 {
    value
        .get(key)
        .and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(default)
}
pub fn key(job: &Job) -> String {
    format!("workflow/{}", job.id.0)
}
pub fn default_workflow(job: &Job) -> Workflow {
    Workflow {
        version: 1,
        group: None,
        settings: serde_json::to_value(job).ok(),
        nodes: vec![
            Node {
                id: "start".into(),
                position: json!({"x":180,"y":30}),
                data: json!({"kind":"trigger","label":"시작"}),
            },
            Node {
                id: "execute".into(),
                position: json!({"x":180,"y":180}),
                data: json!({"kind":"command","label":"명령 실행","program":job.command,"args":job.args,"cwd":job.cwd,"environment":job.env,"command":command_line(&job.command,&job.args),"maxAttempts":1}),
            },
            Node {
                id: "success".into(),
                position: json!({"x":180,"y":390}),
                data: json!({"kind":"finish","label":"완료","result":"succeeded"}),
            },
        ],
        edges: vec![
            Edge {
                id: "start-execute".into(),
                source: "start".into(),
                target: "execute".into(),
                source_handle: "out".into(),
            },
            Edge {
                id: "execute-success".into(),
                source: "execute".into(),
                target: "success".into(),
                source_handle: "out".into(),
            },
        ],
    }
}
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
pub fn command_line(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(shell_quote)
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn validate(flow: &Workflow) -> AppResult<()> {
    let bad = |s: &str| AppError::InvalidRequest(s.into());
    if flow.version != 1 {
        return Err(bad("unsupported workflow format version"));
    }
    if flow.nodes.is_empty() || flow.nodes.len() > 200 || flow.edges.len() > 500 {
        return Err(bad(
            "workflow supports 1..200 steps and at most 500 connections",
        ));
    }
    let mut ids = HashSet::new();
    let mut starts = 0;
    for node in &flow.nodes {
        if node.id.is_empty() || !ids.insert(node.id.clone()) {
            return Err(bad("step IDs must be unique"));
        }
        let kind = text(&node.data, "kind");
        if ![
            "trigger",
            "command",
            "condition",
            "wait",
            "process",
            "health",
            "job",
            "parallel",
            "join",
            "finish",
        ]
        .contains(&kind)
        {
            return Err(bad("unknown step kind"));
        }
        if kind == "trigger" {
            starts += 1;
        }
        if text(&node.data, "label").trim().is_empty() {
            return Err(bad("each step needs a name"));
        }
        if let Some(env) = node.data.get("environment").filter(|v| !v.is_null()) {
            let env = env
                .as_object()
                .ok_or_else(|| bad("environment must be a string map"))?;
            for (key, value) in env {
                if key.is_empty()
                    || key.contains(['=', '\0'])
                    || value.as_str().is_none_or(|value| value.contains('\0'))
                {
                    return Err(bad("invalid environment name or value"));
                }
            }
        }
        if kind == "command"
            && text(&node.data, "command").trim().is_empty()
            && text(&node.data, "program").is_empty()
        {
            return Err(bad("command step needs a command"));
        }
        if kind == "process"
            && (!matches!(text(&node.data, "operation"), "start" | "stop" | "restart")
                || text(&node.data, "processName").is_empty())
        {
            return Err(bad("choose a process and operation"));
        }
        if number(&node.data, "maxAttempts", 1.0) < 1.0
            || number(&node.data, "maxAttempts", 1.0) > 10.0
        {
            return Err(bad("attempts must be between 1 and 10"));
        }
        if number(&node.data, "timeoutSeconds", 60.0) <= 0.0
            || number(&node.data, "timeoutSeconds", 60.0) > 86400.0
        {
            return Err(bad("step timeout must be 1..86400 seconds"));
        }
        if kind == "wait" && !(0.0..=86400.0).contains(&number(&node.data, "seconds", 1.0)) {
            return Err(bad("invalid wait duration"));
        }
        for (key, min, max) in [
            ("seconds", 0., 86400.),
            ("timeoutSeconds", 1., 86400.),
            ("maxAttempts", 1., 10.),
            ("retryDelaySeconds", 0., 86400.),
            ("retryIncrementSeconds", 0., 86400.),
            ("retryMultiplier", 1., 10.),
            ("retryMaxDelaySeconds", 0., 86400.),
            ("branchCount", 2., 12.),
        ] {
            if let Some(value) = node.data.get(key) {
                let number = value
                    .as_f64()
                    .ok_or_else(|| bad("step policy values must be numbers"))?;
                if !number.is_finite() || number < min || number > max {
                    return Err(bad("step policy is outside supported limits"));
                }
            }
        }
        let mut routes = HashSet::new();
        if let Some(values) = node.data.get("resultRoutes").and_then(Value::as_array) {
            for route in values {
                let id = text(route, "id");
                if id.is_empty() || id == "out" || !routes.insert(id) {
                    return Err(bad("result route IDs must be unique"));
                }
                if !["exit_code", "stdout", "stderr", "outcome"].contains(&text(route, "source"))
                    || ![
                        "equals",
                        "not_equals",
                        "includes",
                        "contains",
                        "not_includes",
                        "gte",
                        "lte",
                    ]
                    .contains(&text(route, "operator"))
                {
                    return Err(bad("unsupported result comparison"));
                }
            }
        }
        if kind == "condition"
            && !["exit_code", "stdout", "stderr", "outcome", "file_exists"]
                .contains(&text(&node.data, "source"))
        {
            return Err(bad("invalid condition source"));
        }
        if kind == "health" {
            match text(&node.data, "checkType") {
                "tcp" => {
                    if text(&node.data, "host").is_empty()
                        || !(1.0..=65535.).contains(&number(&node.data, "port", 0.))
                    {
                        return Err(bad("TCP check needs a host and valid port"));
                    }
                }
                "http" => {
                    if !text(&node.data, "url").starts_with("http://")
                        && !text(&node.data, "url").starts_with("https://")
                    {
                        return Err(bad("HTTP check needs an HTTP(S) URL"));
                    }
                }
                "process" => {}
                _ => return Err(bad("invalid health check type")),
            }
        }
        if let Some(circuit) = node.data.get("circuitBreaker") {
            if circuit["enabled"] == true {
                for (key, max) in [
                    ("failureThreshold", 10000.),
                    ("openSeconds", 86400.),
                    ("recoverySuccesses", 10000.),
                ] {
                    let value = number(circuit, key, 0.);
                    if !value.is_finite() || value < 1. || value > max {
                        return Err(bad("invalid circuit breaker policy"));
                    }
                }
            }
        }
    }
    if starts != 1 {
        return Err(bad("workflow needs exactly one start step"));
    }
    let mut outgoing = HashSet::new();
    let mut incoming = HashMap::<String, usize>::new();
    let mut edge_ids = HashSet::new();
    for edge in &flow.edges {
        if !ids.contains(&edge.source) || !ids.contains(&edge.target) || edge.source == edge.target
        {
            return Err(bad("invalid connection"));
        }
        if edge.id.is_empty() || !edge_ids.insert(&edge.id) {
            return Err(bad("connection IDs must be unique"));
        }
        let source = flow.nodes.iter().find(|n| n.id == edge.source).unwrap();
        let kind = text(&source.data, "kind");
        let port_valid = match kind {
            "finish" => false,
            "condition" => ["true", "false"].contains(&edge.source_handle.as_str()),
            "parallel" => edge
                .source_handle
                .strip_prefix("branch-")
                .and_then(|v| v.parse::<usize>().ok())
                .is_some_and(|n| n > 0 && n <= number(&source.data, "branchCount", 2.) as usize),
            _ => {
                edge.source_handle == "out"
                    || source.data["resultRoutes"].as_array().is_some_and(|rows| {
                        rows.iter().any(|r| text(r, "id") == edge.source_handle)
                    })
            }
        };
        if !port_valid {
            return Err(bad("connection refers to an unknown output port"));
        }
        if flow
            .nodes
            .iter()
            .any(|n| n.id == edge.target && text(&n.data, "kind") == "trigger")
        {
            return Err(bad("connections cannot target the start step"));
        }
        if !outgoing.insert((edge.source.clone(), edge.source_handle.clone())) {
            return Err(bad("each output port can have one connection"));
        }
        *incoming.entry(edge.target.clone()).or_default() += 1;
    }
    let mut pending: Vec<_> = flow
        .nodes
        .iter()
        .filter(|n| !incoming.contains_key(&n.id))
        .map(|n| n.id.clone())
        .collect();
    let mut visited = 0;
    while let Some(id) = pending.pop() {
        visited += 1;
        for edge in flow.edges.iter().filter(|e| e.source == id) {
            let count = incoming.get_mut(&edge.target).unwrap();
            *count -= 1;
            if *count == 0 {
                pending.push(edge.target.clone());
            }
        }
    }
    if visited != flow.nodes.len() {
        return Err(bad("workflow connections must not contain a cycle"));
    }
    Ok(())
}
pub fn document_json(document: WorkspaceDocument) -> AppResult<Value> {
    Ok(
        json!({"revision":document.revision,"saved_at":document.saved_at,"definition":serde_json::from_str::<Value>(&document.value).map_err(|e|AppError::Internal(e.to_string()))?}),
    )
}

impl OperationsFacade {
    pub async fn workflow_history(&self, name: &str, limit: usize) -> AppResult<Vec<Value>> {
        let runs = self.list_runs(name, limit.clamp(1, 100)).await?;
        let mut result = Vec::new();
        for run in runs {
            let snapshot = self
                .workspace_deps()
                .state_repo
                .workspace_state(&format!("workflow-run/{}", run.run_id.0))
                .await?
                .and_then(|value| serde_json::from_str::<Value>(&value).ok());
            let mut steps: Vec<_> = snapshot
                .as_ref()
                .and_then(|v| v["steps"].as_object())
                .map(|items| {
                    items
                        .values()
                        .filter(|v| !matches!(text(v, "state"), "pending" | "skipped"))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            steps.sort_by_key(|v| text(v, "startedAt").to_owned());
            let path: Vec<_> = steps
                .iter()
                .map(|v| json!({"id":v["id"],"label":v["label"],"state":v["state"]}))
                .collect();
            result.push(json!({"id":run.run_id.0,"version":snapshot.as_ref().and_then(|v|v["version"].as_u64()),"state":match run.state {my_supervisor_core::domain::JobRunState::TimedOut=>"timed_out".into(),state=>format!("{state:?}").to_lowercase()},"startedAt":run.started_at,"finishedAt":run.ended_at,"path":path,"hasSnapshot":snapshot.is_some()}));
        }
        Ok(result)
    }
    pub async fn get_workflow(&self, name: &str, revision: Option<u64>) -> AppResult<Value> {
        let view = self.get_job(name).await?;
        match self
            .workspace_deps()
            .state_repo
            .workspace_document(&key(&view.job), revision)
            .await?
        {
            Some(document) => document_json(document),
            None if revision.is_none() => {
                Ok(json!({"revision":0,"saved_at":null,"definition":default_workflow(&view.job)}))
            }
            None => Err(AppError::InvalidRequest(
                "workflow version not found".into(),
            )),
        }
    }
    pub async fn save_workflow(
        &self,
        name: &str,
        definition: Value,
        expected: u64,
    ) -> AppResult<Value> {
        let lock = self.workflow_lock(name);
        let _guard = lock.lock().await;
        let view = self.get_job(name).await?;
        let mut flow: Workflow = serde_json::from_value(definition)
            .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
        flow.settings = serde_json::to_value(&view.job).ok();
        validate(&flow)?;
        let serialized =
            serde_json::to_string(&flow).map_err(|e| AppError::Internal(e.to_string()))?;
        if serialized.len() > 1_048_576 {
            return Err(AppError::InvalidRequest("workflow is too large".into()));
        }
        for node in &flow.nodes {
            if text(&node.data, "kind") == "job" {
                let target = text(&node.data, "jobId");
                if target == name || target == view.job.id.0.to_string() {
                    return Err(AppError::InvalidRequest(
                        "a workflow cannot call itself".into(),
                    ));
                }
                self.get_job(target).await?;
                let mut pending = vec![target.to_owned()];
                let mut visited = HashSet::new();
                while let Some(called) = pending.pop() {
                    if called == name {
                        return Err(AppError::InvalidRequest(
                            "job calls must not form a cycle".into(),
                        ));
                    }
                    if !visited.insert(called.clone()) {
                        continue;
                    }
                    if visited.len() > 200 {
                        return Err(AppError::InvalidRequest("too many referenced jobs".into()));
                    }
                    let saved = self.get_workflow(&called, None).await?;
                    if let Some(nodes) = saved["definition"]["nodes"].as_array() {
                        for node in nodes {
                            if text(&node["data"], "kind") == "job" {
                                pending.push(text(&node["data"], "jobId").to_owned());
                            }
                        }
                    }
                }
            }
            if text(&node.data, "kind") == "process" {
                self.get_process(text(&node.data, "processName")).await?;
            }
        }
        document_json(
            self.workspace_deps()
                .state_repo
                .save_workspace_document(&key(&view.job), &serialized, expected)
                .await?,
        )
    }
    pub async fn workflow_versions(&self, name: &str) -> AppResult<Vec<Value>> {
        let view = self.get_job(name).await?;
        self.workspace_deps()
            .state_repo
            .workspace_versions(&key(&view.job))
            .await?
            .into_iter()
            .map(document_json)
            .collect()
    }
    pub async fn workflow_run(
        &self,
        name: &str,
        id: my_supervisor_core::domain::JobRunId,
    ) -> AppResult<Value> {
        let run = self.get_run(name, &id).await?;
        let value = self
            .workspace_deps()
            .state_repo
            .workspace_state(&format!("workflow-run/{}", id.0))
            .await?;
        let state = match run.state {
            my_supervisor_core::domain::JobRunState::TimedOut => "timed_out".into(),
            other => format!("{other:?}").to_lowercase(),
        };
        match value {
            Some(value) => {
                let mut value: Value =
                    serde_json::from_str(&value).map_err(|e| AppError::Internal(e.to_string()))?;
                if !["pending", "running"].contains(&state.as_str()) {
                    value["state"] = json!(state);
                    value["finishedAt"] = json!(run.ended_at);
                    value["exitCode"] = json!(run.exit_code);
                }
                Ok(value)
            }
            None => Ok(
                json!({"id":id.0,"jobId":name,"version":0,"startedAt":run.started_at,"finishedAt":run.ended_at,"state":state,"steps":{},"edges":[],"workflow":null}),
            ),
        }
    }
    pub(crate) async fn record_job_settings(&self, previous: &Job, job: &Job) -> AppResult<()> {
        let document = self
            .workspace_deps()
            .state_repo
            .workspace_document(&key(job), None)
            .await?;
        let revision = document.as_ref().map(|d| d.revision).unwrap_or(0);
        let mut flow = document
            .map(|d| serde_json::from_str::<Workflow>(&d.value))
            .transpose()
            .map_err(|e| AppError::Internal(e.to_string()))?
            .unwrap_or_else(|| default_workflow(previous));
        flow.settings =
            Some(serde_json::to_value(job).map_err(|e| AppError::Internal(e.to_string()))?);
        for node in &mut flow.nodes {
            if node.id == "execute"
                && text(&node.data, "kind") == "command"
                && text(&node.data, "program") == previous.command
                && node.data["args"] == json!(previous.args)
            {
                node.data["program"] = json!(job.command);
                node.data["args"] = json!(job.args);
                node.data["cwd"] = json!(job.cwd);
                node.data["environment"] = json!(job.env);
                node.data["command"] = json!(command_line(&job.command, &job.args));
            }
        }
        self.workspace_deps()
            .state_repo
            .save_workspace_document(
                &key(job),
                &serde_json::to_string(&flow).map_err(|e| AppError::Internal(e.to_string()))?,
                revision,
            )
            .await?;
        Ok(())
    }
    pub async fn reset_step_circuit(&self, name: &str, step: &str) -> AppResult<()> {
        let view = self.get_job(name).await?;
        self.workspace_deps()
            .state_repo
            .save_workspace_state(&format!("circuit/{}/{step}", view.job.id.0), "{}")
            .await?;
        Ok(())
    }
}
