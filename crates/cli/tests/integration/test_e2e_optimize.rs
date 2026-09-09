#[path = "../support/mod.rs"]
mod support;

#[path = "optimization_release.rs"]
mod release;

#[path = "optimization_live_control.rs"]
mod live_control;

#[path = "optimization_resource_limits.rs"]
mod resource_limits;

#[path = "optimization_preflight.rs"]
mod preflight;

use serde_json::Value;
use std::{fs, path::Path};
use support::newton;

fn setup(root: &Path) {
    fs::create_dir_all(root.join(".newton/configs")).unwrap();
    let fixtures = support::fixture_path("optimization");
    for name in [
        "definition.yaml",
        "grade.yaml",
        "plan.yaml",
        "develop.yaml",
        "promote-lie.yaml",
        "promote-mutate.yaml",
        "transient-swap.py",
        "transient-swap-develop.yaml",
        "forged-grade.yaml",
        "fail.yaml",
        "requirements-update.yaml",
    ] {
        fs::copy(fixtures.join(name), root.join(name)).unwrap();
    }
    fs::write(
        root.join(".newton/configs/demo.conf"),
        "definition_file=definition.yaml\noptimize_allowed_actions=merge\n",
    )
    .unwrap();
}

fn run_rejected_cycle(root: &Path) -> Value {
    setup(root);
    fs::write(
        root.join(".newton/configs/demo.conf"),
        "definition_file=definition.yaml\noptimize_allowed_actions=merge\nparameter.reject=true\n",
    )
    .unwrap();
    newton()
        .current_dir(root)
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    journal(root)
}

#[test]
fn safe_local_revision_refreshes_same_artifact_and_rejects_stale_cas() {
    let dir = tempfile::tempdir().unwrap();
    let before = run_rejected_cycle(dir.path());
    let run_id = before["run_id"].as_str().unwrap();
    newton()
        .current_dir(dir.path())
        .args([
            "optimize",
            "demo",
            "--resume",
            run_id,
            "--once",
            "--requirements-update",
            "requirements-update.yaml",
        ])
        .assert()
        .success();
    let after = journal(dir.path());
    assert_eq!(after["binding"]["requirements"]["revision"], 2);
    assert_eq!(
        after["accepted"]["candidate"],
        before["accepted"]["candidate"]
    );
    assert_eq!(after["accepted"]["evaluation"]["requirements_revision"], 2);
    assert_eq!(after["accepted_history"][0], before["accepted"]);
    assert_eq!(after["revisions"][0]["status"], "superseded");
    assert_eq!(after["revisions"][1]["status"], "active");
    newton()
        .current_dir(dir.path())
        .args([
            "optimize",
            "demo",
            "--resume",
            run_id,
            "--requirements-update",
            "requirements-update.yaml",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("stale"));
    assert_eq!(
        journal(dir.path())["evaluation_count"],
        after["evaluation_count"]
    );
    let rejected = fs::read_dir(dir.path().join(".newton/state/optimize").join(run_id))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("requirements-rejected-")
        })
        .unwrap();
    let rejected: Value = serde_json::from_slice(&fs::read(rejected).unwrap()).unwrap();
    assert_eq!(rejected["status"], "rejected");
    assert_eq!(rejected["request"]["base_revision"], 1);
}

#[test]
fn live_owner_retains_pending_request_without_claiming_activation() {
    let dir = tempfile::tempdir().unwrap();
    let before = run_rejected_cycle(dir.path());
    let run_id = before["run_id"].as_str().unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.path().join(".newton/optimize/claim/lock"))
        .unwrap();
    lock.try_lock().unwrap();
    newton()
        .current_dir(dir.path())
        .args([
            "optimize",
            "demo",
            "--resume",
            run_id,
            "--requirements-update",
            "requirements-update.yaml",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("remains Pending"));
    let pending_path = dir
        .path()
        .join(".newton/state/optimize")
        .join(run_id)
        .join("requirements-pending.json");
    let pending: Value = serde_json::from_slice(&fs::read(&pending_path).unwrap()).unwrap();
    assert_eq!(pending["status"], "pending");
    assert_eq!(
        journal(dir.path())["binding"]["requirements"]["revision"],
        1
    );
    drop(lock);
    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--resume", run_id, "--once"])
        .assert()
        .success();
    assert!(!pending_path.exists());
    assert_eq!(
        journal(dir.path())["binding"]["requirements"]["revision"],
        2
    );
}

