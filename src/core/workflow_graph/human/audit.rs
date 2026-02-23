#![allow(clippy::result_large_err)] // Audit helpers return AppError for consistent diagnostics.

use crate::core::error::AppError;
use crate::core::workflow_graph::state::redact_value;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub execution_id: String,
    pub task_id: String,
    pub interviewer_type: String,
    pub prompt: String,
    pub choices: Option<Vec<String>>,
    pub approved: Option<bool>,
    pub choice: Option<String>,
    pub responder: Option<String>,
    pub response_text: Option<String>,
    pub timeout_applied: bool,
    pub default_used: bool,
}

pub fn append_entry(
    workspace_root: &Path,
    audit_path: &Path,
    execution_id: &str,
    entry: &mut AuditEntry,
    redact_keys: &[String],
) -> Result<(), AppError> {
    let base = workspace_root.join(audit_path);
    let target_dir = base.join(execution_id);
    fs::create_dir_all(&target_dir).map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::IoError,
            format!(
                "failed to create audit directory {}: {}",
                target_dir.display(),
                err
            ),
        )
    })?;
    let mut payload = serde_json::to_value(entry).map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::SerializationError,
            format!("failed to serialize audit entry: {}", err),
        )
    })?;
    redact_value(&mut payload, redact_keys);
    let line = serde_json::to_string(&payload).map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::SerializationError,
            format!("failed to serialize audit entry: {}", err),
        )
    })?;
    let audit_file = target_dir.join("audit.jsonl");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&audit_file)
        .map_err(|err| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!(
                    "failed to open audit file {}: {}",
                    audit_file.display(),
                    err
                ),
            )
        })?;
    file.write_all(line.as_bytes()).map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::IoError,
            format!("failed to write audit entry: {}", err),
        )
    })?;
    file.write_all(b"\n").map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::IoError,
            format!("failed to write audit entry newline: {}", err),
        )
    })?;
    Ok(())
}
