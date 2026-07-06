//! Thin HTTP/WS client over the operations API. Reuses `shared` DTOs so a
//! contract change breaks compilation, and maps the §5 error envelope onto the
//! documented exit-code convention (`0` ok, `1` general, `2` no such process,
//! `3` daemon not running).

use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::Value;

use my_supervisor_shared::api::{
    DaemonStatusDto, JobConfigDto, JobListDto, JobRunListDto, LogsResponseDto, ProcessConfigDto,
    ProcessListDto,
};
use my_supervisor_shared::error::ErrorBody;

/// Error carrying the process exit code the CLI should terminate with.
#[derive(Debug)]
pub enum CliError {
    /// `process_not_found` — exit 2.
    NotFound(String),
    /// Connection refused / daemon unreachable — exit 3.
    DaemonDown,
    /// Any other API error or local failure — exit 1.
    Failed(String),
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::NotFound(_) => 2,
            CliError::DaemonDown => 3,
            CliError::Failed(_) => 1,
        }
    }

    pub fn message(&self) -> String {
        match self {
            CliError::NotFound(m) => m.clone(),
            CliError::DaemonDown => {
                "daemon not running (connection refused at the configured URL)".to_string()
            }
            CliError::Failed(m) => m.clone(),
        }
    }
}

pub struct Client {
    base: String,
    http: reqwest::Client,
}

impl Client {
    pub fn new(base: impl Into<String>) -> Self {
        Client {
            base: base.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<reqwest::Response, CliError> {
        let url = format!("{}{path}", self.base);
        let mut req = self.http.request(method, &url);
        if let Some(body) = body {
            req = req.json(&body);
        }
        let resp = req.send().await.map_err(|e| {
            if e.is_connect() {
                CliError::DaemonDown
            } else {
                CliError::Failed(e.to_string())
            }
        })?;

        if resp.status().is_success() {
            return Ok(resp);
        }

        // Map the §5 error envelope onto an exit code.
        let status = resp.status();
        let envelope = resp.json::<ErrorBody>().await.ok();
        match envelope {
            Some(b) if b.error.code == "process_not_found" => {
                Err(CliError::NotFound(b.error.message))
            }
            Some(b) => Err(CliError::Failed(format!(
                "{} ({})",
                b.error.message, b.error.code
            ))),
            None => Err(CliError::Failed(format!("request failed: HTTP {status}"))),
        }
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, CliError> {
        let resp = self.send(Method::GET, path, None).await?;
        resp.json::<T>()
            .await
            .map_err(|e| CliError::Failed(format!("decoding response: {e}")))
    }

    pub async fn list_processes(&self) -> Result<ProcessListDto, CliError> {
        self.get_json("/api/v1/processes").await
    }

    pub async fn process_action(&self, name: &str, action: &str) -> Result<(), CliError> {
        self.send(
            Method::POST,
            &format!("/api/v1/processes/{name}/{action}"),
            None,
        )
        .await
        .map(|_| ())
    }

    pub async fn stop(&self, name: &str, force: bool) -> Result<(), CliError> {
        let q = if force { "?force=true" } else { "" };
        self.send(
            Method::POST,
            &format!("/api/v1/processes/{name}/stop{q}"),
            None,
        )
        .await
        .map(|_| ())
    }

    pub async fn remove(&self, name: &str, force: bool) -> Result<(), CliError> {
        let q = if force { "?force=true" } else { "" };
        self.send(
            Method::DELETE,
            &format!("/api/v1/processes/{name}{q}"),
            None,
        )
        .await
        .map(|_| ())
    }

    pub async fn add_process(&self, dto: &ProcessConfigDto) -> Result<(), CliError> {
        let body = serde_json::to_value(dto).map_err(|e| CliError::Failed(e.to_string()))?;
        self.send(Method::POST, "/api/v1/processes", Some(body))
            .await
            .map(|_| ())
    }

    pub async fn add_job(&self, dto: &JobConfigDto) -> Result<(), CliError> {
        let body = serde_json::to_value(dto).map_err(|e| CliError::Failed(e.to_string()))?;
        self.send(Method::POST, "/api/v1/jobs", Some(body))
            .await
            .map(|_| ())
    }

    pub async fn process_logs(&self, name: &str, tail: usize) -> Result<LogsResponseDto, CliError> {
        self.get_json(&format!("/api/v1/processes/{name}/logs?tail={tail}"))
            .await
    }

    pub async fn reload(&self) -> Result<(), CliError> {
        self.send(Method::POST, "/api/v1/daemon/reload", None)
            .await
            .map(|_| ())
    }

    pub async fn daemon_status(&self) -> Result<DaemonStatusDto, CliError> {
        self.get_json("/api/v1/daemon/status").await
    }

    pub async fn list_jobs(&self) -> Result<JobListDto, CliError> {
        self.get_json("/api/v1/jobs").await
    }

    pub async fn trigger_job(&self, name: &str) -> Result<String, CliError> {
        let resp = self
            .send(Method::POST, &format!("/api/v1/jobs/{name}/trigger"), None)
            .await?;
        let value = resp
            .json::<Value>()
            .await
            .map_err(|e| CliError::Failed(e.to_string()))?;
        Ok(value
            .get("run_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string())
    }

    pub async fn list_runs(&self, name: &str, limit: usize) -> Result<JobRunListDto, CliError> {
        self.get_json(&format!("/api/v1/jobs/{name}/runs?limit={limit}"))
            .await
    }

    /// WebSocket base (`ws://…`) derived from the HTTP base URL.
    pub fn ws_base(&self) -> String {
        if let Some(rest) = self.base.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = self.base.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            self.base.clone()
        }
    }
}
