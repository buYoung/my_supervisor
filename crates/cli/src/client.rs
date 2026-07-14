//! Thin HTTP/WS client over the operations API. Reuses `shared` DTOs so a
//! contract change breaks compilation, and maps the §5 error envelope onto the
//! documented exit-code convention (`0` ok, `1` general, `2` no such process,
//! `3` daemon not running).

use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::Value;

use my_supervisor_shared::api::{
    ConvertRequestDto, DaemonStatusDto, JobConfigDto, JobListDto, JobRunListDto, JobStatusDto,
    LogsResponseDto, ProcessConfigDto, ProcessListDto, ProcessStatusDto, RestartNoopDto,
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

#[cfg(test)]
mod tests {
    use super::Client;

    #[test]
    fn resource_names_are_encoded_as_one_path_segment() {
        assert_eq!(Client::encode_path_segment("name with/slash"), "name%20with%2Fslash");
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
    pub fn new(base: impl Into<String>) -> Result<Self, CliError> {
        let base = base.into();
        let parsed = reqwest::Url::parse(&base)
            .map_err(|error| CliError::Failed(format!("invalid --url: {error}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(CliError::Failed(
                "invalid --url: only http and https are supported".into(),
            ));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(3))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|error| CliError::Failed(format!("building HTTP client: {error}")))?;
        Ok(Client {
            base: base.trim_end_matches('/').to_string(),
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

    pub async fn list_processes(&self) -> Result<ProcessListDto, CliError> {
        self.get_json("/api/v1/processes").await
    }

    pub async fn get_process(&self, name: &str) -> Result<ProcessStatusDto, CliError> {
        let name = Self::encode_path_segment(name);
        self.get_json(&format!("/api/v1/processes/{name}")).await
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

    pub async fn remove_job(&self, name: &str, force: bool) -> Result<(), CliError> {
        let name = Self::encode_path_segment(name);
        let query = if force { "?force=true" } else { "" };
        self.send(Method::DELETE, &format!("/api/v1/jobs/{name}{query}"), None)
            .await
            .map(|_| ())
    }

    pub async fn process_logs(&self, name: &str, tail: usize) -> Result<LogsResponseDto, CliError> {
        let name = Self::encode_path_segment(name);
        self.get_json(&format!("/api/v1/processes/{name}/logs?tail={tail}"))
            .await
    }

    pub async fn reload(&self) -> Result<(), CliError> {
        self.send(Method::POST, "/api/v1/daemon/reload", None)
            .await
            .map(|_| ())
    }

    pub async fn shutdown(&self) -> Result<(), CliError> {
        self.send(Method::POST, "/api/v1/daemon/shutdown", None)
            .await
            .map(|_| ())
    }

    pub async fn daemon_status(&self) -> Result<DaemonStatusDto, CliError> {
        self.get_json("/api/v1/daemon/status").await
    }

    pub async fn list_jobs(&self) -> Result<JobListDto, CliError> {
        self.get_json("/api/v1/jobs").await
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

    pub async fn convert_process(
        &self,
        name: &str,
        request: &ConvertRequestDto,
    ) -> Result<ProcessStatusDto, CliError> {
        let name = Self::encode_path_segment(name);
        let body = serde_json::to_value(request).map_err(|error| CliError::Failed(error.to_string()))?;
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
