use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operators::engine::PromptSource;
use indexmap::IndexMap;
use serde_json::Value;
use std::collections::HashMap;

/// Parsed from task.params at execution time.
#[derive(Debug, Clone)]
pub(super) struct AgentOperatorConfig {
    pub(super) engine: Option<String>,
    pub(super) model: Option<String>,
    pub(super) prompt_source: Option<PromptSource>,
    pub(super) working_dir: Option<String>,
    pub(super) env: HashMap<String, String>,
    pub(super) timeout_seconds: Option<u64>,
    /// Ordered map — signal patterns are matched in insertion order.
    pub(super) signals: IndexMap<String, String>,
    /// YAML key: `loop`. Parsed via params.get("loop").
    pub(super) loop_mode: bool,
    pub(super) max_iterations: Option<u32>,
    /// Required when engine = "command".
    pub(super) engine_command: Option<Vec<String>>,
    /// Whether to stream stdout to the terminal. If None, uses workflow setting.
    pub(super) stream_stdout: Option<bool>,
}

impl AgentOperatorConfig {
    pub(super) fn from_value(params: &Value) -> Result<Self, AppError> {
        let map = params.as_object().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ValidationError,
                "AgentOperator params must be an object",
            )
        })?;

        let engine = map
            .get("engine")
            .and_then(Value::as_str)
            .map(str::to_string);
        let model = map.get("model").and_then(Value::as_str).map(str::to_string);
        let prompt_source = Self::parse_prompt_source(map);
        let working_dir = map
            .get("working_dir")
            .and_then(Value::as_str)
            .map(str::to_string);
        let env = Self::parse_env_variables(map);
        let timeout_seconds = map.get("timeout_seconds").and_then(Value::as_u64);
        let signals = Self::parse_signals(map);
        let loop_mode = map.get("loop").and_then(Value::as_bool).unwrap_or(false);
        let max_iterations = map
            .get("max_iterations")
            .and_then(Value::as_u64)
            .map(|v| v as u32);
        let engine_command = Self::parse_engine_command(map);
        let stream_stdout = map.get("stream_stdout").and_then(Value::as_bool);

        Ok(AgentOperatorConfig {
            engine,
            model,
            prompt_source,
            working_dir,
            env,
            timeout_seconds,
            signals,
            loop_mode,
            max_iterations,
            engine_command,
            stream_stdout,
        })
    }

    /// Parse prompt source: prompt_file takes priority over prompt
    fn parse_prompt_source(map: &serde_json::Map<String, Value>) -> Option<PromptSource> {
        if let Some(pf) = map.get("prompt_file").and_then(Value::as_str) {
            Some(PromptSource::File(pf.to_string()))
        } else {
            map.get("prompt")
                .and_then(Value::as_str)
                .map(|p| PromptSource::Inline(p.to_string()))
        }
    }

    /// Parse environment variables from the params map
    fn parse_env_variables(map: &serde_json::Map<String, Value>) -> HashMap<String, String> {
        map.get("env")
            .and_then(Value::as_object)
            .map(|env_map| {
                env_map
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default()
    }

    /// Parse signals into an ordered map
    fn parse_signals(map: &serde_json::Map<String, Value>) -> IndexMap<String, String> {
        let mut signals = IndexMap::new();
        if let Some(signals_obj) = map.get("signals").and_then(Value::as_object) {
            for (k, v) in signals_obj {
                if let Some(pattern) = v.as_str() {
                    signals.insert(k.clone(), pattern.to_string());
                }
            }
        }
        signals
    }

    /// Parse engine_command: array of strings
    fn parse_engine_command(map: &serde_json::Map<String, Value>) -> Option<Vec<String>> {
        map.get("engine_command")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
    }

    /// Resolve the engine name from params or the workflow default, emitting WFG-AGENT-001 if
    /// neither is set.
    pub(super) fn resolve_engine(&self, default_engine: Option<&str>) -> Result<String, AppError> {
        self.engine
            .as_deref()
            .or(default_engine)
            .ok_or_else(|| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    "no engine resolved: set params.engine or settings.default_engine",
                )
                .with_code("WFG-AGENT-001")
            })
            .map(str::to_string)
    }

    /// Validate that engine:command tasks supply a non-empty engine_command list
    /// (static check, pre-interpolation). Emits WFG-AGENT-007.
    pub(super) fn validate_engine_command(&self) -> Result<(), AppError> {
        if self.engine.as_deref() != Some("command") {
            return Ok(());
        }
        match &self.engine_command {
            None => Err(AppError::new(
                ErrorCategory::ValidationError,
                "engine: command requires engine_command in params",
            )
            .with_code("WFG-AGENT-007")),
            Some(cmds) if cmds.is_empty() => Err(AppError::new(
                ErrorCategory::ValidationError,
                "engine_command must not be empty",
            )
            .with_code("WFG-AGENT-007")),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn config_parses_basic_params() {
        let params = json!({
            "engine": "opencode",
            "model": "gpt-4o",
            "prompt": "do the thing",
            "loop": false,
            "max_iterations": 5,
            "signals": {
                "complete": "<promise>COMPLETE</promise>",
                "blocked": "<promise>BLOCKED:(?P<reason>[^<]+)</promise>"
            }
        });
        let config = AgentOperatorConfig::from_value(&params).unwrap();
        assert_eq!(config.engine.as_deref(), Some("opencode"));
        assert_eq!(config.model.as_deref(), Some("gpt-4o"));
        assert!(!config.loop_mode);
        assert_eq!(config.max_iterations, Some(5));
        assert_eq!(config.signals.len(), 2);
        assert!(config.signals.contains_key("complete"));
        assert!(config.signals.contains_key("blocked"));
    }

    #[test]
    fn config_parses_loop_true() {
        let params = json!({"engine": "opencode", "loop": true, "prompt": "x"});
        let config = AgentOperatorConfig::from_value(&params).unwrap();
        assert!(config.loop_mode);
    }

    #[test]
    fn config_parses_prompt_file() {
        let params = json!({"engine": "opencode", "prompt_file": ".agent/PROMPT.md"});
        let config = AgentOperatorConfig::from_value(&params).unwrap();
        match config.prompt_source {
            Some(PromptSource::File(f)) => assert_eq!(f, ".agent/PROMPT.md"),
            _ => panic!("expected File prompt source"),
        }
    }
}
