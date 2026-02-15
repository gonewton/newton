use assert_cmd::prelude::*;
use newton::cli::{args::MonitorArgs, Command};
use newton::logging;
use predicates::prelude::*;
use std::{
    env,
    fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};
use tempfile::TempDir;

#[test]
fn monitor_context_writes_file_without_console() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let workspace = workspace_path(&temp_dir, "monitor");

    let mut cmd =
        ProcessCommand::cargo_bin("logging_integration_helper").expect("failed to build helper");
    cmd.arg("monitor")
        .arg("--workspace")
        .arg(&workspace)
        .arg("--message")
        .arg("monitor integration test")
        .current_dir(&workspace);
    cmd.assert().success().stderr(predicate::str::is_empty());

    let contents =
        fs::read_to_string(log_file_path(&workspace)).expect("failed to read log file");
    assert!(contents.contains("monitor integration test"));
}

#[test]
fn multi_sink_writes_console_and_file() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let workspace = workspace_path(&temp_dir, "localdev");

    let mut cmd =
        ProcessCommand::cargo_bin("logging_integration_helper").expect("failed to build helper");
    cmd.arg("localdev")
        .arg("--workspace")
        .arg(&workspace)
        .arg("--message")
        .arg("multi sink event");
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("multi sink event"));

    let contents =
        fs::read_to_string(log_file_path(&workspace)).expect("failed to read log file");
    assert!(contents.contains("multi sink event"));
}

#[test]
fn opentelemetry_failure_logs_warning_and_continues() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let workspace = workspace_path(&temp_dir, "otel");

    let mut cmd =
        ProcessCommand::cargo_bin("logging_integration_helper").expect("failed to build helper");
    cmd.arg("localdev")
        .arg("--workspace")
        .arg(&workspace)
        .arg("--message")
        .arg("otel failure test")
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", "bad url");
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("OpenTelemetry exporter disabled"));

    let contents =
        fs::read_to_string(log_file_path(&workspace)).expect("failed to read log file");
    assert!(contents.contains("otel failure test"));
}

#[test]
fn logging_guard_flushes_on_drop() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let workspace = workspace_path(&temp_dir, "guard");
    let original_dir = env::current_dir().expect("failed to read current dir");
    env::set_current_dir(&workspace).expect("failed to switch workspace");

    let command = Command::Monitor(MonitorArgs {
        http_url: None,
        ws_url: None,
    });
    let guard =
        logging::init(&command).expect("failed to initialize logging for guard flush test");
    tracing::info!("logging guard flush test");
    drop(guard);

    let contents =
        fs::read_to_string(log_file_path(&workspace)).expect("failed to read log file");
    assert!(contents.contains("logging guard flush test"));

    env::set_current_dir(original_dir).expect("failed to restore cwd");
}

fn workspace_path(temp_dir: &TempDir, name: &str) -> PathBuf {
    let workspace = temp_dir.path().join(name);
    fs::create_dir_all(workspace.join(".newton")).expect("failed to create .newton directory");
    workspace
}

fn log_file_path(workspace: &Path) -> PathBuf {
    workspace.join(".newton/logs/newton.log")
}
