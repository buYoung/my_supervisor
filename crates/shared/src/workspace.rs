use crate::api::ProcessConfigDto;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessUpdateRequestDto {
    pub config: ProcessConfigDto,
    pub expected: Option<ProcessConfigDto>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSaveRequestDto {
    pub definition: Value,
    pub expected_revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentDto {
    pub revision: u64,
    pub saved_at: String,
    pub definition: Value,
}
