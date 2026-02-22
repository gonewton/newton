#![allow(clippy::unnecessary_cast)] // legacy data structures rely on explicit casts to normalize persisted numeric values.

use crate::ailoop_integration::{AiloopContext, OrchestratorNotifier, OutputForwarder};
use crate::core::entities::*;
use crate::core::entities::{ExecutionConfiguration, Iteration, ToolMetadata};
use crate::core::error::{AppError, ErrorReporter};
use crate::tools::ToolResult;
use crate::utils::serialization::{FileUtils, JsonSerializer};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct OptimizationOrchestrator {
    serializer: JsonSerializer,
    file_serializer: FileUtils,
    reporter: Box<dyn ErrorReporter>,
    ailoop_context: Option<Arc<AiloopContext>>,
}

impl OptimizationOrchestrator {
    pub fn new(
        serializer: JsonSerializer,
        file_serializer: FileUtils,
        reporter: Box<dyn ErrorReporter>,
    ) -> Self {
        OptimizationOrchestrator {
            serializer,
            file_serializer,
            reporter,
            ailoop_context: None,
        }
    }

    /// Create a new orchestrator with ailoop integration.
    pub fn with_ailoop_context(
        serializer: JsonSerializer,
        file_serializer: FileUtils,
        reporter: Box<dyn ErrorReporter>,
        ailoop_context: Option<Arc<AiloopContext>>,
    ) -> Self {
        OptimizationOrchestrator {
            serializer,
            file_serializer,
            reporter,
            ailoop_context,
        }
    }

    pub fn serializer(&self) -> &JsonSerializer {
        &self.serializer
    }

    pub fn file_serializer(&self) -> &FileUtils {
        &self.file_serializer
    }

    pub async fn run_optimization(
        &self,
        workspace_path: &Path,
        configuration: ExecutionConfiguration,
    ) -> Result<OptimizationExecution, AppError> {
        self.run_optimization_with_policy(
            workspace_path,
            configuration,
            &std::collections::HashMap::new(),
            None,
        )
        .await
    }

