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
    /// via the event callback. Returns the full event vec plus an inner `Result` wrapping
    /// the `RunResult` or the mapped SDK error.
    ///
    /// The outer `Result` only fails for fatal conditions (spawn panic, `is_runnable` check).
    /// SDK-level errors (including `RunError::QuotaExceeded`) are returned in the inner
    /// `Result` so that callers can flush the already-collected events to disk before
    /// deciding how to handle the error.
    ///
    /// Signal matching and token usage extraction are driven by typed enum matching
    /// on `aikit_sdk::AgentEventPayload` in the caller (`execute_sdk_engine`).
    pub async fn execute_engine_events(
        &self,
        engine_name: &str,
        prompt: &str,
        model: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<
        (
            Vec<aikit_sdk::AgentEvent>,
            Result<aikit_sdk::RunResult, AppError>,
        ),
        AppError,
    > {
        if !aikit_sdk::is_runnable(engine_name) {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!(
                    "engine '{engine_name}' is not runnable by aikit-sdk; supported: codex, claude, gemini, opencode, agent"
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

        let (events, run_result) = tokio::task::spawn_blocking(
            move || -> (Vec<aikit_sdk::AgentEvent>, Result<aikit_sdk::RunResult, AppError>) {
                let mut events: Vec<aikit_sdk::AgentEvent> = Vec::new();
                let result = aikit_sdk::run_agent_events(
                    &engine_name_owned,
                    &prompt_owned,
                    options,
                    |event| {
                        events.push(event);
                    },
                )
                .map_err(map_run_error);
                // Always return the events alongside the result so the caller can flush
                // them to disk even when the SDK returns an error (e.g. QuotaExceeded).
                (events, result)
            },
        )
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("aikit-sdk task panicked: {e}"),
            )
            .with_code("WFG-SDK-001")
        })?;

        Ok((events, run_result))
    }
}

/// Map aikit_sdk::RunError to Newton AppError with appropriate WFG-SDK codes.
///
/// `aikit_sdk::RunError` is `#[non_exhaustive]`; the `deny(non_exhaustive_omitted_patterns)`
/// attribute below turns any future SDK variant into a compile error here, preventing
/// silent fall-through.
#[allow(unknown_lints)]
#[deny(non_exhaustive_omitted_patterns)]
pub fn map_run_error(err: aikit_sdk::RunError) -> AppError {
    match err {
        aikit_sdk::RunError::AgentNotRunnable(key) => AppError::new(
            ErrorCategory::ValidationError,
            format!("engine '{key}' is not runnable by aikit-sdk (AgentNotRunnable)"),
        )
        .with_code("WFG-SDK-002"),
        aikit_sdk::RunError::SpawnFailed(io_err) => AppError::new(
            ErrorCategory::IoError,
            format!("aikit-sdk engine process failed to start: {io_err}"),
        )
        .with_code("WFG-SDK-001"),
        aikit_sdk::RunError::StdinFailed(io_err) => AppError::new(
            ErrorCategory::IoError,
            format!("aikit-sdk engine stdin write failed: {io_err}"),
        )
        .with_code("WFG-SDK-001"),
        aikit_sdk::RunError::OutputFailed(io_err) => AppError::new(
            ErrorCategory::IoError,
            format!("aikit-sdk engine output read failed: {io_err}"),
        )
        .with_code("WFG-SDK-001"),
        aikit_sdk::RunError::CallbackPanic(_) => AppError::new(
            ErrorCategory::IoError,
            "aikit-sdk event callback panicked".to_string(),
        )
        .with_code("WFG-SDK-001"),
        aikit_sdk::RunError::ReaderFailed { stream, source } => AppError::new(
            ErrorCategory::IoError,
            format!("aikit-sdk reader failed on {stream:?}: {source}"),
        )
        .with_code("WFG-SDK-001"),
        aikit_sdk::RunError::TimedOut { timeout, .. } => AppError::new(
            ErrorCategory::IoError,
            format!("aikit-sdk agent timed out after {timeout:?}"),
        )
        .with_code("WFG-SDK-001"),
        aikit_sdk::RunError::QuotaExceeded(info) => {
            crate::workflow::operators::agent::quota::quota_signal_to_error_minimal(&info)
        }
        // Required by #[non_exhaustive] across a crate boundary; the
        // `non_exhaustive_omitted_patterns` lint above turns any new variant
        // into a compile error before we ever reach this arm.
        _ => AppError::new(
            ErrorCategory::IoError,
            format!("aikit-sdk error (unhandled variant): {err}"),
        )
        .with_code("WFG-SDK-001"),
    }
}

