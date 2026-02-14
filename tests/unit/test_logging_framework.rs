use newton::cli::Args;
use newton::logging::{config::LoggingConfig, detect_context, layers, ExecutionContext};
use serial_test::serial;
use std::env;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn context_mapping_cover_all_commands() {
    let cases = vec![
        (vec!["newton", "run", "."], ExecutionContext::LocalDev),
        (vec!["newton", "init", "."], ExecutionContext::LocalDev),
        (vec!["newton", "batch", "project"], ExecutionContext::Batch),
        (vec!["newton", "step", "."], ExecutionContext::LocalDev),
        (
            vec!["newton", "status", "exec", "--path", "."],
            ExecutionContext::LocalDev,
        ),
        (vec!["newton", "report", "exec"], ExecutionContext::LocalDev),
        (vec!["newton", "error", "exec"], ExecutionContext::LocalDev),
        (vec!["newton", "monitor"], ExecutionContext::Tui),
    ];

    for (args, expected) in cases {
        let parsed = Args::parse_from(args);
        assert_eq!(detect_context(&parsed.command), expected);
    }
}

#[test]
#[serial]
fn remote_override_maps_non_monitor_commands() {
    env::set_var("NEWTON_REMOTE_AGENT", "1");
    let cases = vec![
        (vec!["newton", "run", "."], ExecutionContext::RemoteAgent),
        (vec!["newton", "init", "."], ExecutionContext::RemoteAgent),
        (vec!["newton", "batch", "project"], ExecutionContext::RemoteAgent),
        (vec!["newton", "step", "."], ExecutionContext::RemoteAgent),
        (
            vec!["newton", "status", "exec", "--path", "."],
            ExecutionContext::RemoteAgent,
        ),
        (vec!["newton", "report", "exec"], ExecutionContext::RemoteAgent),
        (vec!["newton", "error", "exec"], ExecutionContext::RemoteAgent),
        (vec!["newton", "monitor"], ExecutionContext::Tui),
    ];

    for (args, expected) in cases {
        let parsed = Args::parse_from(args);
        assert_eq!(detect_context(&parsed.command), expected);
    }

    env::remove_var("NEWTON_REMOTE_AGENT");
}

#[test]
#[serial]
fn config_env_override_opentelemetry_endpoint() {
    let workspace = tempdir().unwrap();
    let config_dir = workspace.path().join(".newton/config");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("logging.toml"),
        r#"[logging]
default_level = "warn"
[logging.opentelemetry]
endpoint = "http://configured"
service_name = "configured-service"
enabled = true
"#,
    )
    .unwrap();

    env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://env");
    let config = LoggingConfig::load(Some(workspace.path())).unwrap();
    assert!(config.opentelemetry.enabled);
    assert_eq!(config.opentelemetry.endpoint.as_deref(), Some("http://env"));
    env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
}

#[test]
#[serial]
fn config_default_level_honors_workspace_file() {
    let workspace = tempdir().unwrap();
    let config_dir = workspace.path().join(".newton/config");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("logging.toml"),
        r#"[logging]
default_level = "warn"
"#
    )
    .unwrap();

    let config = LoggingConfig::load(Some(workspace.path())).unwrap();
    assert_eq!(config.default_level, "warn");
}

#[test]
#[serial]
fn opentelemetry_disabled_when_no_endpoint() {
    env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    let config = LoggingConfig::load(None).unwrap();
    assert!(!config.opentelemetry.enabled);
}

#[test]
#[serial]
fn file_path_uses_workspace_when_available() {
    let workspace = tempdir().unwrap();
    let log_path = layers::file::log_file_path(&LoggingConfig::default(), Some(workspace.path())).unwrap();
    assert!(log_path.ends_with(".newton/logs/newton.log"));
}

#[test]
#[serial]
fn file_path_falls_back_to_home() {
    let workspace = tempdir().unwrap();
    let home = tempdir().unwrap();
    env::set_var("HOME", home.path());
    let log_path = layers::file::log_file_path(&LoggingConfig::default(), None).unwrap();
    assert!(log_path.starts_with(home.path()));
    assert!(log_path.ends_with(".newton/logs/newton.log"));
    env::remove_var("HOME");
}

#[test]
fn console_selection_respects_context() {
    assert_eq!(
        layers::console::select_console_output(ExecutionContext::Tui, Some(layers::console::ConsoleOutput::Stdout)),
        layers::console::ConsoleOutput::None
    );
    assert_eq!(
        layers::console::select_console_output(ExecutionContext::Batch, None),
        layers::console::ConsoleOutput::None
    );
    assert_eq!(
        layers::console::select_console_output(ExecutionContext::LocalDev, None),
        layers::console::ConsoleOutput::Stderr
    );
    assert_eq!(
        layers::console::select_console_output(ExecutionContext::RemoteAgent, None),
        layers::console::ConsoleOutput::None
    );
    assert_eq!(
        layers::console::select_console_output(ExecutionContext::RemoteAgent, Some(layers::console::ConsoleOutput::Stdout)),
        layers::console::ConsoleOutput::Stdout
    );
}

#[test]
#[serial]
fn opentelemetry_enabled_when_endpoint_env_set() {
    env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://example.com");
    let config = LoggingConfig::load(None).unwrap();
    assert!(config.opentelemetry.enabled);
    assert_eq!(config.opentelemetry.endpoint.as_deref(), Some("http://example.com"));
    env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
}
