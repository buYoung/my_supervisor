use super::GuardError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableCandidate {
    pub name: String,
    pub version: String,
    pub path: String,
    pub is_default: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandHelp {
    pub program: String,
    pub text: String,
    pub supported: bool,
    pub cached: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeRelease {
    pub version: String,
    pub label: String,
    pub url: String,
    pub sha256: String,
}
#[async_trait]
pub trait DeveloperTools: Send + Sync {
    async fn executables(&self, name: &str) -> Result<Vec<ExecutableCandidate>, GuardError>;
    async fn command_help(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
    ) -> Result<CommandHelp, GuardError>;
    async fn runtime_releases(
        &self,
        name: &str,
        all: bool,
    ) -> Result<Vec<RuntimeRelease>, GuardError>;
    async fn install_runtime(
        &self,
        name: &str,
        version: &str,
        directory: &str,
    ) -> Result<ExecutableCandidate, GuardError>;
    async fn read_text_file(&self, path: &str) -> Result<String, GuardError>;
    async fn open_folder(&self, path: &str) -> Result<(), GuardError>;
}
