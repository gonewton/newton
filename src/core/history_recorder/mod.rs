#![allow(clippy::result_large_err)]

use crate::core::entities::OptimizationExecution;
use crate::core::error::{AppError, DefaultErrorReporter, ErrorReporter};
use crate::utils::serialization::{JsonSerializer, Serializer};
use std::fs;
use std::path::PathBuf;

pub struct ExecutionHistoryRecorder {
    storage_path: PathBuf,
    serializer: JsonSerializer,
    reporter: Box<dyn ErrorReporter>,
}

impl ExecutionHistoryRecorder {
    pub fn new(storage_path: PathBuf) -> Self {
        ExecutionHistoryRecorder {
            storage_path,
            serializer: JsonSerializer,
            reporter: Box::new(DefaultErrorReporter),
        }
    }

    pub fn record_execution(&self, execution: &OptimizationExecution) -> Result<(), AppError> {
        self.reporter
            .report_debug(&format!("Recording execution: {}", execution.execution_id));

        let execution_path = self
            .storage_path
            .join("executions")
            .join(execution.execution_id.to_string());
        let state_path = execution_path.join("execution.json");
        let _list_path = self.storage_path.join("executions.jsonl");

        if let Some(parent) = state_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                AppError::new(
                    crate::core::types::ErrorCategory::IoError,
                    format!("Failed to create execution directory: {}", e),
                )
                .with_code("HISTORY-001")
            })?;
        }

        let serialized = self.serializer.serialize(execution)?;
        fs::write(&state_path, serialized).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!("Failed to write execution state: {}", e),
            )
            .with_code("HISTORY-002")
        })?;

        self.reporter.report_info("Execution recorded successfully");
        Ok(())
    }

    pub fn load_execution(
        &self,
        execution_id: uuid::Uuid,
    ) -> Result<OptimizationExecution, AppError> {
        self.reporter
            .report_debug(&format!("Loading execution: {}", execution_id));

        let execution_path = self
            .storage_path
            .join("executions")
            .join(execution_id.to_string());
        let state_path = execution_path.join("execution.json");

        let content = fs::read_to_string(state_path).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!("Failed to read execution state: {}", e),
            )
            .with_code("HISTORY-003")
        })?;

        let execution = self.serializer.deserialize(content.as_bytes())?;
        self.reporter.report_info("Execution loaded successfully");
        Ok(execution)
    }
}
