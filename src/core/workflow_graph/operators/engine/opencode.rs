#![allow(clippy::result_large_err)]

use super::{DriverConfig, EngineDriver, EngineInvocation, OutputFormat};
use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use std::path::Path;

pub struct OpencodeDriver;

impl EngineDriver for OpencodeDriver {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn requires_model(&self) -> bool {
        true
    }

    fn build_invocation(
        &self,
        config: &DriverConfig<'_>,
        project_root: &Path,
    ) -> Result<EngineInvocation, AppError> {
        let model = config.model.ok_or_else(|| {
            AppError::new(
                ErrorCategory::ValidationError,
                "opencode driver requires a model",
            )
            .with_code("WFG-AGENT-006")
        })?;

        let mut cmd = vec!["opencode".to_string(), "run".to_string()];
        cmd.push("--model".to_string());
        cmd.push(model.to_string());

        match config.prompt_source {
            Some(super::PromptSource::File(p)) => {
                cmd.push("--prompt-file".to_string());
                cmd.push(p.clone());
            }
            Some(super::PromptSource::Inline(s)) => {
                cmd.push("--prompt".to_string());
                cmd.push(s.clone());
            }
            None => {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "opencode driver requires prompt_file or prompt in params",
                )
                .with_code("WFG-AGENT-007"));
            }
        }

        Ok(EngineInvocation {
            command: cmd,
            env: vec![
                ("OPENCODE_MODEL".to_string(), model.to_string()),
                (
                    "PROJECT_ROOT".to_string(),
                    project_root.display().to_string(),
                ),
            ],
            output_format: OutputFormat::PlainText,
        })
    }
}
