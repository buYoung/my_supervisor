//! Thin HTTP/WS client over the operations API. Reuses `shared` DTOs so a
//! contract change breaks compilation, and maps the §5 error envelope onto the
//! documented exit-code convention (`0` ok, `1` general, `2` no such process,
//! `3` daemon not running).

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::Value;

use chrono::{DateTime, Utc};
use my_supervisor_app_daemon::{
    debug_or_canonical_root, discover_owner, load_control_token, DEFAULT_BASE_URL,
};
use my_supervisor_shared::api::{
    AlertEpisodeDto, BackupResultDto, ConfigApplyResultDto, ConvertRequestDto, DaemonStatusDto,
    DeliveryAttemptDto, JobListDto, JobPreviewDto, JobPreviewRequestDto, JobRunListDto,
    JobStatusDto, LogsResponseDto, MetricSampleDto, ObservabilityPageDto, OperatorEventDto,
    ProcessInstancesDto, ProcessListDto, ProcessOperationDto, ProcessStatusDto,
    RecoveryDiagnosticsDto, RestartNoopDto, RollingRestartRequestDto, ScaleProcessRequestDto,
    TokenRotationDto, UpgradeJournalDto,
};
use my_supervisor_shared::config::ConfigApplyRequestDto;
use my_supervisor_shared::error::ErrorBody;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::http::Request;

/// Error carrying the process exit code the CLI should terminate with.
#[derive(Debug)]
pub enum CliError {
    /// `process_not_found` — exit 2.
    NotFound(String),
    /// Connection refused / daemon unreachable — exit 3.
    DaemonDown,
    Partial(String),
    /// Any other API error or local failure — exit 1.
    Failed(String),
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::Client;

    #[test]
    fn resource_names_are_encoded_as_one_path_segment() {
        assert_eq!(
            Client::encode_path_segment("name with/slash"),
            "name%20with%2Fslash"
        );
    }

    #[test]
    fn unsupported_url_scheme_is_rejected() {
        assert!(Client::new("ftp://localhost").is_err());
    }
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::NotFound(_) => 2,
            CliError::DaemonDown => 3,
            CliError::Partial(_) => 3,
            CliError::Failed(_) => 1,
        }
    }

    pub fn message(&self) -> String {
        match self {
            CliError::NotFound(m) => m.clone(),
            CliError::DaemonDown => {
                "daemon not running (connection refused at the configured URL)".to_string()
            }
            CliError::Partial(m) => m.clone(),
            CliError::Failed(m) => m.clone(),
        }
    }
}

pub struct Client {
    base: String,
    credential_root: PathBuf,
    bearer_token: Arc<RwLock<String>>,
    http: reqwest::Client,
}