/// Extract text for signal matching from an `aikit_sdk::AgentEvent`.
///
/// Follows the deterministic rule order from spec section 4:
/// 1. `RawLine` → use raw string as-is
/// 2. `JsonLine` → ordered field extraction: `.content` → `.result.result` → `.result` → `.part.text`
/// 3. `StreamMessage` with phase=Final and role=Assistant → use `text` field directly.
///    This covers aikit-sdk ≥0.2 (046-agent-stream) where Claude's final assistant turn
///    arrives as StreamMessage rather than JsonLine.
/// 4. All other variants (RawBytes, TokenUsageLine, QuotaExceeded, StreamMessage[non-Final/Assistant],
///    RawTransportLine, Aikit* built-ins) → None.
///
/// `aikit_sdk::AgentEventPayload` is `#[non_exhaustive]`; the
/// `deny(non_exhaustive_omitted_patterns)` attribute below turns any future SDK
/// variant into a compile error here so Newton must explicitly classify it.
#[allow(unknown_lints)]
#[deny(non_exhaustive_omitted_patterns)]
pub fn extract_text_from_sdk_event(event: &aikit_sdk::AgentEvent) -> Option<String> {
    match &event.payload {
        aikit_sdk::AgentEventPayload::RawLine(s) => Some(s.clone()),
        aikit_sdk::AgentEventPayload::JsonLine(json) => extract_text_from_json(json),
        aikit_sdk::AgentEventPayload::StreamMessage(msg)
            if msg.phase == aikit_sdk::MessagePhase::Final
                && msg.role == aikit_sdk::MessageRole::Assistant =>
        {
            Some(msg.text.clone())
        }
        aikit_sdk::AgentEventPayload::StreamMessage(_) => None,
        aikit_sdk::AgentEventPayload::RawBytes(_) => None,
        aikit_sdk::AgentEventPayload::TokenUsageLine { .. } => None,
        aikit_sdk::AgentEventPayload::QuotaExceeded { .. } => None,
        aikit_sdk::AgentEventPayload::RawTransportLine { .. } => None,
        aikit_sdk::AgentEventPayload::AikitTextDelta { .. } => None,
        aikit_sdk::AgentEventPayload::AikitTextFinal { .. } => None,
        aikit_sdk::AgentEventPayload::AikitToolUse { .. } => None,
        aikit_sdk::AgentEventPayload::AikitToolResult { .. } => None,
        aikit_sdk::AgentEventPayload::AikitSubagentSpawn { .. } => None,
        aikit_sdk::AgentEventPayload::AikitSubagentResult { .. } => None,
        aikit_sdk::AgentEventPayload::AikitContextCompressed { .. } => None,
        aikit_sdk::AgentEventPayload::AikitStepFinish { .. } => None,
        // Required by #[non_exhaustive] across a crate boundary; the
        // `non_exhaustive_omitted_patterns` lint above turns any new variant
        // into a compile error before we ever reach this arm.
        _ => None,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(payload: aikit_sdk::AgentEventPayload) -> aikit_sdk::AgentEvent {
        aikit_sdk::AgentEvent {
            agent_key: "claude".to_string(),
            payload,
            stream: aikit_sdk::AgentEventStream::Stdout,
            seq: 0,
        }
    }

    fn make_stream_message(
        text: &str,
        phase: aikit_sdk::MessagePhase,
        role: aikit_sdk::MessageRole,
    ) -> aikit_sdk::AgentEventPayload {
        aikit_sdk::AgentEventPayload::StreamMessage(aikit_sdk::StreamMessage {
            text: text.to_string(),
            phase,
            role,
            kind: aikit_sdk::MessageKind::Message,
            source: aikit_sdk::AgentEventStream::Stdout,
            raw_line_seq: 0,
            turn_id: None,
        })
    }

    #[test]
    fn raw_line_extracted() {
        let event = make_event(aikit_sdk::AgentEventPayload::RawLine(
            "<status>COMPLETED</status>".to_string(),
        ));
        assert_eq!(
            extract_text_from_sdk_event(&event).as_deref(),
            Some("<status>COMPLETED</status>")
        );
    }

    #[test]
    fn stream_message_final_assistant_extracted() {
        // Regression: aikit-sdk >=0.2 emits final assistant text as StreamMessage(Final/Assistant).
        // This must participate in signal matching so <status>COMPLETED</status> is observable.
        let event = make_event(make_stream_message(
            "<status>COMPLETED</status>",
            aikit_sdk::MessagePhase::Final,
            aikit_sdk::MessageRole::Assistant,
        ));
        assert_eq!(
            extract_text_from_sdk_event(&event).as_deref(),
            Some("<status>COMPLETED</status>")
        );
    }

    #[test]
    fn stream_message_delta_skipped() {
        let event = make_event(make_stream_message(
            "partial text",
            aikit_sdk::MessagePhase::Delta,
            aikit_sdk::MessageRole::Assistant,
        ));
        assert_eq!(extract_text_from_sdk_event(&event), None);
    }

    #[test]
    fn stream_message_final_non_assistant_skipped() {
        let event = make_event(make_stream_message(
            "tool output",
            aikit_sdk::MessagePhase::Final,
            aikit_sdk::MessageRole::Tool,
        ));
        assert_eq!(extract_text_from_sdk_event(&event), None);
    }

    #[test]
    fn raw_bytes_skipped() {
        let event = make_event(aikit_sdk::AgentEventPayload::RawBytes(b"binary".to_vec()));
        assert_eq!(extract_text_from_sdk_event(&event), None);
    }
}