#[test]
fn revised_requirements_do_not_substitute_a_different_baseline_artifact() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    // Declare the cycle policy before start: active workflows are content-pinned.
    let plan_path = dir.path().join("plan.yaml");
    let mut plan: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&plan_path).unwrap()).unwrap();
    plan["workflow"]["settings"]["io"]["result_map"]["decision"] = serde_yaml::Value::String(
        "$expr: if triggers.cycle > 1 { \"none\" } else { \"propose\" }".into(),
    );
    fs::write(plan_path, serde_yaml::to_string(&plan).unwrap()).unwrap();
    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let before = journal(dir.path());
    let run_id = before["run_id"].as_str().unwrap();
    newton()
        .current_dir(dir.path())
        .args([
            "optimize",
            "demo",
            "--resume",
            run_id,
            "--once",
            "--requirements-update",
            "requirements-update.yaml",
        ])
        .assert()
        .success();
    let after = journal(dir.path());
    assert!(after["accepted"].is_null());
    assert_eq!(after["accepted_history"][0], before["accepted"]);
    assert_eq!(after["outcome"]["no_acceptable_result_found"], true);
}

#[test]
fn uncertain_work_records_pending_without_activating_or_replaying() {
    let dir = tempfile::tempdir().unwrap();
    let mut before = run_rejected_cycle(dir.path());
    let run_id = before["run_id"].as_str().unwrap().to_owned();
    before["phase"] = serde_json::json!("promoting");
    fs::write(
        dir.path()
            .join(".newton/state/optimize")
            .join(&run_id)
            .join("journal.json"),
        serde_json::to_vec(&before).unwrap(),
    )
    .unwrap();
    newton()
        .current_dir(dir.path())
        .args([
            "optimize",
            "demo",
            "--resume",
            &run_id,
            "--requirements-update",
            "requirements-update.yaml",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("Pending"));
    let after = journal(dir.path());
    assert_eq!(after["binding"]["requirements"]["revision"], 1);
    assert_eq!(after["revisions"][0]["status"], "pending");
    assert_eq!(after["work_count"], before["work_count"]);
}

fn journal(root: &Path) -> Value {
    let runs = root.join(".newton/state/optimize");
    let run = fs::read_dir(runs)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.join("journal.json").is_file())
        .unwrap();
    serde_json::from_slice(&fs::read(run.join("journal.json")).unwrap()).unwrap()
}

fn configure_unsupported_promotion(root: &Path, workflow: &str) {
    let definition = root.join("definition.yaml");
    let source = fs::read_to_string(&definition).unwrap().replace(
        "  develop: develop.yaml\n",
        &format!("  develop: develop.yaml\n  promote: {workflow}\n"),
    );
    fs::write(definition, source).unwrap();
}

#[test]
fn lying_promotion_workflow_is_rejected_before_run_creation() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    configure_unsupported_promotion(dir.path(), "promote-lie.yaml");

    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "generic workflow host cannot independently verify the promoted target state",
        ));

    assert!(!dir.path().join(".newton/state/optimize").exists());
    assert!(!dir.path().join(".newton/state/workflows").exists());
    assert!(!dir.path().join(".newton/optimize/claim").exists());
}

#[test]
fn mutating_promotion_workflow_is_rejected_without_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    configure_unsupported_promotion(dir.path(), "promote-mutate.yaml");

    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("atomic compare-and-swap"));

    assert!(!dir.path().join("promotion-was-invoked").exists());
    assert!(!dir.path().join(".newton/state/optimize").exists());
    assert!(!dir.path().join(".newton/state/workflows").exists());
    assert!(!dir.path().join(".newton/optimize/claim").exists());
}

#[test]
fn transient_snapshot_swap_cannot_forge_evaluator_execution() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let definition = dir.path().join("definition.yaml");
    let source = fs::read_to_string(&definition).unwrap().replace(
        "develop: develop.yaml",
        "develop: transient-swap-develop.yaml",
    ) + "\nassets: [transient-swap.py, forged-grade.yaml]\n";
    fs::write(definition, source).unwrap();
    fs::write(
        dir.path().join(".newton/configs/demo.conf"),
        "definition_file=definition.yaml\noptimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy\n",
    )
    .unwrap();

    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    std::thread::sleep(std::time::Duration::from_millis(600));

    let run = journal(dir.path());
    let run_dir = Path::new(run["definition_root"].as_str().unwrap())
        .parent()
        .unwrap();
    assert!(run_dir.join("swap-observed").is_file());
    assert!(!run_dir.join("forged-evaluator-ran").exists());
    assert_eq!(
        run["accepted"]["evaluation"]["measurements"]["size"]["samples"][0],
        1.0
    );
}

