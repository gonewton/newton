use super::{journal, setup};
use crate::support::{fixture_path, newton};
use std::fs;

fn assert_unmetered_workflow_rejected(fixture: &str, role: &str, diagnostic: &str) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    setup(root);
    let workflow_name = format!("{fixture}.yaml");
    fs::copy(
        fixture_path(&format!("optimization/{workflow_name}")),
        root.join(&workflow_name),
    )
    .unwrap();
    let definition_path = root.join("definition.yaml");
    let mut definition: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&definition_path).unwrap()).unwrap();
    definition["workflows"][role] = workflow_name.into();
    fs::write(definition_path, serde_yaml::to_string(&definition).unwrap()).unwrap();

    for flag in ["--preflight", "--once"] {
        newton()
            .current_dir(root)
            .args(["optimize", "demo", flag])
            .assert()
            .failure()
            .stderr(predicates::str::contains(diagnostic));
        assert!(!root.join(".newton/state/optimize").exists(), "{role}");
        assert!(!root.join(".newton/state/workflows").exists(), "{role}");
        assert!(!root.join(".newton/optimize/claim").exists(), "{role}");
    }
}

#[test]
fn unmetered_task_retries_fail_preflight_before_run_creation() {
    for role in ["grade", "plan", "develop", "alternate_evaluator"] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        setup(root);
        fs::copy(
            fixture_path("optimization/unmetered-retry.yaml"),
            root.join("unmetered-retry.yaml"),
        )
        .unwrap();
        let definition_path = root.join("definition.yaml");
        let mut definition: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(&definition_path).unwrap()).unwrap();
        definition["workflows"][role] = "unmetered-retry.yaml".into();
        if role == "grade" {
            definition["requirements"]["evaluators"]["fixture"]["workflow"] =
                "unmetered-retry.yaml".into();
        }
        fs::write(definition_path, serde_yaml::to_string(&definition).unwrap()).unwrap();

        for flag in ["--preflight", "--once"] {
            newton()
                .current_dir(root)
                .args(["optimize", "demo", flag])
                .assert()
                .failure()
                .stderr(predicates::str::contains("retry.max_attempts=2"))
                .stderr(predicates::str::contains("retrying_task"))
                .stderr(predicates::str::contains(
                    "cannot meter internal task retries",
                ));
            assert!(!root.join(".newton/state/optimize").exists(), "{role}");
            assert!(!root.join(".newton/state/workflows").exists(), "{role}");
            assert!(!root.join(".newton/optimize/claim").exists(), "{role}");
        }
    }
}

#[test]
fn explicit_single_attempt_workflows_retain_metered_dispatch_counts() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    setup(root);
    for role in ["grade", "plan", "develop"] {
        let path = root.join(format!("{role}.yaml"));
        let mut workflow: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        workflow["workflow"]["tasks"][0]["retry"] =
            serde_yaml::from_str("{max_attempts: 1, backoff_ms: 0}").unwrap();
        fs::write(path, serde_yaml::to_string(&workflow).unwrap()).unwrap();
    }

    newton()
        .current_dir(root)
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let run = journal(root);
    assert_eq!(run["evaluation_count"], 2);
    assert_eq!(run["work_count"], 2);
    assert_eq!(run["outcome"]["stop_reason"], "completed");
}

#[test]
fn workflow_transition_cycles_cannot_repeat_inside_one_evaluation_dispatch() {
    assert_unmetered_workflow_rejected("unmetered-cycle", "grade", "contains a transition cycle");
}

#[test]
fn agent_loop_mode_cannot_repeat_inside_one_work_dispatch() {
    assert_unmetered_workflow_rejected(
        "unmetered-agent-loop",
        "develop",
        "cannot meter internal agent-loop iterations",
    );
}

#[test]
fn operator_internal_retries_must_be_reduced_to_one_attempt() {
    assert_unmetered_workflow_rejected(
        "unmetered-operator-retry",
        "plan",
        "without retry_count: 1",
    );
    assert_unmetered_workflow_rejected("unmetered-gh-retry", "plan", "without retry_count: 1");
}
