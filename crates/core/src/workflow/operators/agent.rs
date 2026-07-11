#![allow(clippy::result_large_err)]

mod artifacts;
mod command;
mod config;
mod output;
pub(crate) mod quota;
mod sdk;
mod signals;

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::expression::ExpressionEngine;
use crate::workflow::operator::{ExecutionContext, Operator};
use crate::workflow::operators::engine::passthrough::PassthroughDriver;
use crate::workflow::operators::engine::{AikitEngineManager, DriverConfig, EngineDriver};
use crate::workflow::state::GraphSettings;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct AgentParams {
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub prompt_file: Option<String>,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub signals: Option<serde_json::Value>,
    #[serde(rename = "loop", default)]
    pub loop_mode: bool,
    #[serde(default)]
    pub max_iterations: Option<u32>,
    #[serde(default)]
    pub engine_command: Option<Vec<String>>,
    #[serde(default)]
    pub stream_stdout: Option<bool>,
    #[serde(default)]
    pub require_signal: bool,
}

/// Why the agent operator stopped executing the engine.
///
/// `signal_matched`: a configured `signals` pattern matched the engine's
/// output (for the command engine this is when the child is killed, which
/// is why `exit_code` is `null` in that case). `exited`: the engine process
/// ran to completion on its own (with or without signals configured).
///
/// No `timeout` variant: both the operator-internal timeout
/// (`timeout_seconds`, `WFG-AGENT-005`) and the outer per-task
/// `timeout_ms` (`WFG-TIME-002`) return `Err` before any output value is
/// constructed, so a `timeout` stop reason can never actually appear on an
/// agent operator output today. Adding an enum value that no code path can
/// produce would be exactly the kind of fabricated contract this change is
/// meant to eliminate.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    SignalMatched,
    Exited,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct AgentSchemaOutput {
    pub signal: Option<String>,
    pub stdout_artifact: Option<String>,
    /// `null` when the child was killed after a signal match (it has no
    /// exit code); numeric on a genuine process exit.
    pub exit_code: Option<i32>,
    pub stop_reason: StopReason,
}

use self::command::{ExecParams, ExecPaths};
use self::config::AgentOperatorConfig;
use self::output::AgentOutput;

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

