#![allow(clippy::result_large_err)]

use super::{DriverConfig, EngineDriver, EngineInvocation, OutputFormat, PromptSource};
use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use std::path::Path;

pub struct ClaudeCodeDriver;

impl EngineDriver for ClaudeCodeDriver {
    fn name(&self) -> &'static str {
        "claude_code"
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
                "claude_code driver requires a model",
            )
            .with_code("WFG-AGENT-006")
        })?;

        let mut cmd = vec!["claude".to_string()];
        cmd.push("--dangerously-skip-permissions".to_string());
        cmd.push("--model".to_string());
        cmd.push(model.to_string());
        cmd.push("--output-format".to_string());
        cmd.push("stream-json".to_string());
        cmd.push("--verbose".to_string());

        match config.prompt_source {
            Some(PromptSource::File(p)) => {
                cmd.push("-p".to_string());
                // Read file content inline for prompt flag
                cmd.push(format!("$(cat {})", p));
            }
            Some(PromptSource::Inline(s)) => {
                cmd.push("-p".to_string());
                cmd.push(s.clone());
            }
            None => {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "claude_code driver requires prompt_file or prompt in params",
                )
                .with_code("WFG-AGENT-007"));
            }
        }

        Ok(EngineInvocation {
            command: cmd,
            env: vec![
                ("ANTHROPIC_MODEL".to_string(), model.to_string()),
                (
                    "PROJECT_ROOT".to_string(),
                    project_root.display().to_string(),
                ),
            ],
            output_format: OutputFormat::StreamJson,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_invocation_includes_skip_permissions_flag() {
        let driver = ClaudeCodeDriver;
        let prompt = PromptSource::Inline("Solve task".to_string());
        let config = DriverConfig {
            model: Some("claude-3-7-sonnet"),
            prompt_source: Some(&prompt),
            engine_command: None,
        };

        let invocation = driver
            .build_invocation(&config, Path::new("/tmp/workspace"))
            .unwrap();

        assert_eq!(
            invocation.command,
            vec![
                "claude".to_string(),
                "--dangerously-skip-permissions".to_string(),
                "--model".to_string(),
                "claude-3-7-sonnet".to_string(),
                "--output-format".to_string(),
                "stream-json".to_string(),
                "--verbose".to_string(),
                "-p".to_string(),
                "Solve task".to_string(),
            ]
        );
    }

    #[test]
    fn build_invocation_sets_expected_env() {
        let driver = ClaudeCodeDriver;
        let prompt = PromptSource::Inline("Ping".to_string());
        let config = DriverConfig {
            model: Some("claude-3-7-sonnet"),
            prompt_source: Some(&prompt),
            engine_command: None,
        };

        let invocation = driver
            .build_invocation(&config, Path::new("/repo/project"))
            .unwrap();

        assert_eq!(
            invocation.env,
            vec![
                (
                    "ANTHROPIC_MODEL".to_string(),
                    "claude-3-7-sonnet".to_string(),
                ),
                ("PROJECT_ROOT".to_string(), "/repo/project".to_string()),
            ]
        );
        assert_eq!(invocation.output_format, OutputFormat::StreamJson);
    }
}
