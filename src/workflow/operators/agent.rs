#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::expression::{EvaluationContext, ExpressionEngine};
use crate::workflow::operator::{ExecutionContext, Operator};
use crate::workflow::operators::engine::passthrough::PassthroughDriver;
use crate::workflow::operators::engine::{
    extract_text_from_sdk_event, extract_text_from_stream_json, AikitEngineManager, DriverConfig,
    EngineDriver, EngineInvocation, OutputFormat, PromptSource,
};
use crate::workflow::state::GraphSettings;
use async_trait::async_trait;
use indexmap::IndexMap;
use regex::Regex;
use serde_json::{Map, Number, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

const OUTPUT_CAPTURE_LIMIT_BYTES: usize = 1_048_576;

pub struct AgentOperator {
    workspace_root: PathBuf,
    settings: GraphSettings,
    engine_manager: AikitEngineManager,
}

impl AgentOperator {
    pub fn new(
        workspace_root: PathBuf,
        settings: GraphSettings,
        engine_manager: AikitEngineManager,
    ) -> Self {
        Self {
            workspace_root,
            settings,
            engine_manager,
        }
    }

    /// Construct AgentOperator using aikit-sdk for AI engine delegation.
    pub fn with_aikit_sdk(
        workspace_root: PathBuf,
        settings: GraphSettings,
    ) -> Result<Self, AppError> {
        let engine_manager = AikitEngineManager::new(workspace_root.clone())?;
        Ok(Self::new(workspace_root, settings, engine_manager))
    }

    /// Convenience constructor; delegates to with_aikit_sdk.
    pub fn with_default_registry(workspace_root: PathBuf, settings: GraphSettings) -> Self {
        Self::with_aikit_sdk(workspace_root, settings)
            .expect("AikitEngineManager::new should not fail")
    }
}

/// Parsed from task.params at execution time.
#[derive(Debug, Clone)]
struct AgentOperatorConfig {
    engine: Option<String>,
    model: Option<String>,
    prompt_source: Option<PromptSource>,
    working_dir: Option<String>,
    env: HashMap<String, String>,
    timeout_seconds: Option<u64>,
    /// Ordered map — signal patterns are matched in insertion order.
    signals: IndexMap<String, String>,
    /// YAML key: `loop`. Parsed via params.get("loop").
    loop_mode: bool,
    max_iterations: Option<u32>,
    /// Required when engine = "command".
    engine_command: Option<Vec<String>>,
    /// Whether to stream stdout to the terminal. If None, uses workflow setting.
    stream_stdout: Option<bool>,
}

impl AgentOperatorConfig {
    fn from_value(params: &Value) -> Result<Self, AppError> {
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
}

/// Validate signal patterns in the config.
fn validate_and_compile_signals(
    signals: &IndexMap<String, String>,
) -> Result<IndexMap<String, Regex>, AppError> {
    let mut compiled = IndexMap::new();
    for (name, pattern) in signals {
        if pattern.contains('\n') {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!(
                    "signal '{}' contains \\n; cross-line matching is not supported",
                    name
                ),
            )
            .with_code("WFG-AGENT-004"));
        }
        let re = Regex::new(pattern).map_err(|err| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("invalid regex in signal '{}': {}", name, err),
            )
            .with_code("WFG-AGENT-004")
        })?;
        compiled.insert(name.clone(), re);
    }
    Ok(compiled)
}

/// Match a text line against compiled signals.
/// Returns (signal_name, captured_groups) for the first matching signal.
fn match_signals(
    text: &str,
    signals: &IndexMap<String, Regex>,
) -> Option<(String, HashMap<String, String>)> {
    for (name, re) in signals {
        if let Some(caps) = re.captures(text) {
            let mut data = HashMap::new();
            for cn in re.capture_names().flatten() {
                if let Some(m) = caps.name(cn) {
                    data.insert(cn.to_string(), m.as_str().to_string());
                }
            }
            return Some((name.clone(), data));
        }
    }
    None
}

/// Result from a single engine execution.
struct SingleExecResult {
    signal: Option<String>,
    signal_data: HashMap<String, String>,
    exit_code: i32,
}

/// Bundled paths for an execution run.
struct ExecPaths<'a> {
    working_dir: &'a Path,
    stdout_path: &'a Path,
    stderr_path: &'a Path,
}

/// Execution parameters for agent operations.
struct ExecParams<'a> {
    invocation: &'a EngineInvocation,
    compiled_signals: &'a IndexMap<String, Regex>,
    paths: &'a ExecPaths<'a>,
    extra_env: &'a HashMap<String, String>,
    timeout: Duration,
    start: Instant,
    stream_to_terminal: bool,
}

