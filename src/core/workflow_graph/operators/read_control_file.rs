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
        let Some(path) = params.get("path").and_then(Value::as_str) else {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "ReadControlFileOperator requires params.path string",
            ));
        };
        if path.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "ReadControlFileOperator requires non-empty params.path",
            ));
        }
        Ok(())
    }

    async fn execute(&self, params: Value, ctx: ExecutionContext) -> Result<Value, AppError> {
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    "ReadControlFileOperator requires params.path string",
                )
            })?
            .trim();

        let resolved = resolve_path(path, &ctx.workspace_path);
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
