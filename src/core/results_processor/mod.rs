#![allow(clippy::result_large_err)]

use crate::core::entities::{
    ExecutionConfiguration, ExecutionStatus, Iteration, IterationPhase, OptimizationExecution,
};
use crate::core::error::{AppError, ErrorReporter};
use crate::tools::ToolResult;
use crate::utils::serialization::JsonSerializer;
use serde::{Deserialize, Serialize};

pub struct ResultsProcessor {
    serializer: JsonSerializer,
    reporter: Box<dyn ErrorReporter>,
}

impl ResultsProcessor {
    pub fn new(serializer: JsonSerializer, reporter: Box<dyn ErrorReporter>) -> Self {
        ResultsProcessor {
            serializer,
            reporter,
        }
    }

    pub fn serializer(&self) -> &JsonSerializer {
        &self.serializer
    }

    pub fn generate_report(
        &self,
        execution: &OptimizationExecution,
        output_format: OutputFormat,
    ) -> Result<String, AppError> {
        self.reporter.report_debug(&format!(
            "Generating report for execution: {}",
            execution.execution_id
        ));

        match output_format {
            OutputFormat::Json => self.generate_json_report(execution),
            OutputFormat::Text => self.generate_text_report(execution),
        }
    }

    fn generate_json_report(&self, execution: &OptimizationExecution) -> Result<String, AppError> {
        let report_json = serde_json::to_string_pretty(execution).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::SerializationError,
                format!("Failed to generate JSON report: {}", e),
            )
            .with_code("REPORT-JSON-001")
        })?;

        Ok(report_json)
    }

    fn generate_text_report(&self, execution: &OptimizationExecution) -> Result<String, AppError> {
        let mut builder = ReportBuilder::new();
        builder.add_section(build_header_section(execution));
        builder.add_section(build_iterations_section(&execution.iterations));
        builder.add_section(build_statistics_section(execution));
        builder.add_section(build_configuration_section(&execution.configuration));
        Ok(builder.build())
    }

    pub fn generate_summary(&self, execution: &OptimizationExecution) -> Result<String, AppError> {
        self.reporter.report_debug(&format!(
            "Generating summary for execution: {}",
            execution.execution_id
        ));

        let mut summary = String::new();

        summary.push_str("=== Optimization Summary ===\n\n");
        summary.push_str(&format!("Execution ID: {}\n", execution.execution_id));
        summary.push_str(&format!("Status: {:?}\n", execution.status));

        if let Some(completed_at) = execution.completed_at {
            let duration = completed_at.signed_duration_since(execution.started_at);
            summary.push_str(&format!("Duration: {} seconds\n", duration.num_seconds()));
        }

        summary.push_str(&format!(
            "Iterations: {}/{}\n",
            execution.total_iterations_completed,
            execution.current_iteration.unwrap_or(0)
        ));
        summary.push_str(&format!(
            "Success Rate: {:.1}%\n",
            if execution.total_iterations_completed > 0 {
                (1.0 - execution.total_iterations_failed as f64
                    / execution.total_iterations_completed as f64)
                    * 100.0
            } else {
                0.0
            }
        ));

        if let Some(solution_path) = &execution.final_solution_path {
            summary.push_str(&format!("Final Solution: {}\n", solution_path.display()));
        }

        Ok(summary)
    }

    pub fn generate_execution_statistics(
        &self,
        execution: &OptimizationExecution,
    ) -> Result<ExecutionStatistics, AppError> {
        let execution_id = execution.execution_id;
        let status = execution.status.clone();
        let total_iterations = execution.total_iterations_completed;
        let successful_iterations = execution
            .iterations
            .iter()
            .filter(|i| i.phase == IterationPhase::Complete)
            .count();

        let mut total_evaluator_time: u64 = 0;
        let mut total_advisor_time: u64 = 0;
        let mut total_executor_time: u64 = 0;

        for iteration in &execution.iterations {
            if let Some(result) = &iteration.evaluator_result {
                total_evaluator_time += result.execution_time_ms;
            }
            if let Some(result) = &iteration.advisor_result {
                total_advisor_time += result.execution_time_ms;
            }
            if let Some(result) = &iteration.executor_result {
                total_executor_time += result.execution_time_ms;
            }
        }

        let (avg_evaluator_time_ms, avg_advisor_time_ms, avg_executor_time_ms) =
            if execution.iterations.is_empty() {
                (0, 0, 0)
            } else {
                let len = execution.iterations.len() as u64;
                (
                    total_evaluator_time / len,
                    total_advisor_time / len,
                    total_executor_time / len,
                )
            };

        Ok(ExecutionStatistics {
            execution_id,
            status,
            total_iterations,
            successful_iterations,
            total_evaluator_time_ms: total_evaluator_time,
            total_advisor_time_ms: total_advisor_time,
            total_executor_time_ms: total_executor_time,
            avg_evaluator_time_ms,
            avg_advisor_time_ms,
            avg_executor_time_ms,
            artifacts_count: execution.artifacts.len(),
            start_time: execution.started_at,
            end_time: execution.completed_at,
        })
    }
}

