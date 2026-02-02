use assert_cmd::Command;
use insta::assert_snapshot;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_run_command_snapshot() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("run").arg("--help");
    let _assert = cmd.assert().success();
    let output = _assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_snapshot!(stdout);
}

#[test]
fn test_step_command_snapshot() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("step").arg("--help");
    let _assert = cmd.assert().success();
    let output = _assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_snapshot!(stdout);
}

#[test]
fn test_status_command_snapshot() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("status").arg("--help");
    let _assert = cmd.assert().success();
    let output = _assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_snapshot!(stdout);
}

#[test]
fn test_report_command_snapshot() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("report").arg("--help");
    let _assert = cmd.assert().success();
    let output = _assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_snapshot!(stdout);
}

#[test]
fn test_error_command_snapshot() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("error").arg("--help");
    let _assert = cmd.assert().success();
    let output = _assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_snapshot!(stdout);
}

#[test]
fn test_main_help_snapshot() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("--help");
    let _assert = cmd.assert().success();
    let output = _assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_snapshot!(stdout);
}

#[test]
fn test_version_output_snapshot() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("--version");
    let _assert = cmd.assert().success();
    let output = _assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_snapshot!(stdout);
}

#[tokio::test]
async fn test_step_execution_output() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path().to_str().unwrap();

    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("step").arg(workspace_path);

    let _assert = cmd.assert().success();
    let output = _assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let combined_output = format!("STDOUT:\n{}\nSTDERR:\n{}", stdout, stderr);
    assert_snapshot!(combined_output);
}

#[tokio::test]
async fn test_status_command_execution() {
    let temp_dir = TempDir::new().unwrap();
    let execution_id = "test-exec-123";

    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("status").arg(execution_id);

    let _assert = cmd.assert().success();
    let output = _assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_snapshot!(stdout);
}

#[tokio::test]
async fn test_report_command_execution() {
    let temp_dir = TempDir::new().unwrap();
    let execution_id = "test-exec-123";

    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("report").arg(execution_id);

    let _assert = cmd.assert().success();
    let output = _assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_snapshot!(stdout);
}

#[tokio::test]
async fn test_error_command_execution() {
    let temp_dir = TempDir::new().unwrap();
    let execution_id = "test-exec-123";

    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("error").arg(execution_id);

    let _assert = cmd.assert().success();
    let output = _assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_snapshot!(stdout);
}

#[tokio::test]
#[ignore]
async fn test_run_command_failure_snapshot() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path().to_str().unwrap();

    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.arg("run")
        .arg(workspace_path)
        .arg("--max-iterations")
        .arg("1")
        .arg("--evaluator-cmd")
        .arg("nonexistent_tool_12345");

    // This should fail, and we want to capture the error output
    let _assert = cmd.assert().failure();
    let output = _assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let combined_output = format!("STDOUT:\n{}\nSTDERR:\n{}", stdout, stderr);
    assert_snapshot!(combined_output);
}

#[test]
fn test_complex_args_parsing_snapshot() {
    let mut cmd = Command::cargo_bin("newton").unwrap();
    cmd.args(&[
        "run",
        "--max-iterations",
        "5",
        "--max-time",
        "300",
        "--evaluator-timeout",
        "10",
        "--advisor-timeout",
        "15",
        "--executor-timeout",
        "20",
        "--verbose",
    ])
    .arg("--help");

    let _assert = cmd.assert().success();
    let output = _assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_snapshot!(stdout);
}
