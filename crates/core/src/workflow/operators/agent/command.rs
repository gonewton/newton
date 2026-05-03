//! Subprocess engine path for AgentOperator.
//!
//! NOTE: this is the agent submodule (`super::command`), distinct from the sibling
//! `crates/core/src/workflow/operators/command.rs` which holds `CommandOperator`.

use super::config::AgentOperatorConfig;
use super::signals::match_signals;
use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::expression::{EvaluationContext, ExpressionEngine};
use crate::workflow::operators::engine::{
    extract_text_from_stream_json, EngineInvocation, OutputFormat,
};
use indexmap::IndexMap;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

pub(super) const OUTPUT_CAPTURE_LIMIT_BYTES: usize = 1_048_576;
const CMD_LOG_ARG_MAX_LEN: usize = 200;

/// Result from a single engine execution.
pub(super) struct SingleExecResult {
    pub(super) signal: Option<String>,
    pub(super) signal_data: HashMap<String, String>,
    pub(super) exit_code: i32,
}

/// Bundled paths for an execution run.
pub(super) struct ExecPaths<'a> {
    pub(super) working_dir: &'a Path,
    pub(super) stdout_path: &'a Path,
    pub(super) stderr_path: &'a Path,
}

/// Execution parameters for agent operations.
pub(super) struct ExecParams<'a> {
    pub(super) invocation: &'a EngineInvocation,
    pub(super) compiled_signals: &'a IndexMap<String, Regex>,
    pub(super) paths: &'a ExecPaths<'a>,
    pub(super) extra_env: &'a HashMap<String, String>,
    pub(super) timeout: Duration,
    pub(super) start: Instant,
    pub(super) stream_to_terminal: bool,
}

/// Result of streaming stdout from the engine process.
struct StreamingResult {
    signal: Option<String>,
    signal_data: HashMap<String, String>,
}

/// Interpolate template expressions in env values.
pub(super) fn interpolate_env(
    env: &HashMap<String, String>,
    eval_ctx: &EvaluationContext,
) -> Result<HashMap<String, String>, AppError> {
    let engine = ExpressionEngine::default();
    let mut result = HashMap::new();
    for (k, v) in env {
        let interpolated = engine.interpolate_string(v, eval_ctx)?;
        result.insert(k.clone(), interpolated);
    }
    Ok(result)
}

/// Check if timeout has been exceeded before starting execution.
fn check_timeout_before_execution(params: &ExecParams<'_>) -> Result<(), AppError> {
    if params.start.elapsed() >= params.timeout {
        return Err(AppError::new(
            ErrorCategory::TimeoutError,
            "agent operator timeout exceeded before execution",
        )
        .with_code("WFG-AGENT-005"));
    }
    Ok(())
}

/// Spawn the engine process and set up stderr capture.
async fn spawn_engine_process(
    params: &ExecParams<'_>,
) -> Result<
    (
        tokio::process::Child,
        tokio::process::ChildStdout,
        tokio::task::JoinHandle<()>,
    ),
    AppError,
> {
    use tokio::io::{AsyncReadExt, BufReader};

    let cmd_display: Vec<String> = params
        .invocation
        .command
        .iter()
        .map(|a| {
            if a.len() > CMD_LOG_ARG_MAX_LEN {
                format!("{}... ({} chars)", &a[..CMD_LOG_ARG_MAX_LEN], a.len())
            } else {
                a.clone()
            }
        })
        .collect();
    tracing::debug!(
        cmd = ?cmd_display,
        cwd = ?params.paths.working_dir,
        "AgentOperator executing engine"
    );

    let mut cmd_builder = build_command(
        params.invocation,
        params.paths.working_dir,
        params.extra_env,
    )?;

    let mut child = cmd_builder.spawn().map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to start engine process: {err}"),
        )
        .with_code("WFG-AGENT-002")
    })?;

    let stdout = child.stdout.take().expect("stdout must be piped");
    let stderr = child.stderr.take().expect("stderr must be piped");

    let stderr_path_owned = params.paths.stderr_path.to_owned();
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf).await;
        if !buf.is_empty() {
            let limited = &buf[..buf.len().min(OUTPUT_CAPTURE_LIMIT_BYTES)];
            let _ = std::fs::write(&stderr_path_owned, limited);
        }
    });

    Ok((child, stdout, stderr_task))
}