    pub async fn run_optimization_with_policy(
        &self,
        workspace_path: &Path,
        configuration: ExecutionConfiguration,
        additional_env: &std::collections::HashMap<String, String>,
        success_policy: Option<&crate::core::success_policy::SuccessPolicy>,
    ) -> Result<OptimizationExecution, AppError> {
        self.reporter.report_info("Starting optimization run");

        let execution_id = uuid::Uuid::new_v4();

        // Initialize ailoop notifier if context is available
        let notifier = self.ailoop_context.as_ref().map(|ctx| {
            let notifier = OrchestratorNotifier::new(ctx.clone());
            // Emit execution started event
            if let Err(e) =
                notifier.execution_started(execution_id, workspace_path.display().to_string())
            {
                tracing::warn!("Failed to emit execution_started event: {}", e);
            }
            notifier
        });

        let mut execution = OptimizationExecution {
            id: uuid::Uuid::new_v4(),
            workspace_path: workspace_path.to_path_buf(),
            execution_id,
            status: ExecutionStatus::Running,
            started_at: chrono::Utc::now(),
            completed_at: None,
            resource_limits: Default::default(),
            max_iterations: configuration.max_iterations,
            current_iteration: Some(0),
            final_solution_path: None,
            current_iteration_path: None,
            total_iterations_completed: 0,
            total_iterations_failed: 0,
            iterations: Vec::new(),
            artifacts: Vec::new(),
            configuration: configuration.clone(),
        };

        let mut current_iteration = 0;
        let start_time = Instant::now();
        let mut max_iterations = configuration.max_iterations.unwrap_or(100);

        if let Some(_time_seconds) = configuration.max_time_seconds {
            max_iterations = max_iterations.min(1000);
        }

        self.reporter.report_info(&format!(
            "Starting optimization with max {} iterations",
            max_iterations
        ));

        loop {
            if let Some(limit) = configuration.max_iterations {
                if current_iteration >= limit {
                    self.reporter.report_info("Maximum iterations reached");
                    execution.status = ExecutionStatus::MaxIterationsReached;
                    execution.completed_at = Some(chrono::Utc::now());
                    execution.final_solution_path =
                        Some(workspace_path.join("final_solution.json"));
                    break;
                }
            }

            if let Some(seconds) = configuration.max_time_seconds {
                if start_time.elapsed().as_secs() > seconds {
                    self.reporter.report_info("Maximum execution time reached");
                    execution.status = ExecutionStatus::Timeout;
                    execution.completed_at = Some(chrono::Utc::now());
                    break;
                }
            }

            self.reporter
                .report_info(&format!("Starting iteration {}", current_iteration + 1));

            // Emit iteration started event
            if let Some(ref notifier) = notifier {
                if let Err(e) = notifier.iteration_started(execution_id, current_iteration) {
                    tracing::warn!("Failed to emit iteration_started event: {}", e);
                }
            }

            let iteration_result = self
                .run_iteration(
                    &execution,
                    current_iteration,
                    &configuration,
                    additional_env,
                    success_policy,
                )
                .await;

            match iteration_result {
                Ok(iteration) => {
                    execution.iterations.push(iteration);
                    execution.total_iterations_completed += 1;

                    // Emit iteration completed event
                    if let Some(ref notifier) = notifier {
                        if let Err(e) =
                            notifier.iteration_completed(execution_id, current_iteration)
                        {
                            tracing::warn!("Failed to emit iteration_completed event: {}", e);
                        }
                    }

                    current_iteration += 1;
                    execution.current_iteration = Some(current_iteration);

                    if let Some(policy) = success_policy {
                        if policy.should_stop()? {
                            self.reporter.report_info("Goal reached via success policy");
                            execution.status = ExecutionStatus::Completed;
                            execution.final_solution_path =
                                Some(workspace_path.join("final_solution.json"));
                            execution.current_iteration_path =
                                Some(workspace_path.join("current_solution.json"));
                            execution.completed_at = Some(chrono::Utc::now());
                            break;
                        }
                    }
                }
                Err(e) => {
                    self.reporter.report_error(&e);
                    execution.total_iterations_failed += 1;
                    execution.status = ExecutionStatus::Failed;
                    execution.completed_at = Some(chrono::Utc::now());

                    // Emit execution failed event
                    if let Some(ref notifier) = notifier {
                        if let Err(emit_err) =
                            notifier.execution_failed(execution_id, e.to_string())
                        {
                            tracing::warn!("Failed to emit execution_failed event: {}", emit_err);
                        }
                    }

                    return Err(e);
                }
            }
        }

        if execution.status == ExecutionStatus::Running {
            execution.status = ExecutionStatus::Completed;
            execution.final_solution_path = Some(workspace_path.join("final_solution.json"));
        }
        if execution.completed_at.is_none() {
            execution.completed_at = Some(chrono::Utc::now());
        }

        // Emit execution completed event
        if let Some(ref notifier) = notifier {
            if let Err(e) = notifier.execution_completed(
                execution_id,
                execution.status,
                execution.total_iterations_completed,
            ) {
                tracing::warn!("Failed to emit execution_completed event: {}", e);
            }
        }

        self.reporter
            .report_info("Optimization completed successfully");
        Ok(execution)
    }

