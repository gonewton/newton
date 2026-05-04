#![allow(clippy::result_large_err)]

mod artifacts;
mod command;
mod config;
mod output;
mod quota;
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
use serde_json::Value;
use std::path::PathBuf;
use std::time::{Duration, Instant};

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

        let interpolated_env = command::interpolate_env(&config.env, &eval_ctx)?;

        let paths = artifacts::setup_artifact_paths(&self.workspace_root, &self.settings, &ctx)?;

        let mut sdk_events_artifact: Option<String> = None;
        let mut sdk_events_token_usage: Option<serde_json::Value> = None;

        let (signal, signal_data, exit_code, final_iteration) = if engine_name == "command" {
            config.validate_engine_command()?;
            let resolved_engine_command = {
                let cmds = config.engine_command.as_deref().unwrap_or(&[]);
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
            };

            let driver = PassthroughDriver;
            let driver_config = DriverConfig {
                model: model.as_deref(),
                prompt_source: config.prompt_source.as_ref(),
                engine_command: Some(&resolved_engine_command),
            };
            let invocation = driver.build_invocation(&driver_config, &self.workspace_root)?;

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
                command::execute_loop(&config, &exec_params).await?
            } else {
                let result = command::execute_single(&exec_params).await?;
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

            (
                sdk_result.signal,
                sdk_result.signal_data,
                sdk_result.exit_code,
                sdk_result.iteration,
            )
        };

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
        }))
    }
}
