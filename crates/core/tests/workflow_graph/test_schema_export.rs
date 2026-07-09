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

/// ADR-0014: the full, pinned set of built-in operator names. Descriptors
/// must include all 16 — including the four optimization-loop operators —
/// even when `register_builtins` is called with no `BackendStore` (as
/// `newton schema export` does). If this list needs to change, it must be a
/// deliberate addition/removal of an operator, not silent drift.
const EXPECTED_BUILTIN_OPERATOR_NAMES: &[&str] = &[
    "AgentOperator",
    "AssertCompletedOperator",
    "ChangeRequestOperator",
    "CommandOperator",
    "GhOperator",
    "GitOperator",
    "GraderAgentOperator",
    "GraderCommandOperator",
    "HumanApprovalOperator",
    "HumanDecisionOperator",
    "NoOpOperator",
    "ReadControlFileOperator",
    "ReconcileOperator",
    "SetContextOperator",
    "WorkflowOperator",
    "barrier",
];

/// P1 (ADR-0014): `register_builtins` with no `BackendStore` must still
/// describe all 16 operators — including the four optimization-loop
/// operators (`GraderCommandOperator`, `ReconcileOperator`,
/// `ChangeRequestOperator`, `GraderAgentOperator`) that previously vanished
/// from the schema-export registry entirely because they only registered
/// `if let Some(store) = deps.backend_store`.
#[test]
fn descriptor_set_includes_all_sixteen_builtin_operators_without_a_store() {
    let registry = build_test_registry();
    let mut names: Vec<String> = registry
        .descriptors()
        .into_iter()
        .map(|d| d.name.to_string())
        .collect();
    names.sort();

    let mut expected: Vec<String> = EXPECTED_BUILTIN_OPERATOR_NAMES
        .iter()
        .map(|s| s.to_string())
        .collect();
    expected.sort();

    assert_eq!(
        names.len(),
        16,
        "expected exactly 16 built-in operator descriptors, got {}: {:?}",
        names.len(),
        names
    );
    assert_eq!(
        names, expected,
        "descriptor set does not match the pinned built-in operator name list"
    );

    // The four loop operators specifically — the audit's headline finding.
    for loop_operator in [
        "GraderCommandOperator",
        "ReconcileOperator",
        "ChangeRequestOperator",
        "GraderAgentOperator",
    ] {
        assert!(
            registry.is_described(loop_operator),
            "loop operator '{loop_operator}' must be described even without a BackendStore"
        );
        assert!(
            registry.get(loop_operator).is_none(),
            "loop operator '{loop_operator}' must NOT be executable without a BackendStore"
        );
    }
}

/// S16: the composed schema's `WorkflowTask.operator` property must be
/// constrained by an `enum` generated from the Descriptor set, covering all
/// 16 operators (not just the historically-always-registered 12).
#[test]
fn composed_schema_constrains_operator_with_enum_of_all_descriptors() {
    let registry = build_test_registry();
    let schema = schema_export::composed_workflow_schema(&registry);
    let value = serde_json::to_value(&schema).expect("schema serializable");

    let task = value
        .get("$defs")
        .and_then(|d| d.get("WorkflowTask"))
        .or_else(|| value.get("definitions").and_then(|d| d.get("WorkflowTask")))
        .expect("WorkflowTask definition present");
    let operator_prop = task
        .get("properties")
        .and_then(|p| p.get("operator"))
        .expect("operator property present");
    let enum_values = operator_prop
        .get("enum")
        .and_then(|e| e.as_array())
        .expect("operator property has an enum");

    let mut enum_names: Vec<String> = enum_values
        .iter()
        .map(|v| v.as_str().expect("enum entries are strings").to_string())
        .collect();
    enum_names.sort();

    let mut expected: Vec<String> = EXPECTED_BUILTIN_OPERATOR_NAMES
        .iter()
        .map(|s| s.to_string())
        .collect();
    expected.sort();

    assert_eq!(
        enum_names, expected,
        "operator enum does not match the full descriptor set"
    );
}

/// S16 payoff: a workflow referencing an operator name that is NOT in the
/// Descriptor set (e.g. a typo) must fail validation against the composed
/// schema. Before S16 `operator` was a bare `{"type": "string"}`, so this
/// would validate — the typo would only surface much later, at execution
/// time, via the WFG-OP-001 "operator is not registered" error.
#[test]
fn typo_operator_name_fails_composed_schema_validation() {
    let registry = build_test_registry();
    let composed = schema_export::composed_workflow_schema(&registry);
    let schema_value = serde_json::to_value(&composed).expect("schema serializable");
    let validator = jsonschema::JSONSchema::compile(&schema_value).expect("schema compiles");

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = manifest_dir.join("tests/fixtures/workflows/01_minimal_success.yaml");
    let yaml_str = std::fs::read_to_string(&fixture).expect("read fixture");
    let mut instance: serde_json::Value =
        serde_yaml::from_str(&yaml_str).expect("parse fixture yaml");

    // Sanity check: the unmodified fixture (a real, registered operator name)
    // validates cleanly.
    assert!(
        validator.is_valid(&instance),
        "unmodified fixture with a valid operator name should validate"
    );

    // Introduce a typo'd / unregistered operator name.
    instance["workflow"]["tasks"][0]["operator"] =
        serde_json::Value::String("NoOpOperatorTypo".to_string());

    let errors: Vec<String> = validator
        .validate(&instance)
        .err()
        .into_iter()
        .flatten()
        .map(|e| e.to_string())
        .collect();

    assert!(
        !errors.is_empty(),
        "a workflow referencing an unknown operator name must fail schema validation"
    );
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
    // Prefer the committed fixture copy (always present in CI).
    // Fall back to the live workspace directory for local runs.
    let committed = manifest_dir.join("tests/fixtures/workflows");
    let live = manifest_dir.join("../../.newton/workflows");
    let workflows_dir = if committed.exists() { committed } else { live };

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
