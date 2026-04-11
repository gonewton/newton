#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

/// Token usage metrics emitted by an AI engine execution.
///
/// Mirrors the `aikit-sdk` `TokenUsageLine` contract for SDK delegation.
/// Once aikit-sdk v0.1.75+ is available, replace with direct SDK type.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenUsageLine {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub source: String,
}

/// Typed SDK event payload — mirrors the `aikit-sdk` `AgentEventPayload` enum.
///
/// This enum is designed for direct replacement with the SDK type once
/// `aikit-sdk v0.1.75+` provides `run_agent_events` with typed `AgentEventPayload`.
/// Until then, Newton emits these typed variants from its own event parsing layer
/// wrapping `aikit_sdk::run_agent`.
///
/// Variant semantics match the SDK contract:
/// - `JsonLine` — structured JSON output from the engine
/// - `RawLine` — plain text output line from the engine
/// - `RawBytes` — binary output (MUST NOT participate in signal matching)
/// - `TokenUsageLine` — provider token usage metrics (MUST NOT participate in signal matching)
#[derive(Debug, Clone)]
pub enum AgentEventPayload {
    /// Structured JSON output line from the engine.
    JsonLine(serde_json::Value),
    /// Plain text output line from the engine.
    RawLine(String),
    /// Binary output — excluded from signal matching.
    RawBytes(Vec<u8>),
    /// Provider token usage metrics — excluded from signal matching.
    TokenUsageLine(TokenUsageLine),
}

/// A record representing a single typed event from an AI engine execution.
///
/// The `payload` field holds a typed `AgentEventPayload` variant, mirroring
/// the `aikit-sdk` `AgentEventPayload` contract for future SDK delegation.
///
/// Once `aikit-sdk v0.1.75+` provides `run_agent_events`, replace the
/// `run_agent`-based conversion in `execute_engine_events` with direct
/// consumption of SDK-emitted `AgentEventPayload` values.
#[derive(Debug, Clone)]
pub struct AikitEventRecord {
    pub seq: u64,
    /// "stdout" or "stderr"
    pub stream: String,
    /// Typed event payload — use `AgentEventPayload` variant matching, not JSON string checks.
    pub payload: AgentEventPayload,
}

impl AikitEventRecord {
    /// Serialize this record to a JSON value for NDJSON artifact writing.
    pub fn to_json_value(&self) -> serde_json::Value {
        use serde_json::json;
        let payload_json = match &self.payload {
            AgentEventPayload::JsonLine(v) => json!({"type": "JsonLine", "data": v}),
            AgentEventPayload::RawLine(s) => json!({"type": "RawLine", "data": s}),
            AgentEventPayload::RawBytes(b) => json!({"type": "RawBytes", "length": b.len()}),
            AgentEventPayload::TokenUsageLine(t) => {
                json!({"type": "TokenUsageLine", "data": serde_json::to_value(t).unwrap_or(serde_json::Value::Null)})
            }
        };
        json!({
            "seq": self.seq,
            "stream": self.stream,
            "payload": payload_json,
        })
    }
}

/// Manages AI engine execution by delegating to aikit-sdk.
///
/// Currently wraps `aikit_sdk::run_agent` (available in aikit-sdk v0.1.49) and
/// emits typed `AgentEventPayload` variants from the parsed output.
///
/// Once `aikit-sdk v0.1.75+` provides `run_agent_events` with native
/// `AgentEventPayload` emission (including `TokenUsageLine`), replace the
/// `run_agent` call and `run_result_to_event_records` conversion with direct
/// consumption of SDK events.
pub struct AikitEngineManager {
    pub workspace_root: PathBuf,
}

impl AikitEngineManager {
    pub fn new(workspace_root: PathBuf) -> Result<Self, AppError> {
        Ok(Self { workspace_root })
    }

    /// Execute an AI engine via aikit-sdk and return typed event records.
    ///
    /// Delegates to `aikit_sdk::run_agent` and converts stdout/stderr output
    /// into typed `AgentEventPayload` variants:
    /// - JSON lines → `AgentEventPayload::JsonLine` (or `TokenUsageLine` if token usage fields detected)
    /// - Plain text lines → `AgentEventPayload::RawLine`
    ///
    /// Signal matching and token usage extraction are driven by typed enum matching
    /// in the caller (`execute_sdk_engine`), not by JSON string field inspection.
    pub async fn execute_engine_events(
        &self,
        engine_name: &str,
        prompt: &str,
        model: Option<&str>,
    ) -> Result<Vec<AikitEventRecord>, AppError> {
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
            .with_stream(false);
        if let Some(m) = model {
            options = options.with_model(m);
        }

        let prompt_owned = prompt.to_string();
        let engine_name_owned = engine_name.to_string();

        let result = tokio::task::spawn_blocking(move || {
            aikit_sdk::run_agent(&engine_name_owned, &prompt_owned, options)
        })
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("aikit-sdk task panicked: {}", e),
            )
            .with_code("WFG-SDK-001")
        })?
        .map_err(map_run_error)?;

        // Convert run_agent output to typed AikitEventRecord values using AgentEventPayload enum.
        let events = run_result_to_event_records(result, engine_name);
        Ok(events)
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

