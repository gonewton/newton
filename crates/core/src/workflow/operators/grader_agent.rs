//! GraderAgentOperator — rubric-based grader using AI via aikit-sdk.
//! Spec 065.

#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use crate::workflow::operators::assessment;
use crate::workflow::operators::engine::{extract_text_from_sdk_event, AikitEngineManager};
use async_trait::async_trait;
use chrono::Utc;
use newton_backend::BackendStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub struct GraderAgentOperator {
    _workspace_root: PathBuf,
    store: Arc<dyn BackendStore>,
    engine_manager: AikitEngineManager,
}

impl GraderAgentOperator {
    pub fn new(
        _workspace_root: PathBuf,
        store: Arc<dyn BackendStore>,
        engine_manager: AikitEngineManager,
    ) -> Self {
        Self {
            _workspace_root,
            store,
            engine_manager,
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
}

fn default_engine() -> String {
    "claude".to_string()
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct GraderAgentOutput {
    pub overall_score: f64,
    pub verdict: String,
}

const ASSESSMENT_PROMPT_TEMPLATE: &str = r#"You are a grader evaluating quality against a rubric.

Rubric:
{rubric}

Scope: {scope}/{scope_id}

Evaluate the scope using the rubric above. Return ONLY a valid JSON Assessment object with this structure:
{{
  "overall_score": <number 0-100>,
  "verdict": "<pass|fail|needs_improvement>",
  "summary": "<brief summary>",
  "scores": [
    {{"dimension": "<dim>", "score": <0-100>, "rationale": "<why>"}}
  ],
  "observations": [
    {{
      "dimension": "<dim>",
      "severity": "<critical|high|medium|low>",
      "observation": "<what was observed>",
      "why_it_matters": "<impact>",
      "recommended_action": "<action>",
      "confidence": <0.0-1.0>
    }}
  ]
}}

Return ONLY the JSON object, no markdown fences or other text."#;

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

        // Build prompt
        let prompt = ASSESSMENT_PROMPT_TEMPLATE
            .replace("{rubric}", &parsed.rubric)
            .replace("{scope}", &parsed.scope)
            .replace("{scope_id}", &parsed.scope_id);

        tracing::debug!(
            grader = %parsed.grader,
            engine = %parsed.engine,
            scope = %parsed.scope,
            scope_id = %parsed.scope_id,
            "GraderAgentOperator: invoking AI engine"
        );

        // Call engine
        let (events, run_result) = self
            .engine_manager
            .execute_engine_events(
                &parsed.engine,
                &prompt,
                parsed.model.as_deref(),
                Some(Duration::from_secs(120)),
            )
            .await?;

        // Surface engine errors
        run_result.map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("GraderAgentOperator: AI engine returned error: {e:?}"),
            )
            .with_code("GRADER-AGENT-002")
        })?;

        // Extract text from events
        let text: String = events
            .iter()
            .filter_map(extract_text_from_sdk_event)
            .collect::<Vec<_>>()
            .join("");

        if text.is_empty() {
            return Err(AppError::new(
                ErrorCategory::ToolExecutionError,
                "GraderAgentOperator: AI engine produced no text output",
            )
            .with_code("GRADER-AGENT-003"));
        }

        // Parse extracted text as Assessment JSON
        // Try to find JSON in the text (strip markdown fences if present)
        let json_text = extract_json(&text);
        let mut assessment_json: Value = serde_json::from_str(&json_text).map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!(
                    "GraderAgentOperator: AI output is not valid Assessment JSON: {e}. text: {text}"
                ),
            )
            .with_code("GRADER-AGENT-004")
        })?;

        // Stamp envelope
        let now = Utc::now().to_rfc3339();
        if let Some(obj) = assessment_json.as_object_mut() {
            obj.entry("grader")
                .or_insert_with(|| Value::String(parsed.grader.clone()));
            obj.entry("scope")
                .or_insert_with(|| Value::String(parsed.scope.clone()));
            obj.entry("scope_id")
                .or_insert_with(|| Value::String(parsed.scope_id.clone()));
            obj.entry("evaluated_at")
                .or_insert_with(|| Value::String(now.clone()));
        }

        // Validate assessment
        let content = assessment::validate_assessment(&assessment_json)?;

        // Persist assessment
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

/// Extract JSON from text, stripping markdown code fences if present.
fn extract_json(text: &str) -> String {
    let trimmed = text.trim();
    // Try to strip ```json ... ``` or ``` ... ```
    if let Some(inner) = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
    {
        if let Some(end) = inner.rfind("```") {
            return inner[..end].trim().to_string();
        }
    }
    // Try to find first { and last }
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        return trimmed[start..=end].to_string();
    }
    trimmed.to_string()
}
