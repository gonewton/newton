//! Real concurrent CLI processes and a YAML-controlled in-flight development step.

use super::{journal, newton, setup, support};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for live control boundary"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn start(root: &Path) -> Owner {
    start_with_grade(root, |_| {})
}

fn start_with_grade(root: &Path, customize: impl FnOnce(&mut serde_yaml::Value)) -> Owner {
    setup(root);
    let fixtures = support::fixture_path("optimization");
    fs::copy(
        fixtures.join("live_develop.yaml"),
        root.join("develop.yaml"),
    )
    .unwrap();
    fs::copy(
        fixtures.join("live_develop.py"),
        root.join("live_develop.py"),
    )
    .unwrap();
    let definition = root.join("definition.yaml");
    let source = fs::read_to_string(&definition).unwrap() + "\nassets: [live_develop.py]\n";
    fs::write(definition, source).unwrap();
    let grade = root.join("grade.yaml");
    let mut yaml: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&grade).unwrap()).unwrap();
    yaml["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["constraints"]["live_guard"] =
        serde_yaml::from_str("evaluator: fixture\nstatus: violated\nevidence: [live-guard-proof]")
            .unwrap();
    yaml["workflow"]["tasks"][0]["params"]["patch"]["candidate"]["created_under_revision"] =
        serde_yaml::to_value(json!({"$expr": "if triggers.stage == \"candidate\" { triggers.candidate.created_under_revision } else { triggers.requirements_revision }"})).unwrap();
    customize(&mut yaml);
    fs::write(grade, serde_yaml::to_string(&yaml).unwrap()).unwrap();
    fs::write(root.join(".newton/configs/demo.conf"), "definition_file=definition.yaml\noptimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy\n").unwrap();
    let log = fs::File::create(root.join("owner.log")).unwrap();
    let mut owner = Owner(
        Command::new(assert_cmd::cargo::cargo_bin!("newton"))
            .current_dir(root)
            .arg("--log-dir")
            .arg(root.join("logs"))
            .args(["optimize", "demo", "--once"])
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap(),
    );
    wait_for(|| {
        assert!(
            owner.0.try_wait().unwrap().is_none(),
            "owner exited before development: {}",
            fs::read_to_string(root.join("owner.log")).unwrap()
        );
        root.join("development-ready").exists()
    });
    owner
}

#[test]
fn live_revision_regrades_incumbent_and_rejects_a_worse_candidate() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut owner = start_with_grade(root, |grade| {
        grade["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["measurements"]["size"]["samples"] = serde_yaml::to_value(json!({"$expr": "if triggers.stage == \"baseline\" { [10.0] } else if triggers.candidate.artifact_id == \"original\" { [10.0] } else { [20.0] }"})).unwrap();
    });
    let before = journal(root);
    let run_id = before["run_id"].as_str().unwrap();
    let mut requirements = before["binding"]["requirements"]["requirements"].clone();
    requirements["completion"][0]["target"] = json!(3);
    let request = root.join("live-objective-update.yaml");
    fs::write(
        &request,
        serde_yaml::to_string(&json!({"base_revision":1,"requirements":requirements})).unwrap(),
    )
    .unwrap();
    newton()
        .current_dir(root)
        .arg("--log-dir")
        .arg(root.join("logs"))
        .args([
            "optimize",
            "demo",
            "--resume",
            run_id,
            "--requirements-update",
            request.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("remains Pending"));
    release(root, &mut owner);
    let after = journal(root);
    assert_eq!(after["binding"]["requirements"]["revision"], 2);
    assert_eq!(after["evidence"]["requirements_revision"], 2);
    assert_eq!(
        after["evidence"]["measurements"]["size"]["samples"],
        json!([20.0])
    );
    assert_eq!(
        after["accepted"]["candidate"],
        before["accepted"]["candidate"]
    );
    assert_eq!(after["accepted"]["evaluation"]["requirements_revision"], 2);
    assert_eq!(
        after["accepted"]["evaluation"]["measurements"]["size"]["samples"],
        json!([10.0])
    );
    assert_eq!(after["evaluation_count"], 4);
    assert_eq!(after["accepted_history"][0], before["accepted"]);
}