/// Convert `RunResult` stdout/stderr bytes into typed `AikitEventRecord` values.
///
/// Each line is classified into a typed `AgentEventPayload` variant:
/// - JSON lines containing token usage fields → `AgentEventPayload::TokenUsageLine`
/// - Other JSON lines → `AgentEventPayload::JsonLine`
/// - Plain text lines → `AgentEventPayload::RawLine`
///
/// This function is the compatibility shim that bridges `aikit_sdk::run_agent`
/// (v0.1.49) output to the typed `AgentEventPayload` contract that `run_agent_events`
/// will provide natively in `aikit-sdk v0.1.75+`.
fn run_result_to_event_records(
    result: aikit_sdk::RunResult,
    engine_name: &str,
) -> Vec<AikitEventRecord> {
    let mut events = Vec::new();
    let mut seq: u64 = 0;

    for (raw_bytes, stream_label) in [
        (result.stdout.as_slice(), "stdout"),
        (result.stderr.as_slice(), "stderr"),
    ] {
        if raw_bytes.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(raw_bytes);
        for line in text.lines() {
            let payload = if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(line) {
                // Check if this JSON line represents token usage metrics
                if let Some(token_usage) = try_parse_token_usage(&json_val, engine_name) {
                    AgentEventPayload::TokenUsageLine(token_usage)
                } else {
                    AgentEventPayload::JsonLine(json_val)
                }
            } else {
                AgentEventPayload::RawLine(line.to_string())
            };
            events.push(AikitEventRecord {
                seq,
                stream: stream_label.to_string(),
                payload,
            });
            seq += 1;
        }
    }

    events
}

/// Attempt to parse token usage metrics from a JSON line.
///
/// Detects token usage patterns emitted by AI engines (claude, opencode, etc.)
/// and converts them into a typed `TokenUsageLine`. Returns `None` if the JSON
/// does not contain recognizable token usage fields.
fn try_parse_token_usage(json: &serde_json::Value, engine_name: &str) -> Option<TokenUsageLine> {
    // Direct token usage fields at top level (e.g. from SDK TokenUsageLine format)
    let input_tokens = json
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            json.get("usage")
                .and_then(|u| u.get("input_tokens"))
                .and_then(|v| v.as_u64())
        })?;
    let output_tokens = json
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            json.get("usage")
                .and_then(|u| u.get("output_tokens"))
                .and_then(|v| v.as_u64())
        })?;

    Some(TokenUsageLine {
        input_tokens,
        output_tokens,
        total_tokens: json.get("total_tokens").and_then(|v| v.as_u64()),
        cache_read_tokens: json.get("cache_read_tokens").and_then(|v| v.as_u64()),
        cache_creation_tokens: json.get("cache_creation_tokens").and_then(|v| v.as_u64()),
        reasoning_tokens: json.get("reasoning_tokens").and_then(|v| v.as_u64()),
        source: engine_name.to_string(),
    })
}

/// Extract text for signal matching from an `AikitEventRecord`.
///
/// Follows the deterministic rule order from spec section 4:
/// 1. `RawLine` → use raw string as-is
/// 2. `JsonLine` → ordered field extraction: `.content` → `.result.result` → `.result` → `.part.text`
/// 3. `RawBytes` → None (MUST NOT participate in signal matching)
/// 4. `TokenUsageLine` → None (MUST NOT participate in signal matching)
pub fn extract_text_from_event(record: &AikitEventRecord) -> Option<String> {
    match &record.payload {
        AgentEventPayload::RawLine(s) => Some(s.clone()),
        AgentEventPayload::JsonLine(json) => extract_text_from_json(json),
        // RawBytes and TokenUsageLine MUST NOT participate in signal matching
        AgentEventPayload::RawBytes(_) => None,
        AgentEventPayload::TokenUsageLine(_) => None,
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
