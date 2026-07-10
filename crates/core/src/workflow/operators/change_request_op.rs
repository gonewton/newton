//! ChangeRequestOperator — reads open Findings and synthesizes a ChangeRequest.
//! Spec 064 + 067.

#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use newton_types::{BackendStore, CreateChangeRequestBody};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

pub struct ChangeRequestOperator {
    workspace_root: PathBuf,
    store: Arc<dyn BackendStore>,
}

impl ChangeRequestOperator {
    pub const NAME: &'static str = "ChangeRequestOperator";

    pub fn new(workspace_root: PathBuf, store: Arc<dyn BackendStore>) -> Self {
        Self {
            workspace_root,
            store,
        }
    }

    /// Store-independent Descriptor (name + params/output schema). Used to
    /// describe this operator's vocabulary even when no `BackendStore` is
    /// wired (e.g. `newton schema export`). See ADR-0014.
    pub fn descriptor() -> crate::workflow::operator::Descriptor {
        crate::workflow::operator::Descriptor {
            name: Self::NAME,
            params_schema: schemars::schema_for!(ChangeRequestParams),
            output_schema: schemars::schema_for!(ChangeRequestOutput),
        }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ChangeRequestParams {
    /// Scope kind: product | component | repo | module.
    pub scope: String,
    /// Scope entity ID to read findings for.
    pub scope_id: String,
    /// Maximum number of findings to include (default: 10).
    #[serde(default = "default_max_findings")]
    pub max_findings: usize,
    /// Minimum severity to include (critical, high, medium, low).
    #[serde(default)]
    pub min_severity: Option<String>,
    /// Engine for LLM synthesis (default: "claude").
    #[serde(default = "default_engine")]
    pub engine: String,
    /// Model for LLM synthesis (optional).
    #[serde(default)]
    pub model: Option<String>,
    /// Timeout for LLM synthesis in seconds (default: 60).
    #[serde(default = "default_synthesis_timeout")]
    pub synthesis_timeout_seconds: u64,
}

fn default_max_findings() -> usize {
    10
}

fn default_engine() -> String {
    "claude".to_string()
}

fn default_synthesis_timeout() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ChangeRequestOutput {
    pub decision: String,
    pub change_request_id: Option<String>,
}

fn severity_rank(s: &str) -> u8 {
    match s {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 4,
    }
}

fn rollup_risk<'a>(risks: impl Iterator<Item = &'a str>) -> String {
    let worst = risks.map(severity_rank).min().unwrap_or(2);
    match worst {
        0 => "critical".to_string(),
        1 => "high".to_string(),
        2 => "medium".to_string(),
        _ => "low".to_string(),
    }
}

fn rollup_confidence(confidences: impl Iterator<Item = f64>) -> Option<f64> {
    let mut sum = 0.0_f64;
    let mut count = 0usize;
    for c in confidences {
        sum += c;
        count += 1;
    }
    if count == 0 {
        None
    } else {
        Some(sum / count as f64)
    }
}

fn is_open_status(status: &str) -> bool {
    matches!(
        status,
        "awaiting_triage" | "triaged" | "approved_for_planning"
    )
}

const CR_SYNTHESIS_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "title": {"type": "string", "maxLength": 200},
    "body": {"type": "string"}
  },
  "required": ["title", "body"]
}"#;

#[derive(Debug, Deserialize)]
struct CrSynthesis {
    title: String,
    body: String,
}

