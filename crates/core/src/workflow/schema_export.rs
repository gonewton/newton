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
        let mut params_value = serde_json::to_value(&params_schema).unwrap_or_default();
        // Allow {$expr: "..."} wrappers anywhere a param field value is expected,
        // since authored YAML carries pre-resolution expressions that the engine
        // evaluates before deserializing into the typed param struct.
        relax_schema_for_exprs(&mut params_value);

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

/// Recursively rewrite every property schema (and every HashMap value schema via
/// `additionalProperties`) to accept either its resolved type OR an `{$expr: "…"}`
/// wrapper.  Newton evaluates `$expr` nodes before deserializing into typed param
/// structs, so the authored YAML shape differs from the post-resolution shape that
/// the Rust types describe.  Applying this transform at composition time keeps the
/// param structs clean while making the authored-document schema accurate.
fn relax_schema_for_exprs(schema: &mut serde_json::Value) {
    if !schema.is_object() {
        return;
    }

    let expr_schema = serde_json::json!({
        "type": "object",
        "required": ["$expr"],
        "properties": {"$expr": {"type": "string"}},
        "additionalProperties": false
    });

    // Recurse into anyOf/oneOf/allOf sub-schemas
    for kw in ["anyOf", "oneOf", "allOf"] {
        if let Some(arr) = schema.get_mut(kw).and_then(|v| v.as_array_mut()) {
            for item in arr {
                relax_schema_for_exprs(item);
            }
        }
    }

    // Recurse into $defs and definitions
    for kw in ["$defs", "definitions"] {
        if let Some(obj) = schema.get_mut(kw).and_then(|v| v.as_object_mut()) {
            for (_, def) in obj {
                relax_schema_for_exprs(def);
            }
        }
    }

    // Relax additionalProperties when it is a schema object (i.e. HashMap value
    // schemas like `additionalProperties: {type: "string"}`).  Leave boolean
    // additionalProperties (true/false) unchanged.
    let add_props = schema.get("additionalProperties").cloned();
    if let Some(ap) = add_props {
        if ap.is_object() {
            let mut relaxed = ap;
            relax_schema_for_exprs(&mut relaxed);
            schema["additionalProperties"] =
                serde_json::json!({"anyOf": [relaxed, expr_schema.clone()]});
        }
    }

    // Relax each property schema: clone the map first to avoid simultaneous
    // immutable + mutable borrows on `schema`.
    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .cloned();
    if let Some(props_map) = props {
        for (key, prop_val) in props_map {
            let mut relaxed = prop_val;
            relax_schema_for_exprs(&mut relaxed);
            if let Some(props_obj) = schema.get_mut("properties").and_then(|p| p.as_object_mut()) {
                props_obj.insert(
                    key,
                    serde_json::json!({"anyOf": [relaxed, expr_schema.clone()]}),
                );
            }
        }
    }
}

/// Return a JSON object mapping each registered operator name to its output schema.
/// This is the artifact 058/060 need to generate typed `.out.field` references —
/// it is separate from the composed workflow document schema so consumers can
/// use one or both as required.
///
/// Output schemas describe the **runtime output** shape (post-execution), so they
/// are NOT subject to the `$expr` relaxation applied to params schemas.
pub fn operator_output_schemas(registry: &OperatorRegistry) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for op in registry.list_operators() {
        let schema_value = serde_json::to_value(op.output_schema()).unwrap_or_default();
        map.insert(op.name().to_owned(), schema_value);
    }
    serde_json::Value::Object(map)
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
