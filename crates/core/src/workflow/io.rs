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

/// Validate that the serialized trigger payload does not exceed max_input_bytes.
/// Returns WFG-IO-001 on violation.
///
/// `payload` is a `serde_json::Value`, which cannot fail to serialize back to
/// a string: its public constructors reject non-finite floats, and its map
/// keys are always valid UTF-8 strings. This is documented as an explicit
/// invariant (`.expect`) rather than defensively swallowed via
/// `unwrap_or_default()`, which previously made a payload that somehow
/// failed to serialize pass this size guard vacuously (0 bytes) instead of
/// surfacing as a validation error (spec 074, B11).
pub fn validate_input_size(payload: &Value, max_bytes: usize) -> Result<(), AppError> {
    let serialized = serde_json::to_string(payload)
        .expect("serde_json::Value serialization is infallible (finite numbers, UTF-8 keys)");
    if serialized.len() > max_bytes {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "trigger payload exceeds max_input_bytes ({}): {} bytes",
                max_bytes,
                serialized.len()
            ),
        )
        .with_code("WFG-IO-001"));
    }
    Ok(())
}

/// Validate that the serialized result does not exceed max_output_bytes.
/// Returns WFG-IO-003 on violation.
///
/// See [`validate_input_size`] for why `.expect` (not `unwrap_or_default`) is
/// correct here: a `serde_json::Value` cannot fail to serialize (spec 074,
/// B11).
pub fn validate_output_size(result: &Value, max_bytes: usize) -> Result<(), AppError> {
    let serialized = serde_json::to_string(result)
        .expect("serde_json::Value serialization is infallible (finite numbers, UTF-8 keys)");
    if serialized.len() > max_bytes {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "output exceeds max_output_bytes ({}): {} bytes",
                max_bytes,
                serialized.len()
            ),
        )
        .with_code("WFG-IO-003"));
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
