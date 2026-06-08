use crate::workflow::operator::OperatorRegistry;
use crate::workflow::schema::WorkflowDocument;
use schemars::{schema_for, Schema};

/// Compose a single JSON Schema that validates a complete workflow document,
/// including per-operator params via operator-discriminated if/then branches.
pub fn composed_workflow_schema(registry: &OperatorRegistry) -> Schema {
    // Start with the base WorkflowDocument schema
    let root = schema_for!(WorkflowDocument);

    // Build if/then subschemas for each operator's params
    let operators = registry.list_operators();

    let mut if_then_branches: Vec<serde_json::Value> = Vec::new();
    for op in &operators {
        let name = op.name();
        let params_schema = op.params_schema();
        let params_value = serde_json::to_value(&params_schema).unwrap_or_default();

        if_then_branches.push(serde_json::json!({
            "if": {
                "properties": {
                    "operator": { "const": name }
                }
            },
            "then": {
                "properties": {
                    "params": params_value
                }
            }
        }));
    }

    // Convert root to a mutable Value, patch, convert back
    let mut root_value = serde_json::to_value(&root).unwrap_or_default();

    // Walk into #/$defs/WorkflowTask or definitions/WorkflowTask and add allOf
    patch_task_schema_with_operator_branches(&mut root_value, &if_then_branches);

    serde_json::from_value(root_value).unwrap_or(root)
}

fn patch_task_schema_with_operator_branches(
    schema: &mut serde_json::Value,
    branches: &[serde_json::Value],
) {
    if branches.is_empty() {
        return;
    }

    // Try $defs first, then definitions
    let has_defs = schema
        .get("$defs")
        .and_then(|d| d.get("WorkflowTask"))
        .is_some();
    let has_definitions = schema
        .get("definitions")
        .and_then(|d| d.get("WorkflowTask"))
        .is_some();

    if has_defs {
        if let Some(task) = schema
            .get_mut("$defs")
            .and_then(|defs| defs.get_mut("WorkflowTask"))
        {
            let existing = task.get("allOf").cloned();
            let mut all_of: Vec<serde_json::Value> = existing
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            all_of.extend_from_slice(branches);
            task["allOf"] = serde_json::Value::Array(all_of);
        }
    } else if has_definitions {
        if let Some(task) = schema
            .get_mut("definitions")
            .and_then(|defs| defs.get_mut("WorkflowTask"))
        {
            let existing = task.get("allOf").cloned();
            let mut all_of: Vec<serde_json::Value> = existing
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            all_of.extend_from_slice(branches);
            task["allOf"] = serde_json::Value::Array(all_of);
        }
    }
}