#[async_trait]
impl Operator for ChangeRequestOperator {
    fn name(&self) -> &'static str {
        Self::NAME
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
        Self::descriptor().params_schema
    }

    fn output_schema(&self) -> schemars::Schema {
        Self::descriptor().output_schema
    }

    async fn execute(&self, params: Value, _ctx: ExecutionContext) -> Result<Value, AppError> {
        let parsed: ChangeRequestParams = serde_json::from_value(params.clone()).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("ChangeRequestOperator params invalid: {e}"),
            )
            .with_code("CR-001")
        })?;

        let scope = &parsed.scope;
        let scope_id = &parsed.scope_id;
        let max_findings = parsed.max_findings;
        let min_severity_rank = parsed
            .min_severity
            .as_deref()
            .map(severity_rank)
            .unwrap_or(4);

        // R1: list findings with scope so we actually find them.
        let all_findings = self
            .store
            .list_findings(None, Some(scope.clone()), Some(scope_id.clone()))
            .await
            .map_err(|e| {
                AppError::new(
                    ErrorCategory::ToolExecutionError,
                    format!("ChangeRequestOperator: failed to list findings: {e:?}"),
                )
                .with_code("CR-010")
            })?;

        let mut open: Vec<&newton_types::FindingItem> = all_findings
            .iter()
            .filter(|f| is_open_status(&f.status))
            .collect();

        if parsed.min_severity.is_some() {
            open.retain(|f| severity_rank(&f.severity) <= min_severity_rank);
        }

        open.sort_by_key(|f| severity_rank(&f.severity));

        let selected: Vec<&newton_types::FindingItem> =
            open.into_iter().take(max_findings).collect();

        if selected.is_empty() {
            return Ok(serde_json::json!({
                "decision": "none",
                "change_request_id": null,
            }));
        }

        let finding_ids: Vec<String> = selected.iter().map(|f| f.id.clone()).collect();

        // R2: synthesize title/body via aikit Pipeline.
        let findings_json: Vec<serde_json::Value> = selected
            .iter()
            .map(|f| {
                serde_json::json!({
                    "id": f.id,
                    "dimension": f.dimension,
                    "severity": f.severity,
                    "title": f.title,
                    "why_it_matters": f.why_it_matters,
                    "recommended_action": f.recommended_action,
                })
            })
            .collect();
        let findings_str = serde_json::to_string_pretty(&findings_json).unwrap_or_default();
        let scope_label = format!("{scope}/{scope_id}");

        let engine = parsed.engine.clone();
        let model = parsed.model.clone();
        let timeout_secs = parsed.synthesis_timeout_seconds;
        let workspace_root = self.workspace_root.clone();

        let synthesis: Option<CrSynthesis> = tokio::task::spawn_blocking(move || {
            let template = "You are a change request author. Synthesize a concise, actionable change request from the following open findings for scope {{scope}}.\n\n## Findings\n{{findings}}\n\n## Instructions\n- Write a short, specific title (under 120 chars) describing the overall action needed.\n- Write a markdown body summarizing the findings and why they matter. Reference severity.\n- Focus on what needs to change, not on describing the grader.\n\nReturn ONLY a JSON object matching the schema.";

                let runner = aikit_sdk::AgentRunner::new()
                    .agent(&engine)
                    .working_dir(&workspace_root.to_string_lossy())
                    .timeout(std::time::Duration::from_secs(timeout_secs));
                let runner = if let Some(ref m) = model { runner.model(m) } else { runner };

                let pipeline = aikit_sdk::pipeline::Pipeline::new(template, CR_SYNTHESIS_SCHEMA)
                    .max_retries(1);

                match pipeline.run(&[("scope", &scope_label), ("findings", &findings_str)], runner) {
                    Ok(pr) => serde_json::from_value::<CrSynthesis>(pr.data).ok(),
                    Err(e) => {
                        tracing::warn!("ChangeRequestOperator: LLM synthesis failed, using fallback: {e}");
                        None
                    }
                }
        })
        .await
        .unwrap_or(None);

        // Fallback synthesis when LLM is unavailable.
        let (title, body) = if let Some(s) = synthesis {
            (s.title, s.body)
        } else {
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
            (title, body_lines.join("\n"))
        };

        let cr_id = Uuid::new_v4().to_string();
        let (cr_component_id, cr_repo_id) = match scope.as_str() {
            "component" => (Some(scope_id.clone()), None),
            "repo" => (None, Some(scope_id.clone())),
            _ => (None, None),
        };

        // Roll up risk and confidence from the selected Findings.
        let rolled_risk = rollup_risk(selected.iter().map(|f| f.risk.as_str()));
        let rolled_confidence = rollup_confidence(selected.iter().filter_map(|f| f.confidence));

        self.store
            .create_change_request(CreateChangeRequestBody {
                id: cr_id.clone(),
                title,
                body: Some(body),
                origin: "system".to_string(),
                author: None,
                component_id: cr_component_id,
                repo_id: cr_repo_id,
                finding_ids,
                risk: rolled_risk,
                confidence: rolled_confidence,
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
