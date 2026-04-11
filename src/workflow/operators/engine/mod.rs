#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub mod passthrough;

/// Describes how a coding engine should be invoked as a subprocess.
pub struct EngineInvocation {
    /// Command and arguments to spawn.
    pub command: Vec<String>,
    /// Environment variables to set.
    pub env: Vec<(String, String)>,
    /// Output format, used to guide line parsing.
    pub output_format: OutputFormat,
}

/// Output format for the engine subprocess stdout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputFormat {
    /// Plain text — each line is treated as-is for signal matching.
    PlainText,
    /// Newline-delimited JSON (e.g. Claude stream-json) — each line is parsed
    /// and the `content` or `result.result` field is extracted before signal matching.
    StreamJson,
}

/// Prompt source for the agent operator.
#[derive(Debug, Clone)]
pub enum PromptSource {
    File(String),
    Inline(String),
}

/// Configuration passed to a driver's build_invocation method.
/// This is a view of the resolved AgentOperatorConfig fields needed by drivers.
pub struct DriverConfig<'a> {
    pub model: Option<&'a str>,
    pub prompt_source: Option<&'a PromptSource>,
    pub engine_command: Option<&'a Vec<String>>,
}

/// Trait implemented by each coding engine driver.
pub trait EngineDriver: Send + Sync {
    /// Driver name, matches the `engine:` field value.
    fn name(&self) -> &'static str;

    /// Whether this driver requires a model to be resolved before invocation.
    fn requires_model(&self) -> bool {
        false
    }

    /// Build the invocation from resolved config.
    fn build_invocation(
        &self,
        config: &DriverConfig<'_>,
        project_root: &Path,
    ) -> Result<EngineInvocation, AppError>;
}

/// Build the default engine driver registry.
/// Only includes the command (passthrough) engine; AI engines are handled by AikitEngineManager.
pub fn default_registry() -> HashMap<String, Box<dyn EngineDriver>> {
    let mut m: HashMap<String, Box<dyn EngineDriver>> = HashMap::new();
    m.insert(
        "command".to_string(),
        Box::new(passthrough::PassthroughDriver),
    );
    m
}

/// Manages AI engine execution by delegating to aikit-sdk.
///
/// Wraps `aikit_sdk::run_agent_events` and collects typed `aikit_sdk::AgentEvent`
/// values from the callback stream.
pub struct AikitEngineManager {
    pub workspace_root: PathBuf,
}

impl AikitEngineManager {
    pub fn new(workspace_root: PathBuf) -> Result<Self, AppError> {
        Ok(Self { workspace_root })
    }

    /// Execute an AI engine via aikit-sdk and return SDK event records alongside the run result.
    ///
    /// Delegates to `aikit_sdk::run_agent_events`, collecting each `aikit_sdk::AgentEvent`
    /// via the event callback. Returns the full event vec and the `RunResult` so callers
    /// can access exit status and accumulated stdout/stderr bytes.
    ///
    /// Signal matching and token usage extraction are driven by typed enum matching
    /// on `aikit_sdk::AgentEventPayload` in the caller (`execute_sdk_engine`).
    pub async fn execute_engine_events(
        &self,
        engine_name: &str,
        prompt: &str,
        model: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<(Vec<aikit_sdk::AgentEvent>, aikit_sdk::RunResult), AppError> {
        if !aikit_sdk::is_runnable(engine_name) {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!(
                    "engine '{}' is not runnable by aikit-sdk; supported: codex, claude, gemini, opencode, agent",
                    engine_name
                ),
            )
            .with_code("WFG-SDK-002"));
        }

        let mut options = aikit_sdk::RunOptions::new()
            .with_yolo(true)
            .with_stream(false)
            .with_emit_token_usage_events(true)
            .with_current_dir(self.workspace_root.clone());

        if let Some(t) = timeout {
            options = options.with_timeout(t);
        }
        if let Some(m) = model {
            options = options.with_model(m);
        }

        let prompt_owned = prompt.to_string();
        let engine_name_owned = engine_name.to_string();

        let (events, run_result) = tokio::task::spawn_blocking(move || {
            let mut events: Vec<aikit_sdk::AgentEvent> = Vec::new();
            let result =
                aikit_sdk::run_agent_events(&engine_name_owned, &prompt_owned, options, |event| {
                    events.push(event);
                })
                .map_err(map_run_error)?;
            Ok::<(Vec<aikit_sdk::AgentEvent>, aikit_sdk::RunResult), AppError>((events, result))
        })
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("aikit-sdk task panicked: {}", e),
            )
            .with_code("WFG-SDK-001")
        })??;

        Ok((events, run_result))
    }
}