    async fn run_iteration(
        &self,
        execution: &OptimizationExecution,
        iteration_number: usize,
        configuration: &ExecutionConfiguration,
        additional_env: &std::collections::HashMap<String, String>,
        success_policy: Option<&crate::core::success_policy::SuccessPolicy>,
    ) -> Result<Iteration, AppError> {
        let iteration_id = uuid::Uuid::new_v4();
        let start_time = chrono::Utc::now();

        let mut iteration = Iteration {
            iteration_id,
            execution_id: execution.execution_id,
            iteration_number,
            phase: IterationPhase::Evaluator,
            started_at: start_time,
            completed_at: None,
            evaluator_result: None,
            advisor_result: None,
            executor_result: None,
            predecessor_solution: execution.current_iteration_path.clone(),
            successor_solution: None,
            artifacts: Vec::new(),
            metadata: IterationMetadata::default(),
        };

        let mut current_phase = IterationPhase::Evaluator;

        loop {
            match current_phase {
                IterationPhase::Evaluator => {
                    if let Some(evaluator_cmd) = &configuration.evaluator_cmd {
                        match self
                            .execute_tool(
                                evaluator_cmd,
                                configuration,
                                &execution.workspace_path,
                                execution,
                                iteration_number,
                                additional_env,
                            )
                            .await
                        {
                            Ok(result) => {
                                iteration.evaluator_result = Some(result.clone());
                                if result.success {
                                    self.reporter
                                        .report_info("Evaluator completed successfully");
                                } else {
                                    self.reporter.report_error(&AppError::new(
                                        crate::core::types::ErrorCategory::ToolExecutionError,
                                        "Evaluator failed",
                                    ));
                                    return Err(AppError::new(
                                        crate::core::types::ErrorCategory::ToolExecutionError,
                                        "Evaluator tool failed",
                                    ));
                                }

                                if configuration.verbose {
                                    if !result.stdout.is_empty() {
                                        println!("\n=== Evaluator Output ===");
                                        println!("{}", result.stdout);
                                        println!("=========================\n");
                                    }
                                    if !result.stderr.is_empty() {
                                        eprintln!("\n=== Evaluator Stderr ===");
                                        eprintln!("{}", result.stderr);
                                        eprintln!("==========================\n");
                                    }
                                }

                                // Check success policy after evaluator completes
                                if let Some(policy) = success_policy {
                                    if policy.should_stop()? {
                                        self.reporter
                                            .report_info("Goal reached via success policy");
                                        iteration.metadata.phase = IterationPhase::Complete;
                                        iteration.completed_at = Some(chrono::Utc::now());
                                        return Ok(iteration);
                                    }
                                }

                                current_phase = IterationPhase::Advisor;
                            }
                            Err(e) => {
                                self.reporter.report_error(&e);
                                return Err(e);
                            }
                        }
                    } else {
                        current_phase = IterationPhase::Advisor;
                    }
                }
                IterationPhase::Advisor => {
                    if let Some(advisor_cmd) = &configuration.advisor_cmd {
                        match self
                            .execute_tool(
                                advisor_cmd,
                                configuration,
                                &execution.workspace_path,
                                execution,
                                iteration_number,
                                additional_env,
                            )
                            .await
                        {
                            Ok(result) => {
                                iteration.advisor_result = Some(result.clone());
                                if result.success {
                                    self.reporter.report_info("Advisor completed successfully");
                                } else {
                                    self.reporter.report_error(&AppError::new(
                                        crate::core::types::ErrorCategory::ToolExecutionError,
                                        "Advisor failed",
                                    ));
                                    return Err(AppError::new(
                                        crate::core::types::ErrorCategory::ToolExecutionError,
                                        "Advisor tool failed",
                                    ));
                                }

                                if configuration.verbose {
                                    if !result.stdout.is_empty() {
                                        println!("\n=== Advisor Output ===");
                                        println!("{}", result.stdout);
                                        println!("=======================\n");
                                    }
                                    if !result.stderr.is_empty() {
                                        eprintln!("\n=== Advisor Stderr ===");
                                        eprintln!("{}", result.stderr);
                                        eprintln!("======================\n");
                                    }
                                }
                                current_phase = IterationPhase::Executor;
                            }
                            Err(e) => {
                                self.reporter.report_error(&e);
                                return Err(e);
                            }
                        }
                    } else {
                        current_phase = IterationPhase::Executor;
                    }
                }
                IterationPhase::Executor => {
                    if let Some(executor_cmd) = &configuration.executor_cmd {
                        match self
                            .execute_tool(
                                executor_cmd,
                                configuration,
                                &execution.workspace_path,
                                execution,
                                iteration_number,
                                additional_env,
                            )
                            .await
                        {
                            Ok(result) => {
                                iteration.executor_result = Some(result.clone());
                                if result.success {
                                    self.reporter.report_info("Executor completed successfully");
                                    iteration.metadata.phase = IterationPhase::Complete;
                                    iteration.completed_at = Some(chrono::Utc::now());
                                    iteration.successor_solution =
                                        Some(execution.workspace_path.join("solution.json"));
                                } else {
                                    if !result.stderr.is_empty() {
                                        eprintln!(
                                            "\n=== Executor stderr (exit code: {}) ===",
                                            result.exit_code
                                        );
                                        eprintln!("{}", result.stderr);
                                        eprintln!("=== end executor stderr ===\n");
                                    }
                                    if !result.stdout.is_empty() && configuration.verbose {
                                        println!("\n=== Executor stdout ===");
                                        println!("{}", result.stdout);
                                        println!("========================\n");
                                    }
                                    self.reporter.report_error(&AppError::new(
                                        crate::core::types::ErrorCategory::ToolExecutionError,
                                        "Executor failed",
                                    ));
                                    return Err(AppError::new(
                                        crate::core::types::ErrorCategory::ToolExecutionError,
                                        "Executor tool failed",
                                    ));
                                }

                                if configuration.verbose {
                                    if !result.stdout.is_empty() {
                                        println!("\n=== Executor Output ===");
                                        println!("{}", result.stdout);
                                        println!("========================\n");
                                    }
                                    if !result.stderr.is_empty() {
                                        eprintln!("\n=== Executor Stderr ===");
                                        eprintln!("{}", result.stderr);
                                        eprintln!("=========================\n");
                                    }
                                }
                                break;
                            }
                            Err(e) => {
                                self.reporter.report_error(&e);
                                return Err(e);
                            }
                        }
                    } else {
                        iteration.metadata.phase = IterationPhase::Complete;
                        iteration.completed_at = Some(chrono::Utc::now());
                        break;
                    }
                }
                IterationPhase::Complete => {
                    break;
                }
            }
        }

        Ok(iteration)
    }

