#![allow(clippy::result_large_err)] // Operator returns AppError for consistent structured diagnostics.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub struct ReadControlFileOperator;

impl ReadControlFileOperator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReadControlFileOperator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Operator for ReadControlFileOperator {
    fn name(&self) -> &'static str {
        "ReadControlFileOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        if let Some(path_value) = params.get("path") {
            if path_value.is_null() {
                return Ok(());
            }
            let Some(path) = path_value.as_str() else {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "ReadControlFileOperator params.path must be a string when provided",
                ));
            };
            if path.trim().is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "ReadControlFileOperator requires non-empty params.path",
                ));
            }
        }
        Ok(())
    }

    async fn execute(&self, params: Value, ctx: ExecutionContext) -> Result<Value, AppError> {
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                ctx.state_view
                    .triggers
                    .get("control_file")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .or_else(|| {
                std::env::var("NEWTON_CONTROL_FILE")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or_else(|| "newton_control.json".to_string());

        let resolved = resolve_path(&path, &ctx.workspace_path);
        if !resolved.exists() {
            return Ok(Value::Object(Map::from_iter([
                ("exists".to_string(), Value::Bool(false)),
                ("done".to_string(), Value::Bool(false)),
                ("message".to_string(), Value::Null),
                ("metadata".to_string(), Value::Null),
            ])));
        }

        let bytes = std::fs::read(&resolved).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "failed to read control file {}: {}",
                    resolved.display(),
                    err
                ),
            )
        })?;
        let parsed: Value = serde_json::from_slice(&bytes).map_err(|_| {
            AppError::new(
                ErrorCategory::SerializationError,
                format!("control file is not valid JSON: {}", resolved.display()),
            )
            .with_code("WFG-CTRL-001")
        })?;
        let done = parsed.get("done").and_then(Value::as_bool).unwrap_or(false);
        let message = parsed.get("message").cloned().unwrap_or(Value::Null);
        let metadata = parsed.get("metadata").cloned().unwrap_or(Value::Null);
        Ok(Value::Object(Map::from_iter([
            ("exists".to_string(), Value::Bool(true)),
            ("done".to_string(), Value::Bool(done)),
            ("message".to_string(), message),
            ("metadata".to_string(), metadata),
        ])))
    }
}

fn resolve_path(path: &str, workspace: &Path) -> PathBuf {
    let as_path = PathBuf::from(path);
    if as_path.is_absolute() {
        as_path
    } else {
        workspace.join(as_path)
    }
}
