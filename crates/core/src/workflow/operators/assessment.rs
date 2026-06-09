//! Shared Assessment validate+persist+output core (Specs 062, 065).

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use newton_backend::{BackendStore, CreateEvalRunBody, CreateGradeInlineBody};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct AssessmentScore {
    pub dimension: String,
    pub score: f64,
    pub rationale: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AssessmentObservation {
    pub dimension: String,
    pub severity: Option<String>,
    pub observation: String,
    pub why_it_matters: Option<String>,
    pub recommended_action: Option<String>,
    pub location: Option<Value>,
    pub confidence: Option<f64>,
    pub evidence: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct AssessmentContent {
    pub overall_score: f64,
    pub verdict: String,
    pub summary: Option<String>,
    pub scores: Vec<AssessmentScore>,
    #[serde(default)]
    pub observations: Vec<AssessmentObservation>,
}

pub fn validate_assessment(json: &Value) -> Result<AssessmentContent, AppError> {
    let content: AssessmentContent = serde_json::from_value(json.clone()).map_err(|e| {
        AppError::new(
            ErrorCategory::ToolExecutionError,
            format!("Assessment JSON does not match expected schema: {e}"),
        )
        .with_code("GRADER-001")
    })?;
    if !(0.0..=100.0).contains(&content.overall_score) {
        return Err(AppError::new(
            ErrorCategory::ToolExecutionError,
            "overall_score must be 0–100",
        )
        .with_code("GRADER-002"));
    }
    Ok(content)
}

/// Build a dimension → kpi_id map by listing KPIs and matching by name.
async fn dimension_kpi_map(store: &Arc<dyn BackendStore>) -> HashMap<String, String> {
    match store.list_kpis().await {
        Ok(kpis) => kpis
            .into_iter()
            .map(|k| (k.name.to_lowercase(), k.id))
            .collect(),
        Err(_) => HashMap::new(),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn persist_assessment(
    store: &Arc<dyn BackendStore>,
    run_id: &str,
    grader: &str,
    scope: &str,
    scope_id: &str,
    content: &AssessmentContent,
    raw_json: &Value,
    evaluated_at: &str,
) -> Result<(), AppError> {
    // M5: resolve dimension → kpiId when a matching KPI exists.
    let kpi_map = dimension_kpi_map(store).await;

    let grades: Vec<CreateGradeInlineBody> = content
        .scores
        .iter()
        .map(|s| CreateGradeInlineBody {
            kpi_id: kpi_map.get(&s.dimension.to_lowercase()).cloned(),
            dimension: s.dimension.clone(),
            score: s.score,
            evidence: None,
            evaluated_at: Some(evaluated_at.to_string()),
        })
        .collect();

    let raw_str = serde_json::to_string(raw_json).ok();

    store
        .create_eval_run(CreateEvalRunBody {
            id: run_id.to_string(),
            source: grader.to_string(),
            scope: scope.to_string(),
            scope_id: scope_id.to_string(),
            score: Some(content.overall_score),
            verdict: Some(content.verdict.clone()),
            summary: content.summary.clone(),
            evaluated_at: Some(evaluated_at.to_string()),
            grades: Some(grades),
            raw_assessment: raw_str,
        })
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("Failed to persist Assessment: {e:?}"),
            )
            .with_code("GRADER-010")
        })?;

    Ok(())
}

pub fn build_output(content: &AssessmentContent, raw_json: Value) -> Value {
    let mut score_by_dim = serde_json::Map::new();
    for s in &content.scores {
        score_by_dim.insert(s.dimension.clone(), serde_json::json!(s.score));
    }

    // M3: always emit all four severity buckets + total so gate expressions like
    // `counts.critical == 0` never encounter a missing key.
    let mut counts = serde_json::Map::new();
    let mut by_sev: HashMap<String, usize> = HashMap::new();
    for obs in &content.observations {
        *by_sev
            .entry(obs.severity.clone().unwrap_or_else(|| "medium".to_string()))
            .or_default() += 1;
    }
    counts.insert(
        "total".to_string(),
        serde_json::json!(content.observations.len()),
    );
    for bucket in ["critical", "high", "medium", "low"] {
        counts.insert(
            bucket.to_string(),
            serde_json::json!(by_sev.get(bucket).copied().unwrap_or(0)),
        );
    }

    serde_json::json!({
        "overall_score": content.overall_score,
        "verdict": content.verdict,
        "score_by_dimension": score_by_dim,
        "counts": counts,
        "assessment": raw_json,
    })
}