/// Stream and process stdout from the engine process.
async fn stream_and_process_output(
    stdout: tokio::process::ChildStdout,
    stdout_file: &mut std::fs::File,
    child: &mut tokio::process::Child,
    params: &ExecParams<'_>,
) -> Result<StreamingResult, AppError> {
    use std::io::Write;
    use tokio::io::{AsyncWriteExt, BufReader};

    let mut stdout_bytes_written: usize = 0;
    let mut signal: Option<String> = None;
    let mut signal_data: HashMap<String, String> = HashMap::new();
    let output_format = params.invocation.output_format.clone();

    let mut lines = BufReader::new(stdout).lines();

    let remaining = params.timeout.saturating_sub(params.start.elapsed());
    let stream_result = tokio::time::timeout(remaining, async {
        while let Some(line_result) = lines.next_line().await.transpose() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };

            let text = line.trim_end_matches(['\n', '\r']).to_string();

            let text_for_matching = if output_format == OutputFormat::StreamJson {
                match extract_text_from_stream_json(&text) {
                    Some(t) => t,
                    None => {
                        if stdout_bytes_written + text.len() < OUTPUT_CAPTURE_LIMIT_BYTES {
                            let _ = stdout_file.write_all(text.as_bytes());
                            let _ = stdout_file.write_all(b"\n");
                            stdout_bytes_written += text.len() + 1;
                        }
                        continue;
                    }
                }
            } else {
                text.clone()
            };

            if stdout_bytes_written + text_for_matching.len() < OUTPUT_CAPTURE_LIMIT_BYTES {
                let _ = stdout_file.write_all(text_for_matching.as_bytes());
                let _ = stdout_file.write_all(b"\n");
                stdout_bytes_written += text_for_matching.len() + 1;
            }

            if params.stream_to_terminal {
                let mut terminal_stdout = tokio::io::stdout();
                let _ = terminal_stdout
                    .write_all(text_for_matching.as_bytes())
                    .await;
                let _ = terminal_stdout.write_all(b"\n").await;
                let _ = terminal_stdout.flush().await;
            }

            if let Some((sig_name, sig_data)) =
                match_signals(&text_for_matching, params.compiled_signals)
            {
                signal = Some(sig_name);
                signal_data = sig_data;
                let _ = child.kill().await;
                break;
            }
        }
    })
    .await;

    if stream_result.is_err() {
        let _ = child.kill().await;
        return Err(AppError::new(
            ErrorCategory::TimeoutError,
            "agent operator timeout exceeded during execution",
        )
        .with_code("WFG-AGENT-005"));
    }

    Ok(StreamingResult {
        signal,
        signal_data,
    })
}

/// Wait for the process to complete and return the exit code.
async fn wait_for_process_completion(
    mut child: tokio::process::Child,
    stderr_task: tokio::task::JoinHandle<()>,
) -> Result<i32, AppError> {
    let exit_status = child.wait().await.map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to wait for engine process: {err}"),
        )
    })?;

    let _ = stderr_task.await;

    Ok(exit_status.code().unwrap_or(-1))
}

/// Execute a single engine invocation and stream output.
pub(super) async fn execute_single(params: &ExecParams<'_>) -> Result<SingleExecResult, AppError> {
    check_timeout_before_execution(params)?;

    let (mut child, stdout, stderr_task) = spawn_engine_process(params).await?;
    let mut stdout_file = super::artifacts::open_stdout_artifact_file(params.paths.stdout_path)?;

    let streaming_result =
        stream_and_process_output(stdout, &mut stdout_file, &mut child, params).await?;

    let exit_code = wait_for_process_completion(child, stderr_task).await?;

    Ok(SingleExecResult {
        signal: streaming_result.signal,
        signal_data: streaming_result.signal_data,
        exit_code,
    })
}

