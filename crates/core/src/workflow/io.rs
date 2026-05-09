#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::expression::ExpressionEngine;
use crate::workflow::operator::StateView;
use crate::workflow::schema::IoBlock;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

/// Structured outcome emitted at workflow completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionEnvelope {
    pub schema_version: String,
    pub execution_id: Option<Uuid>,
    pub status: CompletionStatus,
    pub result: Option<Value>,
    pub error: Option<CompletionError>,
}

impl CompletionEnvelope {
    pub fn success(execution_id: Uuid, result: Option<Value>) -> Self {
        Self {
            schema_version: "1".to_string(),
            execution_id: Some(execution_id),
            status: CompletionStatus::Success,
            result,
            error: None,
        }
    }

    pub fn failure(execution_id: Option<Uuid>, error: CompletionError) -> Self {
        Self {
            schema_version: "1".to_string(),
            execution_id,
            status: CompletionStatus::Failure,
            result: None,
            error: Some(error),
        }
    }

    pub fn internal_error(error: CompletionError) -> Self {
        Self {
            schema_version: "1".to_string(),
            execution_id: None,
            status: CompletionStatus::InternalError,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompletionStatus {
    Success,
    Failure,
    InternalError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionError {
    pub code: Option<String>,
    pub category: String,
    pub message: String,
    pub error_payload: Option<Value>,
}

/// Evaluate result_map expressions and validate against output_schema.
/// Returns Ok(None) if no result_map defined, Ok(Some(value)) on success.
pub fn evaluate_result_map(
    io: &IoBlock,
    final_state: &StateView,
    expr_engine: &ExpressionEngine,
) -> Result<Option<Value>, AppError> {
    let result_map = match &io.result_map {
        None => return Ok(None),
        Some(rm) => rm,
    };

    let eval_ctx = final_state.evaluation_context();
    let mut result_obj = Map::new();

    for (key, value) in result_map {
        let resolved = match value {
            Value::String(s) if s.starts_with("$expr:") => {
                let expr = s["$expr:".len()..].trim();
                expr_engine.evaluate(expr, &eval_ctx).map_err(|e| {
                    AppError::new(
                        ErrorCategory::ValidationError,
                        format!("result_map expression error for key '{key}': {}", e.message),
                    )
                    .with_code("WFG-IO-005")
                })?
            }
            other => other.clone(),
        };
        result_obj.insert(key.clone(), resolved);
    }

    Ok(Some(Value::Object(result_obj)))
}

/// Check if the result satisfies the output_schema.
pub fn validate_output_schema(schema: &Value, result: &Value) -> Result<(), AppError> {
    let compiled = jsonschema::JSONSchema::compile(schema).map_err(|e| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("invalid output_schema: {e}"),
        )
        .with_code("WFG-IO-003")
    })?;

    if let Err(errors) = compiled.validate(result) {
        let first = errors
            .into_iter()
            .next()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "validation failed".to_string());
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!("result does not satisfy io.output_schema: {first}"),
        )
        .with_code("WFG-IO-003"));
    }
    Ok(())
}

/// Validate error_payload against error_schema (non-fatal; returns WFG-IO-004 on failure).
pub fn validate_error_schema(schema: &Value, error_payload: &Value) -> Result<(), AppError> {
    let compiled = jsonschema::JSONSchema::compile(schema).map_err(|e| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("invalid error_schema: {e}"),
        )
        .with_code("WFG-IO-004")
    })?;

    if let Err(errors) = compiled.validate(error_payload) {
        let first = errors
            .into_iter()
            .next()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "validation failed".to_string());
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!("error_payload does not satisfy io.error_schema: {first}"),
        )
        .with_code("WFG-IO-004"));
    }
    Ok(())
}

/// Validate the trigger payload against input_schema.
pub fn validate_input_schema(schema: &Value, payload: &Value) -> Result<(), AppError> {
    let compiled = jsonschema::JSONSchema::compile(schema).map_err(|e| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("invalid input_schema: {e}"),
        )
        .with_code("WFG-IO-002")
    })?;

    if let Err(errors) = compiled.validate(payload) {
        let first = errors
            .into_iter()
            .next()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "validation failed".to_string());
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!("trigger payload does not satisfy io.input_schema: {first}"),
        )
        .with_code("WFG-IO-002"));
    }
    Ok(())
}
