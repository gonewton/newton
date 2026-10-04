#[path = "../support/mod.rs"]
mod support;

use predicates::prelude::*;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn setup(root: &Path) {
    fs::create_dir_all(root.join(".newton/configs")).unwrap();
    let fixtures = support::fixture_path("scheduling");
    for name in ["definition.yaml", "evaluate.yaml", "propose.yaml"] {
        fs::copy(fixtures.join(name), root.join(name)).unwrap();
    }
    fs::write(
        root.join(".newton/configs/demo.conf"),
        "definition_file=definition.yaml\n",
    )
    .unwrap();
}

fn command(root: &Path) -> assert_cmd::Command {
    let mut command = support::newton();
    command
        .current_dir(root)
        .arg("--log-dir")
        .arg(root.join("logs"));
    command
}

fn yaml(path: &Path) -> serde_yaml::Value {
    serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn save_yaml(path: &Path, value: &serde_yaml::Value) {
    fs::write(path, serde_yaml::to_string(value).unwrap()).unwrap();
}

fn run_directory(root: &Path) -> PathBuf {
    fs::read_dir(root.join(".newton/state/optimize"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.join("run.json").is_file())
        .unwrap()
}

fn read(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn set_one_cycle_without_completion(root: &Path) {
    let path = root.join("definition.yaml");
    let mut definition = yaml(&path);
    definition["requirements"]["resource_limits"]["max_cycles"] = 1.into();
    definition["requirements"]["completion"][0]["target"] = 0.into();
    save_yaml(&path, &definition);
}

#[test]
fn public_cli_records_before_after_decision_and_best_result() {
    let directory = tempfile::tempdir().unwrap();
    setup(directory.path());
    command(directory.path())
        .args(["optimize", "demo", "--poll-interval", "1"])
        .assert()
        .success();
    let run = run_directory(directory.path());
    let first = read(run.join("cycles/0001.json"));
    let second = read(run.join("cycles/0002.json"));
    assert_eq!(
        first["baseline_evaluations"][0]["evaluation"]["measurements"]["makespan"]["samples"],
        serde_json::json!([10.0])
    );
    assert_eq!(
        first["candidate_evaluations"][0]["evaluation"]["measurements"]["makespan"]["samples"],
        serde_json::json!([7.0])
    );
    assert_eq!(
        second["candidate_evaluations"][0]["evaluation"]["measurements"]["makespan"]["samples"],
        serde_json::json!([4.0])
    );
    assert_eq!(second["decision"]["disposition"], "improvement");
    assert_eq!(read(run.join("outcome.json"))["stop_reason"], "completed");
}

#[test]
fn violated_candidate_preserves_the_incumbent_and_records_rejection() {
    let directory = tempfile::tempdir().unwrap();
    setup(directory.path());
    set_one_cycle_without_completion(directory.path());
    let path = directory.path().join("evaluate.yaml");
    let mut evaluate = yaml(&path);
    evaluate["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["constraints"]["feasible"]
        ["status"] = serde_yaml::from_str(
        "$expr: 'if triggers.stage == \"candidate\" { \"violated\" } else { \"satisfied\" }'\n",
    )
    .unwrap();
    save_yaml(&path, &evaluate);

    command(directory.path())
        .args(["optimize", "demo", "--poll-interval", "1"])
        .assert()
        .success();
    let run = run_directory(directory.path());
    let cycle = read(run.join("cycles/0001.json"));
    let outcome = read(run.join("outcome.json"));
    assert_eq!(cycle["status"], "rejected");
    assert_eq!(cycle["decision"]["disposition"], "rejected");
    assert_eq!(
        outcome["retained_result"]["candidate"]["artifact_id"],
        "schedule:10"
    );
    assert_eq!(
        outcome["accepted_result"]["candidate"]["artifact_id"],
        "schedule:10"
    );
}

#[test]
fn unavailable_required_check_is_inconclusive_and_preserves_the_incumbent() {
    let directory = tempfile::tempdir().unwrap();
    setup(directory.path());
    set_one_cycle_without_completion(directory.path());
    let path = directory.path().join("evaluate.yaml");
    let mut evaluate = yaml(&path);
    evaluate["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["constraints"]["feasible"]
        ["status"] = serde_yaml::from_str(
        "$expr: 'if triggers.stage == \"candidate\" { \"unknown\" } else { \"satisfied\" }'\n",
    )
    .unwrap();
    save_yaml(&path, &evaluate);

    command(directory.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let run = run_directory(directory.path());
    assert_eq!(read(run.join("cycles/0001.json"))["status"], "inconclusive");
    assert_eq!(
        read(run.join("outcome.json"))["retained_result"]["candidate"]["artifact_id"],
        "schedule:10"
    );
}

#[test]
fn evaluation_limit_stops_safely_and_retains_the_qualified_baseline() {
    let directory = tempfile::tempdir().unwrap();
    setup(directory.path());
    let definition_path = directory.path().join("definition.yaml");
    let mut definition = yaml(&definition_path);
    definition["requirements"]["resource_limits"]["max_evaluations"] = 1.into();
    definition["requirements"]["completion"][0]["target"] = 0.into();
    save_yaml(&definition_path, &definition);

    command(directory.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let outcome = read(run_directory(directory.path()).join("outcome.json"));
    assert_eq!(outcome["stop_reason"], "resource_limit");
    assert_eq!(outcome["usage"]["evaluations"], 1);
    assert_eq!(
        outcome["retained_result"]["candidate"]["artifact_id"],
        "schedule:10"
    );
}

#[test]
fn threshold_regression_has_an_explicit_cycle_and_stop_status() {
    let directory = tempfile::tempdir().unwrap();
    setup(directory.path());
    let definition_path = directory.path().join("definition.yaml");
    let mut definition = yaml(&definition_path);
    definition["requirements"]["objective"] = serde_yaml::from_str(
        "mode: thresholds\nobjectives:\n  - objective:\n      id: makespan\n      evaluator: simulator\n      measurement: {kind: grade, dimension: quality}\n    target: 90\n    regression_delta: 5\n    no_progress_cycles: 2\n",
    )
    .unwrap();
    definition["requirements"]["completion"][0]["target"] = 90.into();
    save_yaml(&definition_path, &definition);
    let evaluate_path = directory.path().join("evaluate.yaml");
    let mut evaluate = yaml(&evaluate_path);
    evaluate["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["measurements"]["makespan"]
        ["measurement"] = serde_yaml::from_str("{kind: grade, dimension: quality}").unwrap();
    evaluate["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["measurements"]["makespan"]
        ["samples"] = serde_yaml::from_str(
        "$expr: 'if triggers.stage == \"baseline\" { [80.0] } else { [70.0] }'\n",
    )
    .unwrap();
    save_yaml(&evaluate_path, &evaluate);

    command(directory.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let run = run_directory(directory.path());
    assert_eq!(
        read(run.join("cycles/0001.json"))["status"],
        "threshold_stop"
    );
    assert_eq!(read(run.join("outcome.json"))["stop_reason"], "regression");
}

#[test]
fn requirements_revision_requalifies_retained_state_before_new_acceptance() {
    let directory = tempfile::tempdir().unwrap();
    setup(directory.path());
    command(directory.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let run = run_directory(directory.path());
    let run_id = read(run.join("current.json"))["run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let definition = yaml(&directory.path().join("definition.yaml"));
    let update = serde_yaml::to_string(&serde_json::json!({
        "base_revision": 1,
        "requirements": definition["requirements"],
    }))
    .unwrap();
    fs::write(directory.path().join("requirements-update.yaml"), update).unwrap();

    command(directory.path())
        .args([
            "optimize",
            "demo",
            "--resume",
            &run_id,
            "--requirements-update",
            "requirements-update.yaml",
            "--once",
        ])
        .assert()
        .success();

    let current = read(run.join("current.json"));
    assert_eq!(current["binding"]["requirements"]["revision"], 2);
    assert_eq!(
        current["accepted"]["evaluation"]["requirements_revision"],
        2
    );
    assert_eq!(
        current["accepted_history"][0]["evaluation"]["requirements_revision"],
        1
    );
    assert_eq!(read(run.join("outcome.json"))["stop_reason"], "completed");
}

#[test]
fn known_safe_failure_is_history_and_stagnation_not_operational_failure() {
    let directory = tempfile::tempdir().unwrap();
    setup(directory.path());
    let definition_path = directory.path().join("definition.yaml");
    let mut definition = yaml(&definition_path);
    definition["requirements"]["stagnation_cycles"] = 1.into();
    definition["requirements"]["completion"][0]["target"] = 0.into();
    save_yaml(&definition_path, &definition);
    let proposal_path = directory.path().join("propose.yaml");
    let mut proposal = yaml(&proposal_path);
    proposal["workflow"]["settings"]["io"]["result_map"] = serde_yaml::from_str(
        "decision: failed\nproposal_id: safe-failure\nfailure:\n  reason: simulator declined the move\n  evidence: [no-external-effect]\n",
    )
    .unwrap();
    save_yaml(&proposal_path, &proposal);

    command(directory.path())
        .args(["optimize", "demo", "--poll-interval", "1"])
        .assert()
        .success();
    let run = run_directory(directory.path());
    assert_eq!(
        read(run.join("cycles/0001.json"))["status"],
        "failed_safely"
    );
    assert_eq!(read(run.join("outcome.json"))["stop_reason"], "no_progress");
}

#[test]
fn no_actionable_work_returns_the_retained_baseline_below_target() {
    let directory = tempfile::tempdir().unwrap();
    setup(directory.path());
    set_one_cycle_without_completion(directory.path());
    let proposal_path = directory.path().join("propose.yaml");
    let mut proposal = yaml(&proposal_path);
    proposal["workflow"]["settings"]["io"]["result_map"] =
        serde_yaml::from_str("decision: none\nreason: no useful parameter move remains\n").unwrap();
    save_yaml(&proposal_path, &proposal);

    command(directory.path())
        .args(["optimize", "demo"])
        .assert()
        .success();
    let outcome = read(run_directory(directory.path()).join("outcome.json"));
    assert_eq!(outcome["stop_reason"], "no_actionable_work");
    assert_eq!(outcome["completion"]["status"], "violated");
    assert_eq!(
        outcome["retained_result"]["candidate"]["artifact_id"],
        "schedule:10"
    );
}

#[test]
fn observation_driven_k_is_validated_against_current_assessment() {
    let directory = tempfile::tempdir().unwrap();
    setup(directory.path());
    let definition_path = directory.path().join("definition.yaml");
    let mut definition = yaml(&definition_path);
    definition["strategy"] =
        serde_yaml::from_str("kind: observation_driven\nmax_suggestions: 1\n").unwrap();
    save_yaml(&definition_path, &definition);
    let evaluate_path = directory.path().join("evaluate.yaml");
    let mut evaluate = yaml(&evaluate_path);
    evaluate["workflow"]["settings"]["io"]["result_map"]["assessment"] =
        "$expr: tasks.simulate.output.patch.assessment".into();
    evaluate["workflow"]["tasks"][0]["params"]["patch"]["assessment"] =
        serde_yaml::from_str(
            "id: assessment-1\nsummary: two choices\nobservations:\n  - {id: first, title: First, rationale: first weakness, suggested_action: first move}\n  - {id: second, title: Second, rationale: second weakness, suggested_action: second move}\n",
        )
        .unwrap();
    save_yaml(&evaluate_path, &evaluate);
    let proposal_path = directory.path().join("propose.yaml");
    let mut proposal = yaml(&proposal_path);
    proposal["workflow"]["settings"]["io"]["result_map"]["selected_observations"] =
        "$expr: [#{assessment_id: \"assessment-1\", observation_id: \"first\", rationale: \"first\"}, #{assessment_id: \"assessment-1\", observation_id: \"second\", rationale: \"second\"}]".into();
    save_yaml(&proposal_path, &proposal);

    command(directory.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("maximum is 1"));
}

#[test]
fn optional_execute_workflow_is_used_only_for_execute_proposals() {
    let directory = tempfile::tempdir().unwrap();
    setup(directory.path());
    let definition_path = directory.path().join("definition.yaml");
    let mut definition = yaml(&definition_path);
    definition["workflows"]["execute"] = "execute.yaml".into();
    save_yaml(&definition_path, &definition);
    let proposal_path = directory.path().join("propose.yaml");
    let mut proposal = yaml(&proposal_path);
    proposal["workflow"]["settings"]["io"]["result_map"] = serde_yaml::from_str(
        "decision: execute\nproposal_id: execute-1\nrationale: render a candidate\nattempt: {next: 7}\nselected_observations: []\n",
    )
    .unwrap();
    save_yaml(&proposal_path, &proposal);
    fs::write(
        directory.path().join("execute.yaml"),
        "version: '2.0'\nmode: workflow_graph\nworkflow:\n  settings:\n    entry_task: render\n    io:\n      result_map:\n        status: candidate\n        candidate: '$expr: #{id: triggers.candidate_id, artifact_id: \"schedule:7\", base_artifact_id: \"schedule:base\", created_under_revision: triggers.requirements_revision}'\n  tasks:\n    - id: render\n      operator: NoOpOperator\n      terminal: success\n",
    )
    .unwrap();

    command(directory.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let cycle = read(run_directory(directory.path()).join("cycles/0001.json"));
    assert_eq!(cycle["proposal"]["decision"], "execute");
    assert_eq!(cycle["execution"]["status"], "candidate");
    assert!(cycle["execution_id"].as_str().is_some());
}

#[test]
fn domain_specific_promotion_role_is_rejected_before_run_creation() {
    let directory = tempfile::tempdir().unwrap();
    setup(directory.path());
    let definition_path = directory.path().join("definition.yaml");
    let mut definition = yaml(&definition_path);
    definition["workflows"]["promote"] = "propose.yaml".into();
    save_yaml(&definition_path, &definition);
    command(directory.path())
        .args(["optimize", "demo", "--preflight"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "promotion workflow is unsupported",
        ));
    assert!(!directory.path().join(".newton/state/optimize").exists());
}

#[test]
fn unmetered_role_retries_are_rejected_before_run_creation() {
    let directory = tempfile::tempdir().unwrap();
    setup(directory.path());
    let proposal_path = directory.path().join("propose.yaml");
    let mut proposal = yaml(&proposal_path);
    proposal["workflow"]["tasks"][0]["retry"] =
        serde_yaml::from_str("{max_attempts: 2, backoff_ms: 0}").unwrap();
    save_yaml(&proposal_path, &proposal);

    command(directory.path())
        .args(["optimize", "demo", "--preflight"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "cannot meter internal task retries",
        ));
    assert!(!directory.path().join(".newton/state/optimize").exists());
}

#[test]
fn builtin_template_installs_a_valid_version_two_generic_definition() {
    let directory = tempfile::tempdir().unwrap();
    command(directory.path())
        .args([
            "init",
            directory.path().to_str().unwrap(),
            "--template",
            "builtin",
        ])
        .assert()
        .success();

    let output = command(directory.path())
        .args([
            "optimize",
            "default",
            "--inspect",
            "--param",
            "agent=\"pi\"",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inspection: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(inspection["definition_id"], "software-security");
    assert!(!directory.path().join(".newton/state/optimize").exists());

    let definition = directory
        .path()
        .join(".newton/definitions/software-security");
    let installed = yaml(&definition.join("definition.yaml"));
    assert_eq!(installed["schema_version"], 2);
    assert_eq!(installed["strategy"]["kind"], "measurement_driven");
    assert_eq!(installed["workflows"]["propose"], "plan.yaml");
    assert_eq!(installed["workflows"]["execute"], "develop.yaml");
    for workflow in ["grade.yaml", "plan.yaml", "develop.yaml"] {
        command(directory.path())
            .args([
                "workflow",
                "validate",
                definition.join(workflow).to_str().unwrap(),
            ])
            .assert()
            .success();
    }
}
