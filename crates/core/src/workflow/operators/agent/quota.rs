//! Quota detection and SDK error mapping helpers (WFG-SDK-003, WFG-AGENT-008 quota path).

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use std::path::Path;

/// Construct an `IoError` AppError tagged with `WFG-SDK-003` for SDK NDJSON / event handling
/// I/O failures (open, write, serialize). Centralizes the WFG-SDK-003 emission per spec.
pub(super) fn sdk_io_error(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCategory::IoError, message.into()).with_code("WFG-SDK-003")
}

/// Map an SDK `RunResult.quota_exceeded` payload to an AppError (WFG-AGENT-008).
/// Adds standard context (provider, quota_category, raw_excerpt, events_artifact, stderr_artifact).
pub(super) fn quota_signal_to_error(
    info: &aikit_sdk::QuotaExceededInfo,
    events_artifact_rel: &str,
    stderr_path: &Path,
    stderr_rel: &str,
) -> AppError {
    let category = format!("{:?}", info.category).to_lowercase();
    let mut error = AppError::new(
        ErrorCategory::ResourceError,
        format!(
            "agent '{}' quota exceeded ({}): {}",
            info.agent_key, category, info.raw_message
        ),
    )
    .with_code("WFG-AGENT-008");
    error.add_context("provider", &info.agent_key);
    error.add_context("quota_category", &category);
    error.add_context("raw_excerpt", &info.raw_message);
    error.add_context("events_artifact", events_artifact_rel);
    if stderr_path.exists() {
        error.add_context("stderr_artifact", stderr_rel);
    }
    error
}