/// Map aikit_sdk::RunError to Newton AppError with appropriate WFG-SDK codes.
pub fn map_run_error(err: aikit_sdk::RunError) -> AppError {
    match err {
        aikit_sdk::RunError::AgentNotRunnable(key) => AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "engine '{}' is not runnable by aikit-sdk (AgentNotRunnable)",
                key
            ),
        )
        .with_code("WFG-SDK-002"),
        aikit_sdk::RunError::SpawnFailed(io_err) => AppError::new(
            ErrorCategory::IoError,
            format!("aikit-sdk engine process failed to start: {}", io_err),
        )
        .with_code("WFG-SDK-001"),
        aikit_sdk::RunError::StdinFailed(io_err) => AppError::new(
            ErrorCategory::IoError,
            format!("aikit-sdk engine stdin write failed: {}", io_err),
        )
        .with_code("WFG-SDK-001"),
        aikit_sdk::RunError::OutputFailed(io_err) => AppError::new(
            ErrorCategory::IoError,
            format!("aikit-sdk engine output read failed: {}", io_err),
        )
        .with_code("WFG-SDK-001"),
        aikit_sdk::RunError::CallbackPanic(_) => AppError::new(
            ErrorCategory::IoError,
            "aikit-sdk event callback panicked".to_string(),
        )
        .with_code("WFG-SDK-001"),
        aikit_sdk::RunError::ReaderFailed { stream, source } => AppError::new(
            ErrorCategory::IoError,
            format!("aikit-sdk reader failed on {:?}: {}", stream, source),
        )
        .with_code("WFG-SDK-001"),
        aikit_sdk::RunError::TimedOut { timeout, .. } => AppError::new(
            ErrorCategory::IoError,
            format!("aikit-sdk agent timed out after {:?}", timeout),
        )
        .with_code("WFG-SDK-001"),
        _ => AppError::new(ErrorCategory::IoError, format!("aikit-sdk error: {}", err))
            .with_code("WFG-SDK-001"),
    }
}

/// Extract text for signal matching from an `aikit_sdk::AgentEvent`.
///
/// Follows the deterministic rule order from spec section 4:
/// 1. `RawLine` → use raw string as-is
/// 2. `JsonLine` → ordered field extraction: `.content` → `.result.result` → `.result` → `.part.text`
/// 3. `RawBytes` → None (MUST NOT participate in signal matching)
/// 4. `TokenUsageLine` → None (MUST NOT participate in signal matching)
pub fn extract_text_from_sdk_event(event: &aikit_sdk::AgentEvent) -> Option<String> {
    match &event.payload {
        aikit_sdk::AgentEventPayload::RawLine(s) => Some(s.clone()),
        aikit_sdk::AgentEventPayload::JsonLine(json) => extract_text_from_json(json),
        // RawBytes and TokenUsageLine MUST NOT participate in signal matching
        aikit_sdk::AgentEventPayload::RawBytes(_) => None,
        aikit_sdk::AgentEventPayload::TokenUsageLine { .. } => None,
    }
}

/// Extract candidate text from a JSON payload using ordered field lookup.
///
/// Order: `.content` (string) → `.result.result` (string) → `.result` (string) → `.part.text` (string)
pub fn extract_text_from_json(json: &serde_json::Value) -> Option<String> {
    // .content (string)
    if let Some(content) = json.get("content").and_then(|c| c.as_str()) {
        return Some(content.to_string());
    }
    // .result.result (string)
    if let Some(result_result) = json
        .get("result")
        .and_then(|r| r.get("result"))
        .and_then(|r| r.as_str())
    {
        return Some(result_result.to_string());
    }
    // .result (string)
    if let Some(result) = json.get("result").and_then(|r| r.as_str()) {
        return Some(result.to_string());
    }
    // .part.text (string)
    if let Some(part_text) = json
        .get("part")
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
    {
        return Some(part_text.to_string());
    }
    None
}

/// Extract text content from a stream-json line.
/// Returns the original line if parsing fails or the line is not a content type.
/// Used by the command (passthrough) engine only.
pub fn extract_text_from_stream_json(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    // OpenCode run --format json: type "text" with part.text
    if v.get("type").and_then(|t| t.as_str()) == Some("text") {
        if let Some(text) = v
            .get("part")
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
        {
            return Some(text.to_string());
        }
    }
    // Claude stream-json: content or result.result
    if let Some(content) = v.get("content").and_then(|c| c.as_str()) {
        return Some(content.to_string());
    }
    if let Some(result) = v.get("result") {
        if let Some(result_str) = result.get("result").and_then(|r| r.as_str()) {
            return Some(result_str.to_string());
        }
        if let Some(result_str) = result.as_str() {
            return Some(result_str.to_string());
        }
    }
    None
}