impl Client {
    pub fn new(base: impl Into<String>) -> Result<Self, CliError> {
        let base = base.into();
        let parsed = reqwest::Url::parse(&base)
            .map_err(|error| CliError::Failed(format!("invalid --url: {error}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(CliError::Failed(
                "invalid --url: only http and https are supported".into(),
            ));
        }
        let credential_root = debug_or_canonical_root().map_err(|error| {
            CliError::Failed(format!("discovering daemon credentials: {error}"))
        })?;
        let base = if base.trim_end_matches('/') == DEFAULT_BASE_URL {
            discover_owner(credential_root.clone())
                .map(|owner| owner.endpoint)
                .unwrap_or(base)
        } else {
            base
        };
        let bearer_token = load_control_token(credential_root.clone())
            .map_err(|error| CliError::Failed(format!("reading daemon credentials: {error}")))?;
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(3))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|error| CliError::Failed(format!("building HTTP client: {error}")))?;
        Ok(Client {
            base: base.trim_end_matches('/').to_string(),
            credential_root,
            bearer_token: Arc::new(RwLock::new(bearer_token)),
            http,
        })
    }

    fn encode_path_segment(value: &str) -> String {
        let mut url = reqwest::Url::parse("http://localhost/").expect("static URL is valid");
        url.path_segments_mut()
            .expect("static URL accepts path segments")
            .push(value);
        url.path().trim_start_matches('/').to_string()
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<reqwest::Response, CliError> {
        let url = format!("{}{path}", self.base);
        let mut req = self
            .http
            .request(method, &url)
            .bearer_auth(self.bearer_token());
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
            Some(b) if b.error.code.ends_with("_not_found") => {
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

    async fn post_operation<T: DeserializeOwned, R: serde::Serialize>(
        &self,
        path: &str,
        request: &R,
        operation_id: Option<uuid::Uuid>,
    ) -> Result<T, CliError> {
        let url = format!("{}{path}", self.base);
        let mut builder = self
            .http
            .post(url)
            .bearer_auth(self.bearer_token())
            .json(request);
        if let Some(operation_id) = operation_id {
            builder = builder.header("Idempotency-Key", operation_id.to_string());
        }
        let response = builder.send().await.map_err(|error| {
            if error.is_connect() {
                CliError::DaemonDown
            } else {
                CliError::Failed(error.to_string())
            }
        })?;
        if !response.status().is_success() {
            let status = response.status();
            let envelope = response.json::<ErrorBody>().await.ok();
            return match envelope {
                Some(body) if body.error.code.ends_with("_not_found") => {
                    Err(CliError::NotFound(body.error.message))
                }
                Some(body) => Err(CliError::Failed(format!(
                    "{} ({})",
                    body.error.message, body.error.code
                ))),
                None => Err(CliError::Failed(format!("request failed: HTTP {status}"))),
            };
        }
        response
            .json()
            .await
            .map_err(|error| CliError::Failed(format!("decoding response: {error}")))
    }

    pub async fn list_processes(&self) -> Result<ProcessListDto, CliError> {
        self.get_json("/api/v1/processes").await
    }

    pub async fn observability_events(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ObservabilityPageDto<OperatorEventDto>, CliError> {
        self.get_json(&format!(
            "/api/v1/observability/events?limit={limit}{}",
            cursor
                .map(|v| format!("&cursor={}", v.replace('|', "%7C")))
                .unwrap_or_default()
        ))
        .await
    }
    pub async fn observability_metrics(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ObservabilityPageDto<MetricSampleDto>, CliError> {
        self.get_json(&format!(
            "/api/v1/observability/metrics?limit={limit}{}",
            cursor
                .map(|v| format!("&cursor={}", v.replace('|', "%7C")))
                .unwrap_or_default()
        ))
        .await
    }
    pub async fn observability_alerts(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ObservabilityPageDto<AlertEpisodeDto>, CliError> {
        self.get_json(&format!(
            "/api/v1/observability/alerts?limit={limit}{}",
            cursor
                .map(|v| format!("&cursor={}", v.replace('|', "%7C")))
                .unwrap_or_default()
        ))
        .await
    }
    pub async fn observability_deliveries(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ObservabilityPageDto<DeliveryAttemptDto>, CliError> {
        self.get_json(&format!(
            "/api/v1/observability/deliveries?limit={limit}{}",
            cursor
                .map(|v| format!("&cursor={}", v.replace('|', "%7C")))
                .unwrap_or_default()
        ))
        .await
    }

    pub async fn get_process(&self, name: &str) -> Result<ProcessStatusDto, CliError> {
        let name = Self::encode_path_segment(name);
        self.get_json(&format!("/api/v1/processes/{name}")).await
    }

    pub async fn process_instances(&self, name: &str) -> Result<ProcessInstancesDto, CliError> {
        let name = Self::encode_path_segment(name);
        self.get_json(&format!("/api/v1/processes/{name}/instances"))
            .await
    }

    pub async fn scale_process(
        &self,
        name: &str,
        instances: u16,
        operation_id: Option<uuid::Uuid>,
    ) -> Result<ProcessOperationDto, CliError> {
        let name = Self::encode_path_segment(name);
        self.post_operation(
            &format!("/api/v1/processes/{name}/scale"),
            &ScaleProcessRequestDto {
                instances,
                operation_id,
            },
            operation_id,
        )
        .await
    }

    pub async fn rolling_restart_process(
        &self,
        name: &str,
        operation_id: Option<uuid::Uuid>,
    ) -> Result<ProcessOperationDto, CliError> {
        let name = Self::encode_path_segment(name);
        self.post_operation(
            &format!("/api/v1/processes/{name}/rolling-restart"),
            &RollingRestartRequestDto { operation_id },
            operation_id,
        )
        .await
    }

    pub async fn process_action(&self, name: &str, action: &str) -> Result<(), CliError> {
        let name = Self::encode_path_segment(name);
        self.send(
            Method::POST,
            &format!("/api/v1/processes/{name}/{action}"),
            None,
        )
        .await
        .map(|_| ())
    }

    pub async fn stop(&self, name: &str, force: bool) -> Result<(), CliError> {
        let name = Self::encode_path_segment(name);
        let q = if force { "?force=true" } else { "" };
        self.send(
            Method::POST,
            &format!("/api/v1/processes/{name}/stop{q}"),
            None,
        )
        .await
        .map(|_| ())
    }

    pub async fn restart(&self, name: &str) -> Result<Option<RestartNoopDto>, CliError> {
        let name = Self::encode_path_segment(name);
        let response = self
            .send(
                Method::POST,
                &format!("/api/v1/processes/{name}/restart"),
                None,
            )
            .await?;
        let body = response
            .bytes()
            .await
            .map_err(|error| CliError::Failed(format!("reading response: {error}")))?;
        if body.is_empty() {
            return Ok(None);
        }
        serde_json::from_slice(&body)
            .map(Some)
            .map_err(|error| CliError::Failed(format!("decoding response: {error}")))
    }

    pub async fn remove(&self, name: &str, force: bool) -> Result<(), CliError> {
        let name = Self::encode_path_segment(name);
        let q = if force { "?force=true" } else { "" };
        self.send(
            Method::DELETE,
            &format!("/api/v1/processes/{name}{q}"),
            None,
        )
        .await
        .map(|_| ())
    }

    pub async fn remove_job(&self, name: &str, force: bool) -> Result<(), CliError> {
        let name = Self::encode_path_segment(name);
        let query = if force { "?force=true" } else { "" };
        self.send(Method::DELETE, &format!("/api/v1/jobs/{name}{query}"), None)
            .await
            .map(|_| ())
    }

    pub async fn process_logs(
        &self,
        name: &str,
        tail: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
    ) -> Result<LogsResponseDto, CliError> {
        let name = Self::encode_path_segment(name);
        self.get_json(&log_path(
            &format!("/api/v1/processes/{name}/logs"),
            tail,
            since,
            after_sequence,
        ))
        .await
    }

    pub fn process_log_websocket_url(
        &self,
        name: &str,
        after_sequence: u64,
    ) -> Result<String, CliError> {
        let name = Self::encode_path_segment(name);
        self.websocket_url(&log_path(
            &format!("/api/v1/processes/{name}/logs"),
            0,
            None,
            Some(after_sequence),
        ))
    }

    pub async fn reload(&self) -> Result<(), CliError> {
        self.send(Method::POST, "/api/v1/daemon/reload", None)
            .await
            .map(|_| ())
    }

    pub async fn validate_config(
        &self,
        request: &ConfigApplyRequestDto,
    ) -> Result<ConfigApplyResultDto, CliError> {
        self.config_request("validate", request).await
    }

    pub async fn apply_config(
        &self,
        request: &ConfigApplyRequestDto,
    ) -> Result<ConfigApplyResultDto, CliError> {
        self.config_request("apply", request).await
    }

    async fn config_request(
        &self,
        action: &str,
        request: &ConfigApplyRequestDto,
    ) -> Result<ConfigApplyResultDto, CliError> {
        let body =
            serde_json::to_value(request).map_err(|error| CliError::Failed(error.to_string()))?;
        let response = self
            .send(
                Method::POST,
                &format!("/api/v1/daemon/config/{action}"),
                Some(body),
            )
            .await?;
        response
            .json::<ConfigApplyResultDto>()
            .await
            .map_err(|error| CliError::Failed(format!("decoding response: {error}")))
    }

    pub async fn shutdown(&self) -> Result<(), CliError> {
        self.send(Method::POST, "/api/v1/daemon/shutdown", None)
            .await
            .map(|_| ())
    }

    /// Refreshes only the native secret after a successful public rotation;
    /// the control token never crosses a URL or renderer boundary.
    pub fn refresh_credentials(&self) -> Result<(), CliError> {
        let token = load_control_token(self.credential_root.clone())
            .map_err(|error| CliError::Failed(format!("refreshing daemon credentials: {error}")))?;
        *self
            .bearer_token
            .write()
            .expect("CLI credential lock poisoned") = token;
        Ok(())
    }

    pub async fn rotate_token(&self) -> Result<TokenRotationDto, CliError> {
        let response = self
            .send(Method::POST, "/api/v1/service/rotate-token", None)
            .await?;
        let result = response.json::<TokenRotationDto>().await.map_err(|error| {
            CliError::Failed(format!("decoding token rotation response: {error}"))
        })?;
        self.refresh_credentials()?;
        Ok(result)
    }

    pub async fn backup(&self) -> Result<BackupResultDto, CliError> {
        let response = self
            .send(Method::POST, "/api/v1/service/backup", None)
            .await?;
        response
            .json()
            .await
            .map_err(|error| CliError::Failed(format!("decoding backup response: {error}")))
    }

    pub async fn upgrade(&self) -> Result<UpgradeJournalDto, CliError> {
        let response = self
            .send(Method::POST, "/api/v1/service/upgrade", None)
            .await?;
        response
            .json()
            .await
            .map_err(|error| CliError::Failed(format!("decoding upgrade response: {error}")))
    }

    pub async fn rollback(&self) -> Result<UpgradeJournalDto, CliError> {
        let response = self
            .send(Method::POST, "/api/v1/service/rollback", None)
            .await?;
        response
            .json()
            .await
            .map_err(|error| CliError::Failed(format!("decoding rollback response: {error}")))
    }

    pub async fn daemon_status(&self) -> Result<DaemonStatusDto, CliError> {
        self.get_json("/api/v1/daemon/status").await
    }

    pub async fn recovery_diagnostics(&self) -> Result<RecoveryDiagnosticsDto, CliError> {
        self.get_json("/api/v1/daemon/recovery").await
    }

    pub async fn list_jobs(&self) -> Result<JobListDto, CliError> {
        self.get_json("/api/v1/jobs").await
    }

    pub async fn preview_job(
        &self,
        request: &JobPreviewRequestDto,
    ) -> Result<JobPreviewDto, CliError> {
        let body =
            serde_json::to_value(request).map_err(|error| CliError::Failed(error.to_string()))?;
        let response = self
            .send(Method::POST, "/api/v1/jobs/preview", Some(body))
            .await?;
        response
            .json::<JobPreviewDto>()
            .await
            .map_err(|error| CliError::Failed(format!("decoding response: {error}")))
    }

    pub async fn get_job(&self, name: &str) -> Result<JobStatusDto, CliError> {
        let name = Self::encode_path_segment(name);
        self.get_json(&format!("/api/v1/jobs/{name}")).await
    }

    pub async fn trigger_job(&self, name: &str) -> Result<String, CliError> {
        let name = Self::encode_path_segment(name);
        let resp = self
            .send(Method::POST, &format!("/api/v1/jobs/{name}/trigger"), None)
            .await?;
        let value = resp
            .json::<Value>()
            .await
            .map_err(|e| CliError::Failed(e.to_string()))?;
        value
            .get("run_id")
            .and_then(|v| v.as_str())
            .filter(|run_id| !run_id.is_empty())
            .map(str::to_string)
            .ok_or_else(|| CliError::Failed("response did not contain run_id".into()))
    }

    pub async fn list_runs(&self, name: &str, limit: usize) -> Result<JobRunListDto, CliError> {
        let name = Self::encode_path_segment(name);
        self.get_json(&format!("/api/v1/jobs/{name}/runs?limit={limit}"))
            .await
    }

    pub async fn run_logs(
        &self,
        name: &str,
        run_id: &str,
        tail: usize,
        since: Option<DateTime<Utc>>,
        after_sequence: Option<u64>,
    ) -> Result<LogsResponseDto, CliError> {
        let name = Self::encode_path_segment(name);
        let run_id = Self::encode_path_segment(run_id);
        self.get_json(&log_path(
            &format!("/api/v1/jobs/{name}/runs/{run_id}/logs"),
            tail,
            since,
            after_sequence,
        ))
        .await
    }

    pub fn run_log_websocket_url(
        &self,
        name: &str,
        run_id: &str,
        after_sequence: u64,
    ) -> Result<String, CliError> {
        let name = Self::encode_path_segment(name);
        let run_id = Self::encode_path_segment(run_id);
        self.websocket_url(&log_path(
            &format!("/api/v1/jobs/{name}/runs/{run_id}/logs"),
            0,
            None,
            Some(after_sequence),
        ))
    }

    /// Global event transport URL. The event stream is intentionally separate
    /// from the REST API because terminal events are delivered live and may be
    /// replayed with the same stable `event_id` after a transport failure.
    pub fn events_websocket_url(&self) -> Result<String, CliError> {
        self.websocket_url("/api/v1/events")
    }

    pub fn websocket_request(&self, url: &str) -> Result<Request<()>, CliError> {
        let mut request = url.into_client_request().map_err(|error| {
            CliError::Failed(format!("building WebSocket handshake request: {error}"))
        })?;
        let authorization = format!("Bearer {}", self.bearer_token())
            .parse()
            .map_err(|error| {
                CliError::Failed(format!("building WebSocket authorization header: {error}"))
            })?;
        request.headers_mut().insert(AUTHORIZATION, authorization);
        Ok(request)
    }

    pub async fn cancel_run(&self, name: &str, run_id: &str) -> Result<(), CliError> {
        let name = Self::encode_path_segment(name);
        let run_id = Self::encode_path_segment(run_id);
        self.send(
            Method::POST,
            &format!("/api/v1/jobs/{name}/runs/{run_id}/cancel"),
            None,
        )
        .await
        .map(|_| ())
    }

    fn websocket_url(&self, path: &str) -> Result<String, CliError> {
        let mut url = reqwest::Url::parse(&format!("{}{path}", self.base))
            .map_err(|error| CliError::Failed(format!("building WebSocket URL: {error}")))?;
        let scheme = match url.scheme() {
            "http" => "ws",
            "https" => "wss",
            _ => return Err(CliError::Failed("unsupported WebSocket URL scheme".into())),
        };
        url.set_scheme(scheme)
            .map_err(|_| CliError::Failed("building WebSocket URL".into()))?;
        Ok(url.to_string())
    }

    fn bearer_token(&self) -> String {
        self.bearer_token
            .read()
            .expect("CLI credential lock poisoned")
            .clone()
    }

    pub async fn convert_process(
        &self,
        name: &str,
        request: &ConvertRequestDto,
    ) -> Result<ProcessStatusDto, CliError> {
        let name = Self::encode_path_segment(name);
        let body =
            serde_json::to_value(request).map_err(|error| CliError::Failed(error.to_string()))?;
        let response = self
            .send(
                Method::POST,
                &format!("/api/v1/processes/{name}/convert"),
                Some(body),
            )
            .await?;
        response
            .json::<ProcessStatusDto>()
            .await
            .map_err(|error| CliError::Failed(format!("decoding response: {error}")))
    }
}

fn log_path(
    path: &str,
    tail: usize,
    since: Option<DateTime<Utc>>,
    after_sequence: Option<u64>,
) -> String {
    let mut query = vec![format!("tail={tail}")];
    if let Some(since) = since {
        query.push(format!("since={}", since.to_rfc3339()));
    }
    if let Some(after_sequence) = after_sequence {
        query.push(format!("after_sequence={after_sequence}"));
    }
    format!("{path}?{}", query.join("&"))
}
