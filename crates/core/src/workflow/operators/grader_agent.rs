//! GraderAgentOperator — rubric-based grader using AI via aikit-sdk Pipeline.
//! Spec 065 + 067.

#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use crate::workflow::operators::assessment;
use async_trait::async_trait;
use chrono::Utc;
use newton_backend::BackendStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

pub struct GraderAgentOperator {
    workspace_root: PathBuf,
    store: Arc<dyn BackendStore>,
}

impl GraderAgentOperator {
    pub fn new(
        workspace_root: PathBuf,
        store: Arc<dyn BackendStore>,
        _engine_manager: crate::workflow::operators::engine::AikitEngineManager,
    ) -> Self {
        Self {
            workspace_root,
            store,
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
        "GraderAgentOperator"
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
        schemars::schema_for!(GraderAgentParams)
    }

    fn output_schema(&self) -> schemars::Schema {
        schemars::schema_for!(GraderAgentOutput)
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

        // R2: run via aikit Pipeline (schema-in → schema-out → validate → retry).
        // Pipeline::run is blocking; wrap in spawn_blocking.
        let pipeline_result = tokio::task::spawn_blocking(move || {
            let runner = aikit_sdk::AgentRunner::new()
                .agent(&engine)
                .working_dir(&workspace_root.to_string_lossy())
                .timeout(std::time::Duration::from_secs(timeout_secs));
            let runner = if let Some(ref m) = model {
                runner.model(m)
            } else {
                runner
            };

            let pipeline =
                aikit_sdk::pipeline::Pipeline::new(ASSESSMENT_PROMPT_TEMPLATE, ASSESSMENT_SCHEMA)
                    .max_retries(2);

            pipeline.run(
                &[
                    ("rubric", rubric.as_str()),
                    ("scope", scope.as_str()),
                    ("scope_id", scope_id.as_str()),
                ],
                runner,
            )
        })
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("GraderAgentOperator: spawn_blocking panicked: {e}"),
            )
            .with_code("GRADER-AGENT-002")
        })?
        .map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("GraderAgentOperator: Pipeline failed: {e}"),
            )
            .with_code("GRADER-AGENT-002")
        })?;

        let mut assessment_json = pipeline_result.data;

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
