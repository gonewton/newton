#![allow(
    clippy::unnecessary_cast,
    clippy::result_large_err,
    clippy::too_many_arguments
)]

use crate::core::entities::*;
use crate::core::entities::{ExecutionConfiguration, Iteration, ToolMetadata};
use crate::core::error::{AppError, ErrorReporter};
use crate::core::{ContextManager, NewtonConfig, PromiseDetector, PromptBuilder};
use crate::tools::ToolResult;
use crate::utils::serialization::{FileUtils, JsonSerializer};
use std::env;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct OptimizationOrchestrator {
    serializer: JsonSerializer,
    file_serializer: FileUtils,
    reporter: Box<dyn ErrorReporter>,
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
        let default_config = NewtonConfig::default();
        self.run_optimization_with_policy(
            workspace_path,
            configuration,
            &std::collections::HashMap::new(),
            None,
            &default_config,
        )
        .await
    }

    pub async fn run_optimization_with_policy(
        &self,
        workspace_path: &Path,
        configuration: ExecutionConfiguration,
        additional_env: &std::collections::HashMap<String, String>,
        success_policy: Option<&crate::core::success_policy::SuccessPolicy>,
        config: &NewtonConfig,
    ) -> Result<OptimizationExecution, AppError> {
        self.reporter.report_info("Starting optimization run");

        let execution_id = uuid::Uuid::new_v4();

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
                    execution.status = ExecutionStatus::Completed;
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

            let iteration_result = self
                .run_iteration(
                    &execution,
                    current_iteration,
                    &configuration,
                    additional_env,
                    success_policy,
                    config,
                )
                .await;

            match iteration_result {
                Ok(iteration) => {
                    execution.iterations.push(iteration);
                    execution.total_iterations_completed += 1;
                    current_iteration += 1;
                    execution.current_iteration = Some(current_iteration);

                    if execution
                        .iterations
                        .last()
                        .map(|iter| iter.metadata.should_stop)
                        .unwrap_or(false)
                    {
                        execution.status = ExecutionStatus::Completed;
                        execution.completed_at = Some(chrono::Utc::now());
                        execution.final_solution_path =
                            Some(workspace_path.join("final_solution.json"));
                        execution.current_iteration_path =
                            Some(workspace_path.join("current_solution.json"));
                        break;
                    }

                    if execution.status == ExecutionStatus::Completed {
                        execution.final_solution_path =
                            Some(workspace_path.join("final_solution.json"));
                        execution.current_iteration_path =
                            Some(workspace_path.join("current_solution.json"));
                        break;
                    }
                }
                Err(e) => {
                    self.reporter.report_error(&e);
                    execution.total_iterations_failed += 1;
                    execution.status = ExecutionStatus::Failed;
                    execution.completed_at = Some(chrono::Utc::now());
                    return Err(e);
                }
            }
        }

        execution.status = ExecutionStatus::Completed;
        execution.completed_at = Some(chrono::Utc::now());

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
        config: &NewtonConfig,
    ) -> Result<Iteration, AppError> {
        let workspace_path = &execution.workspace_path;
        let iteration_id = uuid::Uuid::new_v4();
        let start_time = chrono::Utc::now();
        let score_file = workspace_path.join("artifacts").join("score.txt");
        let context_path = workspace_path.join(&config.context.file);
        let promise_path = workspace_path.join(&config.promise.file);
        let prompt_path = Self::executor_prompt_path(workspace_path);

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
                                ToolType::Evaluator,
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

                                let evaluator_score = Self::read_evaluator_score(
                                    &score_file,
                                    self.reporter.as_ref(),
                                )?;
                                iteration.metadata.evaluator_score = evaluator_score;

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
                                ToolType::Advisor,
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
                                if configuration.executor_cmd.is_some() {
                                    Self::build_executor_prompt(
                                        workspace_path,
                                        iteration_number,
                                        &context_path,
                                        &prompt_path,
                                    )?;
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
                        let mut executor_env = additional_env.clone();
                        executor_env.insert(
                            "NEWTON_EXECUTOR_PROMPT_FILE".to_string(),
                            prompt_path.display().to_string(),
                        );
                        executor_env.insert(
                            "NEWTON_CONTEXT_FILE".to_string(),
                            context_path.display().to_string(),
                        );
                        executor_env.insert(
                            "NEWTON_CONTEXT_CLEAR_AFTER_USE".to_string(),
                            config.context.clear_after_use.to_string(),
                        );
                        executor_env.insert(
                            "NEWTON_PROMISE_FILE".to_string(),
                            promise_path.display().to_string(),
                        );

                        match self
                            .execute_tool(
                                ToolType::Executor,
                                executor_cmd,
                                configuration,
                                &execution.workspace_path,
                                execution,
                                iteration_number,
                                &executor_env,
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
                                if config.context.clear_after_use {
                                    ContextManager::clear_context(&context_path)?;
                                }

                                let combined_output = format!("{}{}", result.stdout, result.stderr);
                                if let Some(promise_value) =
                                    PromiseDetector::detect_promise(&combined_output)
                                {
                                    iteration.metadata.promise_value = Some(promise_value.clone());
                                    if let Err(write_err) = PromiseDetector::write_promise(
                                        &promise_path,
                                        &promise_value,
                                    ) {
                                        self.reporter.report_warning(
                                            &format!(
                                                "Failed to persist promise to {}: {}",
                                                promise_path.display(),
                                                write_err
                                            ),
                                            None,
                                        );
                                    }
                                    let meets_threshold = iteration
                                        .metadata
                                        .evaluator_score
                                        .map(|score| score >= config.evaluator.score_threshold)
                                        .unwrap_or(false);
                                    if PromiseDetector::is_complete(&promise_value)
                                        && meets_threshold
                                    {
                                        iteration.metadata.should_stop = true;
                                        self.reporter.report_info("Promise signaled completion");
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
        tool_type: ToolType,
        cmd: &str,
        configuration: &ExecutionConfiguration,
        workspace_path: &PathBuf,
        execution: &OptimizationExecution,
        iteration_number: usize,
        additional_env: &std::collections::HashMap<String, String>,
    ) -> Result<ToolResult, AppError> {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let program = parts[0];
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

        self.reporter
            .report_info(&format!("Executing tool: {}", cmd));

        let iteration_dir = workspace_path
            .join("artifacts")
            .join(format!("iter-{}", iteration_number + 1));
        std::fs::create_dir_all(&iteration_dir)?;

        let evaluator_dir = iteration_dir.join("evaluator");
        let advisor_dir = iteration_dir.join("advisor");
        let executor_dir = iteration_dir.join("executor");
        let score_file = workspace_path.join("artifacts").join("score.txt");

        std::fs::create_dir_all(&evaluator_dir)?;
        std::fs::create_dir_all(&advisor_dir)?;
        std::fs::create_dir_all(&executor_dir)?;
        std::fs::create_dir_all(score_file.parent().unwrap())?;

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
        env_vars.insert(
            "NEWTON_ITERATION_DIR".to_string(),
            iteration_dir.display().to_string(),
        );
        env_vars.insert(
            "NEWTON_EVALUATOR_DIR".to_string(),
            evaluator_dir.display().to_string(),
        );
        env_vars.insert(
            "NEWTON_ADVISOR_DIR".to_string(),
            advisor_dir.display().to_string(),
        );
        env_vars.insert(
            "NEWTON_EXECUTOR_DIR".to_string(),
            executor_dir.display().to_string(),
        );
        env_vars.insert(
            "NEWTON_SCORE_FILE".to_string(),
            score_file.display().to_string(),
        );

        // Merge additional environment variables
        for (key, value) in additional_env {
            env_vars.insert(key.clone(), value.clone());
        }

        let env_vars: Vec<(&str, &str)> = env_vars
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let start_time = Instant::now();
        let output = tokio::process::Command::new(program)
            .args(&args)
            .current_dir(workspace_path)
            .envs(env_vars.clone())
            .output()
            .await
            .map_err(|e| {
                AppError::new(
                    crate::core::types::ErrorCategory::ToolExecutionError,
                    format!("Failed to execute tool: {}", e),
                )
                .with_code("TOOL-001")
            })?;

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_name: cmd.to_string(),
            exit_code: output.status.code().unwrap_or(-1) as i32,
            execution_time_ms,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            success: output.status.success(),
            error: if output.status.success() {
                None
            } else {
                Some("Tool execution failed".to_string())
            },
            metadata: ToolMetadata {
                tool_version: None,
                tool_type,
                arguments: args,
                environment_variables: env_vars
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            },
        })
    }

    fn executor_prompt_path(workspace_path: &Path) -> PathBuf {
        if let Ok(path) = env::var("NEWTON_EXECUTOR_PROMPT_FILE") {
            PathBuf::from(path)
        } else {
            workspace_path.join(".newton/state/executor_prompt.md")
        }
    }

    fn advisor_recommendations_path(workspace_path: &Path, iteration_number: usize) -> PathBuf {
        workspace_path
            .join("artifacts")
            .join(format!("iter-{}", iteration_number + 1))
            .join("advisor")
            .join("recommendations.md")
    }

    fn read_optional_file(path: &Path) -> Option<String> {
        let contents = std::fs::read_to_string(path).ok()?;
        if contents.trim().is_empty() {
            None
        } else {
            Some(contents)
        }
    }

    fn build_executor_prompt(
        workspace_path: &Path,
        iteration_number: usize,
        context_path: &Path,
        prompt_path: &Path,
    ) -> Result<(), AppError> {
        let advisor_path = Self::advisor_recommendations_path(workspace_path, iteration_number);
        let advisor_recommendations = Self::read_optional_file(&advisor_path);
        let context_text = ContextManager::read_context(context_path)?;
        let context_section = if context_text.trim().is_empty() {
            None
        } else {
            Some(context_text)
        };
        let prompt = PromptBuilder::build_prompt(
            workspace_path,
            advisor_recommendations.as_deref(),
            context_section.as_deref(),
        )?;
        Self::write_prompt(prompt_path, &prompt)
    }

    fn write_prompt(prompt_path: &Path, prompt: &str) -> Result<(), AppError> {
        if let Some(parent) = prompt_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(prompt_path, prompt).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!(
                    "Failed to write executor prompt {}: {}",
                    prompt_path.display(),
                    e
                ),
            )
        })
    }

    fn read_evaluator_score(
        score_file: &Path,
        reporter: &dyn ErrorReporter,
    ) -> Result<Option<f64>, AppError> {
        if !score_file.exists() {
            return Ok(None);
        }
        let contents = std::fs::read_to_string(score_file)?;
        let trimmed = contents.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        match trimmed.parse::<f64>() {
            Ok(value) => Ok(Some(value)),
            Err(err) => {
                reporter.report_warning(
                    &format!(
                        "Unable to parse evaluator score from {}: {}",
                        score_file.display(),
                        err
                    ),
                    None,
                );
                Ok(None)
            }
        }
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