#[async_trait]
impl Operator for AgentOperator {
    fn name(&self) -> &'static str {
        "AgentOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let config = AgentOperatorConfig::from_value(params)?;

        // Validate signal patterns
        validate_and_compile_signals(&config.signals)?;

        // engine: command requires engine_command
        if config.engine.as_deref() == Some("command") {
            match &config.engine_command {
                None => {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        "engine: command requires engine_command in params",
                    )
                    .with_code("WFG-AGENT-007"));
                }
                Some(cmds) if cmds.is_empty() => {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        "engine_command must not be empty",
                    )
                    .with_code("WFG-AGENT-007"));
                }
                _ => {}
            }
        }

        Ok(())
    }

    async fn execute(&self, params: Value, ctx: ExecutionContext) -> Result<Value, AppError> {
        let config = AgentOperatorConfig::from_value(&params)?;

        // Resolve engine
        let engine_name = config
            .engine
            .as_deref()
            .or(self.settings.default_engine.as_deref())
            .ok_or_else(|| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    "no engine resolved: set params.engine or settings.default_engine",
                )
                .with_code("WFG-AGENT-001")
            })?
            .to_string();

        // Resolve model
        let model = config
            .model
            .as_deref()
            .or_else(|| {
                self.settings
                    .model_stylesheet
                    .as_ref()
                    .map(|ms| ms.model.as_str())
            })
            .map(|s| s.to_string());

        // Validate and compile signal patterns
        let compiled_signals = validate_and_compile_signals(&config.signals)?;

        // Build evaluation context for template interpolation
        let eval_ctx = ctx.state_view.evaluation_context();

        // Interpolate env values
        let interpolated_env = interpolate_env(&config.env, &eval_ctx)?;

        // Set up artifact paths
        let artifact_base = if self.settings.artifact_storage.base_path.is_absolute() {
            self.settings.artifact_storage.base_path.clone()
        } else {
            self.workspace_root
                .join(&self.settings.artifact_storage.base_path)
        };

        let run_seq = ctx.iteration as usize;
        let task_artifact_dir = artifact_base
            .join("workflows")
            .join(&ctx.execution_id)
            .join("task")
            .join(&ctx.task_id)
            .join(run_seq.to_string());

        let stdout_abs_path = task_artifact_dir.join("stdout.txt");
        let stderr_abs_path = task_artifact_dir.join("stderr.txt");

        std::fs::create_dir_all(&task_artifact_dir).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to create artifact directory: {}", err),
            )
        })?;

        // Workspace-relative paths for output
        let stdout_rel_path = stdout_abs_path
            .strip_prefix(&self.workspace_root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| stdout_abs_path.to_string_lossy().to_string());
        let stderr_rel_path = stderr_abs_path
            .strip_prefix(&self.workspace_root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| stderr_abs_path.to_string_lossy().to_string());

        // events_artifact and token_usage are only set for AI engine (SDK) paths
        let mut sdk_events_artifact: Option<String> = None;
        // token_usage from the SDK run result.
        let mut sdk_events_token_usage: Option<serde_json::Value> = None;

        let (signal, signal_data, exit_code, final_iteration) = if engine_name == "command" {
            // Command engine: use PassthroughDriver and subprocess execution
            let resolved_engine_command = match &config.engine_command {
                None => {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        "engine: command requires engine_command in params",
                    )
                    .with_code("WFG-AGENT-007"));
                }
                Some(cmds) => {
                    let expr_engine = ExpressionEngine::default();
                    let mut result = Vec::new();
                    for entry in cmds {
                        let interpolated = expr_engine.interpolate_string(entry, &eval_ctx)?;
                        result.push(interpolated);
                    }
                    if result.is_empty() {
                        return Err(AppError::new(
                            ErrorCategory::ValidationError,
                            "engine_command evaluates to empty list",
                        )
                        .with_code("WFG-AGENT-007"));
                    }
                    result
                }
            };

            let driver = PassthroughDriver;
            let driver_config = DriverConfig {
                model: model.as_deref(),
                prompt_source: config.prompt_source.as_ref(),
                engine_command: Some(&resolved_engine_command),
            };
            let invocation = driver.build_invocation(&driver_config, &self.workspace_root)?;

            // Resolve timeout
            let timeout_duration = config
                .timeout_seconds
                .map(Duration::from_secs)
                .unwrap_or_else(|| Duration::from_secs(self.settings.max_time_seconds));

            // Resolve working directory
            let working_dir = config
                .working_dir
                .as_deref()
                .map(|d| self.workspace_root.join(d))
                .unwrap_or_else(|| self.workspace_root.clone());

            let stream_to_terminal = config
                .stream_stdout
                .unwrap_or(self.settings.stream_agent_stdout);

            let exec_paths = ExecPaths {
                working_dir: &working_dir,
                stdout_path: &stdout_abs_path,
                stderr_path: &stderr_abs_path,
            };

            let start = Instant::now();
            let exec_params = ExecParams {
                invocation: &invocation,
                compiled_signals: &compiled_signals,
                paths: &exec_paths,
                extra_env: &interpolated_env,
                timeout: timeout_duration,
                start,
                stream_to_terminal,
            };

            if config.loop_mode {
                execute_loop(&config, &exec_params).await?
            } else {
                let result = execute_single(&exec_params).await?;
                (result.signal, result.signal_data, result.exit_code, 1u32)
            }
        } else {
            // AI engine: delegate to aikit-sdk via AikitEngineManager
            let prompt = resolve_prompt(&config, &self.engine_manager.workspace_root)?;

            // Resolve timeout
            let timeout_duration = config
                .timeout_seconds
                .map(Duration::from_secs)
                .unwrap_or_else(|| Duration::from_secs(self.settings.max_time_seconds));

            // Path for the NDJSON events artifact
            let events_ndjson_abs_path = task_artifact_dir.join("events.ndjson");

            let sdk_result = execute_sdk_engine(
                &self.engine_manager,
                &engine_name,
                &prompt,
                model.as_deref(),
                &config,
                &compiled_signals,
                &stdout_abs_path,
                &stderr_abs_path,
                &events_ndjson_abs_path,
                &self.workspace_root,
                timeout_duration,
            )
            .await?;

            sdk_events_artifact = sdk_result.events_artifact_path;
            sdk_events_token_usage = sdk_result.token_usage;

            (
                sdk_result.signal,
                sdk_result.signal_data,
                sdk_result.exit_code,
                sdk_result.iteration,
            )
        };

        // Determine stderr artifact
        let stderr_artifact = if stderr_abs_path.exists()
            && stderr_abs_path
                .metadata()
                .map(|m| m.len() > 0)
                .unwrap_or(false)
        {
            Value::String(stderr_rel_path)
        } else {
            Value::Null
        };

        // Build output
        let mut output_map = Map::new();

        let signal_value = match signal {
            Some(ref s) => Value::String(s.clone()),
            None => {
                if config.signals.is_empty() {
                    Value::String("exited".to_string())
                } else {
                    Value::Null
                }
            }
        };
        output_map.insert("signal".to_string(), signal_value);
        output_map.insert(
            "signal_data".to_string(),
            Value::Object(
                signal_data
                    .into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect(),
            ),
        );
        output_map.insert(
            "exit_code".to_string(),
            Value::Number(Number::from(exit_code)),
        );
        output_map.insert(
            "stdout_artifact".to_string(),
            Value::String(stdout_rel_path),
        );
        output_map.insert("stderr_artifact".to_string(), stderr_artifact);
        if config.loop_mode {
            output_map.insert(
                "iteration".to_string(),
                Value::Number(Number::from(final_iteration)),
            );
        }

        // AI engine SDK outputs: token_usage from the SDK run result,
        // and events_artifact NDJSON path.
        if engine_name != "command" {
            // Use SDK token_usage when available; null otherwise.
            let token_usage_value = sdk_events_token_usage.unwrap_or(Value::Null);
            output_map.insert("token_usage".to_string(), token_usage_value);
        }

        if let Some(events_path) = sdk_events_artifact {
            output_map.insert("events_artifact".to_string(), Value::String(events_path));
        }

        Ok(Value::Object(output_map))
    }
}

