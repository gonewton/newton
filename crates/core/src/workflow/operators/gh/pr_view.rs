use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use serde_json::{json, Map, Value};

use super::utils::get_pr_identifier;
use super::GhOperator;

impl GhOperator {
    pub(super) async fn execute_pr_view(
        &self,
        map: &Map<String, Value>,
        workspace: &std::path::Path,
    ) -> Result<Value, AppError> {
        let pr = get_pr_identifier(map)?;

        let pr_number = pr.parse::<u64>().map_err(|_| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("pr must be a valid number, got: {pr}"),
            )
        })?;

        let output = self
            .runner
            .run(&["pr", "view", &pr, "--json", "state"], workspace)
            .await?;

        let pr_json: Value = serde_json::from_str(&output.stdout).map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to parse pr view JSON: {e}"),
            )
            .with_code("WFG-GH-002")
        })?;

        let state = pr_json["state"].as_str().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                "pr view missing state field",
            )
            .with_code("WFG-GH-002")
        })?;

        let normalized_state = state.to_uppercase();

        Ok(json!({
            "state": normalized_state,
            "pr_number": pr_number
        }))
    }
}
