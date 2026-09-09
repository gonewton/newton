//! Software-strategy recovery exercised through actual YAML and CLI/store APIs.
#[path = "support/mod.rs"]
mod support;

use serde_json::{json, Value};
use std::{fs, path::Path};

fn command(root: &Path) -> assert_cmd::Command {
    let mut command = support::newton();
    command
        .current_dir(root)
        .arg("--log-dir")
        .arg(root.join("logs"));
    command
}

fn data(root: &Path, verb: &str, resource: &str, id: Option<&str>, body: Option<Value>) -> Value {
    let mut cmd = command(root);
    cmd.args(["data", verb, resource]);
    if let Some(id) = id {
        cmd.arg(id);
    }
    if let Some(body) = body {
        cmd.args(["--body", &body.to_string()]);
    }
    let output = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).unwrap()
}

fn setup(root: &Path) {
    fs::create_dir_all(root.join(".newton/configs")).unwrap();
    fs::create_dir_all(root.join(".newton/state")).unwrap();
    let fixtures = support::fixture_path("optimization");
    for (source, target) in [
        ("grade.yaml", "grade.yaml"),
        ("software-plan.yaml", "plan.yaml"),
        ("software-develop.yaml", "develop.yaml"),
    ] {
        fs::copy(fixtures.join(source), root.join(target)).unwrap();
    }
    let mut definition: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(fixtures.join("definition.yaml")).unwrap())
            .unwrap();
    definition["strategy"] = "software-improvement".into();
    definition["workflows"]
        .as_mapping_mut()
        .unwrap()
        .remove(serde_yaml::Value::from("promote"));
    definition["requirements"]["resource_limits"]["max_cycles"] = 6.into();
    definition["requirements"]["resource_limits"]["max_work"] = 20.into();
    fs::write(
        root.join("definition.yaml"),
        serde_yaml::to_string(&definition).unwrap(),
    )
    .unwrap();
    let mut grade: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(root.join("grade.yaml")).unwrap()).unwrap();
    grade["workflow"]["settings"]["io"]["result_map"]["change_request_id"] =
        "$expr: if triggers.blocked_work_count > 0 { \"cr-good\" } else { \"cr-bad\" }".into();
    fs::write(
        root.join("grade.yaml"),
        serde_yaml::to_string(&grade).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join(".newton/configs/demo.conf"),
        "definition_file=definition.yaml\nparameter.max_failed_attempts=2\n",
    )
    .unwrap();
    for suffix in ["bad", "good"] {
        data(
            root,
            "post",
            "finding",
            None,
            Some(
                json!({"id":format!("finding-{suffix}"),"source":"fixture","module":"fixture-scope","dimension":"size","fingerprint":suffix,"title":suffix,"whyItMatters":"fixture","recommendedAction":"fixture","severity":"medium","risk":"low","status":"approved_for_planning"}),
            ),
        );
        data(
            root,
            "post",
            "change-request",
            None,
            Some(
                json!({"id":format!("cr-{suffix}"),"title":suffix,"findingIds":[format!("finding-{suffix}")]}),
            ),
        );
    }
    for cycle in 1..=3 {
        data(
            root,
            "post",
            "plan",
            None,
            Some(
                json!({"id":format!("plan-{cycle}"),"title":"fixture","linkedChangeRequestId":if cycle < 3 {"cr-bad"} else {"cr-good"},"status":"ready","confidence":100,"risk":"low"}),
            ),
        );
    }
}