/// Resolve the prompt string from config.
fn resolve_prompt(
    config: &AgentOperatorConfig,
    workspace_root: &std::path::Path,
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

/// Result of SDK engine execution (signal, signal_data, exit_code, iteration, events_artifact_rel_path, token_usage).
struct SdkExecResult {
    signal: Option<String>,
    signal_data: HashMap<String, String>,
    exit_code: i32,
    iteration: u32,
    events_artifact_path: Option<String>,
    /// Aggregated token usage from the SDK run.
    /// `None` if no token usage was available from the engine during execution.
    token_usage: Option<serde_json::Value>,
}

/// Execute an AI engine via aikit-sdk, handling loop mode and signal matching.
/// Writes a NDJSON events artifact using SDK AgentEvent JSON serialization.
#[allow(clippy::too_many_arguments)]
async fn execute_sdk_engine(
    manager: &AikitEngineManager,
    engine_name: &str,
    prompt: &str,
    model: Option<&str>,
    config: &AgentOperatorConfig,
    compiled_signals: &IndexMap<String, Regex>,
    stdout_path: &std::path::Path,
    stderr_path: &std::path::Path,
    events_ndjson_path: &std::path::Path,
    workspace_root: &std::path::Path,
    timeout: Duration,
) -> Result<SdkExecResult, AppError> {
    use std::io::Write;

    let max_iters = if config.loop_mode {
        config.max_iterations.unwrap_or(u32::MAX)
    } else {
        1
    };

    let mut iteration: u32 = 0;
    let mut last_signal: Option<String> = None;
    let mut last_signal_data: HashMap<String, String> = HashMap::new();
    let last_exit_code: i32 = 0;
    let start = Instant::now();
    // Fallback token usage: latest TokenUsageLine seen in event stream, if RunResult.token_usage is None.
    let mut fallback_token_usage: Option<serde_json::Value> = None;
    // Primary token usage from the most recent RunResult.token_usage (precedence over fallback).
    let mut primary_token_usage: Option<serde_json::Value> = None;

    // NDJSON events artifact writer (opened once, appended across iterations)
    let mut events_ndjson_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(events_ndjson_path)
        .map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to open events NDJSON artifact: {}", e),
            )
            .with_code("WFG-SDK-003")
        })?;

    loop {
        iteration += 1;
        if iteration > max_iters {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("agent exceeded max_iterations ({}) in loop mode", max_iters),
            )
            .with_code("WFG-AGENT-003"));
        }

        // Check timeout
        if start.elapsed() >= timeout {
            return Err(AppError::new(
                ErrorCategory::TimeoutError,
                "agent operator timeout exceeded during SDK execution",
            )
            .with_code("WFG-AGENT-005"));
        }

        // Execute via SDK, passing remaining wall-clock time as the SDK timeout
        let remaining = timeout.saturating_sub(start.elapsed());
        let (events, iter_run_result) = tokio::time::timeout(
            remaining,
            manager.execute_engine_events(engine_name, prompt, model, Some(remaining)),
        )
        .await
        .map_err(|_| {
            AppError::new(
                ErrorCategory::TimeoutError,
                "agent operator timeout exceeded during SDK execution",
            )
            .with_code("WFG-AGENT-005")
        })??;

        // Write stdout events to artifact file
        let mut stdout_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(stdout_path)
            .map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to open stdout artifact: {}", e),
                )
            })?;

        let mut stdout_bytes: usize = 0;
        let mut signal_found: Option<String> = None;
        let mut signal_data_found: HashMap<String, String> = HashMap::new();

        for event in &events {
            // Write every event to NDJSON artifact using SDK AgentEvent JSON serialization
            let event_json = serde_json::to_string(event).map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to serialize event to NDJSON artifact: {}", e),
                )
                .with_code("WFG-SDK-003")
            })?;
            events_ndjson_file
                .write_all(event_json.as_bytes())
                .and_then(|_| events_ndjson_file.write_all(b"\n"))
                .map_err(|e| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!("failed to write event to NDJSON artifact: {}", e),
                    )
                    .with_code("WFG-SDK-003")
                })?;

            // Use SDK AgentEventPayload enum matching per spec
            match &event.payload {
                // TokenUsageLine: track for fallback; MUST NOT participate in signal matching
                aikit_sdk::AgentEventPayload::TokenUsageLine { usage, .. } => {
                    fallback_token_usage = serde_json::to_value(usage).ok();
                    continue;
                }
                // RawBytes: MUST NOT participate in signal matching or stdout artifact
                aikit_sdk::AgentEventPayload::RawBytes(_) => {
                    continue;
                }
                // RawLine and JsonLine: write to stdout artifact and attempt signal matching
                aikit_sdk::AgentEventPayload::RawLine(_)
                | aikit_sdk::AgentEventPayload::JsonLine(_) => {}
            }

            if let Some(text) = extract_text_from_sdk_event(event) {
                if matches!(event.stream, aikit_sdk::AgentEventStream::Stdout)
                    && stdout_bytes + text.len() < OUTPUT_CAPTURE_LIMIT_BYTES
                {
                    let _ = stdout_file.write_all(text.as_bytes());
                    let _ = stdout_file.write_all(b"\n");
                    stdout_bytes += text.len() + 1;
                }

                // Both stdout and stderr events participate in signal matching
                if signal_found.is_none() {
                    if let Some((sig_name, sig_data)) = match_signals(&text, compiled_signals) {
                        signal_found = Some(sig_name);
                        signal_data_found = sig_data;
                    }
                }
            }
        }

        // Write stderr events to stderr artifact — use SDK enum matching
        let stderr_events: Vec<&aikit_sdk::AgentEvent> = events
            .iter()
            .filter(|e| matches!(e.stream, aikit_sdk::AgentEventStream::Stderr))
            .collect();
        if !stderr_events.is_empty() {
            let mut stderr_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(stderr_path)
                .map_err(|e| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!("failed to open stderr artifact: {}", e),
                    )
                })?;
            let mut stderr_bytes: usize = 0;
            for event in &stderr_events {
                // Only write RawLine/JsonLine to stderr artifact; skip RawBytes
                match &event.payload {
                    aikit_sdk::AgentEventPayload::RawLine(s) => {
                        if stderr_bytes + s.len() < OUTPUT_CAPTURE_LIMIT_BYTES {
                            let _ = stderr_file.write_all(s.as_bytes());
                            let _ = stderr_file.write_all(b"\n");
                            stderr_bytes += s.len() + 1;
                        }
                    }
                    aikit_sdk::AgentEventPayload::JsonLine(v) => {
                        let text = v.to_string();
                        if stderr_bytes + text.len() < OUTPUT_CAPTURE_LIMIT_BYTES {
                            let _ = stderr_file.write_all(text.as_bytes());
                            let _ = stderr_file.write_all(b"\n");
                            stderr_bytes += text.len() + 1;
                        }
                    }
                    aikit_sdk::AgentEventPayload::RawBytes(_)
                    | aikit_sdk::AgentEventPayload::TokenUsageLine { .. } => {}
                }
            }
        }

        // Update primary token usage from this iteration's RunResult (precedence over stream events).
        if let Some(ref usage) = iter_run_result.token_usage {
            primary_token_usage = serde_json::to_value(usage).ok();
        }

        if let Some(sig) = signal_found {
            last_signal = Some(sig);
            last_signal_data = signal_data_found;
            break;
        }

        if !config.loop_mode {
            break;
        }
    }

    // Compute relative path to events artifact
    let events_artifact_path = events_ndjson_path
        .strip_prefix(workspace_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| events_ndjson_path.to_string_lossy().to_string());

    // Token usage precedence: RunResult.token_usage when Some, else latest TokenUsageLine fallback.
    let token_usage = primary_token_usage.or(fallback_token_usage);

    Ok(SdkExecResult {
        signal: last_signal,
        signal_data: last_signal_data,
        exit_code: last_exit_code,
        iteration,
        events_artifact_path: Some(events_artifact_path),
        token_usage,
    })
}