#[async_trait]
impl Operator for AgentOperator {
    fn name(&self) -> &'static str {
        "AgentOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let config = AgentOperatorConfig::from_value(params)?;
        signals::validate_and_compile_signals(&config.signals)?;
        config.validate_engine_command()?;
        Ok(())
    }

    fn params_schema(&self) -> schemars::Schema {
        schemars::schema_for!(AgentParams)
    }

    fn output_schema(&self) -> schemars::Schema {
        schemars::schema_for!(AgentSchemaOutput)
    }

    async fn execute(&self, params: Value, ctx: ExecutionContext) -> Result<Value, AppError> {
        let config = AgentOperatorConfig::from_value(&params)?;

        let engine_name = config.resolve_engine(self.settings.default_engine.as_deref())?;

        let model = config
            .model
            .as_deref()
            .or_else(|| {
                self.settings
                    .model_stylesheet
                    .as_ref()
                    .map(|ms| ms.model.as_str())
            })
            .map(std::string::ToString::to_string);

        let compiled_signals = signals::validate_and_compile_signals(&config.signals)?;

        let eval_ctx = ctx.state_view.evaluation_context();

        let mut interpolated_env =
            command::interpolate_env(&config.env, &eval_ctx, self.settings.allow_env_fn)?;

        let paths = artifacts::setup_artifact_paths(&self.workspace_root, &self.settings, &ctx)?;

        let mut sdk_events_artifact: Option<String> = None;
        let mut sdk_events_token_usage: Option<serde_json::Value> = None;
        // Surfaces truncation of the stdout/stderr capture artifacts (either
        // a genuine write failure or hitting `OUTPUT_CAPTURE_LIMIT_BYTES`) on
        // the task result, since the artifact file itself only gets a
        // best-effort `[capture truncated: ...]` marker line — see spec 074
        // S15 and `output::build_agent_output`.
        let stdout_capture_warning: Option<String>;
        let stderr_capture_warning: Option<String>;

        let (signal, signal_data, exit_code, final_iteration) = if engine_name == "command" {
            config.validate_engine_command()?;
            let resolved_engine_command = {
                let cmds = config.engine_command.as_deref().unwrap_or(&[]);
                let expr_engine = ExpressionEngine::new(self.settings.allow_env_fn);
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
            };

            let driver = PassthroughDriver;
            let driver_config = DriverConfig {
                model: model.as_deref(),
                prompt_source: config.prompt_source.as_ref(),
                engine_command: Some(&resolved_engine_command),
            };
            let invocation = driver.build_invocation(&driver_config, &self.workspace_root)?;

            // Inject NEWTON_STATE_DIR only if neither the explicit workflow
            // YAML `env` nor the driver-built invocation env already set it —
            // explicit config always wins. `build_command` (command.rs)
            // applies `invocation.env` first and `extra_env` second, so an
            // unconditional insert here would silently override an explicit
            // `invocation.env` entry.
            if let Some(state_dir) = &ctx.execution_overrides.state_dir {
                let already_set = interpolated_env.contains_key("NEWTON_STATE_DIR")
                    || invocation.env.iter().any(|(k, _)| k == "NEWTON_STATE_DIR");
                if !already_set {
                    interpolated_env.insert(
                        "NEWTON_STATE_DIR".to_string(),
                        state_dir.display().to_string(),
                    );
                }
            }

            let timeout_duration = config.timeout_seconds.map_or_else(
                || Duration::from_secs(self.settings.max_time_seconds),
                Duration::from_secs,
            );
            let working_dir = config.working_dir.as_deref().map_or_else(
                || self.workspace_root.clone(),
                |d| self.workspace_root.join(d),
            );
            let stream_to_terminal = config
                .stream_stdout
                .unwrap_or(self.settings.stream_agent_stdout);
            let exec_paths = ExecPaths {
                working_dir: &working_dir,
                stdout_path: &paths.stdout_abs,
                stderr_path: &paths.stderr_abs,
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
                let loop_result = command::execute_loop(&config, &exec_params).await?;
                stdout_capture_warning = loop_result.stdout_capture_warning;
                stderr_capture_warning = loop_result.stderr_capture_warning;
                (
                    loop_result.signal,
                    loop_result.signal_data,
                    loop_result.exit_code,
                    loop_result.iteration,
                )
            } else {
                let result = command::execute_single(&exec_params).await?;
                stdout_capture_warning = result.stdout_capture_warning;
                stderr_capture_warning = result.stderr_capture_warning;
                (result.signal, result.signal_data, result.exit_code, 1u32)
            }
        } else {
            let prompt = output::resolve_prompt(&config, &self.engine_manager.workspace_root)?;
            let timeout_duration = config.timeout_seconds.map_or_else(
                || Duration::from_secs(self.settings.max_time_seconds),
                Duration::from_secs,
            );
            let events_ndjson_abs_path = paths.task_artifact_dir.join("events.ndjson");

            let sdk_result = sdk::execute_sdk_engine(
                &self.engine_manager,
                &engine_name,
                &prompt,
                model.as_deref(),
                &config,
                &compiled_signals,
                &paths.stdout_abs,
                &paths.stderr_abs,
                &events_ndjson_abs_path,
                &self.workspace_root,
                timeout_duration,
            )
            .await?;

            sdk_events_artifact = sdk_result.events_artifact_path;
            sdk_events_token_usage = sdk_result.token_usage;
            stdout_capture_warning = sdk_result.stdout_capture_warning;
            stderr_capture_warning = sdk_result.stderr_capture_warning;

            (
                sdk_result.signal,
                sdk_result.signal_data,
                sdk_result.exit_code,
                sdk_result.iteration,
            )
        };

        if config.require_signal && !config.signals.is_empty() && signal.is_none() {
            let mut err = AppError::new(
                ErrorCategory::ValidationError,
                "agent did not emit any configured signal",
            )
            .with_code("WFG-AGENT-009");
            err.add_context("stdout_artifact", &paths.stdout_rel);
            err.add_context(
                "stderr_artifact",
                if paths.stderr_abs.exists() {
                    &paths.stderr_rel
                } else {
                    "null"
                },
            );
            err.add_context("engine", &engine_name);
            if let Some(ref m) = config.model {
                err.add_context("model", m.as_str());
            }
            return Err(err);
        }

        Ok(output::build_agent_output(AgentOutput {
            signal,
            signal_data,
            exit_code,
            final_iteration,
            stdout_rel: paths.stdout_rel,
            stderr_abs: paths.stderr_abs,
            stderr_rel: paths.stderr_rel,
            loop_mode: config.loop_mode,
            signals_empty: config.signals.is_empty(),
            engine_is_command: engine_name == "command",
            sdk_token_usage: sdk_events_token_usage,
            sdk_events_artifact,
            stdout_capture_warning,
            stderr_capture_warning,
        }))
    }
}
