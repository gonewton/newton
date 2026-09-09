//! Validation-only parameter schemas; exported workflow schemas remain unchanged.
#![allow(clippy::result_large_err)]

use crate::core::{error::AppError, types::ErrorCategory};
use schemars::Schema;
use serde_json::{json, Value};

/// Validate the known schema shape of authored parameters without evaluating
/// expressions. Literal siblings, required keys and closed objects remain checked.
/// Expression result types and union exclusivity require normal runtime validation.
pub fn validate_authored_params(schema: &Schema, params: &Value) -> Result<(), AppError> {
    if params
        .as_object()
        .is_some_and(|map| map.len() == 1 && map.get("$expr").is_some_and(Value::is_string))
    {
        return Ok(());
    }
    let mut schema = serde_json::to_value(schema).map_err(|error| invalid(error.to_string()))?;
    relax_constraints(&mut schema);
    let compiled = jsonschema::JSONSchema::compile(&schema)
        .map_err(|error| invalid(format!("invalid operator parameter schema: {error}")))?;
    if let Err(mut errors) = compiled.validate(params) {
        if let Some(error) = errors.next() {
            return Err(invalid(format!(
                "authored operator parameters violate schema at {}: {error}",
                error.instance_path
            )));
        }
    }
    Ok(())
}

fn invalid(message: String) -> AppError {
    AppError::new(ErrorCategory::ValidationError, message).with_code("WFG-PARAMS-001")
}

fn relax_value(schema: &mut Value) {
    if !schema.is_object() {
        // In particular, false forbids a value regardless of its future type.
        return;
    }
    relax_constraints(schema);
    let literal = std::mem::replace(schema, Value::Null);
    *schema = json!({"anyOf": [literal, {
        "type": "object",
        "properties": {"$expr": {"type": "string"}},
        "required": ["$expr"],
        "additionalProperties": false
    }]});
}

fn relax_constraints(schema: &mut Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    // Definitions stay at their original locations so local references resolve.
    for keyword in ["$defs", "definitions"] {
        if let Some(definitions) = object.get_mut(keyword).and_then(Value::as_object_mut) {
            for definition in definitions.values_mut() {
                relax_constraints(definition);
            }
        }
    }
    for keyword in ["anyOf", "oneOf", "allOf"] {
        if let Some(branches) = object.get_mut(keyword).and_then(Value::as_array_mut) {
            for branch in branches {
                relax_constraints(branch);
            }
        }
    }
    // Dynamic discriminators can leave several alternatives possible. Preserve
    // their constraints but defer exclusivity until the values have resolved.
    if let Some(alternatives) = object.remove("oneOf") {
        let alternatives = json!({"anyOf": alternatives});
        if let Some(Value::Array(branches)) = object.get_mut("allOf") {
            branches.push(alternatives);
        } else {
            object.insert("allOf".into(), json!([alternatives]));
        }
    }
    for keyword in ["properties", "patternProperties"] {
        if let Some(properties) = object.get_mut(keyword).and_then(Value::as_object_mut) {
            for property in properties.values_mut() {
                relax_value(property);
            }
        }
    }
    if let Some(additional) = object.get_mut("additionalProperties") {
        relax_value(additional);
    }
    if let Some(items) = object.get_mut("items") {
        if let Value::Array(items) = items {
            for item in items {
                relax_value(item);
            }
        } else {
            relax_value(items);
        }
    }
    if let Some(items) = object.get_mut("prefixItems").and_then(Value::as_array_mut) {
        for item in items {
            relax_value(item);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validate_authored_params;
    use schemars::{schema_for, JsonSchema};
    use serde::Deserialize;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[allow(dead_code)]
    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct TestParams {
        command: String,
        count: u64,
    }

    #[allow(dead_code)]
    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct NestedParams {
        label: String,
    }

    #[allow(dead_code)]
    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct CompositeParams {
        nested: NestedParams,
        commands: Vec<String>,
        environment: BTreeMap<String, String>,
    }

    #[test]
    fn rejects_unknown_literal_keys_beside_an_expression() {
        let error = validate_authored_params(
            &schema_for!(TestParams),
            &json!({
                "command": {"$expr": "triggers.command"},
                "count": 1,
                "unknown": true,
            }),
        )
        .expect_err("unknown literal key must fail authored-parameter validation");

        assert_eq!(error.code, "WFG-PARAMS-001");
    }

    #[test]
    fn accepts_expressions_while_checking_literal_siblings() {
        validate_authored_params(
            &schema_for!(TestParams),
            &json!({
                "command": {"$expr": "triggers.command"},
                "count": 1,
            }),
        )
        .expect("expression result type is deferred until runtime");
    }

    #[test]
    fn accepts_a_whole_parameter_expression() {
        validate_authored_params(
            &schema_for!(TestParams),
            &json!({"$expr": "triggers.params"}),
        )
        .expect("a whole parameter expression has no static shape to validate");
    }

    #[test]
    fn accepts_expressions_in_references_arrays_and_map_values() {
        validate_authored_params(
            &schema_for!(CompositeParams),
            &json!({
                "nested": {"label": {"$expr": "triggers.label"}},
                "commands": [{"$expr": "triggers.command"}],
                "environment": {"MODE": {"$expr": "triggers.mode"}},
            }),
        )
        .expect("supported nested value positions must defer expression types");
    }

    #[test]
    fn rejects_unknown_keys_inside_referenced_objects() {
        let error = validate_authored_params(
            &schema_for!(CompositeParams),
            &json!({
                "nested": {
                    "label": {"$expr": "triggers.label"},
                    "unknown": true,
                },
                "commands": [],
                "environment": {},
            }),
        )
        .expect_err("referenced objects must retain closed-key validation");

        assert_eq!(error.code, "WFG-PARAMS-001");
    }
}
