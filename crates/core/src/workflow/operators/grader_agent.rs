//! GraderAgentOperator — rubric-based grader using AI via aikit-sdk Pipeline.
//! Spec 065 + 067.

#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::grading::assessment;
use crate::workflow::operator::{ExecutionContext, Operator};
use crate::workflow::operators::llm_client::{AgentClient, RealAgentClient};
use async_trait::async_trait;
use chrono::Utc;
use newton_types::BackendStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

pub struct GraderAgentOperator {
    workspace_root: PathBuf,
    store: Arc<dyn BackendStore>,
    agent_client: Arc<dyn AgentClient>,
}

impl GraderAgentOperator {
    pub const NAME: &'static str = "GraderAgentOperator";

    pub fn new(
        workspace_root: PathBuf,
        store: Arc<dyn BackendStore>,
        _engine_manager: crate::workflow::operators::engine::AikitEngineManager,
    ) -> Self {
        Self {
            workspace_root,
            store,
            agent_client: Arc::new(RealAgentClient),
        }
    }

    /// Test/injection seam (spec 074 S8): construct with a stubbed
    /// `AgentClient` instead of the real `aikit_sdk`-backed one, so tests
    /// can drive `execute`'s grading logic without a real agent subprocess.
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
            params_schema: schemars::schema_for!(GraderAgentParams),
            output_schema: schemars::schema_for!(GraderAgentOutput),
        }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct GraderAgentParams {
    /// Grader identifier (e.g. "docs-quality-grader").
    pub grader: String,
    /// Scope kind (e.g. "repo", "component").
    pub scope: String,
    /// Scope entity ID.
    pub scope_id: String,
    /// Grading rubric (plain text or markdown describing dimensions and scoring criteria).
    pub rubric: String,
    /// Model to use (optional, uses aikit default if not set).
    #[serde(default)]
    pub model: Option<String>,
    /// Engine to use (default: "claude").
    #[serde(default = "default_engine")]
    pub engine: String,
    /// Timeout in seconds (default: 120).
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_engine() -> String {
    "claude".to_string()
}

fn default_timeout() -> u64 {
    120
}

/// Assessment output schema for Pipeline validation.
const ASSESSMENT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "overall_score": {"type": "number", "minimum": 0, "maximum": 100},
    "verdict": {"type": "string", "enum": ["pass", "fail", "needs_improvement"]},
    "summary": {"type": "string"},
    "scores": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "dimension": {"type": "string"},
          "score": {"type": "number"},
          "rationale": {"type": "string"}
        },
        "required": ["dimension", "score"]
      }
    },
    "observations": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "dimension": {"type": "string"},
          "severity": {"type": "string"},
          "observation": {"type": "string"},
          "why_it_matters": {"type": "string"},
          "recommended_action": {"type": "string"},
          "confidence": {"type": "number"}
        },
        "required": ["dimension", "observation"]
      }
    }
  },
  "required": ["overall_score", "verdict", "scores"]
}"#;

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct GraderAgentOutput {
    pub overall_score: f64,
    pub verdict: String,
    pub score_by_dimension: Value,
    pub counts: Value,
    pub assessment: Value,
}

const ASSESSMENT_PROMPT_TEMPLATE: &str = r#"You are a grader evaluating quality against a rubric.

Rubric:
{{rubric}}

Scope: {{scope}}/{{scope_id}}

Evaluate the scope using the rubric above. Return ONLY a valid JSON Assessment object matching the schema provided. Include:
- overall_score (0-100)
- verdict (pass | fail | needs_improvement)
- summary (brief summary)
- scores: array of {dimension, score, rationale}
- observations: array of {dimension, severity, observation, why_it_matters, recommended_action, confidence}"#;

