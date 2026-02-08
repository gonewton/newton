#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use std::fs;
use std::path::Path;

const PROMPT_HEADER: &str = "# Goal\n\n";

/// Builds the executor prompt from workspace artifacts.
pub struct PromptBuilder;

impl PromptBuilder {
    /// Read the goal file, advisor content, and optional context to craft the prompt.
    pub fn build_prompt(
        workspace_path: &Path,
        advisor_recommendations: Option<&str>,
        context: Option<&str>,
    ) -> Result<String, AppError> {
        let goal = Self::read_goal(workspace_path)?;
        let mut sections = vec![format!("{}{}", PROMPT_HEADER, goal)];

        if let Some(advisor) = advisor_recommendations {
            if !advisor.trim().is_empty() {
                sections.push(format!("{}\n", advisor.trim()));
            }
        }

        if let Some(ctx) = context {
            if !ctx.trim().is_empty() {
                sections.push(format!("## Context\n\n{}\n", ctx.trim()));
            }
        }

        sections.push("---\n\nIMPORTANT: When the task is GENUINELY COMPLETE, output:\n<promise>COMPLETE</promise>\n".to_string());

        Ok(sections.join("\n"))
    }

    /// Read the goal file located at workspace root/GOAL.md.
    pub fn read_goal(workspace_path: &Path) -> Result<String, AppError> {
        let goal_path = workspace_path.join("GOAL.md");
        if !goal_path.exists() {
            return Ok(String::new());
        }
        let content = fs::read_to_string(&goal_path).map_err(AppError::from)?;
        Ok(content)
    }

    /// Resolve advisor recommendations from the Newton artifacts directory.
    pub fn read_advisor_recommendations(advisor_dir: &Path) -> Result<Option<String>, AppError> {
        let recommendations = advisor_dir.join("recommendations.md");
        if !recommendations.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(recommendations).map_err(AppError::from)?;
        Ok(Some(content))
    }

    /// Persist the built prompt to the configured executor prompt file.
    pub fn write_prompt(prompt_file: &Path, prompt: &str) -> Result<(), AppError> {
        if let Some(parent) = prompt_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(prompt_file, prompt).map_err(AppError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn builds_prompt_with_components() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path();
        fs::write(workspace.join("GOAL.md"), "Test goal").unwrap();
        let advisor_dir = workspace.join("advisor");
        fs::create_dir_all(&advisor_dir).unwrap();
        fs::write(advisor_dir.join("recommendations.md"), "Recommendation").unwrap();
        let context = "Context entry";

        let prompt =
            PromptBuilder::build_prompt(workspace, Some("Recommendation"), Some(context)).unwrap();
        assert!(prompt.contains("Test goal"));
        assert!(prompt.contains("Recommendation"));
        assert!(prompt.contains("Context entry"));
        assert!(prompt.contains("<promise>COMPLETE</promise>"));
    }

    #[test]
    fn builds_prompt_without_goal() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path();
        let prompt = PromptBuilder::build_prompt(workspace, None, None).unwrap();
        assert!(prompt.contains("# Goal"));
        assert!(prompt.contains("<promise>COMPLETE</promise>"));
    }
}
