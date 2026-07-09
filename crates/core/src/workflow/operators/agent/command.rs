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
use crate::workflow::subprocess::{prepare_command_for_group_kill, ProcessGroupKillGuard};
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

// `ProcessGroupKillGuard` used below (armed right after spawn in
// `spawn_engine_process`, disarmed right after a clean wait in
// `wait_for_process_completion`) now lives in `workflow::subprocess`, shared
// with `GitOperator`, `GhOperator`, and `CommandOperator`'s
// `run_guarded`-based runners. The agent operator keeps its own bespoke
// streaming flow (it must observe stdout line-by-line for signal matching
// while the child is still running, which `run_guarded`'s
// spawn-then-`wait_with_output` shape can't do) but reuses the same guard
// type and disarm-ordering discipline documented on
// `subprocess::ProcessGroupKillGuard`.

/// Result from a single engine execution.
pub(super) struct SingleExecResult {
    pub(super) signal: Option<String>,
    pub(super) signal_data: HashMap<String, String>,
    /// `None` when the child was killed after a signal match (no exit code
    /// to report); `Some(_)` on a genuine process exit.
    pub(super) exit_code: Option<i32>,
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
        ProcessGroupKillGuard,
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

    // Armed immediately after spawn, before any await point that could be
    // cancelled by the outer per-task timeout. See `ProcessGroupKillGuard`
    // docs for why this must be created here rather than deferred.
    let kill_guard =
        ProcessGroupKillGuard::new(child.id().expect("freshly spawned child must have a pid"));

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

    Ok((child, stdout, stderr_task, kill_guard))
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
///
/// Returns `None` when `exit_status.code()` is `None` — on unix that means
/// the process was terminated by a signal, which in this codebase happens
/// exactly when [`stream_and_process_output`] called `child.kill()` after a
/// configured signal matched. The caller must not paper over that with a
/// fabricated exit code (e.g. `-1`): a signal-terminated process genuinely
/// has no exit code, and a workflow gate on `exit_code == 0` must not see a
/// successful signal-stop as a failure. See spec 074 decision 4 / B9.
///
/// Disarms `kill_guard` IMMEDIATELY after a successful `child.wait()` —
/// before awaiting `stderr_task`. `stderr_task.await` is itself an await
/// point; if it were reached while the guard was still armed, the *outer*
/// per-task timeout could drop this whole future there, and the guard's
/// `Drop` would `killpg()` a pgid whose leader was already reaped by the
/// `child.wait()` above. If the rest of the group had also already exited,
/// that pgid could have been recycled by the OS for an unrelated process
/// group, which the drop would then SIGKILL.
///
/// Trade-off accepted deliberately: disarming here means a future-drop
/// during the `stderr_task` await no longer group-kills any leftover
/// grandchildren of *this* child. That window is narrow (stderr draining is
/// just reading the pipe to EOF, which is fast once the process has already
/// exited) and is judged acceptable in exchange for eliminating the
/// kill-innocent-pgid risk described above.
///
/// On a failed `wait()` the child's process/group state is unknown, so the
/// guard is deliberately left armed — its `Drop` remains the safety net.
async fn wait_for_process_completion(
    mut child: tokio::process::Child,
    stderr_task: tokio::task::JoinHandle<()>,
    kill_guard: &mut ProcessGroupKillGuard,
) -> Result<Option<i32>, AppError> {
    let exit_status = child.wait().await.map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to wait for engine process: {err}"),
        )
    })?;

    // Clean wait: the direct child is confirmed reaped. Disarm before the
    // stderr await below so a drop during that await is a no-op.
    kill_guard.disarm();

    let _ = stderr_task.await;

    Ok(exit_status.code())
}