/// Execute in loop mode.
pub(super) async fn execute_loop(
    config: &AgentOperatorConfig,
    params: &ExecParams<'_>,
) -> Result<(Option<String>, HashMap<String, String>, i32, u32), AppError> {
    let max_iters = config.max_iterations.unwrap_or(u32::MAX);
    let mut iteration: u32 = 0;
    let mut last_exit_code: i32;
    let mut last_signal: Option<String>;
    let mut last_signal_data: HashMap<String, String>;

    loop {
        iteration += 1;
        if iteration > max_iters {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("agent exceeded max_iterations ({max_iters}) in loop mode"),
            )
            .with_code("WFG-AGENT-003"));
        }

        last_signal = None;
        last_signal_data = HashMap::new();

        let result = execute_single(params).await?;

        last_exit_code = result.exit_code;

        if let Some(sig) = result.signal {
            last_signal = Some(sig);
            last_signal_data = result.signal_data;
            break;
        }

        if result.exit_code != 0 {
            break;
        }
    }

    Ok((last_signal, last_signal_data, last_exit_code, iteration))
}

/// Build a tokio Command from the invocation.
fn build_command(
    invocation: &EngineInvocation,
    working_dir: &Path,
    extra_env: &HashMap<String, String>,
) -> Result<Command, AppError> {
    if invocation.command.is_empty() {
        return Err(
            AppError::new(ErrorCategory::ValidationError, "engine command is empty")
                .with_code("WFG-AGENT-002"),
        );
    }

    let mut cmd = Command::new(&invocation.command[0]);
    if invocation.command.len() > 1 {
        cmd.args(&invocation.command[1..]);
    }

    cmd.current_dir(working_dir);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::null());

    for (k, v) in &invocation.env {
        cmd.env(k, v);
    }

    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use crate::workflow::executor::GraphHandle;
    use crate::workflow::operator::{ExecutionContext, Operator, OperatorRegistry, StateView};
    use crate::workflow::operators::agent::AgentOperator;
    use crate::workflow::schema::WorkflowSettings;
    use serde_json::json;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_ctx(workspace: &TempDir) -> ExecutionContext {
        ExecutionContext {
            workspace_path: workspace.path().to_path_buf(),
            execution_id: "test-exec-001".to_string(),
            task_id: "agent".to_string(),
            iteration: 1,
            state_view: StateView::new(json!({}), json!({}), json!({})),
            graph: GraphHandle::new(HashMap::new()),
            workflow_file: workspace.path().join("workflow.yaml"),
            nesting_depth: 0,
            execution_overrides: crate::workflow::executor::ExecutionOverrides {
                parallel_limit: None,
                max_time_seconds: None,
                checkpoint_base_path: None,
                artifact_base_path: None,
                max_nesting_depth: None,
                verbose: false,
                server_notifier: None,
                pre_seed_nodes: true,
            },
            operator_registry: OperatorRegistry::new(),
        }
    }

    fn make_settings_with_engine(engine: &str) -> WorkflowSettings {
        WorkflowSettings {
            default_engine: Some(engine.to_string()),
            ..WorkflowSettings::default()
        }
    }

    #[test]
    fn validate_params_rejects_invalid_regex() {
        let tmp = TempDir::new().unwrap();
        let settings = make_settings_with_engine("opencode");
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let params = json!({
            "engine": "command",
            "engine_command": ["echo", "hello"],
            "signals": { "bad": "[" }
        });
        let err = op.validate_params(&params).unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-004");
    }

    #[test]
    fn validate_params_rejects_newline_in_signal() {
        let tmp = TempDir::new().unwrap();
        let settings = make_settings_with_engine("opencode");
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let params = json!({
            "engine": "command",
            "engine_command": ["echo"],
            "signals": { "bad": "foo\nbar" }
        });
        let err = op.validate_params(&params).unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-004");
    }

    #[test]
    fn validate_params_rejects_command_without_engine_command() {
        let tmp = TempDir::new().unwrap();
        let settings = make_settings_with_engine("command");
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let params = json!({"engine": "command"});
        let err = op.validate_params(&params).unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-007");
    }

    #[tokio::test]
    async fn execute_no_engine_returns_agent_001() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({});
        let err = op.execute(params, ctx).await.unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-001");
    }

    #[tokio::test]
    async fn execute_non_runnable_ai_engine_returns_sdk_002() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings {
            default_engine: Some("copilot".to_string()),
            ..WorkflowSettings::default()
        };
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({ "prompt": "test prompt" });
        let err = op.execute(params, ctx).await.unwrap_err();
        assert_eq!(err.code, "WFG-SDK-002");
    }

    #[tokio::test]
    async fn execute_command_engine_no_engine_command_returns_agent_007() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({"engine": "command"});
        let err = op.execute(params, ctx).await.unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-007");
    }

    #[tokio::test]
    async fn execute_single_shot_captures_signal() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo '<promise>COMPLETE</promise>'"],
            "signals": { "complete": "<promise>COMPLETE</promise>" }
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("complete"));
    }

    #[tokio::test]
    async fn execute_captures_named_group_in_signal_data() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo '<promise>BLOCKED:cannot find file</promise>'"],
            "signals": { "blocked": "<promise>BLOCKED:(?P<reason>[^<]+)</promise>" }
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("blocked"));
        assert_eq!(result["signal_data"]["reason"], json!("cannot find file"));
    }

    #[tokio::test]
    async fn execute_no_signals_sets_exited() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo hello"]
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("exited"));
    }

    #[tokio::test]
    async fn execute_engine_resolution_from_settings() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings {
            default_engine: Some("command".to_string()),
            ..WorkflowSettings::default()
        };
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({ "engine_command": ["bash", "-c", "echo hi"] });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("exited"));
    }

    #[tokio::test]
    async fn execute_loop_mode_signals_on_second_iteration() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);

        let counter_file = tmp.path().join("counter.txt");
        std::fs::write(&counter_file, "0").unwrap();
        let script = format!(
            r#"COUNT=$(cat {0})
COUNT=$((COUNT + 1))
echo $COUNT > {0}
if [ "$COUNT" -ge 2 ]; then
  echo '<promise>COMPLETE</promise>'
fi"#,
            counter_file.display()
        );

        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", script],
            "loop": true,
            "max_iterations": 5,
            "signals": { "complete": "<promise>COMPLETE</promise>" }
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("complete"));
        assert_eq!(result["iteration"], json!(2));
    }

    #[tokio::test]
    async fn execute_loop_mode_exceeds_max_iterations_returns_agent_003() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo nothing"],
            "loop": true,
            "max_iterations": 2,
            "signals": { "complete": "<promise>COMPLETE</promise>" }
        });
        let err = op.execute(params, ctx).await.unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-003");
    }

    #[tokio::test]
    async fn execute_model_from_stylesheet() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings {
            model_stylesheet: Some(crate::workflow::schema::ModelStylesheet {
                model: "test-model".to_string(),
                context_fidelity: crate::workflow::schema::ContextFidelity::Summary,
            }),
            ..WorkflowSettings::default()
        };
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo hi"]
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("exited"));
    }

    #[tokio::test]
    async fn execute_stderr_artifact_set_when_stderr_produced() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo error >&2; echo hello"]
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert!(result["stderr_artifact"].is_string());
    }

    #[tokio::test]
    async fn execute_first_matching_signal_wins() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo '<promise>COMPLETE</promise>'"],
            "signals": {
                "aaa_complete": "<promise>COMPLETE</promise>",
                "bbb_any": "COMPLETE"
            }
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("aaa_complete"));
    }
}