/// Interpolate template expressions in env values.
fn interpolate_env(
    env: &HashMap<String, String>,
    eval_ctx: &EvaluationContext,
) -> Result<HashMap<String, String>, AppError> {
    let engine = ExpressionEngine::default();
    let mut result = HashMap::new();
    for (k, v) in env {
        let interpolated = engine.interpolate_string(v, eval_ctx)?;
        result.insert(k.clone(), interpolated);
    }
    Ok(result)
}

/// Check if timeout has been exceeded before starting execution.
fn check_timeout_before_execution(params: &ExecParams<'_>) -> Result<(), AppError> {
    if params.start.elapsed() >= params.timeout {
        return Err(AppError::new(
            ErrorCategory::TimeoutError,
            "agent operator timeout exceeded before execution",
        )
        .with_code("WFG-AGENT-005"));
    }
    Ok(())
}

const CMD_LOG_ARG_MAX_LEN: usize = 200;

/// Spawn the engine process and set up stderr capture.
async fn spawn_engine_process(
    params: &ExecParams<'_>,
) -> Result<
    (
        tokio::process::Child,
        tokio::process::ChildStdout,
        tokio::task::JoinHandle<()>,
    ),
    AppError,
> {
    use tokio::io::{AsyncReadExt, BufReader};

    let cmd_display: Vec<String> = params
        .invocation
        .command
        .iter()
        .map(|a| {
            if a.len() > CMD_LOG_ARG_MAX_LEN {
                format!("{}... ({} chars)", &a[..CMD_LOG_ARG_MAX_LEN], a.len())
            } else {
                a.clone()
            }
        })
        .collect();
    tracing::debug!(
        cmd = ?cmd_display,
        cwd = ?params.paths.working_dir,
        "AgentOperator executing engine"
    );

    let mut cmd_builder = build_command(
        params.invocation,
        params.paths.working_dir,
        params.extra_env,
    )?;

    let mut child = cmd_builder.spawn().map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to start engine process: {}", err),
        )
        .with_code("WFG-AGENT-002")
    })?;

    let stdout = child.stdout.take().expect("stdout must be piped");
    let stderr = child.stderr.take().expect("stderr must be piped");

    // Spawn stderr capture task
    let stderr_path_owned = params.paths.stderr_path.to_owned();
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf).await;
        if !buf.is_empty() {
            let limited = &buf[..buf.len().min(OUTPUT_CAPTURE_LIMIT_BYTES)];
            let _ = std::fs::write(&stderr_path_owned, limited);
        }
    });

    Ok((child, stdout, stderr_task))
}

