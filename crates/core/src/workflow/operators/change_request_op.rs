//! ChangeRequestOperator — reads open Findings and synthesizes a ChangeRequest.
//! Spec 064.

#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use newton_backend::{BackendStore, CreateChangeRequestBody};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

pub struct ChangeRequestOperator {
    _workspace_root: PathBuf,
    store: Arc<dyn BackendStore>,
}

impl ChangeRequestOperator {
    pub fn new(workspace_root: PathBuf, store: Arc<dyn BackendStore>) -> Self {
        Self {
            _workspace_root: workspace_root,
            store,
        }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ChangeRequestParams {
    /// Scope entity ID to read findings for.
    pub scope_id: String,
    /// Maximum number of findings to include (default: 10).
    #[serde(default = "default_max_findings")]
    pub max_findings: usize,
    /// Minimum severity to include (critical, high, medium, low).
    #[serde(default)]
    pub min_severity: Option<String>,
}

fn default_max_findings() -> usize {
    10
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ChangeRequestOutput {
    pub decision: String,
    pub change_request_id: Option<String>,
}

/// Severity rank for sorting. Lower = more severe.
fn severity_rank(s: &str) -> u8 {
    match s {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 4,
    }
}

fn is_open_status(status: &str) -> bool {
    matches!(
        status,
        "awaiting_triage" | "triaged" | "approved_for_planning"
    )
}

#[async_trait]
impl Operator for ChangeRequestOperator {
    fn name(&self) -> &'static str {
        "ChangeRequestOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let parsed: ChangeRequestParams = serde_json::from_value(params.clone()).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("ChangeRequestOperator params invalid: {e}"),
            )
            .with_code("CR-001")
        })?;
        if parsed.scope_id.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "ChangeRequestOperator requires a non-empty scope_id",
            )
            .with_code("CR-001"));
        }
        Ok(())
    }

    fn params_schema(&self) -> schemars::Schema {
        schemars::schema_for!(ChangeRequestParams)
    }

    fn output_schema(&self) -> schemars::Schema {
        schemars::schema_for!(ChangeRequestOutput)
    }

    async fn execute(&self, params: Value, _ctx: ExecutionContext) -> Result<Value, AppError> {
        let parsed: ChangeRequestParams = serde_json::from_value(params.clone()).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("ChangeRequestOperator params invalid: {e}"),
            )
            .with_code("CR-001")
        })?;

        let scope_id = &parsed.scope_id;
        let max_findings = parsed.max_findings;
        let min_severity_rank = parsed
            .min_severity
            .as_deref()
            .map(severity_rank)
            .unwrap_or(4);

        // List all findings for this scope_id
        let all_findings = self
            .store
            .list_findings(None, Some(scope_id.clone()))
            .await
            .map_err(|e| {
                AppError::new(
                    ErrorCategory::ToolExecutionError,
                    format!("ChangeRequestOperator: failed to list findings: {e:?}"),
                )
                .with_code("CR-010")
            })?;

        // Filter to open statuses
        let mut open: Vec<&newton_backend::FindingItem> = all_findings
            .iter()
            .filter(|f| is_open_status(&f.status))
            .collect();

        // Filter by min_severity if set
        if parsed.min_severity.is_some() {
            open.retain(|f| severity_rank(&f.severity) <= min_severity_rank);
        }

        // Sort by severity rank ascending (critical first)
        open.sort_by_key(|f| severity_rank(&f.severity));

        // Take max_findings
        let selected: Vec<&newton_backend::FindingItem> =
            open.into_iter().take(max_findings).collect();

        // Convergence: nothing actionable
        if selected.is_empty() {
            return Ok(serde_json::json!({
                "decision": "none",
                "change_request_id": null,
            }));
        }

        // Deterministic synthesis (no LLM call).
        // TODO: replace with LLM-generated title/body for richer summaries.
        let dims: Vec<String> = {
            let mut seen = std::collections::HashSet::new();
            selected
                .iter()
                .filter_map(|f| {
                    if seen.insert(f.dimension.clone()) {
                        Some(f.dimension.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };
        let title = format!(
            "Address {} finding{}: {}",
            selected.len(),
            if selected.len() == 1 { "" } else { "s" },
            dims.join(", ")
        );

        let body_lines: Vec<String> = selected
            .iter()
            .map(|f| format!("- [{}] {}", f.severity, f.recommended_action))
            .collect();
        let body = body_lines.join("\n");

        let finding_ids: Vec<String> = selected.iter().map(|f| f.id.clone()).collect();
        let cr_id = Uuid::new_v4().to_string();

        self.store
            .create_change_request(CreateChangeRequestBody {
                id: cr_id.clone(),
                title,
                body: Some(body),
                origin: "system".to_string(),
                author: None,
                component_id: None,
                repo_id: None,
                finding_ids,
            })
            .await
            .map_err(|e| {
                AppError::new(
                    ErrorCategory::ToolExecutionError,
                    format!("ChangeRequestOperator: failed to create change request: {e:?}"),
                )
                .with_code("CR-011")
            })?;

        Ok(serde_json::json!({
            "decision": "propose",
            "change_request_id": cr_id,
        }))
    }
}
