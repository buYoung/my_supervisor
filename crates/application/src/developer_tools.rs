use crate::{AppError, AppResult, OperationsFacade};
use serde_json::{json, Value};
impl OperationsFacade {
    pub async fn developer_tool(&self, request: Value) -> AppResult<Value> {
        let tools = self
            .workspace_deps()
            .lifecycle
            .developer_tools()
            .ok_or_else(|| AppError::NotSupported("developer tools".into()))?;
        let get = |key: &str| request.get(key).and_then(Value::as_str).unwrap_or("");
        let result = match get("operation") {
            "paths" => Ok(
                json!({"runtime_root":self.workspace_deps().meta.data_root().join("runtimes").display().to_string()}),
            ),
            "executables" => {
                let mut entries = tools
                    .executables(get("name"))
                    .await
                    .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
                if let Some(saved) = self
                    .workspace_deps()
                    .state_repo
                    .workspace_state(&format!("runtimes/{}", get("name")))
                    .await?
                {
                    if let Ok(saved) = serde_json::from_str::<
                        Vec<my_supervisor_core::ports::ExecutableCandidate>,
                    >(&saved)
                    {
                        for entry in saved {
                            if !entries.iter().any(|item| item.path == entry.path) {
                                entries.push(entry);
                            }
                        }
                    }
                }
                serde_json::to_value(entries)
            }
            "help" => {
                let args = request["args"]
                    .as_array()
                    .map(|v| {
                        v.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                serde_json::to_value(
                    tools
                        .command_help(get("program"), &args, request["cwd"].as_str())
                        .await
                        .map_err(|e| AppError::InvalidRequest(e.to_string()))?,
                )
            }
            "versions" => serde_json::to_value(
                tools
                    .runtime_releases(get("name"), request["all"].as_bool().unwrap_or(false))
                    .await
                    .map_err(|e| AppError::InvalidRequest(e.to_string()))?,
            ),
            "install" => {
                let installed = tools
                    .install_runtime(get("name"), get("version"), get("directory"))
                    .await
                    .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
                let key = format!("runtimes/{}", get("name"));
                let saved = self
                    .workspace_deps()
                    .state_repo
                    .workspace_state(&key)
                    .await?;
                let mut entries = saved
                    .and_then(|v| {
                        serde_json::from_str::<Vec<my_supervisor_core::ports::ExecutableCandidate>>(
                            &v,
                        )
                        .ok()
                    })
                    .unwrap_or_default();
                entries.retain(|e| e.path != installed.path);
                entries.push(installed.clone());
                self.workspace_deps()
                    .state_repo
                    .save_workspace_state(&key, &serde_json::to_string(&entries).unwrap())
                    .await?;
                serde_json::to_value(installed)
            }
            "read_text" => Ok(
                json!({"text":tools.read_text_file(get("path")).await.map_err(|e|AppError::InvalidRequest(e.to_string()))?}),
            ),
            "open_folder" => {
                tools
                    .open_folder(get("path"))
                    .await
                    .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
                Ok(json!({"opened":true}))
            }
            _ => {
                return Err(AppError::InvalidRequest(
                    "unknown developer operation".into(),
                ))
            }
        };
        result.map_err(|e| AppError::Internal(e.to_string()))
    }
}