#[test]
fn failed_revision_regrade_leaves_only_historical_incumbent_evidence() {
    for failure in ["budget", "evaluator"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut owner = start_with_grade(root, |grade| {
            if failure == "evaluator" {
                grade["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["measurements"]["size"] = serde_yaml::to_value(json!({"$expr":"if triggers.requirements_revision == 2 { #{status: \"error\", message: \"revision evaluator failed\"} } else { #{status: \"produced\", measurement: #{kind: \"numeric\", unit: \"bytes\", direction: \"minimize\"}, samples: [10.0]} }"})).unwrap();
            }
        });
        let before = journal(root);
        let run_id = before["run_id"].as_str().unwrap();
        let mut requirements = before["binding"]["requirements"]["requirements"].clone();
        if failure == "budget" {
            requirements["resource_limits"]["max_evaluations"] = json!(2);
        }
        let path = root.join("failing-regrade-update.yaml");
        fs::write(
            &path,
            serde_yaml::to_string(&json!({"base_revision":1,"requirements":requirements})).unwrap(),
        )
        .unwrap();
        newton()
            .current_dir(root)
            .arg("--log-dir")
            .arg(root.join("logs"))
            .args([
                "optimize",
                "demo",
                "--resume",
                run_id,
                "--requirements-update",
                path.to_str().unwrap(),
            ])
            .assert()
            .failure()
            .stderr(predicates::str::contains("remains Pending"));
        fs::write(root.join("development-release"), "continue").unwrap();
        wait_for(|| owner.0.try_wait().unwrap().is_some());
        let after = journal(root);
        assert_eq!(after["binding"]["requirements"]["revision"], 2);
        assert!(
            after["accepted"].is_null(),
            "{failure}: active journal retained stale acceptance"
        );
        assert_eq!(after["accepted_history"][0], before["accepted"]);
        assert!(after["outcome"]["accepted_result"].is_null());
        assert_eq!(after["outcome"]["no_acceptable_result_found"], true);
        assert_eq!(
            after["outcome"]["stop_reason"],
            if failure == "budget" {
                "resource_limit"
            } else {
                "operational_failure"
            }
        );
    }
}

#[test]
fn authorized_live_evaluator_change_executes_the_new_pinned_workflow() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut owner = start_with_grade(root, |grade| {
        grade["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["evaluator_revisions"]
            ["fixture"] = serde_yaml::to_value(
            json!({"$expr":"triggers.requirements.evaluators.fixture.revision"}),
        )
        .unwrap();
        grade["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["measurements"]["size"]["samples"] = serde_yaml::to_value(json!({"$expr":"if triggers.stage == \"baseline\" { [10.0] } else if triggers.candidate.artifact_id == \"original\" { [10.0] } else { [20.0] }"})).unwrap();
        let mut alternative = grade.clone();
        alternative["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["measurements"]["size"]["samples"] = serde_yaml::to_value(json!({"$expr":"if triggers.candidate.artifact_id == \"original\" { [100.0] } else { [200.0] }"})).unwrap();
        fs::write(
            root.join("grade-b.yaml"),
            serde_yaml::to_string(&alternative).unwrap(),
        )
        .unwrap();
        let definition = root.join("definition.yaml");
        let mut source: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(&definition).unwrap()).unwrap();
        source["workflows"]["alternate_grade"] = "grade-b.yaml".into();
        fs::write(definition, serde_yaml::to_string(&source).unwrap()).unwrap();
    });
    let before = journal(root);
    let mut requirements = before["binding"]["requirements"]["requirements"].clone();
    requirements["evaluators"]["fixture"] = json!({"workflow":"grade-b.yaml","revision":"2"});
    let request = root.join("switch-evaluator.yaml");
    fs::write(
        &request,
        serde_yaml::to_string(&json!({"base_revision":1,"requirements":requirements})).unwrap(),
    )
    .unwrap();
    newton()
        .current_dir(root)
        .arg("--log-dir")
        .arg(root.join("logs"))
        .args([
            "optimize",
            "demo",
            "--resume",
            before["run_id"].as_str().unwrap(),
            "--requirements-update",
            request.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("remains Pending"));
    release(root, &mut owner);
    let after = journal(root);
    assert_eq!(
        after["binding"]["requirements"]["requirements"]["evaluators"]["fixture"]["workflow"],
        "grade-b.yaml"
    );
    assert_eq!(
        after["accepted"]["evaluation"]["measurements"]["size"]["samples"],
        json!([100.0])
    );
    assert_eq!(
        after["evidence"]["measurements"]["size"]["samples"],
        json!([200.0])
    );
    assert_eq!(after["evidence"]["evaluator_revisions"]["fixture"], "2");
    assert_eq!(after["evaluation_count"], 4);
}

