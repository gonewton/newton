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

/// Token usage metrics emitted by AI engine execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    /// Engine key that produced this usage (codex|claude|gemini|opencode|agent).
    pub source: String,
}

/// Typed event payload from AI engine output — mirrors the aikit-sdk AgentEventPayload contract.
///
/// Processing rules:
/// - `RawLine`: use raw string as-is for signal matching
/// - `JsonLine`: extract text via ordered field lookup (.content → .result.result → .result → .part.text)
/// - `RawBytes`: MUST NOT participate in signal matching
/// - `TokenUsageLine`: MUST NOT participate in signal matching; used for token usage accounting
#[derive(Debug, Clone)]
pub enum AgentEventPayload {
    /// Structured JSON output line from the engine.
    JsonLine(serde_json::Value),
    /// Plain text output line from the engine.
    RawLine(String),
    /// Binary output (excluded from signal matching).
    RawBytes(Vec<u8>),
    /// Provider token usage metrics (excluded from signal matching).
    TokenUsageLine(TokenUsage),
}

/// A record representing a single typed event from an AI engine execution.
#[derive(Debug, Clone)]
pub struct AikitEventRecord {
    pub seq: u64,
    /// "stdout" or "stderr"
    pub stream: String,
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
            AgentEventPayload::TokenUsageLine(u) => json!({"type": "TokenUsageLine", "data": u}),
        };
        json!({
            "seq": self.seq,
            "stream": self.stream,
            "payload": payload_json,
        })
    }
}

/// Manages AI engine execution by delegating to aikit-sdk.
pub struct AikitEngineManager {
    pub workspace_root: PathBuf,
}

impl AikitEngineManager {
    pub fn new(workspace_root: PathBuf) -> Result<Self, AppError> {
        Ok(Self { workspace_root })
    }

    /// Execute an AI engine via aikit-sdk and return collected typed events.
    ///
    /// Delegates to `aikit_sdk::run_agent` and classifies output lines into
    /// `AgentEventPayload` variants (`JsonLine`, `RawLine`, `TokenUsageLine`).
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

        let options = aikit_sdk::RunOptions {
            model: model.map(str::to_string),
            yolo: true,
            stream: false,
        };

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

        // Classify RunResult output into typed AgentEventPayload events
        let events = classify_run_result_to_events(result, engine_name);
        Ok(events)
    }
}

/// Map aikit_sdk::RunError to Newton AppError with appropriate WFG-SDK codes.
/// Exhaustively covers all RunError variants.
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
    }
}

/// Attempt to detect token usage metadata in a JSON value.
///
/// Recognizes common patterns from AI providers:
/// - Claude: `{"input_tokens": N, "output_tokens": N, ...}`
/// - OpenAI/Codex: `{"usage": {"prompt_tokens": N, "completion_tokens": N, "total_tokens": N}}`
/// - Generic nested: `{"usage": {"input_tokens": N, "output_tokens": N}}`
fn detect_token_usage(json: &serde_json::Value, engine_name: &str) -> Option<TokenUsage> {
    // Pattern 1: top-level input_tokens + output_tokens
    if let (Some(input), Some(output)) = (
        json.get("input_tokens").and_then(|v| v.as_u64()),
        json.get("output_tokens").and_then(|v| v.as_u64()),
    ) {
        return Some(TokenUsage {
            input_tokens: input,
            output_tokens: output,
            total_tokens: json.get("total_tokens").and_then(|v| v.as_u64()),
            cache_read_tokens: json.get("cache_read_tokens").and_then(|v| v.as_u64()),
            cache_creation_tokens: json.get("cache_creation_tokens").and_then(|v| v.as_u64()),
            reasoning_tokens: json.get("reasoning_tokens").and_then(|v| v.as_u64()),
            source: engine_name.to_string(),
        });
    }

    if let Some(usage) = json.get("usage") {
        // Pattern 2: {"usage": {"input_tokens": N, "output_tokens": N}}
        if let (Some(input), Some(output)) = (
            usage.get("input_tokens").and_then(|v| v.as_u64()),
            usage.get("output_tokens").and_then(|v| v.as_u64()),
        ) {
            return Some(TokenUsage {
                input_tokens: input,
                output_tokens: output,
                total_tokens: usage.get("total_tokens").and_then(|v| v.as_u64()),
                cache_read_tokens: usage.get("cache_read_tokens").and_then(|v| v.as_u64()),
                cache_creation_tokens: usage.get("cache_creation_tokens").and_then(|v| v.as_u64()),
                reasoning_tokens: usage.get("reasoning_tokens").and_then(|v| v.as_u64()),
                source: engine_name.to_string(),
            });
        }

        // Pattern 3: OpenAI/Codex {"usage": {"prompt_tokens": N, "completion_tokens": N}}
        if let (Some(input), Some(output)) = (
            usage.get("prompt_tokens").and_then(|v| v.as_u64()),
            usage.get("completion_tokens").and_then(|v| v.as_u64()),
        ) {
            let total = usage.get("total_tokens").and_then(|v| v.as_u64());
            return Some(TokenUsage {
                input_tokens: input,
                output_tokens: output,
                total_tokens: total,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                reasoning_tokens: usage.get("reasoning_tokens").and_then(|v| v.as_u64()),
                source: engine_name.to_string(),
            });
        }
    }

    None
}

/// Classify a single text line into an `AgentEventPayload`.
fn classify_line(line: &str, engine_name: &str) -> AgentEventPayload {
    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(line) {
        // Check if this JSON event represents token usage
        if let Some(usage) = detect_token_usage(&json_val, engine_name) {
            AgentEventPayload::TokenUsageLine(usage)
        } else {
            AgentEventPayload::JsonLine(json_val)
        }
    } else {
        AgentEventPayload::RawLine(line.to_string())
    }
}

/// Classify `RunResult` stdout/stderr into typed `AgentEventPayload` events.
fn classify_run_result_to_events(
    result: aikit_sdk::RunResult,
    engine_name: &str,
) -> Vec<AikitEventRecord> {
    let mut events = Vec::new();
    let mut seq: u64 = 0;

    // Classify stdout lines as typed events
    if !result.stdout.is_empty() {
        let stdout_str = String::from_utf8_lossy(&result.stdout);
        for line in stdout_str.lines() {
            let payload = classify_line(line, engine_name);
            events.push(AikitEventRecord {
                seq,
                stream: "stdout".to_string(),
                payload,
            });
            seq += 1;
        }
    }

    // Classify stderr lines as typed events
    if !result.stderr.is_empty() {
        let stderr_str = String::from_utf8_lossy(&result.stderr);
        for line in stderr_str.lines() {
            let payload = classify_line(line, engine_name);
            events.push(AikitEventRecord {
                seq,
                stream: "stderr".to_string(),
                payload,
            });
            seq += 1;
        }
    }

    events
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
