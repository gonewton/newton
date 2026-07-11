//! ChangeRequestOperator — reads open Findings and synthesizes a ChangeRequest.
//! Spec 064 + 067.

#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use crate::workflow::operators::llm_client::{AgentClient, RealAgentClient};
use async_trait::async_trait;
use newton_types::{BackendStore, CreateChangeRequestBody, FindingStatus, Severity};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

pub struct ChangeRequestOperator {
    workspace_root: PathBuf,
    store: Arc<dyn BackendStore>,
    agent_client: Arc<dyn AgentClient>,
}

impl ChangeRequestOperator {
    pub const NAME: &'static str = "ChangeRequestOperator";

    pub fn new(workspace_root: PathBuf, store: Arc<dyn BackendStore>) -> Self {
        Self {
            workspace_root,
            store,
            agent_client: Arc::new(RealAgentClient),
        }
    }

    /// Test/injection seam (spec 074 S8): construct with a stubbed
    /// `AgentClient` instead of the real `aikit_sdk`-backed one, so tests
    /// can drive `execute`'s synthesis logic (including the graceful
    /// LLM-unavailable fallback below) without a real agent subprocess.
    pub fn with_agent_client(
        workspace_root: PathBuf,
        store: Arc<dyn BackendStore>,
        agent_client: Arc<dyn AgentClient>,
    ) -> Self {
        Self {
            workspace_root,
            store,
            agent_client,
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

/// Ranks the `risk` portfolio-metadata field (kept `String`, out of S3's
/// scope — see spec 074 S3 commit message) for `rollup_risk` below.
fn severity_rank(s: &str) -> u8 {
    match s {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 4,
    }
}

/// Ranks the typed `Finding.severity` enum. Exhaustive match — no `_` catch-all,
/// since every `Severity` variant is now a real value (unlike the free-form
/// `risk` string above, which can legitimately fail to parse).
fn finding_severity_rank(s: &Severity) -> u8 {
    match s {
        Severity::Critical => 0,
        Severity::High => 1,
        Severity::Medium => 2,
        Severity::Low => 3,
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

/// See the identical, independently-duplicated `is_open_status` in
/// `reconcile.rs` for the exhaustive-match rationale (spec 074 S3).
fn is_open_status(status: &FindingStatus) -> bool {
    match status {
        FindingStatus::AwaitingTriage
        | FindingStatus::Triaged
        | FindingStatus::ApprovedForPlanning => true,
        FindingStatus::Structured
        | FindingStatus::Deferred
        | FindingStatus::Rejected
        | FindingStatus::Resolved
        | FindingStatus::Blocked => false,
    }
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
        // `min_severity` is a free-form DSL param (stays `Option<String>` — not
        // in S3's scope), so parse it into `Severity` here at the seam; an
        // unrecognized string falls back to rank 4, same as the pre-enum
        // behavior (matches every real `Severity`, i.e. no-op filter).
        let min_severity_rank = parsed
            .min_severity
            .as_deref()
            .and_then(|s| s.parse::<Severity>().ok())
            .as_ref()
            .map(finding_severity_rank)
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
            open.retain(|f| finding_severity_rank(&f.severity) <= min_severity_rank);
        }

        open.sort_by_key(|f| finding_severity_rank(&f.severity));

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

        const CR_SYNTHESIS_TEMPLATE: &str = "You are a change request author. Synthesize a concise, actionable change request from the following open findings for scope {{scope}}.\n\n## Findings\n{{findings}}\n\n## Instructions\n- Write a short, specific title (under 120 chars) describing the overall action needed.\n- Write a markdown body summarizing the findings and why they matter. Reference severity.\n- Focus on what needs to change, not on describing the grader.\n\nReturn ONLY a JSON object matching the schema.";

        // R2: run via the injected AgentClient (real impl: aikit Pipeline,
        // wrapped in spawn_blocking — see llm_client.rs). Synthesis failure
        // (including a stub explicitly returning Err) is tolerated here —
        // unlike ReconcileOperator's adjudication, a change request can
        // always fall back to a deterministic title/body below.
        let synthesis: Option<CrSynthesis> = match self
            .agent_client
            .run_pipeline(
                CR_SYNTHESIS_TEMPLATE,
                CR_SYNTHESIS_SCHEMA,
                &[("scope", &scope_label), ("findings", &findings_str)],
                &engine,
                model.as_deref(),
                &workspace_root,
                std::time::Duration::from_secs(timeout_secs),
                1,
            )
            .await
        {
            Ok(v) => serde_json::from_value::<CrSynthesis>(v).ok(),
            Err(e) => {
                tracing::warn!("ChangeRequestOperator: LLM synthesis failed, using fallback: {e}");
                None
            }
        };

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::executor::{ExecutionOverrides, GraphHandle};
    use crate::workflow::operator::{OperatorRegistry, StateView};
    use newton_backend::SqliteBackendStore;
    use newton_types::{CreateFindingBody, Origin};
    use serde_json::json;

    fn make_ctx() -> crate::workflow::operator::ExecutionContext {
        crate::workflow::operator::ExecutionContext {
            workspace_path: std::path::PathBuf::from("/tmp"),
            execution_id: "test-exec".to_string(),
            task_id: "test-task".to_string(),
            iteration: 1,
            state_view: StateView::new(json!({}), json!({}), json!({})),
            graph: GraphHandle::new(std::collections::HashMap::new()),
            workflow_file: std::path::PathBuf::from("/tmp/test.yaml"),
            nesting_depth: 0,
            execution_overrides: ExecutionOverrides {
                parallel_limit: None,
                max_time_seconds: None,
                checkpoint_base_path: None,
                artifact_base_path: None,
                max_nesting_depth: None,
                verbose: false,
                sink: None,
                pre_seed_nodes: true,
                state_dir: None,
            },
            operator_registry: OperatorRegistry::new(),
        }
    }

    /// Stub `AgentClient` returning a canned result (or a canned failure),
    /// ignoring its inputs — proving `execute` uses the injected client
    /// rather than a real LLM (spec 074 S8).
    struct StubAgentClient {
        response: Result<Value, String>,
    }

    #[async_trait]
    impl AgentClient for StubAgentClient {
        async fn run_pipeline(
            &self,
            _template: &str,
            _schema: &str,
            _vars: &[(&str, &str)],
            _engine: &str,
            _model: Option<&str>,
            _workspace_root: &std::path::Path,
            _timeout: std::time::Duration,
            _max_retries: u32,
        ) -> Result<Value, String> {
            self.response.clone()
        }
    }

    async fn seed_finding(store: &Arc<dyn BackendStore>, scope_id: &str) {
        store
            .create_finding(CreateFindingBody {
                id: Uuid::new_v4().to_string(),
                source: "test-grader".to_string(),
                origin: Origin::System,
                component_id: None,
                module: Some(scope_id.to_string()),
                repo_id: None,
                kpi_id: None,
                dimension: "tests".to_string(),
                location: None,
                fingerprint: format!("module:{scope_id}:tests:"),
                title: "Coverage is low".to_string(),
                why_it_matters: "matters".to_string(),
                recommended_action: "fix it".to_string(),
                severity: Severity::High,
                risk: "high".to_string(),
                confidence: Some(0.8),
                evidence: None,
                expected_value: None,
                effort: None,
                status: FindingStatus::AwaitingTriage,
                last_seen_at: None,
                depends_on: vec![],
                blocks: vec![],
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn execute_uses_injected_agent_client_title_and_body() {
        let store: Arc<dyn BackendStore> =
            Arc::new(SqliteBackendStore::new_in_memory().await.unwrap());
        seed_finding(&store, "mod-cr-001").await;

        let stub = StubAgentClient {
            response: Ok(json!({
                "title": "Stub-authored change request",
                "body": "Stub body text"
            })),
        };
        let op = ChangeRequestOperator::with_agent_client(
            std::path::PathBuf::from("/tmp"),
            store.clone(),
            Arc::new(stub),
        );

        let params = json!({
            "scope": "module",
            "scope_id": "mod-cr-001",
        });

        let result = op.execute(params, make_ctx()).await.unwrap();
        assert_eq!(result["decision"], "propose");
        let cr_id = result["change_request_id"].as_str().unwrap().to_string();

        let cr = store.get_change_request(&cr_id).await.unwrap();
        assert_eq!(
            cr.title, "Stub-authored change request",
            "must use the stub's synthesized title, not the deterministic fallback"
        );
        assert_eq!(cr.body.as_deref(), Some("Stub body text"));
    }

    /// Proves the graceful-fallback behavior (untouched by this seam) still
    /// works when the injected `AgentClient` fails — same tolerance the
    /// operator had for a real LLM outage, now directly testable.
    #[tokio::test]
    async fn execute_falls_back_when_agent_client_fails() {
        let store: Arc<dyn BackendStore> =
            Arc::new(SqliteBackendStore::new_in_memory().await.unwrap());
        seed_finding(&store, "mod-cr-002").await;

        let stub = StubAgentClient {
            response: Err("stub: synthesis intentionally failing".to_string()),
        };
        let op = ChangeRequestOperator::with_agent_client(
            std::path::PathBuf::from("/tmp"),
            store.clone(),
            Arc::new(stub),
        );

        let params = json!({
            "scope": "module",
            "scope_id": "mod-cr-002",
        });

        let result = op.execute(params, make_ctx()).await.unwrap();
        assert_eq!(
            result["decision"], "propose",
            "synthesis failure must not fail the whole operator"
        );
        let cr_id = result["change_request_id"].as_str().unwrap().to_string();

        let cr = store.get_change_request(&cr_id).await.unwrap();
        assert!(
            cr.title.starts_with("Address 1 finding"),
            "must use the deterministic fallback title when the AgentClient fails, got: {}",
            cr.title
        );
    }
}
