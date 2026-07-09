//! ReconcileOperator — reads observations from Assessment output and reconciles with stored Findings.
//! Spec 063 + 067.

#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use chrono::Utc;
use newton_backend::{BackendStore, CreateFindingBody, FindingItem, PatchFindingBody};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

pub struct ReconcileOperator {
    workspace_root: PathBuf,
    store: Arc<dyn BackendStore>,
}

impl ReconcileOperator {
    pub fn new(workspace_root: PathBuf, store: Arc<dyn BackendStore>) -> Self {
        Self {
            workspace_root,
            store,
        }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ReconcileParams {
    /// Scope kind: product | component | repo | module.
    pub scope: String,
    /// Scope entity ID to reconcile findings for.
    pub scope_id: String,
    /// Grader name (used as source when creating/matching findings).
    #[serde(default = "default_grader")]
    pub grader: String,
    /// Assessment JSON passed inline (caller resolves from task output).
    pub assessment: Value,
    /// Engine to use for LLM adjudication (default: "claude").
    #[serde(default = "default_engine")]
    pub engine: String,
    /// Model for LLM adjudication (optional).
    #[serde(default)]
    pub model: Option<String>,
    /// Timeout for LLM adjudication in seconds (default: 60).
    #[serde(default = "default_adjudication_timeout")]
    pub adjudication_timeout_seconds: u64,
}

fn default_grader() -> String {
    "unknown".to_string()
}

fn default_engine() -> String {
    "claude".to_string()
}

fn default_adjudication_timeout() -> u64 {
    60
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

/// Stable natural key: scope + dimension + normalized_location.
/// Does NOT use the free observation text so that rewording by a
/// non-deterministic grader produces the same fingerprint.
fn stable_fingerprint(
    scope: &str,
    scope_id: &str,
    dimension: &str,
    location: Option<&Value>,
) -> String {
    let loc_key = match location {
        Some(v) => {
            // Normalize: sort object keys, strip whitespace.
            normalize_location(v)
        }
        None => String::new(),
    };
    format!("{}:{}:{}:{}", scope, scope_id, dimension, loc_key)
}

fn normalize_location(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .filter_map(|k| {
                    map.get(*k)
                        .map(|val| format!("{}={}", k, val.to_string().replace(' ', "")))
                })
                .collect();
            parts.join(",")
        }
        Value::String(s) => s.clone(),
        other => other.to_string().replace(' ', ""),
    }
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

fn is_open_status(status: &str) -> bool {
    matches!(
        status,
        "awaiting_triage" | "triaged" | "approved_for_planning"
    )
}

/// Adjudication result from the LLM.
#[derive(Debug, Deserialize)]
struct AdjudicationPlan {
    /// Each entry: (observation_index, finding_id) — LLM says these match.
    matched: Vec<AdjudicationMatch>,
    /// Finding IDs the LLM says are resolved (not seen in this run at all).
    resolved: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AdjudicationMatch {
    observation_index: usize,
    finding_id: String,
}

const RECONCILE_ADJUDICATION_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "matched": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "observation_index": {"type": "integer"},
          "finding_id": {"type": "string"}
        },
        "required": ["observation_index", "finding_id"]
      }
    },
    "resolved": {
      "type": "array",
      "items": {"type": "string"}
    }
  },
  "required": ["matched", "resolved"]
}"#;

