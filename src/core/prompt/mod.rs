#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const PROMISE_INSTRUCTION: &str =
    "---\n\nIMPORTANT: When the task is GENUINELY COMPLETE, output:\n<promise>COMPLETE</promise>\n";

/// Constructs executor prompts from the goal, advisor notes, context, and
/// completion instructions.
pub struct PromptBuilder;

impl PromptBuilder {
    /// Build the executor prompt that joins the goal, advisor notes, context,
    /// and completion reminder.
    pub fn build_prompt(
        workspace_path: &Path,
        advisor_recommendations: Option<&str>,
        context: Option<&str>,
    ) -> Result<String, AppError> {
        let goal = Self::read_goal(workspace_path)?;
        let mut prompt = String::from("# Goal\n\n");
        prompt.push_str(goal.trim());
        prompt.push_str("\n\n");

        if let Some(recommendations) = advisor_recommendations {
            let trimmed = recommendations.trim();
            if !trimmed.is_empty() {
                prompt.push_str("## Advisor Recommendations\n\n");
                prompt.push_str(trimmed);
                prompt.push_str("\n\n");
            }
        }

        if let Some(context_text) = context {
            let trimmed = context_text.trim();
            if !trimmed.is_empty() {
                prompt.push_str("## Context\n\n");
                prompt.push_str(trimmed);
                prompt.push_str("\n\n");
            }
        }

        prompt.push_str(PROMISE_INSTRUCTION);
        Ok(prompt)
    }

    /// Read the goal description either from `NEWTON_GOAL_FILE` or the workspace.
    pub fn read_goal(workspace_path: &Path) -> Result<String, AppError> {
        let goal_path = env::var("NEWTON_GOAL_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| workspace_path.join("GOAL.md"));
        if !goal_path.exists() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("Goal file not found at {}", goal_path.display()),
            ));
        }
        fs::read_to_string(goal_path).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("Failed to read goal file: {}", e),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_build_prompt_includes_sections() {
        std::env::remove_var("NEWTON_GOAL_FILE");
        let tmp = TempDir::new().unwrap();
        let goal_path = tmp.path().join("GOAL.md");
        fs::write(&goal_path, "Reduce bugs\n").unwrap();

        let prompt = PromptBuilder::build_prompt(
            tmp.path(),
            Some("- Keep tests green"),
            Some("Context entry"),
        )
        .unwrap();

        assert!(prompt.contains("# Goal"));
        assert!(prompt.contains("Reduce bugs"));
        assert!(prompt.contains("Advisor Recommendations"));
        assert!(prompt.contains("Context entry"));
        assert!(prompt.contains("<promise>COMPLETE</promise>"));
    }

    #[test]
    fn test_read_goal_missing() {
        std::env::remove_var("NEWTON_GOAL_FILE");
        let tmp = TempDir::new().unwrap();
        let err = PromptBuilder::read_goal(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("Goal file not found"));
    }
}