/// Open the stdout artifact file for writing.
fn open_stdout_artifact_file(stdout_path: &std::path::Path) -> Result<std::fs::File, AppError> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(stdout_path)
        .map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to open stdout artifact: {}", err),
            )
        })
}

/// Result of streaming stdout from the engine process.
struct StreamingResult {
    signal: Option<String>,
    signal_data: HashMap<String, String>,
}

/// Stream and process stdout from the engine process.
async fn stream_and_process_output(
    stdout: tokio::process::ChildStdout,
    stdout_file: &mut std::fs::File,
    child: &mut tokio::process::Child,
    params: &ExecParams<'_>,
) -> Result<StreamingResult, AppError> {
    use std::io::Write;
    use tokio::io::{AsyncWriteExt, BufReader};

    let mut stdout_bytes_written: usize = 0;
    let mut signal: Option<String> = None;
    let mut signal_data: HashMap<String, String> = HashMap::new();
    let output_format = params.invocation.output_format.clone();

    let mut lines = BufReader::new(stdout).lines();

    // Stream stdout with timeout
    let remaining = params.timeout.saturating_sub(params.start.elapsed());
    let stream_result = tokio::time::timeout(remaining, async {
        while let Some(line_result) = lines.next_line().await.transpose() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };

            let text = line.trim_end_matches(['\n', '\r']).to_string();

            // Extract text for stream-json format
            let text_for_matching = if output_format == OutputFormat::StreamJson {
                match extract_text_from_stream_json(&text) {
                    Some(t) => t,
                    None => {
                        // Write raw line to artifact even if not a content line
                        if stdout_bytes_written + text.len() < OUTPUT_CAPTURE_LIMIT_BYTES {
                            let _ = stdout_file.write_all(text.as_bytes());
                            let _ = stdout_file.write_all(b"\n");
                            stdout_bytes_written += text.len() + 1;
                        }

                        // For stream-json, we don't stream raw lines that aren't content
                        // Only the extracted text should be streamed
                        continue;
                    }
                }
            } else {
                text.clone()
            };

            // Write to artifact
            if stdout_bytes_written + text_for_matching.len() < OUTPUT_CAPTURE_LIMIT_BYTES {
                let _ = stdout_file.write_all(text_for_matching.as_bytes());
                let _ = stdout_file.write_all(b"\n");
                stdout_bytes_written += text_for_matching.len() + 1;
            }

            // Stream to terminal if enabled
            if params.stream_to_terminal {
                let mut terminal_stdout = tokio::io::stdout();
                let _ = terminal_stdout
                    .write_all(text_for_matching.as_bytes())
                    .await;
                let _ = terminal_stdout.write_all(b"\n").await;
                let _ = terminal_stdout.flush().await;
            }

            // Signal matching
            if let Some((sig_name, sig_data)) =
                match_signals(&text_for_matching, params.compiled_signals)
            {
                signal = Some(sig_name);
                signal_data = sig_data;
                // Single-shot: kill the process
                let _ = child.kill().await;
                break;
            }
        }
    })
    .await;

    if stream_result.is_err() {
        // Timeout
        let _ = child.kill().await;
        return Err(AppError::new(
            ErrorCategory::TimeoutError,
            "agent operator timeout exceeded during execution",
        )
        .with_code("WFG-AGENT-005"));
    }

    Ok(StreamingResult {
        signal,
        signal_data,
    })
}