fn build_header_section(execution: &OptimizationExecution) -> String {
    let mut section = String::new();
    section.push_str("=== Newton Loop Optimization Report ===\n\n");
    section.push_str(&format!("Execution ID: {}\n", execution.execution_id));
    section.push_str(&format!("Status: {:?}\n", execution.status));
    section.push_str(&format!(
        "Started At: {}\n",
        execution.started_at.format("%Y-%m-%d %H:%M:%S UTC")
    ));
    if let Some(completed_at) = execution.completed_at {
        section.push_str(&format!(
            "Completed At: {}\n",
            completed_at.format("%Y-%m-%d %H:%M:%S UTC")
        ));
    }
    if let Some(max_iter) = execution.max_iterations {
        section.push_str(&format!("Max Iterations: {}\n", max_iter));
    }
    if let Some(curr_iter) = execution.current_iteration {
        section.push_str(&format!("Current Iteration: {}\n", curr_iter));
    }
    section.push_str(&format!(
        "Total Iterations Completed: {}\n",
        execution.total_iterations_completed
    ));
    section.push_str(&format!(
        "Total Iterations Failed: {}\n",
        execution.total_iterations_failed
    ));
    section
}

fn build_iterations_section(iterations: &[Iteration]) -> String {
    let mut section = String::new();
    section.push_str("=== Iterations ===\n");
    if iterations.is_empty() {
        section.push_str("No iterations recorded.\n");
        return section;
    }

    for iteration in iterations {
        section.push('\n');
        section.push_str(&build_iteration_summary(iteration));
    }
    section
}

fn build_iteration_summary(iteration: &Iteration) -> String {
    let mut summary = String::new();
    summary.push_str(&format!(
        "Iteration {} ({}):\n",
        iteration.iteration_number, iteration.iteration_id
    ));
    summary.push_str(&format!("  Phase: {:?}\n", iteration.phase));
    if let Some(result) = &iteration.evaluator_result {
        summary.push_str(&format_tool_result("Evaluator", result));
    }
    if let Some(result) = &iteration.advisor_result {
        summary.push_str(&format_tool_result("Advisor", result));
    }
    if let Some(result) = &iteration.executor_result {
        summary.push_str(&format_tool_result("Executor", result));
    }
    summary.push_str(&format!(
        "  Artifacts Generated: {}\n",
        iteration.metadata.artifacts_generated
    ));
    summary
}

fn format_tool_result(label: &str, result: &ToolResult) -> String {
    format!(
        "  {}: {}\n    Exit Code: {}\n    Success: {}\n    Time: {}ms\n",
        label, result.tool_name, result.exit_code, result.success, result.execution_time_ms
    )
}

fn build_statistics_section(execution: &OptimizationExecution) -> String {
    let mut section = String::new();
    section.push_str("=== Statistics ===\n");

    if execution.iterations.is_empty() {
        section.push_str("No iteration timing data available.\n");
    } else {
        let total_time = execution
            .iterations
            .iter()
            .map(|iteration| {
                iteration
                    .completed_at
                    .unwrap_or(iteration.started_at)
                    .signed_duration_since(iteration.started_at)
                    .num_milliseconds()
            })
            .sum::<i64>();
        let avg_time = total_time as f64 / execution.iterations.len() as f64;
        section.push_str(&format!("Total Time: {}ms\n", total_time));
        section.push_str(&format!("Average Iteration Time: {:.2}ms\n", avg_time));
    }

    section.push_str("\n=== Resource Usage ===\n");
    if let Some(max_iter) = execution.max_iterations {
        let progress = if max_iter == 0 {
            0.0
        } else {
            execution.current_iteration.unwrap_or(0) as f64 / max_iter as f64 * 100.0
        };
        section.push_str(&format!("Progress: {:.1}%\n", progress));
    } else {
        section.push_str("Progress: n/a\n");
    }
    section
}

fn build_configuration_section(config: &ExecutionConfiguration) -> String {
    let mut section = String::new();
    section.push_str("=== Configuration ===\n");
    section.push_str(&format!("Evaluator: {:?}\n", config.evaluator_cmd));
    section.push_str(&format!("Advisor: {:?}\n", config.advisor_cmd));
    section.push_str(&format!("Executor: {:?}\n", config.executor_cmd));
    section.push_str(&format!("Strict Mode: {}\n", config.strict_toolchain_mode));
    section.push_str(&format!(
        "Resource Monitoring: {}\n",
        config.resource_monitoring
    ));
    if let Some(timeout) = config.global_timeout_ms {
        section.push_str(&format!("Global Timeout: {}ms\n", timeout));
    }
    section
}

struct ReportBuilder {
    sections: Vec<String>,
}

impl ReportBuilder {
    fn new() -> Self {
        ReportBuilder {
            sections: Vec::new(),
        }
    }

    fn add_section(&mut self, section: String) {
        if !section.trim().is_empty() {
            self.sections.push(section);
        }
    }

    fn build(self) -> String {
        self.sections.join("\n\n")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionStatistics {
    pub execution_id: uuid::Uuid,
    pub status: ExecutionStatus,
    pub total_iterations: usize,
    pub successful_iterations: usize,
    pub total_evaluator_time_ms: u64,
    pub total_advisor_time_ms: u64,
    pub total_executor_time_ms: u64,
    pub avg_evaluator_time_ms: u64,
    pub avg_advisor_time_ms: u64,
    pub avg_executor_time_ms: u64,
    pub artifacts_count: usize,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputFormat {
    Json,
    Text,
}
