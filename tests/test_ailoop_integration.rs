use newton::ailoop_integration::config::{init_context, AiloopContext};
use newton::cli::args::{RunArgs, StepArgs};
use newton::cli::Command;
use serial_test::serial;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

struct EnvGuard(Vec<(String, Option<String>)>);

impl EnvGuard {
    fn set(vars: &[(&str, &str)]) -> Self {
        let saved = vars
            .iter()
            .map(|(key, _)| (key.to_string(), env::var(key).ok()))
            .collect();
        for (key, value) in vars {
            env::set_var(key, value);
        }
        Self(saved)
    }

    fn clear(keys: &[&str]) -> Self {
        let saved = keys
            .iter()
            .map(|key| (key.to_string(), env::var(key).ok()))
            .collect();
        for key in keys {
            env::remove_var(key);
        }
        Self(saved)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.0.iter().rev() {
            if let Some(ref previous) = value {
                env::set_var(key, previous);
            } else {
                env::remove_var(key);
            }
        }
    }
}

fn run_args(workspace: &Path, control_file: Option<PathBuf>) -> RunArgs {
    RunArgs {
        path: workspace.to_path_buf(),
        max_iterations: 1,
        max_time: 60,
        evaluator_cmd: Some("echo 'eval'".to_string()),
        advisor_cmd: Some("echo 'adv'".to_string()),
        executor_cmd: Some("echo 'exec'".to_string()),
        evaluator_status_file: workspace.join("artifacts/evaluator_status.md"),
        advisor_recommendations_file: workspace.join("artifacts/advisor_recommendations.md"),
        executor_log_file: workspace.join("artifacts/executor_log.md"),
        tool_timeout_seconds: 30,
        evaluator_timeout: Some(5),
        advisor_timeout: Some(5),
        executor_timeout: Some(5),
        verbose: false,
        config: None,
        goal: None,
        goal_file: None,
        control_file,
        feedback: None,
    }
}

fn step_args(workspace: &Path) -> StepArgs {
    StepArgs {
        path: workspace.to_path_buf(),
        execution_id: None,
        verbose: false,
    }
}

fn create_monitor_conf(workspace: &Path, content: &str) {
    let configs_dir = workspace.join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).expect("configs dir");
    fs::write(configs_dir.join("monitor.conf"), content).expect("write config");
}

fn default_keys() -> Vec<&'static str> {
    vec![
        "NEWTON_AILOOP_INTEGRATION",
        "NEWTON_AILOOP_HTTP_URL",
        "NEWTON_AILOOP_WS_URL",
        "NEWTON_AILOOP_CHANNEL",
        "NEWTON_AILOOP_FAIL_FAST",
    ]
}

fn assert_option_disabled(result: newton::Result<Option<AiloopContext>>, message: &str) {
    let value = result.unwrap();
    assert!(
        value.is_none(),
        "{}: expected init_context to be disabled",
        message
    );
}

#[test]
#[serial]
fn test_init_context_no_config_disables_by_default() {
    let _guard = EnvGuard::clear(&default_keys());
    let workspace = TempDir::new().unwrap();
    let command = Command::Run(run_args(workspace.path(), None));

    let result = init_context(workspace.path(), &command);
    assert_option_disabled(result, "No config should disable ailoop");
}

#[test]
#[serial]
fn test_init_context_integration_disabled_env() {
    let _guard = EnvGuard::set(&[("NEWTON_AILOOP_INTEGRATION", "0")]);
    let workspace = TempDir::new().unwrap();
    create_monitor_conf(
        workspace.path(),
        "ailoop_server_http_url = http://localhost:8080\nailoop_server_ws_url = ws://localhost:8080\nailoop_channel = file-channel\n",
    );
    let command = Command::Run(run_args(workspace.path(), None));

    let result = init_context(workspace.path(), &command);
    assert_option_disabled(result, "Integration disabled by env should return None");
}

#[test]
#[serial]
fn test_init_context_non_run_command_disabled() {
    let _guard = EnvGuard::clear(&default_keys());
    let workspace = TempDir::new().unwrap();
    let command = Command::Step(step_args(workspace.path()));

    let result = init_context(workspace.path(), &command);
    assert_option_disabled(result, "Non-run/batch command should skip ailoop");
}