    async fn execute_tool(
        &self,
        cmd: &str,
        configuration: &ExecutionConfiguration,
        workspace_path: &PathBuf,
        execution: &OptimizationExecution,
        iteration_number: usize,
        additional_env: &std::collections::HashMap<String, String>,
    ) -> Result<ToolResult, AppError> {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ToolExecutionError,
                "command must not be empty",
            )
            .with_code("TOOL-002"));
        }
        let program = parts[0];
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

        self.reporter
            .report_info(&format!("Executing tool: {}", cmd));

        let mut env_vars = std::collections::HashMap::new();

        if let Some(eval_cmd) = &configuration.evaluator_cmd {
            env_vars.insert("NEWTON_EVALUATOR_CMD".to_string(), eval_cmd.clone());
        }
        if let Some(adv_cmd) = &configuration.advisor_cmd {
            env_vars.insert("NEWTON_ADVISOR_CMD".to_string(), adv_cmd.clone());
        }
        if let Some(exec_cmd) = &configuration.executor_cmd {
            env_vars.insert("NEWTON_EXECUTOR_CMD".to_string(), exec_cmd.clone());
        }
        if let Some(eval_timeout) = configuration.evaluator_timeout_ms {
            env_vars.insert(
                "NEWTON_EVALUATOR_TIMEOUT_MS".to_string(),
                eval_timeout.to_string(),
            );
        }
        if let Some(adv_timeout) = configuration.advisor_timeout_ms {
            env_vars.insert(
                "NEWTON_ADVISOR_TIMEOUT_MS".to_string(),
                adv_timeout.to_string(),
            );
        }
        if let Some(exec_timeout) = configuration.executor_timeout_ms {
            env_vars.insert(
                "NEWTON_EXECUTOR_TIMEOUT_MS".to_string(),
                exec_timeout.to_string(),
            );
        }
        env_vars.insert(
            "NEWTON_WORKSPACE_PATH".to_string(),
            workspace_path.display().to_string(),
        );
        env_vars.insert(
            "NEWTON_ITERATION_NUMBER".to_string(),
            (iteration_number + 1).to_string(),
        );
        env_vars.insert(
            "NEWTON_EXECUTION_ID".to_string(),
            execution.execution_id.to_string(),
        );

        // Add ailoop environment variables if integration is enabled
        if let Some(ref ctx) = self.ailoop_context {
            env_vars.insert("NEWTON_AILOOP_ENABLED".to_string(), "1".to_string());
            env_vars.insert(
                "NEWTON_AILOOP_HTTP_URL".to_string(),
                ctx.http_url().to_string(),
            );
            env_vars.insert("NEWTON_AILOOP_WS_URL".to_string(), ctx.ws_url().to_string());
            env_vars.insert(
                "NEWTON_AILOOP_CHANNEL".to_string(),
                ctx.channel().to_string(),
            );
        }

        // Merge additional environment variables
        for (key, value) in additional_env {
            env_vars.insert(key.clone(), value.clone());
        }

        let env_vars: Vec<(&str, &str)> = env_vars
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let start_time = Instant::now();
        let mut child = tokio::process::Command::new(program)
            .args(&args)
            .current_dir(workspace_path)
            .envs(env_vars.clone())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                AppError::new(
                    crate::core::types::ErrorCategory::ToolExecutionError,
                    format!("Failed to execute tool: {}", e),
                )
                .with_code("TOOL-001")
            })?;

        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();

        // Initialize output forwarder if ailoop is enabled
        let forwarder = self
            .ailoop_context
            .as_ref()
            .map(|ctx| OutputForwarder::new(ctx.clone()));

        let (stdout_buf, stderr_buf) = tokio::join!(
            stream_child_output(
                stdout_pipe,
                tokio::io::stdout(),
                "stdout",
                forwarder.clone(),
                Some(execution.execution_id),
            ),
            stream_child_output(
                stderr_pipe,
                tokio::io::stderr(),
                "stderr",
                forwarder,
                Some(execution.execution_id),
            ),
        );

        let status = child.wait().await.map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::ToolExecutionError,
                format!("Failed to wait for tool completion: {}", e),
            )
            .with_code("TOOL-001")
        })?;

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_name: cmd.to_string(),
            exit_code: status.code().unwrap_or(-1) as i32,
            execution_time_ms,
            stdout: String::from_utf8_lossy(&stdout_buf).to_string(),
            stderr: String::from_utf8_lossy(&stderr_buf).to_string(),
            success: status.success(),
            error: if status.success() {
                None
            } else {
                Some("Tool execution failed".to_string())
            },
            metadata: ToolMetadata {
                tool_version: None,
                tool_type: ToolType::Executor,
                arguments: args,
                environment_variables: env_vars
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            },
        })
    }
}

