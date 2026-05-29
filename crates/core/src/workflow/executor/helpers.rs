use std::collections::HashMap;
use std::path::Path;

use serde_json::{Map, Value};

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::schema::WorkflowDocument;
use crate::workflow::state::{TaskRunRecord, WorkflowTaskRunRecord};

pub(super) fn extract_trigger_payload(document: &WorkflowDocument) -> Value {
    document.triggers.as_ref().map_or_else(
        || Value::Object(Map::new()),
        |trigger| trigger.payload.clone(),
    )
}

pub(super) fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

pub(super) fn shallow_merge_objects(base: &Value, overlay: &Value) -> Result<Value, AppError> {
    let overlay_obj = overlay.as_object().ok_or_else(|| {
        AppError::new(
            ErrorCategory::ValidationError,
            "merge value must be an object",
        )
    })?;
    let mut merged = base.as_object().cloned().ok_or_else(|| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "merge base must be a JSON object, got {}",
                json_type_name(base)
            ),
        )
        .with_code("WFG-NEST-005")
    })?;
    for (key, value) in overlay_obj {
        merged.insert(key.clone(), value.clone());
    }
    Ok(Value::Object(merged))
}

pub(super) fn validate_required_triggers(
    required: &[String],
    payload: &Value,
) -> Result<(), AppError> {
    if required.is_empty() {
        return Ok(());
    }
    for key in required {
        if payload.as_object().and_then(|map| map.get(key)).is_none() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("trigger payload missing required key '{key}'"),
            )
            .with_code("WFG-TRIG-001"));
        }
    }
    Ok(())
}

pub(super) fn hydrate_completed_records(
    records: &HashMap<String, WorkflowTaskRunRecord>,
    workspace_root: &Path,
) -> Result<HashMap<String, TaskRunRecord>, AppError> {
    let mut map = HashMap::new();
    for (task_id, record) in records {
        let output = record.output_ref.materialize(workspace_root)?;
        let duration_ms = record
            .completed_at
            .signed_duration_since(record.started_at)
            .num_milliseconds() as u64;
        map.insert(
            task_id.clone(),
            TaskRunRecord {
                status: record.status,
                output,
                error_code: record.error.as_ref().map(|err| err.code.clone()),
                duration_ms,
                run_seq: record.run_seq as u64,
            },
        );
    }
    Ok(map)
}