#[test]
#[serial]
fn test_init_context_enabled_without_configs_errors() {
    let _guard = EnvGuard::set(&[("NEWTON_AILOOP_INTEGRATION", "1")]);
    let workspace = TempDir::new().unwrap();
    let command = Command::Run(run_args(workspace.path(), None));

    let result = init_context(workspace.path(), &command);
    assert!(
        result.is_err(),
        "Expected error when enabled but no configs"
    );
}

#[test]
#[serial]
fn test_init_context_partial_http_env_ws_file_merges() {
    let _guard1 = EnvGuard::clear(&default_keys());
    let _guard2 = EnvGuard::set(&[
        ("NEWTON_AILOOP_INTEGRATION", "1"),
        ("NEWTON_AILOOP_HTTP_URL", "http://env:8080"),
        ("NEWTON_AILOOP_CHANNEL", "env-channel"),
    ]);
    let workspace = TempDir::new().unwrap();
    create_monitor_conf(workspace.path(), "ailoop_server_ws_url = ws://file:8080\n");
    let command = Command::Run(run_args(workspace.path(), None));

    let context = init_context(workspace.path(), &command)
        .expect("Context should load")
        .expect("Context should be Some");

    assert_eq!(context.config.http_url.as_str(), "http://env:8080/");
    assert_eq!(context.config.ws_url.as_str(), "ws://file:8080/");
    assert_eq!(context.config.channel, "env-channel");
}

#[test]
#[serial]
fn test_init_context_partial_ws_env_http_file_merges() {
    let _guard1 = EnvGuard::clear(&default_keys());
    let _guard2 = EnvGuard::set(&[
        ("NEWTON_AILOOP_INTEGRATION", "1"),
        ("NEWTON_AILOOP_WS_URL", "ws://env:8080"),
        ("NEWTON_AILOOP_CHANNEL", "env-channel"),
    ]);
    let workspace = TempDir::new().unwrap();
    create_monitor_conf(
        workspace.path(),
        "ailoop_server_http_url = http://file:8080\n",
    );
    let command = Command::Run(run_args(workspace.path(), None));

    let context = init_context(workspace.path(), &command)
        .expect("Context should load")
        .expect("Context should be Some");

    assert_eq!(context.config.http_url.as_str(), "http://file:8080/");
    assert_eq!(context.config.ws_url.as_str(), "ws://env:8080/");
    assert_eq!(context.config.channel, "env-channel");
}

#[test]
#[serial]
fn test_init_context_default_channel_from_workspace_name() {
    let _guard1 = EnvGuard::clear(&default_keys());
    let _guard2 = EnvGuard::set(&[("NEWTON_AILOOP_INTEGRATION", "1")]);
    let temp_dir = TempDir::new().unwrap();
    let workspace_name = "workspace-name";
    let workspace = temp_dir.path().join(workspace_name);
    fs::create_dir_all(&workspace).unwrap();
    create_monitor_conf(
        &workspace,
        "ailoop_server_http_url = http://file:8080\nailoop_server_ws_url = ws://file:8080\n",
    );
    let command = Command::Run(run_args(&workspace, None));

    let context = init_context(&workspace, &command)
        .expect("Context should load")
        .expect("Context should be Some");

    assert_eq!(context.config.channel, workspace_name);
}

#[test]
#[serial]
fn test_init_context_fail_fast_env_all_env_path() {
    let _guard1 = EnvGuard::clear(&default_keys());
    let _guard2 = EnvGuard::set(&[
        ("NEWTON_AILOOP_INTEGRATION", "1"),
        ("NEWTON_AILOOP_HTTP_URL", "http://env:8080"),
        ("NEWTON_AILOOP_WS_URL", "ws://env:8080"),
        ("NEWTON_AILOOP_CHANNEL", "env-channel"),
        ("NEWTON_AILOOP_FAIL_FAST", "1"),
    ]);
    let workspace = TempDir::new().unwrap();
    let command = Command::Run(run_args(workspace.path(), None));

    let context = init_context(workspace.path(), &command)
        .expect("Context should load")
        .expect("Context should be Some");
    assert!(context.config.fail_fast, "Fail fast flag should be honored");
}