#[test]
fn multiple_evaluator_identities_can_share_one_aggregate_workflow() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut owner = start_with_grade(root, |grade| {
        grade["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["evaluator_revisions"]
            ["secondary"] = "1".into();
        grade["workflow"]["tasks"][0]["params"]["patch"]["evaluation"]["constraints"]
            ["secondary_check"] = serde_yaml::from_str(
            "evaluator: secondary\nstatus: satisfied\nevidence: [secondary-check]",
        )
        .unwrap();
        let definition = root.join("definition.yaml");
        let mut source: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(&definition).unwrap()).unwrap();
        source["requirements"]["evaluators"]["secondary"] =
            serde_yaml::from_str("workflow: grade.yaml\nrevision: '1'").unwrap();
        source["requirements"]["acceptance_constraints"]
            .as_sequence_mut()
            .unwrap()
            .push(serde_yaml::from_str("id: secondary_check\nevaluator: secondary").unwrap());
        fs::write(definition, serde_yaml::to_string(&source).unwrap()).unwrap();
    });
    release(root, &mut owner);
    let after = journal(root);
    assert_eq!(
        after["accepted"]["evaluation"]["evaluator_revisions"],
        json!({"fixture":"1","secondary":"1"})
    );
    assert_eq!(after["evaluation_count"], 2);
}

#[test]
fn incompatible_mixed_evaluator_workflows_fail_before_execution() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup(root);
    fs::copy(root.join("grade.yaml"), root.join("grade-b.yaml")).unwrap();
    let path = root.join("definition.yaml");
    let mut definition: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    definition["requirements"]["evaluators"]["secondary"] =
        serde_yaml::from_str("workflow: grade-b.yaml\nrevision: '1'").unwrap();
    fs::write(path, serde_yaml::to_string(&definition).unwrap()).unwrap();
    newton()
        .current_dir(root)
        .arg("--log-dir")
        .arg(root.join("logs"))
        .args(["optimize", "demo", "--once"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "aggregate GradeOutput requires all active evaluators to share one workflow",
        ));
    assert!(!root.join(".newton/state/optimize").exists());
}

fn update(root: &Path, before: &Value) -> std::path::PathBuf {
    let mut requirements = before["binding"]["requirements"]["requirements"].clone();
    requirements["acceptance_constraints"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "live_guard", "evaluator": "fixture", "human_judged": false}));
    let path = root.join("live-update.yaml");
    fs::write(
        &path,
        serde_yaml::to_string(&json!({"base_revision": 1, "requirements": requirements})).unwrap(),
    )
    .unwrap();
    path
}

fn release(root: &Path, owner: &mut Owner) {
    fs::write(root.join("development-release"), "continue").unwrap();
    let mut status = None;
    wait_for(|| {
        status = owner.0.try_wait().unwrap();
        status.is_some()
    });
    assert!(
        status.unwrap().success(),
        "{}",
        fs::read_to_string(root.join("owner.log")).unwrap()
    );
}

