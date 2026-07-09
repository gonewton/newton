use crate::workflow::operator::OperatorRegistry;
use crate::workflow::schema::WorkflowDocument;
use schemars::{schema_for, Schema};

/// Compose a single JSON Schema that validates a complete workflow document,
/// including per-operator params via operator-discriminated if/then branches.
///
/// Iterates the registry's Descriptor set — never executable registrations —
/// so an operator whose runtime deps (e.g. `BackendStore`) are absent in the
/// calling context (as with `newton schema export`'s store-free registry)
/// still appears in the exported schema. See ADR-0014.
pub fn composed_workflow_schema(registry: &OperatorRegistry) -> Schema {
    // Start with the base WorkflowDocument schema
    let root = schema_for!(WorkflowDocument);

    // Build if/then subschemas for each operator's params, from Descriptors
    // (store-independent) sorted by name for deterministic output.
    let mut descriptors = registry.descriptors();
    descriptors.sort_by(|a, b| a.name.cmp(b.name));

    let mut if_then_branches: Vec<serde_json::Value> = Vec::new();
    let mut operator_names: Vec<String> = Vec::with_capacity(descriptors.len());
    for descriptor in &descriptors {
        let name = descriptor.name;
        operator_names.push(name.to_string());
        let mut params_value = serde_json::to_value(&descriptor.params_schema).unwrap_or_default();
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

    // S16: constrain `operator` with an enum generated from the Descriptor
    // set — the legal operator vocabulary now has exactly one source instead
    // of being pinned only indirectly via the if/then branches above.
    patch_task_operator_enum(&mut root_value, &operator_names);

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
///
/// Iterates the registry's Descriptor set (ADR-0014), so operators without a
/// wired executable instance (e.g. the loop operators in a store-free
/// registry) still contribute their output schema.
pub fn operator_output_schemas(registry: &OperatorRegistry) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for descriptor in registry.descriptors() {
        let schema_value = serde_json::to_value(&descriptor.output_schema).unwrap_or_default();
        map.insert(descriptor.name.to_owned(), schema_value);
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

/// S16: patch `$defs.WorkflowTask.properties.operator` (or the `definitions`
/// equivalent for older schemars output) with an `enum` of the legal operator
/// names, generated from the registry's Descriptor set. Previously `operator`
/// was a bare `{"type": "string"}` — the vocabulary was pinned only
/// indirectly via the if/then branches above, so a typo'd operator name
/// would "validate" against the schema and only fail much later, at
/// execution time.
fn patch_task_operator_enum(schema: &mut serde_json::Value, operator_names: &[String]) {
    if operator_names.is_empty() {
        return;
    }
    let enum_values: Vec<serde_json::Value> = operator_names
        .iter()
        .cloned()
        .map(serde_json::Value::String)
        .collect();

    for defs_key in ["$defs", "definitions"] {
        if let Some(operator_prop) = schema
            .get_mut(defs_key)
            .and_then(|defs| defs.get_mut("WorkflowTask"))
            .and_then(|task| task.get_mut("properties"))
            .and_then(|props| props.get_mut("operator"))
        {
            operator_prop["enum"] = serde_json::Value::Array(enum_values.clone());
        }
    }
}