fn journal(root: &Path) -> Value {
    let path = fs::read_dir(root.join(".newton/state/optimize"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("journal.json"))
        .find(|path| path.is_file())
        .unwrap();
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn fresh_plans_share_cr_retry_budget_quarantine_findings_and_continue_unrelated_work() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    command(dir.path())
        .args(["optimize", "demo", "--poll-interval", "1"])
        .assert()
        .success();
    let journal = journal(dir.path());
    assert_eq!(journal["outcome"]["stop_reason"], "completed");
    assert_eq!(journal["outcome"]["blocked_work"], json!(["cr-bad"]));
    assert_eq!(journal["outcome"]["usage"]["cycles"], 3);
    let finding = data(dir.path(), "get", "finding", Some("finding-bad"), None);
    assert_eq!(finding["status"], "blocked");
    assert_eq!(finding["blockedByPlanId"], "plan-2");
    for cycle in 1..=2 {
        let plan = data(
            dir.path(),
            "get",
            "plan",
            Some(&format!("plan-{cycle}")),
            None,
        );
        assert_eq!(plan["status"], "failed");
        assert_eq!(plan["attempts"], cycle);
        assert!(plan["executionId"].is_string());
    }
    let cycles = command(dir.path())
        .args([
            "data",
            "get",
            "optimize-cycles",
            "--run-id",
            journal["run_id"].as_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let cycles: Value = serde_json::from_slice(&cycles).unwrap();
    assert_eq!(cycles[0]["changeRequestId"], "cr-bad");
    assert_eq!(cycles[1]["planId"], "plan-2");
    assert_eq!(cycles[2]["changeRequestId"], "cr-good");
}

#[test]
fn rejected_candidates_retry_the_unresolved_cr_and_quarantine_at_the_shared_cap() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup(root);
    fs::copy(
        support::fixture_path("optimization/develop.yaml"),
        root.join("develop.yaml"),
    )
    .unwrap();
    let path = root.join("grade.yaml");
    let mut grade: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    grade["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["constraints"]["behavior"]["status"] = serde_yaml::to_value(json!({"$expr":"if triggers.stage == \"candidate\" && triggers.change_request_id == \"cr-bad\" { \"violated\" } else { \"satisfied\" }"})).unwrap();
    fs::write(path, serde_yaml::to_string(&grade).unwrap()).unwrap();
    command(root)
        .args(["optimize", "demo", "--poll-interval", "1"])
        .assert()
        .success();
    let result = journal(root);
    assert_eq!(result["outcome"]["stop_reason"], "completed");
    assert_eq!(result["outcome"]["blocked_work"], json!(["cr-bad"]));
    assert_eq!(result["outcome"]["usage"]["cycles"], 3);
    assert_eq!(
        result["software_work"]["work"]["cr-bad"]["completed"],
        false
    );
    assert_eq!(result["software_work"]["work"]["cr-bad"]["failures"], 2);
    assert_eq!(
        result["software_work"]["work"]["cr-good"]["completed"],
        true
    );
    assert_eq!(
        data(root, "get", "finding", Some("finding-bad"), None)["status"],
        "blocked"
    );
    for cycle in 1..=2 {
        let plan = data(root, "get", "plan", Some(&format!("plan-{cycle}")), None);
        assert_eq!(plan["status"], "failed");
        assert_eq!(plan["attempts"], cycle);
    }
}

