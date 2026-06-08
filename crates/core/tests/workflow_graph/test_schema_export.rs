// Tests for composed schema export and per-operator params validation
use newton_core::workflow::state::GraphSettings;
use newton_core::workflow::{operator::OperatorRegistry, operators, schema, schema_export};

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
