//! Value resolution and expression evaluation utilities for workflow execution.
#![allow(clippy::result_large_err)] // This module returns AppError to preserve structured diagnostic context; boxing would discard run-time state.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::expression::{EvaluationContext, ExpressionEngine};
use crate::workflow::operator::StateView;
use crate::workflow::schema;
use crate::workflow::state::TaskRunRecord;
use serde_json::{Map, Number, Value};
use std::collections::HashMap;

/// Recursively resolves a JSON value, evaluating any embedded expressions.
///
/// This function traverses a JSON value structure and evaluates any objects
/// containing a single "$expr" key as expressions using the provided engine.
pub fn resolve_value(
    value: &Value,
    engine: &ExpressionEngine,
    ctx: &EvaluationContext,
) -> Result<Value, AppError> {
    match value {
        Value::Object(map) => {
            if map.len() == 1 && map.contains_key("$expr") {
                if let Some(Value::String(expr)) = map.get("$expr") {
                    return engine.evaluate(expr, ctx);
                }
            }
            let mut resolved = Map::new();
            for (key, child) in map {
                resolved.insert(key.clone(), resolve_value(child, engine, ctx)?);
            }
            Ok(Value::Object(resolved))
        }
        Value::Array(items) => {
            let mut collection = Vec::new();
            for item in items {
                collection.push(resolve_value(item, engine, ctx)?);
            }
            Ok(Value::Array(collection))
        }
        other => Ok(other.clone()),
    }
}

/// Evaluates a workflow transition condition to determine if it should fire.
///
/// Checks both the `include_if` guard condition and the `when` condition.
/// Returns false if either condition evaluates to false, true otherwise.
pub fn evaluate_transition(
    transition: &schema::Transition,
    engine: &ExpressionEngine,
    snapshot: &StateView,
) -> Result<bool, AppError> {
    let ctx = snapshot.evaluation_context();

    // Check include_if (compile-time/init-time condition, but we evaluate here if not already filtered)
    if let Some(ref guard) = transition.include_if {
        if !evaluate_condition(guard, engine, &ctx)? {
            return Ok(false);
        }
    }

    match &transition.when {
        None => Ok(true),
        Some(cond) => evaluate_condition(cond, engine, &ctx),
    }
}

/// Evaluates a condition (bool literal or expression) to a boolean result.
///
/// Returns an error if an expression condition evaluates to a non-boolean value.
pub fn evaluate_condition(
    condition: &schema::Condition,
    engine: &ExpressionEngine,
    ctx: &EvaluationContext,
) -> Result<bool, AppError> {
    match condition {
        schema::Condition::Bool(flag) => Ok(*flag),
        schema::Condition::Expr { expr } => {
            let result = engine.evaluate(expr, ctx)?;
            if let Value::Bool(flag) = result {
                Ok(flag)
            } else {
                Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "expression in condition evaluated to a non-boolean value at runtime: {:?}",
                        result
                    ),
                )
                .with_code("WFG-EXPR-BOOL-001"))
            }
        }
    }
}

/// Resolves initial workflow context by evaluating expressions with trigger data.
pub fn resolve_initial_context(
    context: &Value,
    engine: &ExpressionEngine,
    triggers: &Value,
) -> Result<Value, AppError> {
    let eval = EvaluationContext::new(context.clone(), Value::Object(Map::new()), triggers.clone());
    resolve_value(context, engine, &eval)
}

/// Creates an evaluation context for initial workflow execution.
pub fn resolve_initial_evaluation_context(
    context: &Value,
    engine: &ExpressionEngine,
    triggers: &Value,
) -> Result<EvaluationContext, AppError> {
    let resolved_context = resolve_initial_context(context, engine, triggers)?;
    Ok(EvaluationContext::new(
        resolved_context,
        Value::Object(Map::new()),
        triggers.clone(),
    ))
}

/// Recursively applies a JSON patch to a target value.
///
/// For objects, merges the patch into the target, recursively applying
/// patches to nested objects. For other types, replaces the target value.
pub fn apply_patch(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(target_map), Value::Object(patch_map)) => {
            for (key, value) in patch_map {
                match target_map.get_mut(key) {
                    Some(existing) => apply_patch(existing, value),
                    None => {
                        target_map.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (target_value, patch_value) => {
            *target_value = patch_value.clone();
        }
    }
}

/// Builds a tasks JSON object from completed task records for use in context.
///
/// Creates a structured representation of task execution state that can be
/// used in expression evaluation contexts.
pub fn build_tasks_value(completed: &HashMap<String, TaskRunRecord>) -> Value {
    let mut map = Map::new();
    for (task_id, record) in completed {
        let mut entry = Map::new();
        entry.insert(
            "status".to_string(),
            Value::String(record.status.as_str().to_string()),
        );
        entry.insert("output".to_string(), record.output.clone());
        entry.insert(
            "error_code".to_string(),
            record
                .error_code
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        entry.insert(
            "duration_ms".to_string(),
            Value::Number(Number::from(record.duration_ms)),
        );
        entry.insert(
            "run_seq".to_string(),
            Value::Number(Number::from(record.run_seq)),
        );
        map.insert(task_id.clone(), Value::Object(entry));
    }
    Value::Object(map)
}

/// Extracts a context patch from task output if present.
///
/// Returns the "patch" field from the output if it exists and the output
/// is a JSON object, otherwise returns None.
pub fn extract_context_patch(output: &Value) -> Option<Value> {
    if let Value::Object(map) = output {
        map.get("patch").cloned()
    } else {
        None
    }
}