fn threshold_setup(root: &Path, regress: bool) {
    setup(root);
    fs::copy(
        support::fixture_path("optimization/develop.yaml"),
        root.join("develop.yaml"),
    )
    .unwrap();
    let mut definition: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(root.join("definition.yaml")).unwrap()).unwrap();
    definition["strategy"] = "direct-search".into();
    definition["requirements"]["completion"] = serde_yaml::from_str("[{kind: objective_target, objective: quality, target: 95}, {kind: objective_target, objective: security, target: 95}]").unwrap();
    definition["requirements"]["objective"] = serde_yaml::from_str("mode: thresholds\nobjectives:\n  - objective: {id: quality, evaluator: fixture, measurement: {kind: grade, dimension: quality}}\n    target: 95\n    regression_delta: 5\n    no_progress_cycles: 2\n  - objective: {id: security, evaluator: fixture, measurement: {kind: grade, dimension: security}}\n    target: 95\n    regression_delta: 5\n    no_progress_cycles: 2\n").unwrap();
    fs::write(
        root.join("definition.yaml"),
        serde_yaml::to_string(&definition).unwrap(),
    )
    .unwrap();
    let mut grade: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(root.join("grade.yaml")).unwrap()).unwrap();
    grade["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["measurements"] = serde_yaml::from_str("quality: {status: produced, measurement: {kind: grade, dimension: quality}, samples: [80]}\nsecurity: {status: produced, measurement: {kind: grade, dimension: security}, samples: [60]}").unwrap();
    if regress {
        grade["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["measurements"]
            ["security"]["samples"] = serde_yaml::from_str(
            "{$expr: 'if triggers.stage == \"baseline\" { [60.0] } else { [40.0] }'}",
        )
        .unwrap();
    }
    fs::write(
        root.join("grade.yaml"),
        serde_yaml::to_string(&grade).unwrap(),
    )
    .unwrap();
}

#[test]
fn threshold_regression_in_one_objective_stops_before_acceptance() {
    let dir = tempfile::tempdir().unwrap();
    threshold_setup(dir.path(), true);
    command(dir.path())
        .args(["optimize", "demo", "--poll-interval", "1"])
        .assert()
        .success();
    let result = journal(dir.path());
    assert_eq!(result["outcome"]["stop_reason"], "regression");
    assert_eq!(result["outcome"]["usage"]["cycles"], 1);
    assert_eq!(
        result["outcome"]["accepted_result"]["candidate"]["artifact_id"],
        "original"
    );
}

#[test]
fn per_objective_no_progress_stops_at_its_durable_cycle_limit() {
    let dir = tempfile::tempdir().unwrap();
    threshold_setup(dir.path(), false);
    command(dir.path())
        .args(["optimize", "demo", "--poll-interval", "1"])
        .assert()
        .success();
    let result = journal(dir.path());
    assert_eq!(result["outcome"]["stop_reason"], "no_progress");
    assert_eq!(result["outcome"]["usage"]["cycles"], 2);
    assert_ne!(result["outcome"]["completion"]["status"], "satisfied");
}

fn replace_plan_output(root: &Path, output: &str) {
    let path = root.join("plan.yaml");
    let mut document: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    document["workflow"]["settings"]["io"]["result_map"] = serde_yaml::from_str(output).unwrap();
    fs::write(path, serde_yaml::to_string(&document).unwrap()).unwrap();
}

#[test]
fn planner_cannot_discard_or_substitute_the_current_change_request() {
    for output in [
        "decision: none",
        "decision: propose\nplan_id: plan-1\nchange_request_id: cr-good",
        "decision: invalid",
    ] {
        let dir = tempfile::tempdir().unwrap();
        setup(dir.path());
        replace_plan_output(dir.path(), output);
        command(dir.path())
            .args(["optimize", "demo", "--once"])
            .assert()
            .failure();
        let result = journal(dir.path());
        assert_eq!(result["outcome"]["stop_reason"], "operational_failure");
        assert_eq!(result["change_request_id"], "cr-bad");
        assert!(result["execution_id"].is_null());
    }
}

#[test]
fn malformed_reconciliation_fails_without_mutating_findings() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    fs::copy(
        support::fixture_path("optimization/malformed-reconciliation.yaml"),
        dir.path().join("plan.yaml"),
    )
    .unwrap();
    fs::write(dir.path().join(".newton/configs/demo.conf"), "definition_file=definition.yaml\noptimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy\n").unwrap();
    command(dir.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .failure();
    let result = journal(dir.path());
    assert_eq!(result["outcome"]["stop_reason"], "operational_failure");
    let all = data(dir.path(), "get", "findings", None, None);
    assert_eq!(all.as_array().unwrap().len(), 2);
    assert!(all
        .as_array()
        .unwrap()
        .iter()
        .all(|finding| finding["status"] == "approved_for_planning"));
    assert!(result["outcome"]["diagnostics"]
        .to_string()
        .contains("task reconcile failed"));
}

#[test]
fn software_threshold_finding_progress_prevents_false_no_progress() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    threshold_setup(root, false);
    let mut definition: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(root.join("definition.yaml")).unwrap()).unwrap();
    definition["strategy"] = "software-improvement".into();
    definition["requirements"]["resource_limits"]["max_cycles"] = 2.into();
    for objective in definition["requirements"]["objective"]["objectives"]
        .as_sequence_mut()
        .unwrap()
    {
        objective["no_progress_cycles"] = 1.into();
    }
    fs::write(
        root.join("definition.yaml"),
        serde_yaml::to_string(&definition).unwrap(),
    )
    .unwrap();
    replace_plan_output(root, "decision: propose\nplan_id: '$expr: if triggers.cycle == 1 { \"plan-1\" } else { \"plan-3\" }'\nchange_request_id: '$expr: triggers.change_request_id'");
    let mut grade: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(root.join("grade.yaml")).unwrap()).unwrap();
    grade["workflow"]["settings"]["io"]["result_map"]["change_request_id"] =
        "$expr: if triggers.cycle == 1 { \"cr-bad\" } else { \"cr-good\" }".into();
    grade["workflow"]["settings"]["io"]["result_map"]["open_findings"] = "$expr: if triggers.stage == \"baseline\" { #{quality: 5, security: 5} } else if triggers.cycle == 1 { #{quality: 4, security: 4} } else { #{quality: 3, security: 3} }".into();
    fs::write(
        root.join("grade.yaml"),
        serde_yaml::to_string(&grade).unwrap(),
    )
    .unwrap();
    command(root)
        .args(["optimize", "demo", "--poll-interval", "1"])
        .assert()
        .success();
    let result = journal(root);
    assert_eq!(result["outcome"]["stop_reason"], "resource_limit");
    assert_eq!(result["outcome"]["usage"]["cycles"], 2);
}

#[test]
fn regression_uses_the_current_cycle_baseline_after_prior_improvement() {
    let dir = tempfile::tempdir().unwrap();
    threshold_setup(dir.path(), false);
    let path = dir.path().join("grade.yaml");
    let mut grade: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    grade["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["measurements"]["security"]["samples"] = serde_yaml::from_str("{$expr: 'if triggers.cycle == 1 { if triggers.stage == \"baseline\" { [60.0] } else { [90.0] } } else { if triggers.stage == \"baseline\" { [90.0] } else { [84.0] } }'}").unwrap();
    fs::write(path, serde_yaml::to_string(&grade).unwrap()).unwrap();
    command(dir.path())
        .args(["optimize", "demo", "--poll-interval", "1"])
        .assert()
        .success();
    let result = journal(dir.path());
    assert_eq!(result["outcome"]["stop_reason"], "regression");
    assert_eq!(result["outcome"]["usage"]["cycles"], 2);
}
