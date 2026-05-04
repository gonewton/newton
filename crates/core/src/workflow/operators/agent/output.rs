use super::config::AgentOperatorConfig;
use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operators::engine::PromptSource;
use serde_json::{Map, Number, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Parameters for assembling the agent task output JSON.
pub(super) struct AgentOutput {
    pub(super) signal: Option<String>,
    pub(super) signal_data: HashMap<String, String>,
    pub(super) exit_code: i32,
    pub(super) final_iteration: u32,
    pub(super) stdout_rel: String,
    pub(super) stderr_abs: PathBuf,
    pub(super) stderr_rel: String,
    pub(super) loop_mode: bool,
    pub(super) signals_empty: bool,
    pub(super) engine_is_command: bool,
    pub(super) sdk_token_usage: Option<serde_json::Value>,
    pub(super) sdk_events_artifact: Option<String>,
}

/// Assemble the `Value::Object` returned by `AgentOperator::execute`.
pub(super) fn build_agent_output(out: AgentOutput) -> Value {
    let stderr_artifact = if out.stderr_abs.exists()
        && out
            .stderr_abs
            .metadata()
            .map(|m| m.len() > 0)
            .unwrap_or(false)
    {
        Value::String(out.stderr_rel)
    } else {
        Value::Null
    };

    let signal_value = match out.signal {
        Some(ref s) => Value::String(s.clone()),
        None => {
            if out.signals_empty {
                Value::String("exited".to_string())
            } else {
                Value::Null
            }
        }
    };

    let mut map = Map::new();
    map.insert("signal".to_string(), signal_value);
    map.insert(
        "signal_data".to_string(),
        Value::Object(
            out.signal_data
                .into_iter()
                .map(|(k, v)| (k, Value::String(v)))
                .collect(),
        ),
    );
    map.insert(
        "exit_code".to_string(),
        Value::Number(Number::from(out.exit_code)),
    );
    map.insert("stdout_artifact".to_string(), Value::String(out.stdout_rel));
    map.insert("stderr_artifact".to_string(), stderr_artifact);
    if out.loop_mode {
        map.insert(
            "iteration".to_string(),
            Value::Number(Number::from(out.final_iteration)),
        );
    }
    if !out.engine_is_command {
        let token_usage = out.sdk_token_usage.unwrap_or(Value::Null);
        map.insert("token_usage".to_string(), token_usage);
    }
    if let Some(events_path) = out.sdk_events_artifact {
        map.insert("events_artifact".to_string(), Value::String(events_path));
    }

    Value::Object(map)
}

/// Resolve the prompt string from config.
pub(super) fn resolve_prompt(
    config: &AgentOperatorConfig,
    workspace_root: &Path,
) -> Result<String, AppError> {
    match &config.prompt_source {
        Some(PromptSource::Inline(s)) => Ok(s.clone()),
        Some(PromptSource::File(f)) => {
            let path = workspace_root.join(f);
            std::fs::read_to_string(&path).map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to read prompt_file '{}': {}", path.display(), e),
                )
            })
        }
        None => Ok(String::new()),
    }
}
