#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use std::collections::HashMap;
use std::path::Path;

pub mod claude_code;
pub mod opencode;
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
pub fn default_registry() -> HashMap<String, Box<dyn EngineDriver>> {
    let mut m: HashMap<String, Box<dyn EngineDriver>> = HashMap::new();
    m.insert("opencode".to_string(), Box::new(opencode::OpencodeDriver));
    m.insert(
        "claude_code".to_string(),
        Box::new(claude_code::ClaudeCodeDriver),
    );
    m.insert(
        "command".to_string(),
        Box::new(passthrough::PassthroughDriver),
    );
    m
}

/// Extract text content from a stream-json line.
/// Returns the original line if parsing fails or the line is not a content type.
pub fn extract_text_from_stream_json(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    // Try various content field paths used by Claude stream-json
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
    // Not a content-bearing line; skip
    None
}