#[async_trait]
impl Operator for GraderAgentOperator {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let parsed: GraderAgentParams = serde_json::from_value(params.clone()).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("GraderAgentOperator params invalid: {e}"),
            )
            .with_code("GRADER-AGENT-001")
        })?;
        if parsed.grader.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "GraderAgentOperator requires a non-empty grader",
            )
            .with_code("GRADER-AGENT-001"));
        }
        if parsed.rubric.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "GraderAgentOperator requires a non-empty rubric",
            )
            .with_code("GRADER-AGENT-001"));
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
        let parsed: GraderAgentParams = serde_json::from_value(params.clone()).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("GraderAgentOperator params invalid: {e}"),
            )
            .with_code("GRADER-AGENT-001")
        })?;

        let engine = parsed.engine.clone();
        let model = parsed.model.clone();
        let rubric = parsed.rubric.clone();
        let scope = parsed.scope.clone();
        let scope_id = parsed.scope_id.clone();
        let timeout_secs = parsed.timeout_seconds;
        let workspace_root = self.workspace_root.clone();

        tracing::debug!(
            grader = %parsed.grader,
            engine = %engine,
            scope = %scope,
            scope_id = %scope_id,
            "GraderAgentOperator: invoking AI engine via Pipeline"
        );

        // R2: run via the injected AgentClient (real impl: aikit Pipeline,
        // schema-in → schema-out → validate → retry, wrapped in
        // spawn_blocking since Pipeline::run is blocking — see llm_client.rs).
        let mut assessment_json = self
            .agent_client
            .run_pipeline(
                ASSESSMENT_PROMPT_TEMPLATE,
                ASSESSMENT_SCHEMA,
                &[
                    ("rubric", rubric.as_str()),
                    ("scope", scope.as_str()),
                    ("scope_id", scope_id.as_str()),
                ],
                &engine,
                model.as_deref(),
                &workspace_root,
                std::time::Duration::from_secs(timeout_secs),
                2,
            )
            .await
            .map_err(|e| {
                AppError::new(
                    ErrorCategory::ToolExecutionError,
                    format!("GraderAgentOperator: Pipeline failed: {e}"),
                )
                .with_code("GRADER-AGENT-002")
            })?;

        // M1: overwrite envelope fields authoritatively (ignore whatever the grader emitted).
        let now = Utc::now().to_rfc3339();
        if let Some(obj) = assessment_json.as_object_mut() {
            obj.insert("grader".to_string(), Value::String(parsed.grader.clone()));
            obj.insert("scope".to_string(), Value::String(parsed.scope.clone()));
            obj.insert(
                "scope_id".to_string(),
                Value::String(parsed.scope_id.clone()),
            );
            obj.insert("evaluated_at".to_string(), Value::String(now.clone()));
        }

        let content = assessment::validate_assessment(&assessment_json)?;

        let run_id = Uuid::new_v4().to_string();
        assessment::persist_assessment(
            &self.store,
            &run_id,
            &parsed.grader,
            &parsed.scope,
            &parsed.scope_id,
            &content,
            &assessment_json,
            &now,
        )
        .await?;

        Ok(assessment::build_output(&content, assessment_json))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::executor::{ExecutionOverrides, GraphHandle};
    use crate::workflow::operator::{OperatorRegistry, StateView};
    use newton_backend::SqliteBackendStore;
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

    /// Stub `AgentClient` that returns a canned Assessment JSON, ignoring
    /// its inputs — proving `execute` uses the injected client rather than
    /// a real LLM (spec 074 S8).
    struct StubAgentClient {
        response: Value,
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
            Ok(self.response.clone())
        }
    }

    #[tokio::test]
    async fn execute_uses_injected_agent_client_not_a_real_llm() {
        let store: Arc<dyn BackendStore> =
            Arc::new(SqliteBackendStore::new_in_memory().await.unwrap());
        // `persist_assessment` -> `create_eval_run` validates the scope
        // entity exists; "product" is the only scope level with no FK
        // dependencies of its own, so seed one.
        let product = store
            .create_product(newton_types::CreateProductBody {
                name: "test-product".to_string(),
            })
            .await
            .unwrap();

        let stub = StubAgentClient {
            response: json!({
                "overall_score": 82.0,
                "verdict": "pass",
                "summary": "stubbed",
                "scores": [{"dimension": "tests", "score": 90.0, "rationale": "ok"}],
                "observations": []
            }),
        };
        let op = GraderAgentOperator::with_agent_client(
            std::path::PathBuf::from("/tmp"),
            store.clone(),
            Arc::new(stub),
        );

        let params = json!({
            "grader": "stub-grader",
            "scope": "product",
            "scope_id": product.id,
            "rubric": "Evaluate test coverage.",
        });

        let result = op.execute(params, make_ctx()).await.unwrap();

        assert_eq!(
            result["overall_score"], 82.0,
            "must reflect stub, not a real LLM call"
        );
        assert_eq!(result["verdict"], "pass");
        assert_eq!(result["score_by_dimension"]["tests"], 90.0);
    }
}
