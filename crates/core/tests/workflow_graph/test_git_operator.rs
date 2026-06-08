use newton_core::workflow::operator::{Operator, OperatorRegistry};
use newton_core::workflow::operators::{self, BuiltinOperatorDeps};
use serde_json::Value;
use tempfile::tempdir;

fn build_registry() -> OperatorRegistry {
    let workspace = tempdir().expect("workspace");
    let mut builder = OperatorRegistry::builder();
    operators::register_builtins_with_deps(
        &mut builder,
        workspace.path().to_path_buf(),
        Default::default(),
        BuiltinOperatorDeps::default(),
    );
    builder.build()
}

/// GitOperator is registered and can be found by name.
#[test]
fn git_operator_is_registered() {
    let registry = build_registry();
    let op = registry.get("GitOperator");
    assert!(op.is_some(), "GitOperator must be registered");
    assert_eq!(op.unwrap().name(), "GitOperator");
}

/// params_schema() returns a valid JSON object (not null / empty).
#[test]
fn git_operator_params_schema_is_object() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();
    let schema = op.params_schema();
    let json: Value = serde_json::to_value(&schema).expect("schema must serialize");
    assert!(
        json.is_object(),
        "params_schema must serialize to a JSON object, got: {json}"
    );
    // The schema must have some content — at minimum a type or oneOf/anyOf key.
    let obj = json.as_object().unwrap();
    assert!(!obj.is_empty(), "params_schema must not be an empty object");
}

/// output_schema() returns a valid JSON object.
#[test]
fn git_operator_output_schema_is_object() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();
    let schema = op.output_schema();
    let json: Value = serde_json::to_value(&schema).expect("output schema must serialize");
    assert!(
        json.is_object(),
        "output_schema must serialize to a JSON object, got: {json}"
    );
}

/// validate_params rejects an unknown operation.
#[test]
fn git_operator_validate_rejects_unknown_operation() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();
    let params = serde_json::json!({ "operation": "does_not_exist" });
    assert!(
        op.validate_params(&params).is_err(),
        "unknown operation must fail validation"
    );
}

/// validate_params accepts each known operation with minimal/default params.
#[test]
fn git_operator_validate_accepts_known_operations() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();

    let cases = vec![
        serde_json::json!({ "operation": "clean_check" }),
        serde_json::json!({ "operation": "sync_main" }),
        serde_json::json!({ "operation": "create_branch", "name": "feature/x" }),
        serde_json::json!({ "operation": "stage" }),
        serde_json::json!({ "operation": "commit", "message": "test commit" }),
        serde_json::json!({ "operation": "push" }),
        serde_json::json!({ "operation": "diff" }),
        serde_json::json!({ "operation": "cleanup_merge" }),
    ];

    for params in &cases {
        assert!(
            op.validate_params(params).is_ok(),
            "valid params must pass validation: {params}"
        );
    }
}

/// validate_params rejects create_branch with empty name.
#[test]
fn git_operator_validate_rejects_empty_branch_name() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();
    let params = serde_json::json!({ "operation": "create_branch", "name": "" });
    let err = op
        .validate_params(&params)
        .expect_err("empty name must fail");
    assert_eq!(err.code, "WFG-GIT-010");
}

/// validate_params rejects commit with empty message.
#[test]
fn git_operator_validate_rejects_empty_commit_message() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();
    let params = serde_json::json!({ "operation": "commit", "message": "" });
    let err = op
        .validate_params(&params)
        .expect_err("empty message must fail");
    assert_eq!(err.code, "WFG-GIT-011");
}

/// validate_params rejects push with invalid remote.
#[test]
fn git_operator_validate_rejects_invalid_remote() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();

    let bad_remotes = vec!["", "-origin", "bad remote"];
    for remote in bad_remotes {
        let params = serde_json::json!({ "operation": "push", "remote": remote });
        assert!(
            op.validate_params(&params).is_err(),
            "invalid remote {remote:?} must fail validation"
        );
    }
}
