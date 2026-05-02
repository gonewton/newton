use assert_cmd::prelude::*;
use newton_core::logging;
use newton_core::logging::{LogInvocation, LogInvocationKind};
use predicates::prelude::*;
use std::{
    env, fs,
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

    let contents = fs::read_to_string(log_file_path(&workspace)).expect("failed to read log file");
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

    let contents = fs::read_to_string(log_file_path(&workspace)).expect("failed to read log file");
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
        .stderr(predicate::str::contains("OpenTelemetry disabled"));

    let contents = fs::read_to_string(log_file_path(&workspace)).expect("failed to read log file");
    assert!(contents.contains("otel failure test"));
}

#[test]
fn logging_guard_flushes_on_drop() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let workspace = workspace_path(&temp_dir, "guard");
    let original_dir = env::current_dir().expect("failed to read current dir");
    env::set_current_dir(&workspace).expect("failed to switch workspace");

    let command = LogInvocation::new(LogInvocationKind::Monitor, None);
    let guard =
        logging::init(&command, None).expect("failed to initialize logging for guard flush test");
    tracing::info!("logging guard flush test");
    drop(guard);

    let contents = fs::read_to_string(log_file_path(&workspace)).expect("failed to read log file");
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

#[test]
fn log_dir_override_flag_is_accepted() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let log_dir = temp_dir.path().join("custom_logs");
    fs::create_dir_all(&log_dir).expect("failed to create custom log dir");
    let workspace = workspace_path(&temp_dir, "logdir_test");

    let mut cmd =
        ProcessCommand::cargo_bin("logging_integration_helper").expect("failed to build helper");
    cmd.arg("localdev")
        .arg("--workspace")
        .arg(&workspace)
        .arg("--log-dir")
        .arg(&log_dir)
        .arg("--message")
        .arg("log dir override test");
    cmd.assert().success();

    // The log file must appear in the custom directory, not in the default workspace location.
    let custom_log_file = log_dir.join("newton.log");
    assert!(
        custom_log_file.exists(),
        "newton.log must be created in the custom --log-dir directory: {}",
        custom_log_file.display()
    );

    let default_log_file = log_file_path(&workspace);
    assert!(
        !default_log_file.exists(),
        "newton.log must NOT appear in the default workspace location when --log-dir overrides it"
    );

    let contents = fs::read_to_string(&custom_log_file).expect("failed to read custom log file");
    assert!(
        contents.contains("log dir override test"),
        "custom log file must contain the emitted message"
    );
}
