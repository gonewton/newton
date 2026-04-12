#![allow(clippy::result_large_err)]

use super::{DriverConfig, EngineDriver, EngineInvocation, OutputFormat};
use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use std::path::Path;

pub struct PassthroughDriver;

impl EngineDriver for PassthroughDriver {
    fn name(&self) -> &'static str {
        "command"
    }

    fn requires_model(&self) -> bool {
        false
    }

    fn build_invocation(
        &self,
        config: &DriverConfig<'_>,
        _project_root: &Path,
    ) -> Result<EngineInvocation, AppError> {
        let engine_command = config.engine_command.ok_or_else(|| {
            AppError::new(
                ErrorCategory::ValidationError,
                "engine: command requires engine_command in params",
            )
            .with_code("WFG-AGENT-007")
        })?;

        if engine_command.is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "engine_command must not be empty",
            )
            .with_code("WFG-AGENT-007"));
        }

        Ok(EngineInvocation {
            command: engine_command.clone(),
            env: vec![],
            output_format: OutputFormat::PlainText,
        })
    }
}