/// Execute a single engine invocation and stream output.
pub(super) async fn execute_single(params: &ExecParams<'_>) -> Result<SingleExecResult, AppError> {
    check_timeout_before_execution(params)?;

    let (mut child, stdout, stderr_task, mut kill_guard) = spawn_engine_process(params).await?;
    let mut stdout_file = super::artifacts::open_stdout_artifact_file(params.paths.stdout_path)?;

    // If either of the two calls below returns early (internal
    // `timeout_seconds` expiry, or this whole future getting dropped because
    // the outer per-task `timeout_ms` fired), `kill_guard` is still armed
    // and its `Drop` sends SIGKILL to the process group.
    let streaming_result =
        stream_and_process_output(stdout, &mut stdout_file, &mut child, params).await?;

    // `wait_for_process_completion` disarms `kill_guard` itself, right after
    // its internal `child.wait()` succeeds and before it awaits
    // `stderr_task` — see that function's doc comment for why the disarm
    // can't be deferred to here.
    let exit_code = wait_for_process_completion(child, stderr_task, &mut kill_guard).await?;

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
) -> Result<(Option<String>, HashMap<String, String>, Option<i32>, u32), AppError> {
    let max_iters = config.max_iterations.unwrap_or(u32::MAX);
    let mut iteration: u32 = 0;
    let mut last_exit_code: Option<i32>;
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

        // `result.exit_code` is only `None` when the child was killed on a
        // signal match, and that path already broke out above via
        // `result.signal`, so this comparison never observes `None`.
        if result.exit_code != Some(0) {
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

    // Belt-and-braces: reap the direct child if the `Child` handle itself is
    // dropped without an explicit kill/wait (`kill_on_drop(true)`) and make
    // it the leader of its own process group so grandchildren it spawns
    // (e.g. `sleep 300 &`) share a group we can kill as a unit via `killpg`.
    // `kill_on_drop` alone does NOT reach grandchildren — that's
    // `ProcessGroupKillGuard`'s job. Non-unix: no portable equivalent of
    // `process_group`; grandchildren gap is documented on
    // `subprocess::ProcessGroupKillGuard`.
    prepare_command_for_group_kill(&mut cmd);

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
    use serde_json::Value;
    use std::collections::HashMap;
    #[cfg(unix)]
    use std::path::Path;
    use std::time::{Duration, Instant};
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
                sink: None,
                pre_seed_nodes: true,
                state_dir: None,
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

    // ── B9: honest agent stop contract ───────────────────────────────────
    //
    // A configured-signal match kills the child (see
    // `stream_and_process_output`'s `child.kill().await` on match), so
    // `exit_status.code()` is `None` — there is no genuine exit code to
    // report. The operator output must say so explicitly (`stop_reason:
    // "signal_matched"`, `exit_code: null`) instead of fabricating a
    // sentinel like `-1` that a `exit_code == 0` gate would misread as
    // failure.

    #[tokio::test]
    async fn execute_signal_matched_sets_stop_reason_and_null_exit_code() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        // The script must still be alive when `child.kill()` fires after the
        // signal match, or the process may have already exited on its own
        // (racing the kill with a genuine `exit_code: 0`) and this test
        // would not actually exercise the kill path. `sleep` after the
        // signal line keeps the process running long enough to be killed.
        let params = json!({
            "engine": "command",
            "engine_command": [
                "bash", "-c",
                "echo '<promise>COMPLETE</promise>'; sleep 30"
            ],
            "signals": { "complete": "<promise>COMPLETE</promise>" }
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("complete"));
        assert_eq!(result["stop_reason"], json!("signal_matched"));
        assert_eq!(result["exit_code"], Value::Null);
    }

    #[tokio::test]
    async fn execute_clean_exit_sets_stop_reason_exited_with_numeric_exit_code() {
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
        assert_eq!(result["stop_reason"], json!("exited"));
        assert_eq!(result["exit_code"], json!(0));
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

    #[tokio::test]
    async fn execute_require_signal_true_fails_with_no_signal() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo hello"],
            "signals": { "complete": "<status>COMPLETED</status>" },
            "require_signal": true
        });
        let err = op.execute(params, ctx).await.unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-009");
    }

    #[tokio::test]
    async fn execute_require_signal_false_returns_null_on_no_match() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo hello"],
            "signals": { "complete": "<status>COMPLETED</status>" }
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], Value::Null);
    }

    #[tokio::test]
    async fn execute_require_signal_with_matching_signal_succeeds() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo '<status>COMPLETED</status>'"],
            "signals": { "complete": "<status>COMPLETED</status>" },
            "require_signal": true
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("complete"));
    }

    #[tokio::test]
    async fn execute_require_signal_includes_context_keys() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["bash", "-c", "echo hello"],
            "signals": { "valid": "<status>VALID</status>" },
            "require_signal": true,
            "model": "test-model"
        });
        let err = op.execute(params, ctx).await.unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-009");
        assert!(err.context.contains_key("stdout_artifact"));
        assert!(err.context.contains_key("engine"));
        assert!(err.context.contains_key("model"));
    }

    // ── B7: process-tree cleanup on timeout ──────────────────────────────
    //
    // Fixture: a shell script that backgrounds a `sleep 300` grandchild
    // (writing its pid to a file so the test can find it), then loops
    // forever appending a byte to a "heartbeat" file — the direct child's
    // portable "still alive" signal. The script never exits on its own; each
    // scenario below relies entirely on the kill-guard mechanism under test
    // to end it.
    //
    // Two convergent kill paths are exercised separately, per the spec:
    //   (a) the outer per-task `timeout_ms`, which drops the operator future
    //       without ever calling `Child::kill` (`task_execution.rs:247`);
    //   (b) the operator-internal `timeout_seconds`, which does call
    //       `Child::kill` on the direct child only (`command.rs:215`).
    // Both must leave the direct child AND the grandchild dead.

    #[cfg(unix)]
    fn grandchild_leak_script(heartbeat: &Path, grandchild_pid_file: &Path) -> String {
        format!(
            r#"( sleep 300 & echo $! > "{grandchild_pid}" )
while true; do printf x >> "{heartbeat}"; sleep 0.02; done"#,
            grandchild_pid = grandchild_pid_file.display(),
            heartbeat = heartbeat.display(),
        )
    }

    /// Polls `path`'s size until it stops growing for a full `quiet` window
    /// (bounded by `max_wait`). This is the direct-child-dead check: it uses
    /// only filesystem polling, no OS process APIs, so it is portable to any
    /// platform even though the fixture script above is unix-only.
    #[cfg(unix)]
    async fn wait_for_heartbeat_to_stop(path: &Path, quiet: Duration, max_wait: Duration) -> bool {
        let poll = Duration::from_millis(20);
        let deadline = Instant::now() + max_wait;
        let mut last_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let mut quiet_since = Instant::now();
        loop {
            tokio::time::sleep(poll).await;
            let size = std::fs::metadata(path)
                .map(|m| m.len())
                .unwrap_or(last_size);
            if size != last_size {
                last_size = size;
                quiet_since = Instant::now();
            } else if quiet_since.elapsed() >= quiet {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
        }
    }

    /// Polls for `path` to exist and be non-empty, bounded by `max_wait`.
    #[cfg(unix)]
    async fn wait_for_file_nonempty(path: &Path, max_wait: Duration) -> bool {
        let poll = Duration::from_millis(20);
        let deadline = Instant::now() + max_wait;
        loop {
            if std::fs::metadata(path)
                .map(|m| m.len() > 0)
                .unwrap_or(false)
            {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(poll).await;
        }
    }

    #[cfg(unix)]
    fn read_pid_file(path: &Path) -> libc::pid_t {
        std::fs::read_to_string(path)
            .expect("read pid file")
            .trim()
            .parse()
            .expect("pid file contains a valid pid")
    }

    /// Grandchild-dead assertion: unix-only, per spec, since it relies on
    /// `kill(pid, 0)` (a pure liveness probe — no signal is actually
    /// delivered) to determine whether the process-group kill reached the
    /// grandchild too.
    #[cfg(unix)]
    async fn wait_for_pid_death(pid: libc::pid_t, max_wait: Duration) -> bool {
        let poll = Duration::from_millis(20);
        let deadline = Instant::now() + max_wait;
        loop {
            // SAFETY: signal 0 touches no memory; it only probes whether the
            // pid exists and is signalable by us.
            let alive = unsafe { libc::kill(pid, 0) == 0 };
            if !alive {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(poll).await;
        }
    }

    /// Scenario (b): a small operator-internal `timeout_seconds` triggers the
    /// existing direct-child `Child::kill` (command.rs:215-221) AND, via the
    /// kill guard converging on the same `Drop`, a `killpg` that reaches the
    /// grandchild too.
    #[cfg(unix)]
    #[tokio::test]
    async fn execute_agent_timeout_seconds_kills_process_group() {
        let tmp = TempDir::new().unwrap();
        let heartbeat = tmp.path().join("heartbeat");
        let grandchild_pid_file = tmp.path().join("grandchild.pid");
        let script = grandchild_leak_script(&heartbeat, &grandchild_pid_file);

        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["sh", "-c", script],
            "timeout_seconds": 1,
        });

        let err = op.execute(params, ctx).await.unwrap_err();
        assert_eq!(err.code, "WFG-AGENT-005");

        assert!(
            wait_for_file_nonempty(&grandchild_pid_file, Duration::from_secs(2)).await,
            "grandchild pid file was never written"
        );
        let grandchild_pid = read_pid_file(&grandchild_pid_file);

        assert!(
            wait_for_heartbeat_to_stop(
                &heartbeat,
                Duration::from_millis(200),
                Duration::from_secs(3)
            )
            .await,
            "direct child kept writing to heartbeat after internal timeout; not killed"
        );
        assert!(
            wait_for_pid_death(grandchild_pid, Duration::from_secs(3)).await,
            "grandchild process survived the process-group kill (internal timeout path)"
        );
    }

    /// Fast-success companion to the timeout scenarios: proves that arming
    /// (and disarming) the kill guard on a normal completion has zero effect
    /// on the operator's success behavior — a regression here would mean
    /// dropping the guard after a clean wait still kills something.
    #[cfg(unix)]
    #[tokio::test]
    async fn execute_fast_success_unaffected_by_kill_guard() {
        let tmp = TempDir::new().unwrap();
        let settings = WorkflowSettings::default();
        let op = AgentOperator::with_default_registry(tmp.path().to_path_buf(), settings);
        let ctx = make_ctx(&tmp);
        let params = json!({
            "engine": "command",
            "engine_command": ["sh", "-c", "echo ok"],
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["signal"], json!("exited"));
        assert_eq!(result["exit_code"], json!(0));
    }

    /// Scenario (a): a small task-level `timeout_ms` makes
    /// `task_execution::execute_with_timeout` drop the whole operator future
    /// without ever calling `Child::kill` — the outer future-drop path. The
    /// kill guard living inside that dropped future's stack must still reach
    /// the process group.
    #[cfg(unix)]
    #[tokio::test]
    async fn task_level_timeout_ms_kills_process_group_via_future_drop() {
        use crate::workflow::executor::ExecutionOverrides;
        use crate::workflow::expression::ExpressionEngine;
        use crate::workflow::operators::register_builtins;
        use crate::workflow::schema::WorkflowTask;
        use crate::workflow::task_execution::run_task;
        use std::sync::Arc;

        let tmp = TempDir::new().unwrap();
        let heartbeat = tmp.path().join("heartbeat");
        let grandchild_pid_file = tmp.path().join("grandchild.pid");
        let script = grandchild_leak_script(&heartbeat, &grandchild_pid_file);

        // Default max_time_seconds (3600) is far above timeout_ms below, so
        // the operator's own internal timeout cannot be the thing that fires
        // first — only the outer task-level timeout can.
        let settings = WorkflowSettings::default();
        let mut builder = OperatorRegistry::builder();
        register_builtins(&mut builder, tmp.path().to_path_buf(), settings);
        let registry = builder.build();

        let task: WorkflowTask = serde_json::from_value(json!({
            "id": "leaky",
            "operator": "AgentOperator",
            "params": {
                "engine": "command",
                "engine_command": ["sh", "-c", script],
            },
            "name": null,
            "classes": [],
            "timeout_ms": 200,
            "retry": null,
            "max_iterations": null,
            "parallel_group": null,
            "transitions": [],
            "goal_gate": false,
            "terminal": null
        }))
        .expect("construct WorkflowTask");

        let outcome = run_task(
            task,
            registry,
            Arc::new(ExpressionEngine::default()),
            tmp.path().to_path_buf(),
            StateView::new(json!({}), json!({}), json!({})),
            "test-exec-b7".to_string(),
            1,
            Arc::new(Vec::new()),
            GraphHandle::new(HashMap::new()),
            tmp.path().join("workflow.yaml"),
            0,
            ExecutionOverrides {
                parallel_limit: None,
                max_time_seconds: None,
                checkpoint_base_path: None,
                artifact_base_path: None,
                max_nesting_depth: None,
                verbose: false,
                sink: None,
                pre_seed_nodes: true,
                state_dir: None,
            },
        )
        .await
        .expect("run_task itself must not error");

        assert!(
            outcome.failed,
            "expected timed-out task to be marked failed"
        );
        assert_eq!(outcome.record.error_code.as_deref(), Some("WFG-TIME-002"));

        assert!(
            wait_for_file_nonempty(&grandchild_pid_file, Duration::from_secs(2)).await,
            "grandchild pid file was never written"
        );
        let grandchild_pid = read_pid_file(&grandchild_pid_file);

        assert!(
            wait_for_heartbeat_to_stop(
                &heartbeat,
                Duration::from_millis(200),
                Duration::from_secs(3)
            )
            .await,
            "direct child kept writing to heartbeat after outer timeout dropped the future"
        );
        assert!(
            wait_for_pid_death(grandchild_pid, Duration::from_secs(3)).await,
            "grandchild process survived the process-group kill (future-drop path)"
        );
    }
}
