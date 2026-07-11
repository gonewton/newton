//! Injectable LLM-call seam for the optimization-loop operators
//! (`ReconcileOperator`, `GraderAgentOperator`, `ChangeRequestOperator`).
//!
//! Spec 074 S8: the `spawn_blocking` + `aikit_sdk::AgentRunner`/`Pipeline`
//! calls that used to live directly inside each operator's `execute` body
//! are wrapped behind these traits so tests can inject a deterministic stub
//! instead of driving a real agent subprocess. This is also the enabler for
//! testing B2's failure path — see the "Fuzziness is not failure tolerance"
//! note in `reconcile.rs`, which relies on `LlmAdjudicator::adjudicate`
//! being independently stubbable to fail.

use async_trait::async_trait;
use std::path::Path;
use std::time::Duration;

// ---------------------------------------------------------------------------
// AgentClient — general aikit-sdk Pipeline seam.
// ---------------------------------------------------------------------------

/// Render `template` against `vars`, run it through `engine`/`model` inside
/// `workspace_root`, validate the response against `schema`, and return the
/// raw JSON result. Captures the shape shared by `GraderAgentOperator`'s
/// rubric-grading call and `ChangeRequestOperator`'s change-request-synthesis
/// call — the two current inline `spawn_blocking { AgentRunner ... Pipeline
/// ... }` blocks differ only in these parameters.
#[async_trait]
pub trait AgentClient: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    async fn run_pipeline(
        &self,
        template: &str,
        schema: &str,
        vars: &[(&str, &str)],
        engine: &str,
        model: Option<&str>,
        workspace_root: &Path,
        timeout: Duration,
        max_retries: u32,
    ) -> Result<serde_json::Value, String>;
}

/// Real implementation: the exact `aikit_sdk::AgentRunner` + `Pipeline`
/// construction every operator used to build inline, run inside
/// `spawn_blocking` since `Pipeline::run` is itself a blocking call.
pub struct RealAgentClient;

#[async_trait]
impl AgentClient for RealAgentClient {
    async fn run_pipeline(
        &self,
        template: &str,
        schema: &str,
        vars: &[(&str, &str)],
        engine: &str,
        model: Option<&str>,
        workspace_root: &Path,
        timeout: Duration,
        max_retries: u32,
    ) -> Result<serde_json::Value, String> {
        let template = template.to_string();
        let schema = schema.to_string();
        let vars_owned: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let engine = engine.to_string();
        let model = model.map(|m| m.to_string());
        let workspace_root = workspace_root.to_path_buf();

        let join_result = tokio::task::spawn_blocking(move || {
            let runner = aikit_sdk::AgentRunner::new()
                .agent(&engine)
                .working_dir(&workspace_root.to_string_lossy())
                .timeout(timeout);
            let runner = if let Some(ref m) = model {
                runner.model(m)
            } else {
                runner
            };

            let pipeline = aikit_sdk::pipeline::Pipeline::new(template.as_str(), schema.as_str())
                .max_retries(max_retries);

            let vars_refs: Vec<(&str, &str)> = vars_owned
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();

            pipeline.run(&vars_refs, runner)
        })
        .await;

        match join_result {
            Ok(Ok(pr)) => Ok(pr.data),
            Ok(Err(e)) => Err(format!("Pipeline failed: {e}")),
            Err(join_err) => Err(format!("spawn_blocking panicked: {join_err}")),
        }
    }
}

// ---------------------------------------------------------------------------
// LlmAdjudicator — narrow seam purpose-built for ReconcileOperator.
// ---------------------------------------------------------------------------

/// Semantic-matching adjudication call used by `ReconcileOperator` to decide
/// whether unmatched observations correspond to existing open Findings.
/// Narrower than `AgentClient`: the template/schema are fixed to the
/// adjudication shape, callers only supply the observations/findings JSON
/// plus engine/model/timeout.
#[async_trait]
pub trait LlmAdjudicator: Send + Sync {
    async fn adjudicate(
        &self,
        observations_json: &str,
        findings_json: &str,
        engine: &str,
        model: Option<&str>,
        workspace_root: &Path,
        timeout: Duration,
    ) -> Result<super::reconcile::AdjudicationPlan, String>;
}

const ADJUDICATION_TEMPLATE: &str = "You are a semantic matching agent. Your job is ONLY to judge whether observations from a grader run match existing findings by meaning — not to evaluate quality or add new analysis.\n\n## Unmatched observations (this run)\n{{observations}}\n\n## Candidate open findings (existing)\n{{findings}}\n\n## Task\nFor each observation, decide:\n- Does it semantically describe the same issue as an existing finding? If so, record the match.\n- Is it genuinely new? Record its index in `new`.\n- Are any candidate findings NOT covered by any observation (resolved)? Record their IDs in `resolved`.\n\nReturn a JSON object matching the schema. Keep temperature low — judge sameness strictly.";

/// Real implementation: the exact `aikit_sdk::AgentRunner` + `Pipeline`
/// construction `ReconcileOperator` used to build inline for its
/// adjudication call, run inside `spawn_blocking`.
pub struct RealLlmAdjudicator;

#[async_trait]
impl LlmAdjudicator for RealLlmAdjudicator {
    async fn adjudicate(
        &self,
        observations_json: &str,
        findings_json: &str,
        engine: &str,
        model: Option<&str>,
        workspace_root: &Path,
        timeout: Duration,
    ) -> Result<super::reconcile::AdjudicationPlan, String> {
        let obs_str = observations_json.to_string();
        let findings_str = findings_json.to_string();
        let engine = engine.to_string();
        let model = model.map(|m| m.to_string());
        let workspace_root = workspace_root.to_path_buf();

        let join_result = tokio::task::spawn_blocking(move || {
            let runner = aikit_sdk::AgentRunner::new()
                .agent(&engine)
                .working_dir(&workspace_root.to_string_lossy())
                .timeout(timeout);
            let runner = if let Some(ref m) = model {
                runner.model(m)
            } else {
                runner
            };

            let pipeline = aikit_sdk::pipeline::Pipeline::new(
                ADJUDICATION_TEMPLATE,
                super::reconcile::RECONCILE_ADJUDICATION_SCHEMA,
            )
            .max_retries(1);

            match pipeline.run(
                &[("observations", &obs_str), ("findings", &findings_str)],
                runner,
            ) {
                Ok(pr) => serde_json::from_value::<super::reconcile::AdjudicationPlan>(pr.data)
                    .map_err(|e| format!("failed to parse adjudication plan: {e}")),
                Err(e) => Err(format!("LLM adjudication failed: {e}")),
            }
        })
        .await;

        match join_result {
            Ok(Ok(plan)) => Ok(plan),
            Ok(Err(msg)) => Err(msg),
            Err(join_err) => Err(format!("adjudication task did not complete: {join_err}")),
        }
    }
}
