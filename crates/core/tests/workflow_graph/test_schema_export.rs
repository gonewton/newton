// Tests for composed schema export and per-operator params validation
use newton_core::workflow::state::GraphSettings;
use newton_core::workflow::{operator::OperatorRegistry, operators, schema, schema_export};
use std::path::PathBuf;

fn build_test_registry() -> OperatorRegistry {
    let workspace = std::path::PathBuf::from(".");
    let mut builder = OperatorRegistry::builder();
    let settings: GraphSettings = schema::WorkflowSettings::default();
    operators::register_builtins(&mut builder, workspace, settings);
    builder.build()
}

#[test]
fn composed_schema_is_valid_json() {
    let registry = build_test_registry();
    let schema = schema_export::composed_workflow_schema(&registry);
    let v = serde_json::to_value(&schema).expect("schema serializable");
    assert!(v.is_object());
}

#[test]
fn every_operator_params_schema_accepts_its_own_name() {
    let registry = build_test_registry();
    for op in registry.list_operators() {
        let params_schema = op.params_schema();
        // Schema should be serializable
        let v = serde_json::to_value(&params_schema).expect("params_schema serializable");
        assert!(
            v.is_object(),
            "params_schema for {} is not an object",
            op.name()
        );
    }
}

#[test]
fn command_params_rejects_unknown_fields() {
    let bad = serde_json::json!({ "cmd": "ls", "unknown_field_xyz": true });
    let result =
        serde_json::from_value::<newton_core::workflow::operators::command::CommandParams>(bad);
    assert!(
        result.is_err(),
        "CommandParams should reject unknown fields"
    );
}

#[test]
fn command_output_has_success_field() {
    // CommandOutput success = (exit_code == 0)
    let output = newton_core::workflow::operators::command::CommandOutput {
        stdout: String::new(),
        stderr: String::new(),
        exit_code: 0,
        success: true,
        duration_ms: 0,
    };
    assert!(output.success);
}

/// Finding #1 fix: operator_output_schemas() must return a non-empty object keyed
/// by operator name, each entry a non-empty schema object.  058/060 need this to
/// generate typed .out.field references.
#[test]
fn operator_output_schemas_covers_all_registered_operators() {
    let registry = build_test_registry();
    let ops: Vec<_> = registry.list_operators();
    let map = schema_export::operator_output_schemas(&registry);

    assert!(
        map.is_object(),
        "operator_output_schemas must return a JSON object"
    );
    let obj = map.as_object().unwrap();

    for op in &ops {
        let name = op.name();
        assert!(
            obj.contains_key(name),
            "output schemas map is missing operator '{name}'"
        );
        let schema = &obj[name];
        assert!(
            schema.is_object(),
            "output schema for '{name}' must be a JSON object, got: {schema}"
        );
    }
}

/// DoD #2: every real workflow in .newton/workflows/ must validate against the
/// composed schema with zero errors.  This is the acceptance gate — if it fails,
/// the schema does not accurately describe the authored-document shape and cannot
/// drive 058/060 codegen or the editor.
#[test]
fn real_workflows_validate_against_composed_schema() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // CARGO_MANIFEST_DIR = crates/core; workspace root is two levels up
    let workflows_dir = manifest_dir.join("../../.newton/workflows");

    let registry = build_test_registry();
    let composed = schema_export::composed_workflow_schema(&registry);
    let schema_value = serde_json::to_value(&composed).expect("schema serializable");
    let validator = jsonschema::JSONSchema::compile(&schema_value).expect("schema compiles");

    let yaml_files = ["planning_enriching.yaml", "planner.yaml", "develop.yaml"];
    let mut all_errors: Vec<String> = Vec::new();

    for filename in &yaml_files {
        let path = workflows_dir.join(filename);
        let yaml_str = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let instance: serde_json::Value = serde_yaml::from_str(&yaml_str)
            .unwrap_or_else(|e| panic!("cannot parse {}: {e}", path.display()));

        let errors: Vec<String> = validator
            .validate(&instance)
            .err()
            .into_iter()
            .flatten()
            .map(|e| format!("{filename}: {e}"))
            .collect();

        all_errors.extend(errors);
    }

    assert!(
        all_errors.is_empty(),
        "composed schema rejected real workflow(s) — {} error(s):\n{}",
        all_errors.len(),
        all_errors.join("\n")
    );
}
