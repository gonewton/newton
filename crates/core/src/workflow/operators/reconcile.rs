//! ReconcileOperator — reads observations from Assessment output and reconciles with stored Findings.
//! Spec 063.

#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use chrono::Utc;
use newton_backend::{BackendStore, CreateFindingBody, PatchFindingBody};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

pub struct ReconcileOperator {
    _workspace_root: PathBuf,
    store: Arc<dyn BackendStore>,
}

impl ReconcileOperator {
    pub fn new(workspace_root: PathBuf, store: Arc<dyn BackendStore>) -> Self {
        Self {
            _workspace_root: workspace_root,
            store,
        }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ReconcileParams {
    /// Scope entity ID to reconcile findings for.
    pub scope_id: String,
    /// Grader name (used as source when creating new findings).
    #[serde(default = "default_grader")]
    pub grader: String,
    /// Assessment JSON passed inline (caller resolves from task output).
    pub assessment: Value,
}

fn default_grader() -> String {
    "unknown".to_string()
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ReconcileOutput {
    pub created: usize,
    pub refreshed: usize,
    pub reopened: usize,
    pub resolved: usize,
}

/// Observation extracted from assessment JSON.
struct Observation {
    dimension: String,
    severity: Option<String>,
    observation: String,
    why_it_matters: Option<String>,
    recommended_action: Option<String>,
    location: Option<Value>,
    confidence: Option<f64>,
    evidence: Option<Vec<String>>,
}

fn fingerprint(scope_id: &str, dimension: &str, observation_text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    scope_id.hash(&mut hasher);
    dimension.hash(&mut hasher);
    observation_text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn parse_observations(assessment: &Value) -> Vec<Observation> {
    let Some(arr) = assessment.get("observations").and_then(|v| v.as_array()) else {
        return vec![];
    };
    arr.iter()
        .filter_map(|item| {
            let dimension = item.get("dimension")?.as_str()?.to_string();
            let observation = item.get("observation")?.as_str()?.to_string();
            Some(Observation {
                dimension,
                severity: item
                    .get("severity")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                observation,
                why_it_matters: item
                    .get("why_it_matters")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                recommended_action: item
                    .get("recommended_action")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                location: item.get("location").cloned(),
                confidence: item.get("confidence").and_then(|v| v.as_f64()),
                evidence: item.get("evidence").and_then(|v| v.as_array()).map(|arr| {
                    arr.iter()
                        .filter_map(|e| e.as_str().map(|s| s.to_string()))
                        .collect()
                }),
            })
        })
        .collect()
}

/// Determine if a finding status is "open" for the purposes of auto-resolution.
fn is_open_status(status: &str) -> bool {
    matches!(
        status,
        "awaiting_triage" | "triaged" | "approved_for_planning"
    )
}

#[async_trait]
impl Operator for ReconcileOperator {
    fn name(&self) -> &'static str {
        "ReconcileOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let parsed: ReconcileParams = serde_json::from_value(params.clone()).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("ReconcileOperator params invalid: {e}"),
            )
            .with_code("RECONCILE-001")
        })?;
        if parsed.scope_id.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "ReconcileOperator requires a non-empty scope_id",
            )
            .with_code("RECONCILE-001"));
        }
        Ok(())
    }

    fn params_schema(&self) -> schemars::Schema {
        schemars::schema_for!(ReconcileParams)
    }

    fn output_schema(&self) -> schemars::Schema {
        schemars::schema_for!(ReconcileOutput)
    }

    async fn execute(&self, params: Value, _ctx: ExecutionContext) -> Result<Value, AppError> {
        let parsed: ReconcileParams = serde_json::from_value(params.clone()).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("ReconcileOperator params invalid: {e}"),
            )
            .with_code("RECONCILE-001")
        })?;

        let now = Utc::now().to_rfc3339();
        let scope_id = &parsed.scope_id;
        let grader = &parsed.grader;

        // Parse observations from the assessment JSON
        let observations = parse_observations(&parsed.assessment);

        // List all findings for this scope_id
        let existing_findings = self
            .store
            .list_findings(None, Some(scope_id.clone()))
            .await
            .map_err(|e| {
                AppError::new(
                    ErrorCategory::ToolExecutionError,
                    format!("ReconcileOperator: failed to list findings: {e:?}"),
                )
                .with_code("RECONCILE-010")
            })?;

        // Build fingerprint index from existing findings
        let mut fp_index: HashMap<String, &newton_backend::FindingItem> = HashMap::new();
        for finding in &existing_findings {
            fp_index.insert(finding.fingerprint.clone(), finding);
        }

        let mut created = 0usize;
        let mut refreshed = 0usize;
        let mut reopened = 0usize;

        // Track fingerprints seen in this run
        let mut seen_fps: std::collections::HashSet<String> = std::collections::HashSet::new();

        for obs in &observations {
            let fp = fingerprint(scope_id, &obs.dimension, &obs.observation);
            seen_fps.insert(fp.clone());

            if let Some(existing) = fp_index.get(&fp) {
                let status = existing.status.as_str();
                if status == "resolved" {
                    // Reopen
                    self.store
                        .patch_finding(
                            &existing.id,
                            PatchFindingBody {
                                status: Some("awaiting_triage".to_string()),
                                last_seen_at: Some(now.clone()),
                                expected_value: None,
                                effort: None,
                                risk: None,
                            },
                        )
                        .await
                        .map_err(|e| {
                            AppError::new(
                                ErrorCategory::ToolExecutionError,
                                format!("ReconcileOperator: failed to patch finding: {e:?}"),
                            )
                            .with_code("RECONCILE-011")
                        })?;
                    reopened += 1;
                } else {
                    // Refresh (open or rejected/deferred — keep status, update last_seen_at)
                    self.store
                        .patch_finding(
                            &existing.id,
                            PatchFindingBody {
                                status: None,
                                last_seen_at: Some(now.clone()),
                                expected_value: None,
                                effort: None,
                                risk: None,
                            },
                        )
                        .await
                        .map_err(|e| {
                            AppError::new(
                                ErrorCategory::ToolExecutionError,
                                format!("ReconcileOperator: failed to patch finding: {e:?}"),
                            )
                            .with_code("RECONCILE-011")
                        })?;
                    refreshed += 1;
                }
            } else {
                // Create new finding
                let id = Uuid::new_v4().to_string();
                let title: String = obs.observation.chars().take(120).collect();
                self.store
                    .create_finding(CreateFindingBody {
                        id,
                        source: grader.clone(),
                        origin: "system".to_string(),
                        component_id: None,
                        module: None,
                        repo_id: None,
                        kpi_id: None,
                        dimension: obs.dimension.clone(),
                        location: obs.location.clone(),
                        fingerprint: fp.clone(),
                        title,
                        why_it_matters: obs.why_it_matters.clone().unwrap_or_default(),
                        recommended_action: obs.recommended_action.clone().unwrap_or_default(),
                        severity: obs.severity.clone().unwrap_or_else(|| "medium".to_string()),
                        risk: obs.severity.clone().unwrap_or_else(|| "medium".to_string()),
                        confidence: obs.confidence,
                        evidence: obs
                            .evidence
                            .as_ref()
                            .map(|e| serde_json::to_value(e).unwrap_or(Value::Null)),
                        expected_value: None,
                        effort: None,
                        status: "awaiting_triage".to_string(),
                        last_seen_at: Some(now.clone()),
                        depends_on: vec![],
                        blocks: vec![],
                    })
                    .await
                    .map_err(|e| {
                        AppError::new(
                            ErrorCategory::ToolExecutionError,
                            format!("ReconcileOperator: failed to create finding: {e:?}"),
                        )
                        .with_code("RECONCILE-012")
                    })?;
                created += 1;
            }
        }

        // Resolve open findings not seen in this run
        let mut resolved = 0usize;
        for finding in &existing_findings {
            if is_open_status(&finding.status) && !seen_fps.contains(&finding.fingerprint) {
                self.store
                    .patch_finding(
                        &finding.id,
                        PatchFindingBody {
                            status: Some("resolved".to_string()),
                            last_seen_at: Some(now.clone()),
                            expected_value: None,
                            effort: None,
                            risk: None,
                        },
                    )
                    .await
                    .map_err(|e| {
                        AppError::new(
                            ErrorCategory::ToolExecutionError,
                            format!("ReconcileOperator: failed to resolve finding: {e:?}"),
                        )
                        .with_code("RECONCILE-013")
                    })?;
                resolved += 1;
            }
        }

        Ok(serde_json::json!({
            "created": created,
            "refreshed": refreshed,
            "reopened": reopened,
            "resolved": resolved,
        }))
    }
}
