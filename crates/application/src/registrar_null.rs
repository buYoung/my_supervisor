//! Null `ProcessServiceRegistrar` for the Direct-mode walking skeleton. Every
//! call returns `NotSupported` until child 06 injects the real macOS
//! `LaunchdAgentProcess`. Lives in `application` because it depends only on a
//! `core` port (DD-017 holds).

use async_trait::async_trait;

use my_supervisor_core::domain::{LogLine, ProcessSpec, ProcessState};
use my_supervisor_core::ports::error::RegistrarError;
use my_supervisor_core::ports::ProcessServiceRegistrar;

#[derive(Debug, Clone, Copy, Default)]
pub struct NullProcessServiceRegistrar;

#[async_trait]
impl ProcessServiceRegistrar for NullProcessServiceRegistrar {
    async fn register(&self, _unit_name: &str, _spec: &ProcessSpec) -> Result<(), RegistrarError> {
        Err(RegistrarError::NotSupported)
    }
    async fn unregister(&self, _unit_name: &str) -> Result<(), RegistrarError> {
        Err(RegistrarError::NotSupported)
    }
    async fn start(&self, _unit_name: &str) -> Result<(), RegistrarError> {
        Err(RegistrarError::NotSupported)
    }
    async fn stop(&self, _unit_name: &str) -> Result<(), RegistrarError> {
        Err(RegistrarError::NotSupported)
    }
    async fn query_status(&self, _unit_name: &str) -> Result<ProcessState, RegistrarError> {
        Err(RegistrarError::NotSupported)
    }
    async fn tail_logs(
        &self,
        _unit_name: &str,
        _lines: usize,
    ) -> Result<Vec<LogLine>, RegistrarError> {
        Err(RegistrarError::NotSupported)
    }
}
