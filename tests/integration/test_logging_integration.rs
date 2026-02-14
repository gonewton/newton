use newton::cli::{args::MonitorArgs, Args, Command};
use newton::logging::{self, layers, reset_for_tests};
use newton::logging::ConsoleOutput;
use serial_test::serial;
use std::env;
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tracing::info;

#[test]
#[serial]
fn monitor_context_disables_console() {
    reset_for_tests();
    let original_dir = env::current_dir().unwrap();
    let workspace = tempdir().unwrap();
    env::set_current_dir(workspace.path()).unwrap();

    let command = Command::Monitor(MonitorArgs {
        http_url: None,
        ws_url: None,
    });
    let guard = logging::init(&command).unwrap();

    info!("monitor integration event");

    assert_eq!(guard.console_output(), ConsoleOutput::None);
    let log_path = guard.log_file_path().to_path_buf();
    drop(guard);
    let contents = fs::read_to_string(log_path).unwrap();
    assert!(contents.contains("monitor integration event"));

    env::set_current_dir(original_dir).unwrap();
}

#[test]
#[serial]
fn multi_sink_writes_to_file_and_console() {
    reset_for_tests();
    let workspace = tempdir().unwrap();
    let args = Args::parse_from([
        "newton",
        "run",
        workspace.path().to_str().unwrap(),
    ]);

    let buffer = Arc::new(Mutex::new(Vec::new()));
    layers::console::set_test_output(buffer.clone());

    let guard = logging::init(&args.command).unwrap();
    info!("multi sink event");

    let log_path = guard.log_file_path().to_path_buf();
    drop(guard);
    let file_contents = fs::read_to_string(log_path).unwrap();
    assert!(file_contents.contains("multi sink event"));

    let console_contents = String::from_utf8_lossy(&buffer.lock().unwrap());
    assert!(console_contents.contains("multi sink event"));

    layers::console::clear_test_output();
}

#[test]
#[serial]
fn opentelemetry_failure_degrades_to_file_only() {
    reset_for_tests();
    env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://example.com");
    layers::opentelemetry::simulate_failure(true);

    let workspace = tempdir().unwrap();
    let args = Args::parse_from([
        "newton",
        "run",
        workspace.path().to_str().unwrap(),
    ]);

    let guard = logging::init(&args.command).unwrap();
    info!("otel fallback event");

    let log_path = guard.log_file_path().to_path_buf();
    drop(guard);
    let file_contents = fs::read_to_string(log_path).unwrap();
    assert!(file_contents.contains("otel fallback event"));

    layers::opentelemetry::simulate_failure(false);
    env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
}

#[test]
#[serial]
fn initialization_guard_flushes_logs() {
    reset_for_tests();
    let workspace = tempdir().unwrap();
    let args = Args::parse_from([
        "newton",
        "run",
        workspace.path().to_str().unwrap(),
    ]);

    let guard = logging::init(&args.command).unwrap();
    info!("guard flush event");

    let log_path = guard.log_file_path().to_path_buf();
    drop(guard);

    let contents = fs::read_to_string(log_path).unwrap();
    assert!(contents.contains("guard flush event"));
}