#[test]
fn live_acceptance_update_is_activated_before_candidate_acceptance() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut owner = start(root);
    let before = journal(root);
    let run_id = before["run_id"].as_str().unwrap();
    let request = update(root, &before);
    newton()
        .current_dir(root)
        .args([
            "optimize",
            "demo",
            "--resume",
            run_id,
            "--requirements-update",
            request.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("remains Pending"));
    assert_eq!(journal(root)["binding"]["requirements"]["revision"], 1);
    release(root, &mut owner);
    let after = journal(root);
    assert_eq!(after["binding"]["requirements"]["revision"], 2);
    assert_eq!(after["evidence"]["requirements_revision"], 2);
    assert_eq!(after["evidence"]["artifact_id"], "better");
    assert!(after["accepted"].is_null());
    assert!(after["binding"]["requirements"]["prior_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str().unwrap().contains("completed")));
    assert!(!root
        .join(".newton/state/optimize")
        .join(run_id)
        .join("requirements-pending.json")
        .exists());
}

#[test]
fn unsupported_restriction_is_rejected_without_acknowledging_live_work_paused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut owner = start(root);
    let before = journal(root);
    let run_id = before["run_id"].as_str().unwrap();
    let request = update(root, &before);
    let mut update: Value = serde_yaml::from_str(&fs::read_to_string(&request).unwrap()).unwrap();
    update["requirements"]["execution_restrictions"]["denied_actions"] = json!(["network"]);
    fs::write(&request, serde_yaml::to_string(&update).unwrap()).unwrap();
    newton()
        .current_dir(root)
        .args([
            "optimize",
            "demo",
            "--resume",
            run_id,
            "--requirements-update",
            request.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot prohibit"));
    assert_eq!(journal(root)["binding"]["requirements"]["revision"], 1);
    let run = root.join(".newton/state/optimize").join(run_id);
    assert!(!run.join("requirements-pending.json").exists());
    assert!(fs::read_dir(&run).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("requirements-rejected-")));
    release(root, &mut owner);
    let after = journal(root);
    assert_eq!(after["binding"]["requirements"]["revision"], 1);
    assert_eq!(after["accepted"]["candidate"]["artifact_id"], "better");
}

#[test]
fn owner_revalidates_stale_and_unsupported_pending_inbox_before_activation() {
    for invalid in ["stale", "evaluator", "restriction"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut owner = start(root);
        let before = journal(root);
        let run_id = before["run_id"].as_str().unwrap();
        let request = update(root, &before);
        newton()
            .current_dir(root)
            .args([
                "optimize",
                "demo",
                "--resume",
                run_id,
                "--requirements-update",
                request.to_str().unwrap(),
            ])
            .assert()
            .failure()
            .stderr(predicates::str::contains("remains Pending"));
        // Fault injection at the documented durable inbox boundary. A prior
        // submitter check must not substitute for owner-side CAS/authority checks.
        let run = root.join(".newton/state/optimize").join(run_id);
        let inbox = run.join("requirements-pending.json");
        let mut pending: Value = serde_json::from_slice(&fs::read(&inbox).unwrap()).unwrap();
        match invalid {
            "stale" => pending["base_revision"] = json!(0),
            "evaluator" => {
                pending["requirements"]["evaluators"]["fixture"]["workflow"] =
                    json!("missing-evaluator.yaml")
            }
            "restriction" => {
                pending["requirements"]["execution_restrictions"]["denied_actions"] =
                    json!(["network"])
            }
            _ => unreachable!(),
        }
        fs::write(&inbox, serde_json::to_vec(&pending).unwrap()).unwrap();
        release(root, &mut owner);
        let after = journal(root);
        assert_eq!(after["binding"]["requirements"]["revision"], 1, "{invalid}");
        assert_eq!(
            after["accepted"]["candidate"]["artifact_id"], "better",
            "{invalid}"
        );
        assert!(
            after["revisions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["status"] == "rejected"),
            "{invalid}"
        );
        assert!(!inbox.exists());
    }
}
