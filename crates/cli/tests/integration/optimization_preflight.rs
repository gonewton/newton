use super::{journal, setup};
use crate::support::{fixture_path, newton};
use serde_json::json;
use std::{fs, path::Path};

fn configure_command_plan(root: &Path) {
    setup(root);
    fs::copy(
        fixture_path("optimization/static-command-params.yaml"),
        root.join("plan.yaml"),
    )
    .unwrap();
    fs::write(
        root.join(".newton/configs/demo.conf"),
        "definition_file=definition.yaml\noptimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy\n",
    )
    .unwrap();
}

#[test]
fn invalid_static_operator_params_fail_before_run_creation() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    configure_command_plan(root);

    for flag in ["--preflight", "--once"] {
        newton()
            .current_dir(root)
            .args(["optimize", "demo", flag])
            .assert()
            .failure()
            .stderr(predicates::str::contains("plan.yaml"))
            .stderr(predicates::str::contains("invalid_command"))
            .stderr(predicates::str::contains("unknown field `args`"));
        assert!(!root.join("command-ran").exists());
        assert!(!root.join(".newton/state").exists());
        assert!(!root.join(".newton/optimize/claim").exists());
    }
}

fn replace_plan_params(root: &Path, operator: &str, params: serde_json::Value) {
    let path = root.join("plan.yaml");
    let mut workflow: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    workflow["workflow"]["tasks"][0]["operator"] = operator.into();
    workflow["workflow"]["tasks"][0]["params"] = serde_yaml::to_value(params).unwrap();
    fs::write(path, serde_yaml::to_string(&workflow).unwrap()).unwrap();
}

#[test]
fn preflight_uses_semantic_validators_including_store_gated_operators() {
    for (operator, params, diagnostic) in [
        (
            "CommandOperator",
            json!({"cmd": ""}),
            "CommandOperator requires a non-empty cmd",
        ),
        (
            "GraderCommandOperator",
            json!({"cmd": "", "grader": "fixture", "scope": "repo", "scope_id": "fixture"}),
            "GRADER-CMD-001",
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        configure_command_plan(root);
        replace_plan_params(root, operator, params);
        newton()
            .current_dir(root)
            .args(["optimize", "demo", "--preflight"])
            .assert()
            .failure()
            .stderr(predicates::str::contains(diagnostic));
        assert!(!root.join(".newton/state").exists());
    }
}

#[test]
fn expression_bearing_params_are_resolved_and_validated_only_at_runtime() {
    for invalid in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        configure_command_plan(root);
        let mut params = json!({
            "cmd": {"$expr": "if triggers.role == \"plan\" { \"printf dynamic\" } else { \"\" }"},
            "write_stdout": "command-ran"
        });
        if invalid {
            params["cmd"] = json!({"$expr": "true"});
        }
        replace_plan_params(root, "CommandOperator", params);
        newton()
            .current_dir(root)
            .args(["optimize", "demo", "--preflight"])
            .assert()
            .success();
        assert!(!root.join("command-ran").exists());
        assert!(!root.join(".newton/state").exists());

        let assertion = newton()
            .current_dir(root)
            .args(["optimize", "demo", "--once"])
            .assert();
        if invalid {
            assertion
                .failure()
                .stderr(predicates::str::contains("invalid type: boolean"));
            assert!(!root.join("command-ran").exists());
            assert_eq!(
                journal(root)["outcome"]["stop_reason"],
                "operational_failure"
            );
        } else {
            assertion.success();
            assert_eq!(
                fs::read_to_string(root.join("command-ran")).unwrap(),
                "dynamic"
            );
            assert_eq!(
                journal(root)["outcome"]["stop_reason"],
                "no_actionable_work"
            );
        }
    }
}

#[test]
fn dynamic_values_cannot_hide_statically_unknown_parameter_keys() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    configure_command_plan(root);
    replace_plan_params(
        root,
        "CommandOperator",
        json!({"cmd": {"$expr": "\"printf dynamic\""}, "args": []}),
    );
    for flag in ["--preflight", "--once"] {
        newton()
            .current_dir(root)
            .args(["optimize", "demo", flag])
            .assert()
            .failure()
            .stderr(predicates::str::contains("plan.yaml"))
            .stderr(predicates::str::contains("invalid_command"))
            .stderr(predicates::str::contains("WFG-PARAMS-001"))
            .stderr(predicates::str::contains("args"));
        assert!(!root.join("command-ran").exists());
        assert!(!root.join(".newton/state").exists());
        assert!(!root.join(".newton/optimize/claim").exists());
    }
}

#[test]
fn dynamic_command_cannot_hide_a_static_absolute_working_directory() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    configure_command_plan(root);
    replace_plan_params(
        root,
        "CommandOperator",
        json!({
            "cmd": {"$expr": "\"printf dynamic\""},
            "cwd": root,
            "write_stdout": "command-ran"
        }),
    );
    for flag in ["--preflight", "--once"] {
        newton()
            .current_dir(root)
            .args(["optimize", "demo", flag])
            .assert()
            .failure()
            .stderr(predicates::str::contains("plan.yaml"))
            .stderr(predicates::str::contains("invalid_command"))
            .stderr(predicates::str::contains("WFG-CMD-001"))
            .stderr(predicates::str::contains("cwd must be relative"));
        assert!(!root.join("command-ran").exists());
        assert!(!root.join(".newton/state").exists());
        assert!(!root.join(".newton/optimize/claim").exists());
    }
}