#[test]
fn native_once_grades_before_plan_and_acceptance() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let j = journal(dir.path());
    assert_eq!(j["phase"], "finished");
    assert_eq!(j["evaluation_count"], 2);
    assert_eq!(j["work_count"], 2);
    assert_eq!(j["outcome"]["stop_reason"], "completed");
    assert_eq!(
        j["outcome"]["accepted_result"]["candidate"]["artifact_id"],
        "better"
    );
    assert!(!dir
        .path()
        .join(".newton/optimize/claim/owner.json")
        .exists());
    let run_id = j["run_id"].as_str().unwrap();
    // Completed resume returns the durable result and never repeats work.
    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--resume", run_id])
        .assert()
        .success();
    assert_eq!(journal(dir.path())["work_count"], 2);
}

#[test]
fn constraint_failure_preserves_incumbent_and_skips_candidate_acceptance() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    fs::write(
        dir.path().join(".newton/configs/demo.conf"),
        "definition_file=definition.yaml\noptimize_allowed_actions=merge\nparameter.reject=true\n",
    )
    .unwrap();
    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let j = journal(dir.path());
    assert_eq!(j["work_count"], 2);
    assert_eq!(j["evaluation_count"], 2);
    assert_eq!(j["outcome"]["stop_reason"], "cycle_complete");
    assert_eq!(j["outcome"]["completion"]["status"], "violated");
    assert_eq!(j["accepted"]["candidate"]["artifact_id"], "original");
}

#[test]
fn grading_failure_is_not_no_work_or_completion() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    fs::copy(dir.path().join("fail.yaml"), dir.path().join("grade.yaml")).unwrap();
    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .failure();
    let j = journal(dir.path());
    assert_eq!(j["phase"], "failed");
    assert_eq!(j["work_count"], 0);
    assert_eq!(j["outcome"]["stop_reason"], "operational_failure");
    assert_eq!(j["outcome"]["no_acceptable_result_found"], true);
    // An interrupted or failed external phase is never replayed automatically.
    newton()
        .current_dir(dir.path())
        .args([
            "optimize",
            "demo",
            "--resume",
            j["run_id"].as_str().unwrap(),
        ])
        .assert()
        .failure();
    assert_eq!(journal(dir.path())["evaluation_count"], 1);
}

#[test]
fn stale_run_evidence_cannot_start_planning() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let grade = dir.path().join("grade.yaml");
    fs::write(
        &grade,
        fs::read_to_string(&grade).unwrap().replace(
            "run_id: { $expr: triggers.run_id }",
            "run_id: unrelated-run",
        ),
    )
    .unwrap();
    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .failure();
    assert_eq!(journal(dir.path())["work_count"], 0);
}

#[test]
fn old_plan_queue_is_not_silently_consumed() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".newton/configs")).unwrap();
    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("requires --definition"));
}

#[test]
fn repeated_policy_dispatches_and_counts_real_evaluator_workflows() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let definition = dir.path().join("definition.yaml");
    fs::write(
        &definition,
        fs::read_to_string(&definition).unwrap().replace(
            "comparison: {kind: exact}",
            "comparison: {kind: repeated, samples: 2, min_improvement: 1}",
        ),
    )
    .unwrap();
    newton()
        .current_dir(dir.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let j = journal(dir.path());
    assert_eq!(j["evaluation_count"], 4);
    assert_eq!(
        j["accepted"]["evaluation"]["measurements"]["size"]["samples"],
        serde_json::json!([1.0, 1.0])
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        use newton_types::BackendStore;
        let store = newton_backend::SqliteBackendStore::new(&format!(
            "sqlite:{}?mode=rwc",
            dir.path().join(".newton/state/backend.sqlite").display()
        ))
        .await
        .unwrap();
        let stored = store
            .get_optimize_run(j["run_id"].as_str().unwrap())
            .await
            .unwrap();
        assert_eq!(stored.run.status, "converged");
        let cycles = store
            .list_optimize_cycles(j["run_id"].as_str().unwrap())
            .await
            .unwrap();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].decision, "accepted");
        assert!(cycles[0].execution_id.is_some());
        assert!(cycles[0].plan_id.is_some());
    });
}