async fn stream_child_output<R, W>(
    reader: Option<R>,
    mut writer: W,
    label: &'static str,
    forwarder: Option<OutputForwarder>,
    execution_id: Option<uuid::Uuid>,
) -> Vec<u8>
where
    R: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let Some(mut reader) = reader else {
        return Vec::new();
    };

    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut line_buffer = Vec::new();

    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => {
                flush_remaining_line(&line_buffer, &forwarder, label, execution_id).await;
                break;
            }
            Ok(n) => {
                buffer.extend_from_slice(&chunk[..n]);

                if write_chunk_to_output(&mut writer, &chunk[..n], label)
                    .await
                    .is_err()
                {
                    break;
                }

                forward_lines_to_ailoop(
                    &chunk[..n],
                    &mut line_buffer,
                    &forwarder,
                    label,
                    execution_id,
                )
                .await;
            }
            Err(err) => {
                tracing::error!(%label, ?err, "Failed to read {label} from tool");
                break;
            }
        }
    }

    buffer
}

async fn flush_remaining_line(
    line_buffer: &[u8],
    forwarder: &Option<OutputForwarder>,
    label: &'static str,
    execution_id: Option<uuid::Uuid>,
) {
    if line_buffer.is_empty() {
        return;
    }

    let Some(fwd) = forwarder else {
        return;
    };

    let line = String::from_utf8_lossy(line_buffer).to_string();
    let _ = forward_line(fwd, label, line, execution_id).await;
}

async fn write_chunk_to_output<W>(
    writer: &mut W,
    chunk: &[u8],
    label: &'static str,
) -> Result<(), ()>
where
    W: AsyncWriteExt + Unpin,
{
    if let Err(err) = writer.write_all(chunk).await {
        tracing::error!(%label, ?err, "Failed to forward {label} output");
        return Err(());
    }
    if let Err(err) = writer.flush().await {
        tracing::error!(%label, ?err, "Failed to flush parent {label} output");
    }
    Ok(())
}

async fn forward_lines_to_ailoop(
    chunk: &[u8],
    line_buffer: &mut Vec<u8>,
    forwarder: &Option<OutputForwarder>,
    label: &'static str,
    execution_id: Option<uuid::Uuid>,
) {
    let Some(fwd) = forwarder else {
        return;
    };

    for &byte in chunk {
        line_buffer.push(byte);
        if byte == b'\n' {
            let line = String::from_utf8_lossy(line_buffer).to_string();
            let _ = forward_line(fwd, label, line, execution_id).await;
            line_buffer.clear();
        }
    }
}

async fn forward_line(
    fwd: &OutputForwarder,
    label: &'static str,
    line: String,
    execution_id: Option<uuid::Uuid>,
) -> Result<(), crate::ailoop_integration::output_forwarder::ForwardError> {
    match label {
        "stdout" => fwd.forward_stdout(line, execution_id).await,
        "stderr" => fwd.forward_stderr(line, execution_id).await,
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::DefaultErrorReporter;

    #[test]
    fn test_orchestrator_creation() {
        let serializer = JsonSerializer;
        let file_serializer = FileUtils;
        let reporter = Box::new(DefaultErrorReporter);

        let _orchestrator = OptimizationOrchestrator::new(serializer, file_serializer, reporter);
    }
}
