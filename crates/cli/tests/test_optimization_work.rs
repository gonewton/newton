//! Generic optimizer behavior through the public CLI and production driver.

#[path = "support/mod.rs"]
mod support;

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn setup_scheduling(root: &Path) {
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

#[test]
fn measurement_only_search_completes_two_cycles_without_git_plan_execute_or_sqlite() {
    let directory = tempfile::tempdir().unwrap();
    setup_scheduling(directory.path());
    command(directory.path())
        .args(["optimize", "demo", "--poll-interval", "1"])
        .assert()
        .success();

    let run = run_directory(directory.path());
    let outcome = read(run.join("outcome.json"));
    assert_eq!(outcome["stop_reason"], "completed");
    assert_eq!(outcome["usage"]["cycles"], 2);
    assert_eq!(
        outcome["accepted_result"]["candidate"]["artifact_id"],
        "schedule:4"
    );
    assert!(run.join("cycles/0001.json").is_file());
    assert!(run.join("cycles/0002.json").is_file());
    let report = read(run.join("report.json"));
    assert_eq!(
        report["before"][0]["candidate"]["artifact_id"],
        "schedule:10"
    );
    assert_eq!(report["after"]["candidate"]["artifact_id"], "schedule:4");
    assert_eq!(report["cycles"].as_array().unwrap().len(), 2);
    assert!(!directory
        .path()
        .join(".newton/state/backend.sqlite")
        .exists());
    for cycle in ["0001.json", "0002.json"] {
        let record = read(run.join("cycles").join(cycle));
        assert_eq!(record["status"], "accepted");
        assert!(record["proposal"]["plan"].is_null());
        assert!(record["execution"].is_null());
        assert!(record["baseline_evaluations"][0]["assessment"].is_null());
    }
}

#[test]
fn published_cycle_advances_an_older_checkpoint_without_replaying_it() {
    let directory = tempfile::tempdir().unwrap();
    setup_scheduling(directory.path());
    command(directory.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let run = run_directory(directory.path());
    let first = fs::read(run.join("cycles/0001.json")).unwrap();
    let mut current = read(run.join("current.json"));
    let run_id = current["run_id"].as_str().unwrap().to_owned();
    current["phase"] = serde_json::json!("evaluating");
    current["outcome"] = Value::Null;
    fs::write(
        run.join("current.json"),
        serde_json::to_vec_pretty(&current).unwrap(),
    )
    .unwrap();

    command(directory.path())
        .args(["optimize", "demo", "--resume", &run_id, "--once"])
        .assert()
        .success();

    assert_eq!(fs::read(run.join("cycles/0001.json")).unwrap(), first);
    assert!(run.join("cycles/0002.json").is_file());
}

#[test]
fn malformed_immutable_cycle_fails_closed_instead_of_disappearing_from_history() {
    let directory = tempfile::tempdir().unwrap();
    setup_scheduling(directory.path());
    command(directory.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let run = run_directory(directory.path());
    let run_id = read(run.join("current.json"))["run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    fs::write(run.join("cycles/0001.json"), "{malformed").unwrap();

    command(directory.path())
        .args(["optimize", "demo", "--resume", &run_id, "--once"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cycles/0001.json"));
    assert_eq!(
        read(run.join("current.json"))["requires_reconciliation"],
        true
    );
}

#[test]
fn one_cycle_can_be_resumed_without_rewriting_published_history() {
    let directory = tempfile::tempdir().unwrap();
    setup_scheduling(directory.path());
    command(directory.path())
        .args(["optimize", "demo", "--once"])
        .assert()
        .success();
    let run = run_directory(directory.path());
    let first = fs::read(run.join("cycles/0001.json")).unwrap();
    let current = read(run.join("current.json"));
    let run_id = current["run_id"].as_str().unwrap();

    command(directory.path())
        .args(["optimize", "demo", "--resume", run_id, "--once"])
        .assert()
        .success();

    assert_eq!(fs::read(run.join("cycles/0001.json")).unwrap(), first);
    assert!(run.join("cycles/0002.json").is_file());
    assert_eq!(read(run.join("outcome.json"))["stop_reason"], "completed");
}

#[test]
fn pre_generic_journal_has_an_explicit_non_migration_error() {
    let directory = tempfile::tempdir().unwrap();
    let run_id = "00000000-0000-4000-8000-000000000001";
    fs::create_dir_all(directory.path().join(".newton/state/optimize").join(run_id)).unwrap();
    fs::write(
        directory
            .path()
            .join(".newton/state/optimize")
            .join(run_id)
            .join("journal.json"),
        "{}",
    )
    .unwrap();
    command(directory.path())
        .args(["optimize", "demo", "--resume", run_id])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "pre-generic journal/SQLite format",
        ));
}