/// Populate the correct scope column on a CreateFindingBody based on scope kind.
fn apply_scope(body: &mut CreateFindingBody, scope: &str, scope_id: &str) {
    match scope {
        "component" => body.component_id = Some(scope_id.to_string()),
        "repo" => body.repo_id = Some(scope_id.to_string()),
        "module" => body.module = Some(scope_id.to_string()),
        _ => {}
    }
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
        let scope = &parsed.scope;
        let scope_id = &parsed.scope_id;
        let grader = &parsed.grader;

        // Parse observations from the assessment JSON.
        let observations = parse_observations(&parsed.assessment);

        // R1 + R3: list only this grader's findings for this scope.
        let all_scope_findings = self
            .store
            .list_findings(None, Some(scope.clone()), Some(scope_id.clone()))
            .await
            .map_err(|e| {
                AppError::new(
                    ErrorCategory::ToolExecutionError,
                    format!("ReconcileOperator: failed to list findings: {e:?}"),
                )
                .with_code("RECONCILE-010")
            })?;

        // R3: restrict to this grader's source only.
        let existing_findings: Vec<&FindingItem> = all_scope_findings
            .iter()
            .filter(|f| f.source == *grader)
            .collect();

        // Build stable-fingerprint index from existing findings.
        let mut fp_index: HashMap<String, &FindingItem> = HashMap::new();
        for finding in &existing_findings {
            fp_index.insert(finding.fingerprint.clone(), finding);
        }

        // Compute stable fingerprint for each observation and partition into:
        // - deterministically matched (fp hit)
        // - unmatched (need LLM adjudication or create)
        let mut matched_by_fp: Vec<(usize, String)> = vec![]; // (obs_idx, fp)
        let mut unmatched: Vec<(usize, &Observation)> = vec![];

        for (idx, obs) in observations.iter().enumerate() {
            let fp = stable_fingerprint(scope, scope_id, &obs.dimension, obs.location.as_ref());
            // Deterministic match only when location is present. Location-less observations
            // share a bare-dimension fingerprint and cannot be distinguished structurally —
            // route them to the LLM adjudicator which judges semantic sameness.
            if obs.location.is_some() && fp_index.contains_key(&fp) {
                matched_by_fp.push((idx, fp));
            } else {
                unmatched.push((idx, obs));
            }
        }

        // Open candidates not yet matched — these are the LLM adjudication targets.
        let matched_fps_set: std::collections::HashSet<&str> =
            matched_by_fp.iter().map(|(_, fp)| fp.as_str()).collect();
        let unmatched_candidates: Vec<&FindingItem> = existing_findings
            .iter()
            .filter(|f| {
                is_open_status(&f.status) && !matched_fps_set.contains(f.fingerprint.as_str())
            })
            .copied()
            .collect();

        // Run LLM adjudication for unmatched observations (if any candidates exist).
        let workspace_root = self.workspace_root.clone();
        let engine = parsed.engine.clone();
        let model = parsed.model.clone();
        let timeout_secs = parsed.adjudication_timeout_seconds;

        // Collect data needed for the spawn_blocking call (owned copies).
        let unmatched_owned: Vec<(usize, String, String, Option<Value>)> = unmatched
            .iter()
            .map(|(idx, obs)| {
                (
                    *idx,
                    obs.dimension.clone(),
                    obs.observation.clone(),
                    obs.location.clone(),
                )
            })
            .collect();
        let candidates_owned: Vec<(String, String, String, String, String)> = unmatched_candidates
            .iter()
            .map(|f| {
                (
                    f.id.clone(),
                    f.dimension.clone(),
                    f.title.clone(),
                    f.fingerprint.clone(),
                    f.status.clone(),
                )
            })
            .collect();

        // NOTE (Fuzziness is not failure tolerance — CONTEXT.md "Reconciliation"):
        // the fuzzy-optimizer tolerance below covers Observation<->Finding
        // mis-matches, not a Reconciliation running without its semantic-matching
        // half. If adjudication itself fails, we must `?` out of `execute` here,
        // BEFORE any store mutation (Phase 1/2/3 below), so that a transient LLM
        // outage can never create duplicate Findings or wrongly auto-resolve real
        // ones (including `blocked` Findings, which only a human may clear).
        let adj_plan: Option<AdjudicationPlan> = if !unmatched_owned.is_empty()
            && !candidates_owned.is_empty()
        {
            let obs_json: Vec<serde_json::Value> = unmatched_owned
                .iter()
                .map(|(idx, dim, obs_text, loc)| {
                    serde_json::json!({
                        "index": idx,
                        "dimension": dim,
                        "observation": obs_text,
                        "location": loc,
                    })
                })
                .collect();
            let findings_json: Vec<serde_json::Value> = candidates_owned
                .iter()
                .map(|(id, dim, title, fp, status)| {
                    serde_json::json!({
                        "id": id,
                        "dimension": dim,
                        "title": title,
                        "fingerprint": fp,
                        "status": status,
                    })
                })
                .collect();
            let obs_str = serde_json::to_string_pretty(&obs_json).unwrap_or_default();
            let findings_str = serde_json::to_string_pretty(&findings_json).unwrap_or_default();

            let join_result = tokio::task::spawn_blocking(move || {
                let template = "You are a semantic matching agent. Your job is ONLY to judge whether observations from a grader run match existing findings by meaning — not to evaluate quality or add new analysis.\n\n## Unmatched observations (this run)\n{{observations}}\n\n## Candidate open findings (existing)\n{{findings}}\n\n## Task\nFor each observation, decide:\n- Does it semantically describe the same issue as an existing finding? If so, record the match.\n- Is it genuinely new? Record its index in `new`.\n- Are any candidate findings NOT covered by any observation (resolved)? Record their IDs in `resolved`.\n\nReturn a JSON object matching the schema. Keep temperature low — judge sameness strictly.";

                let runner = aikit_sdk::AgentRunner::new()
                    .agent(&engine)
                    .working_dir(&workspace_root.to_string_lossy())
                    .timeout(std::time::Duration::from_secs(timeout_secs));
                let runner = if let Some(ref m) = model {
                    runner.model(m)
                } else {
                    runner
                };

                let pipeline = aikit_sdk::pipeline::Pipeline::new(template, RECONCILE_ADJUDICATION_SCHEMA)
                    .max_retries(1);

                match pipeline.run(&[("observations", &obs_str), ("findings", &findings_str)], runner) {
                    Ok(pr) => serde_json::from_value::<AdjudicationPlan>(pr.data)
                        .map_err(|e| format!("failed to parse adjudication plan: {e}")),
                    Err(e) => Err(format!("LLM adjudication failed: {e}")),
                }
            })
            .await;

            // Adjudication failure (LLM error, unparseable plan, or a panicked
            // blocking task) fails the whole operator BEFORE any store mutation —
            // see the "Fuzziness is not failure tolerance" note above. This is
            // NOT a ValidationError (which task_execution::is_retryable treats as
            // hard-non-retryable): it is a ToolExecutionError so the normal
            // transient-failure retry/backoff applies, and only a persistent
            // failure fails the task/cycle.
            let plan = match join_result {
                Ok(Ok(plan)) => plan,
                Ok(Err(msg)) => {
                    return Err(AppError::new(
                        ErrorCategory::ToolExecutionError,
                        format!("ReconcileOperator: adjudication failed: {msg}"),
                    )
                    .with_code("WFG-RECONCILE-ADJ-001"));
                }
                Err(join_err) => {
                    return Err(AppError::new(
                        ErrorCategory::ToolExecutionError,
                        format!(
                            "ReconcileOperator: adjudication task did not complete: {join_err}"
                        ),
                    )
                    .with_code("WFG-RECONCILE-ADJ-001"));
                }
            };
            Some(plan)
        } else {
            None
        };

        // Build sets of: llm-matched obs indices → finding_id, llm-resolved finding IDs.
        let mut llm_matched: HashMap<usize, String> = HashMap::new(); // obs_idx → finding_id
        let mut llm_resolved: std::collections::HashSet<String> = std::collections::HashSet::new();

        if let Some(ref plan) = adj_plan {
            for m in &plan.matched {
                llm_matched.insert(m.observation_index, m.finding_id.clone());
            }
            for fid in &plan.resolved {
                llm_resolved.insert(fid.clone());
            }
        }

        let mut created = 0usize;
        let mut refreshed = 0usize;
        let mut reopened = 0usize;

        // Track finding IDs seen/touched in this run (for auto-resolve sweep).
        let mut seen_finding_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        // Track FPs already patched to avoid double-counting when multiple observations
        // hit the same fingerprint within a single run.
        let mut patched_fps: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Phase 1: handle deterministically matched observations.
        for (idx, fp) in &matched_by_fp {
            let _ = idx; // used implicitly via fp
            if let Some(existing) = fp_index.get(fp.as_str()) {
                seen_finding_ids.insert(existing.id.clone());
                if !patched_fps.insert(fp.clone()) {
                    // Already patched this finding in this run — skip to avoid double-count.
                    continue;
                }
                if existing.status == "resolved" {
                    self.store
                        .patch_finding(
                            &existing.id,
                            PatchFindingBody {
                                status: Some("awaiting_triage".to_string()),
                                last_seen_at: Some(now.clone()),
                                expected_value: None,
                                effort: None,
                                risk: None,
                                blocked_by_plan_id: None,
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
                    self.store
                        .patch_finding(
                            &existing.id,
                            PatchFindingBody {
                                status: None,
                                last_seen_at: Some(now.clone()),
                                expected_value: None,
                                effort: None,
                                risk: None,
                                blocked_by_plan_id: None,
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
            }
        }

        // Phase 2: handle unmatched observations (LLM-matched or create new).
        for (idx, obs) in &unmatched {
            if let Some(finding_id) = llm_matched.get(idx) {
                // LLM says this matches an existing finding — refresh it.
                seen_finding_ids.insert(finding_id.clone());
                self.store
                    .patch_finding(
                        finding_id,
                        PatchFindingBody {
                            status: None,
                            last_seen_at: Some(now.clone()),
                            expected_value: None,
                            effort: None,
                            risk: None,
                            blocked_by_plan_id: None,
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
            } else {
                // Genuinely new — create.
                let id = Uuid::new_v4().to_string();
                let fp = stable_fingerprint(scope, scope_id, &obs.dimension, obs.location.as_ref());
                let title: String = obs.observation.chars().take(120).collect();
                let mut body = CreateFindingBody {
                    id: id.clone(),
                    source: grader.clone(),
                    origin: "system".to_string(),
                    component_id: None,
                    module: None,
                    repo_id: None,
                    kpi_id: None,
                    dimension: obs.dimension.clone(),
                    location: obs.location.clone(),
                    fingerprint: fp,
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
                };
                // R1: populate correct scope column.
                apply_scope(&mut body, scope, scope_id);

                self.store.create_finding(body).await.map_err(|e| {
                    AppError::new(
                        ErrorCategory::ToolExecutionError,
                        format!("ReconcileOperator: failed to create finding: {e:?}"),
                    )
                    .with_code("RECONCILE-012")
                })?;
                seen_finding_ids.insert(id);
                created += 1;
            }
        }

        // Phase 3: R3-scoped resolution — only resolve this grader's open findings
        // not seen in this run (and not LLM-resolved, which is handled here too).
        let mut resolved = 0usize;
        for finding in &existing_findings {
            if is_open_status(&finding.status)
                && !seen_finding_ids.contains(&finding.id)
                && !llm_resolved.contains(&finding.id)
            {
                self.store
                    .patch_finding(
                        &finding.id,
                        PatchFindingBody {
                            status: Some("resolved".to_string()),
                            last_seen_at: Some(now.clone()),
                            expected_value: None,
                            effort: None,
                            risk: None,
                            blocked_by_plan_id: None,
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

        // Also resolve findings explicitly flagged by LLM as resolved.
        for fid in &llm_resolved {
            if let Some(finding) = existing_findings.iter().find(|f| &f.id == fid) {
                if is_open_status(&finding.status) && !seen_finding_ids.contains(fid) {
                    self.store
                        .patch_finding(
                            fid,
                            PatchFindingBody {
                                status: Some("resolved".to_string()),
                                last_seen_at: Some(now.clone()),
                                expected_value: None,
                                effort: None,
                                risk: None,
                                blocked_by_plan_id: None,
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
        }

        Ok(serde_json::json!({
            "created": created,
            "refreshed": refreshed,
            "reopened": reopened,
            "resolved": resolved,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::executor::{ExecutionOverrides, GraphHandle};
    use crate::workflow::operator::{OperatorRegistry, StateView};
    use newton_backend::{BackendStore, PatchFindingBody, SqliteBackendStore};
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

    fn make_assessment(observations: Vec<serde_json::Value>) -> Value {
        json!({
            "overall_score": 75.0,
            "verdict": "needs_improvement",
            "summary": "test",
            "scores": [{"dimension": "tests", "score": 75.0, "rationale": "ok"}],
            "observations": observations
        })
    }

    fn make_obs(dimension: &str, observation: &str, location: Option<&str>) -> serde_json::Value {
        let mut v = json!({
            "dimension": dimension,
            "severity": "medium",
            "observation": observation,
            "why_it_matters": "matters",
            "recommended_action": "fix it",
            "confidence": 0.9
        });
        if let Some(loc) = location {
            v["location"] = json!({"file": loc});
        }
        v
    }

    #[tokio::test]
    async fn r1_round_trip_grade_reconcile_list_findings() {
        let store: Arc<dyn BackendStore> =
            Arc::new(SqliteBackendStore::new_in_memory().await.unwrap());

        // Uses module scope to avoid FK constraints on component/repo.
        let operator = ReconcileOperator::new(std::path::PathBuf::from("/tmp"), store.clone());

        let assessment = make_assessment(vec![
            make_obs("tests", "Coverage is below threshold", Some("src/main.rs")),
            make_obs("security", "Dependency has known CVE", Some("Cargo.toml")),
        ]);

        let params = json!({
            "scope": "module",
            "scope_id": "mod-001",
            "grader": "test-grader",
            "assessment": assessment,
        });

        let result = operator.execute(params, make_ctx()).await.unwrap();

        assert_eq!(result["created"], 2, "should create 2 findings");
        assert_eq!(result["refreshed"], 0);
        assert_eq!(result["resolved"], 0);

        // R1: list_findings must return both findings when scope is provided.
        let findings = store
            .list_findings(
                None,
                Some("module".to_string()),
                Some("mod-001".to_string()),
            )
            .await
            .unwrap();
        assert_eq!(
            findings.len(),
            2,
            "list_findings must return both created findings"
        );

        // Second identical reconcile: creates 0 new, refreshes 2, resolves 0.
        let assessment2 = make_assessment(vec![
            make_obs("tests", "Coverage is below threshold", Some("src/main.rs")),
            make_obs("security", "Dependency has known CVE", Some("Cargo.toml")),
        ]);
        let params2 = json!({
            "scope": "module",
            "scope_id": "mod-001",
            "grader": "test-grader",
            "assessment": assessment2,
        });
        let result2 = operator.execute(params2, make_ctx()).await.unwrap();

        assert_eq!(result2["created"], 0, "second identical run must create 0");
        assert_eq!(result2["refreshed"], 2, "second run must refresh 2");
        assert_eq!(result2["resolved"], 0, "second run must resolve 0");
    }

    #[tokio::test]
    async fn r3_cross_grader_isolation() {
        let store: Arc<dyn BackendStore> =
            Arc::new(SqliteBackendStore::new_in_memory().await.unwrap());

        let op = ReconcileOperator::new(std::path::PathBuf::from("/tmp"), store.clone());

        // Grader A creates findings.
        let params_a = json!({
            "scope": "module",
            "scope_id": "mod-002",
            "grader": "grader-a",
            "assessment": make_assessment(vec![make_obs("tests", "No tests", Some("src/lib.rs"))]),
        });
        op.execute(params_a, make_ctx()).await.unwrap();

        // Grader B runs on same scope — must NOT resolve grader A's findings.
        let params_b = json!({
            "scope": "module",
            "scope_id": "mod-002",
            "grader": "grader-b",
            "assessment": make_assessment(vec![make_obs("docs", "No docs", Some("README.md"))]),
        });
        let result_b = op.execute(params_b, make_ctx()).await.unwrap();

        assert_eq!(result_b["created"], 1, "grader-b creates its own finding");
        assert_eq!(
            result_b["resolved"], 0,
            "grader-b must not resolve grader-a findings"
        );

        // Grader A's finding must still be open.
        let findings = store
            .list_findings(
                None,
                Some("module".to_string()),
                Some("mod-002".to_string()),
            )
            .await
            .unwrap();
        let grader_a_findings: Vec<_> =
            findings.iter().filter(|f| f.source == "grader-a").collect();
        assert_eq!(grader_a_findings.len(), 1);
        assert_eq!(grader_a_findings[0].status, "awaiting_triage");
    }

    /// Two distinct observations with no location in the same dimension must produce
    /// two separate findings, not one (fingerprint-collision bug fixed in spec 067).
    #[tokio::test]
    async fn no_location_observations_are_not_merged() {
        let store: Arc<dyn BackendStore> =
            Arc::new(SqliteBackendStore::new_in_memory().await.unwrap());
        let op = ReconcileOperator::new(std::path::PathBuf::from("/tmp"), store.clone());

        let assessment = make_assessment(vec![
            make_obs(
                "architecture",
                "Circular dependency between layers A and B",
                None,
            ),
            make_obs(
                "architecture",
                "Service boundary violation in module X",
                None,
            ),
        ]);

        let params = json!({
            "scope": "module",
            "scope_id": "mod-003",
            "grader": "arch-grader",
            "assessment": assessment,
        });

        let result = op.execute(params, make_ctx()).await.unwrap();
        assert_eq!(
            result["created"], 2,
            "two distinct no-location observations must produce two findings, not one"
        );

        let findings = store
            .list_findings(
                None,
                Some("module".to_string()),
                Some("mod-003".to_string()),
            )
            .await
            .unwrap();
        assert_eq!(
            findings.len(),
            2,
            "both findings must be stored and retrievable"
        );
    }

    /// PR-4 / B2 — "Reconciliation fails closed". When the LLM adjudicator
    /// fails, `execute` must return an `Err` with code `WFG-RECONCILE-ADJ-001`
    /// and must not mutate the Finding store at all: no new Findings, no
    /// `resolved` transitions, and `blocked` Findings must be left completely
    /// untouched (they are cleared only by a human — CONTEXT.md "Finding").
    ///
    /// The adjudicator is forced to fail deterministically and without any
    /// subprocess/network I/O by passing an `engine` name that is not in
    /// aikit-sdk's runnable-agent allow-list (`codex|claude|gemini|opencode|
    /// agent|aikit`); `AgentRunner`/`Pipeline::run` fails fast with
    /// `RunError::AgentNotRunnable` before spawning anything — the same seam
    /// `AgentOperator`'s `execute_non_runnable_ai_engine_returns_sdk_002` test
    /// uses.
    #[tokio::test]
    async fn adjudication_failure_fails_closed_with_zero_mutations() {
        let store: Arc<dyn BackendStore> =
            Arc::new(SqliteBackendStore::new_in_memory().await.unwrap());
        let op = ReconcileOperator::new(std::path::PathBuf::from("/tmp"), store.clone());

        // Seed run: no existing findings yet, so no adjudication candidates exist
        // and this call succeeds without needing any LLM call at all.
        //   - F1 (location-based "tests" observation): will get a stable fp and
        //     is later flipped to `blocked` to prove blocked Findings survive.
        //   - F2 (location-less "security" observation): location-less
        //     observations never match deterministically, so a repeat run always
        //     routes them to the LLM adjudicator.
        let seed_assessment = make_assessment(vec![
            make_obs("tests", "Coverage is below threshold", Some("src/main.rs")),
            make_obs("security", "Dependency has known CVE", None),
        ]);
        let seed_params = json!({
            "scope": "module",
            "scope_id": "mod-adj-001",
            "grader": "adj-grader",
            "assessment": seed_assessment,
        });
        let seed_result = op.execute(seed_params, make_ctx()).await.unwrap();
        assert_eq!(seed_result["created"], 2, "seed run creates both findings");

        let seeded = store
            .list_findings(
                None,
                Some("module".to_string()),
                Some("mod-adj-001".to_string()),
            )
            .await
            .unwrap();
        assert_eq!(seeded.len(), 2);
        let f1_id = seeded
            .iter()
            .find(|f| f.dimension == "tests")
            .unwrap()
            .id
            .clone();

        // Simulate the loop having auto-blocked F1 (a Plan implementing it
        // failed develop after the retry budget) — only a human may clear this.
        store
            .patch_finding(
                &f1_id,
                PatchFindingBody {
                    status: Some("blocked".to_string()),
                    last_seen_at: None,
                    expected_value: None,
                    effort: None,
                    risk: None,
                    blocked_by_plan_id: Some("plan-x".to_string()),
                },
            )
            .await
            .unwrap();

        let before = store
            .list_findings(
                None,
                Some("module".to_string()),
                Some("mod-adj-001".to_string()),
            )
            .await
            .unwrap();
        let before_json: Vec<serde_json::Value> = before
            .iter()
            .map(|f| serde_json::to_value(f).unwrap())
            .collect();

        // Second run: repeats the location-less "security" observation (F2 is
        // therefore an open, unmatched adjudication candidate) with an engine
        // that aikit-sdk refuses to run — this must trigger adjudication and
        // fail it before any Phase 1/2/3 store mutation runs.
        let rerun_assessment =
            make_assessment(vec![make_obs("security", "Dependency has known CVE", None)]);
        let rerun_params = json!({
            "scope": "module",
            "scope_id": "mod-adj-001",
            "grader": "adj-grader",
            "assessment": rerun_assessment,
            "engine": "not-a-real-agent",
        });

        let err = op
            .execute(rerun_params, make_ctx())
            .await
            .expect_err("adjudication failure must fail the operator");
        assert_eq!(err.code, "WFG-RECONCILE-ADJ-001");
        assert_eq!(err.category, ErrorCategory::ToolExecutionError);

        let after = store
            .list_findings(
                None,
                Some("module".to_string()),
                Some("mod-adj-001".to_string()),
            )
            .await
            .unwrap();
        let after_json: Vec<serde_json::Value> = after
            .iter()
            .map(|f| serde_json::to_value(f).unwrap())
            .collect();

        assert_eq!(
            after.len(),
            2,
            "no Finding may be created or removed on the failure path"
        );
        assert_eq!(
            before_json, after_json,
            "Finding store must be byte-identical before/after a failed adjudication"
        );
        let f1_after = after.iter().find(|f| f.id == f1_id).unwrap();
        assert_eq!(
            f1_after.status, "blocked",
            "blocked Findings must be untouched by the failure path"
        );
    }
}
