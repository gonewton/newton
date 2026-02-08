use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_run_command_help() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("run").arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Newton Loop optimization run"));
}

#[test]
fn test_step_command_help() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("step").arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("single step execution"));
}

#[test]
fn test_status_command_help() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("status").arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("execution status"));
}

#[test]
fn test_report_command_help() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("report").arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Generating report"));
}

#[test]
fn test_error_command_help() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("error").arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Analyzing errors"));
}

#[test]
fn test_batch_command_help() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("batch").arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Process queued work items"));
}

#[test]
fn test_version_command() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("newton"));
}
