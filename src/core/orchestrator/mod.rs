use crate::core::entities::*;
use crate::core::entities::{ExecutionConfiguration, Iteration, ToolMetadata};
use crate::core::error::{AppError, ErrorReporter};
use crate::tools::ToolResult;
use crate::utils::serialization::{FileUtils, JsonSerializer};
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
                .run_iteration(&execution, current_iteration, &configuration)
                .await;

            match iteration_result {
                Ok(iteration) => {
                    execution.iterations.push(iteration);
                    execution.total_iterations_completed += 1;
                    current_iteration += 1;
                    execution.current_iteration = Some(current_iteration);

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