/// Wait for the process to complete and return the exit code.
async fn wait_for_process_completion(
    mut child: tokio::process::Child,
    stderr_task: tokio::task::JoinHandle<()>,
) -> Result<i32, AppError> {
    let exit_status = child.wait().await.map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to wait for engine process: {}", err),
        )
    })?;

    let _ = stderr_task.await;

    Ok(exit_status.code().unwrap_or(-1))
}

/// Execute a single engine invocation and stream output.
async fn execute_single(params: &ExecParams<'_>) -> Result<SingleExecResult, AppError> {
    check_timeout_before_execution(params)?;

    let (mut child, stdout, stderr_task) = spawn_engine_process(params).await?;
    let mut stdout_file = open_stdout_artifact_file(params.paths.stdout_path)?;

    let streaming_result =
        stream_and_process_output(stdout, &mut stdout_file, &mut child, params).await?;

    let exit_code = wait_for_process_completion(child, stderr_task).await?;

    Ok(SingleExecResult {
        signal: streaming_result.signal,
        signal_data: streaming_result.signal_data,
        exit_code,
    })
}

/// Execute in loop mode.
async fn execute_loop(
    config: &AgentOperatorConfig,
    params: &ExecParams<'_>,
) -> Result<(Option<String>, HashMap<String, String>, i32, u32), AppError> {
    let max_iters = config.max_iterations.unwrap_or(u32::MAX);
    let mut iteration: u32 = 0;
    let mut last_exit_code: i32;
    let mut last_signal: Option<String>;
    let mut last_signal_data: HashMap<String, String>;

    loop {
        iteration += 1;
        if iteration > max_iters {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("agent exceeded max_iterations ({}) in loop mode", max_iters),
            )
            .with_code("WFG-AGENT-003"));
        }

        // Reset per-iteration signal state
        last_signal = None;
        last_signal_data = HashMap::new();

        let result = execute_single(params).await?;

        last_exit_code = result.exit_code;

        if let Some(sig) = result.signal {
            last_signal = Some(sig);
            last_signal_data = result.signal_data;
            break;
        }

        if result.exit_code != 0 {
            break;
        }
    }

    Ok((last_signal, last_signal_data, last_exit_code, iteration))
}

/// Build a tokio Command from the invocation.
fn build_command(
    invocation: &EngineInvocation,
    working_dir: &Path,
    extra_env: &HashMap<String, String>,
) -> Result<Command, AppError> {
    if invocation.command.is_empty() {
        return Err(
            AppError::new(ErrorCategory::ValidationError, "engine command is empty")
                .with_code("WFG-AGENT-002"),
        );
    }

    let mut cmd = Command::new(&invocation.command[0]);
    if invocation.command.len() > 1 {
        cmd.args(&invocation.command[1..]);
    }

    cmd.current_dir(working_dir);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::null());

    // Apply driver env
    for (k, v) in &invocation.env {
        cmd.env(k, v);
    }

    // Apply extra env (from params)
    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::executor::GraphHandle;
    use crate::workflow::operator::StateView;
    use crate::workflow::schema::WorkflowSettings;
    use serde_json::json;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_ctx(workspace: &TempDir) -> ExecutionContext {
        ExecutionContext {
            workspace_path: workspace.path().to_path_buf(),
            execution_id: "test-exec-001".to_string(),
            task_id: "agent".to_string(),
            iteration: 1,
            state_view: StateView::new(json!({}), json!({}), json!({})),
            graph: GraphHandle::new(HashMap::new()), // Empty graph for test
        }
    }

    fn make_settings_with_engine(engine: &str) -> WorkflowSettings {
        WorkflowSettings {
            default_engine: Some(engine.to_string()),
            ..WorkflowSettings::default()
        }
    }

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

    #[test]
    fn validate_params_rejects_invalid_regex() {
        let tmp = TempDir::new().unwrap();
        let settings = make_settings_with_engine("opencode");
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let params = json!({
            "engine": "command",
            "engine_command": ["echo", "hello"],
            "signals": {
                "bad": "["
            }
        });
        let err = op.validate_params(&params).unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-004");
    }

    #[test]
    fn validate_params_rejects_newline_in_signal() {
        let tmp = TempDir::new().unwrap();
        let settings = make_settings_with_engine("opencode");
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let params = json!({
            "engine": "command",
            "engine_command": ["echo"],
            "signals": {
                "bad": "foo\nbar"
            }
        });
        let err = op.validate_params(&params).unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-004");
    }

    #[test]
    fn validate_params_rejects_command_without_engine_command() {
        let tmp = TempDir::new().unwrap();
        let settings = make_settings_with_engine("command");
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let params = json!({"engine": "command"});
        let err = op.validate_params(&params).unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-007");
    }

    #[tokio::test]
    async fn execute_no_engine_returns_agent_001() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default(); // no default_engine
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({}); // no engine field
        let err = op.execute(params, ctx).await.unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-001");
    }

    #[tokio::test]
    async fn execute_non_runnable_ai_engine_returns_sdk_002() {
        // After SDK delegation, engines that are not runnable (not in aikit-sdk runnable list)
        // return AgentNotRunnable → WFG-SDK-002.
        // "copilot" is a known agent key but NOT in the runnable list.
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings {
            default_engine: Some("copilot".to_string()),
            ..WorkflowSettings::default()
        };
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "prompt": "test prompt"
        });
        let err = op.execute(params, ctx).await.unwrap_err();
        // SDK delegation: AgentNotRunnable → WFG-SDK-002
        assert_eq!(err.code, "WFG-SDK-002");
    }

    #[tokio::test]
    async fn execute_command_engine_no_engine_command_returns_agent_007() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({"engine": "command"});
        let err = op.execute(params, ctx).await.unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-007");
    }

    #[tokio::test]
    async fn execute_single_shot_captures_signal() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo '<promise>COMPLETE</promise>'"],
            "signals": {
                "complete": "<promise>COMPLETE</promise>"
            }
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("complete"));
    }

    #[tokio::test]
    async fn execute_captures_named_group_in_signal_data() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo '<promise>BLOCKED:cannot find file</promise>'"],
            "signals": {
                "blocked": "<promise>BLOCKED:(?P<reason>[^<]+)</promise>"
            }
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("blocked"));
        assert_eq!(result["signal_data"]["reason"], json!("cannot find file"));
    }

    #[tokio::test]
    async fn execute_no_signals_sets_exited() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo hello"]
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("exited"));
    }

    #[tokio::test]
    async fn execute_engine_resolution_from_settings() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings {
            default_engine: Some("command".to_string()),
            ..WorkflowSettings::default()
        };
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            // no engine in params - should resolve from settings
            "engine_command": ["bash", "-c", "echo hi"]
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("exited"));
    }

    #[tokio::test]
    async fn execute_loop_mode_signals_on_second_iteration() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);

        let counter_file = tmp.path().join("counter.txt");
        std::fs::write(&counter_file, "0").unwrap();
        let script = format!(
            r#"COUNT=$(cat {0})
COUNT=$((COUNT + 1))
echo $COUNT > {0}
if [ "$COUNT" -ge 2 ]; then
  echo '<promise>COMPLETE</promise>'
fi"#,
            counter_file.display()
        );

        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", script],
            "loop": true,
            "max_iterations": 5,
            "signals": {
                "complete": "<promise>COMPLETE</promise>"
            }
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("complete"));
        assert_eq!(result["iteration"], json!(2));
    }

    #[tokio::test]
    async fn execute_loop_mode_exceeds_max_iterations_returns_agent_003() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo nothing"],
            "loop": true,
            "max_iterations": 2,
            "signals": {
                "complete": "<promise>COMPLETE</promise>"
            }
        });
        // Iteration 1: no signal, exit 0 → continue
        // Iteration 2: no signal, exit 0 → continue
        // Iteration 3: would exceed → WFG-AGENT-003
        let err = op.execute(params, ctx).await.unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-003");
    }

    #[tokio::test]
    async fn execute_model_from_stylesheet() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings {
            model_stylesheet: Some(crate::workflow::schema::ModelStylesheet {
                model: "test-model".to_string(),
                context_fidelity: crate::workflow::schema::ContextFidelity::Summary,
            }),
            ..WorkflowSettings::default()
        };
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo hi"]
        });
        // No model error for command driver
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("exited"));
    }

    #[tokio::test]
    async fn execute_stderr_artifact_set_when_stderr_produced() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo error >&2; echo hello"]
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert!(result["stderr_artifact"].is_string());
    }

    #[tokio::test]
    async fn execute_first_matching_signal_wins() {
        // Test that only ONE signal is set per line (not both).
        // The spec says "first matching pattern wins; remaining patterns are not tested".
        // Signal names are matched in iteration order of the IndexMap, which is built
        // from the serde_json Map (BTreeMap) in alphabetical key order.
        // "aaa_complete" < "bbb_any" alphabetically, so "aaa_complete" wins.
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo '<promise>COMPLETE</promise>'"],
            "signals": {
                "aaa_complete": "<promise>COMPLETE</promise>",
                "bbb_any": "COMPLETE"
            }
        });
        let result = op.execute(params, ctx).await.unwrap();
        // "aaa_complete" is alphabetically first and matches → it wins
        assert_eq!(result["signal"], json!("aaa_complete"));
    }
}
