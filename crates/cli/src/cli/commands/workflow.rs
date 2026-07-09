#![allow(clippy::result_large_err)]

use crate::cli::args::{
    DotArgs, ExplainArgs, LintArgs, OutputFormat, ResumeArgs, RunArgs, ValidateArgs,
};
use crate::cli::exit::CliExit;
use crate::cli::workspace_paths::{resolve_state_dir, state_checkpoints_dir};
use newton_core::core::error::AppError;
use newton_core::core::types::ErrorCategory;
use newton_core::workflow::io::{CompletionEnvelope, CompletionError};
use newton_core::workflow::{
    checkpoint, dot as workflow_dot,
    executor::{self as workflow_executor},
    explain,
    expression::ExpressionEngine,
    lint::{LintRegistry, LintSeverity},
    schema as workflow_schema, transform as workflow_transform,
};
use serde_json::Value;
use std::{fs, result::Result as StdResult};

/// Emits the completion envelope, then either exits (via the returned error,
/// mapped to `std::process::exit` only in `main.rs`) or returns the
/// underlying `AppError` for normal (non `--emit-completion-json`) dispatch.
///
/// The envelope is always printed to stdout *before* the error is
/// constructed, so a served invocation (MCP/chat) that turns the `Err` into
/// an error frame instead of exiting still gets the same stdout envelope a
/// direct CLI invocation would have seen before exiting.
fn emit_or_return(
    emit_json: bool,
    envelope: CompletionEnvelope,
    err: AppError,
    exit_code: i32,
) -> anyhow::Result<()> {
    if emit_json {
        println!("{}", serde_json::to_string(&envelope).unwrap_or_default());
        return Err(CliExit::new(exit_code, err.to_string()).into());
    }
    Err(err.into())
}

async fn execute_run_command(args: &RunArgs) -> anyhow::Result<()> {
    let emit_json = args.emit_completion_json;
    let workflow_path = args.workflow.clone();
    let workspace = super::resolve_workflow_workspace(args.workspace.clone())?;
    let state_dir = resolve_state_dir(&workspace, args.state_dir.as_deref());
    let (mut document, lint_results) =
        newton_core::workflow::loader::load_and_lint_workflow(&workflow_path)?;
    if !lint_results.is_empty() {
        super::print_lint_results_text(&lint_results)?;
    }
    super::apply_context_overrides(&mut document.workflow.context, &args.context);
    document.validate(&ExpressionEngine::default())?;

    if let Some(payload) = super::build_trigger_payload(&args.parameters_json, &args.trigger)? {
        document.triggers = Some(workflow_schema::WorkflowTrigger::manual(payload));
    }

    {
        let settings = &document.workflow.settings;
        let empty_payload = serde_json::json!({});
        let payload = document
            .triggers
            .as_ref()
            .map(|t| &t.payload)
            .unwrap_or(&empty_payload);
        if let Some(max_bytes) = settings.io_settings.max_input_bytes {
            let serialized = serde_json::to_string(payload).unwrap_or_default();
            if serialized.len() > max_bytes {
                let err = AppError::new(
                    ErrorCategory::ValidationError,
                    format!("trigger payload exceeds max_input_bytes ({})", max_bytes),
                )
                .with_code("WFG-IO-001");
                let envelope = CompletionEnvelope::internal_error(CompletionError {
                    code: Some("WFG-IO-001".to_string()),
                    category: ErrorCategory::ValidationError.to_string(),
                    message: err.message.clone(),
                    error_payload: None,
                });
                return emit_or_return(emit_json, envelope, err, 1);
            }
        }
        if let Some(schema) = &settings.io.input_schema {
            if let Err(e) = newton_core::workflow::io::validate_input_schema(schema, payload) {
                let envelope = CompletionEnvelope::internal_error(CompletionError {
                    code: Some(e.code.clone()),
                    category: e.category.to_string(),
                    message: e.message.clone(),
                    error_payload: None,
                });
                return emit_or_return(emit_json, envelope, e, 1);
            }
        }
    }
    let io_settings = document.workflow.settings.io_settings.clone();
    let io_block = document.workflow.settings.io.clone();

    let exec_setup = super::shared_execution::build_execution_setup(
        state_dir.clone(),
        args.parallel_limit,
        args.timeout_seconds,
        args.server.as_deref(),
    )
    .await?;

    let settings = document.workflow.settings.clone();
    let ailoop_ctx =
        newton_core::integrations::ailoop::init_context_for_command_name(&workspace, "run")
            .ok()
            .flatten();
    let registry =
        super::build_operator_registry(workspace.clone(), &state_dir, &settings, ailoop_ctx).await;

    let summary_result = workflow_executor::execute_workflow(
        document,
        workflow_path,
        registry,
        workspace.clone(),
        exec_setup.overrides,
    )
    .await;

    match summary_result {
        Ok(summary) => {
            if let (Some(schema), Some(ref result_val)) = (&io_block.output_schema, &summary.result)
            {
                use newton_core::workflow::io::validate_output_schema;
                if let Err(e) = validate_output_schema(schema, result_val) {
                    let err = AppError::new(ErrorCategory::ValidationError, e.message.clone());
                    let envelope = CompletionEnvelope::failure(
                        Some(summary.execution_id),
                        CompletionError {
                            code: Some("WFG-IO-003".to_string()),
                            category: "ValidationError".to_string(),
                            message: e.message.clone(),
                            error_payload: None,
                        },
                    );
                    return emit_or_return(emit_json, envelope, err, 2);
                }
            }
            if let (Some(max_bytes), Some(ref result_val)) =
                (io_settings.max_output_bytes, &summary.result)
            {
                let serialized = serde_json::to_string(result_val).unwrap_or_default();
                if serialized.len() > max_bytes {
                    let err = AppError::new(
                        ErrorCategory::ValidationError,
                        "output exceeds max_output_bytes: WFG-IO-003".to_string(),
                    );
                    let envelope = CompletionEnvelope::failure(
                        Some(summary.execution_id),
                        CompletionError {
                            code: Some("WFG-IO-003".to_string()),
                            category: "ValidationError".to_string(),
                            message: "output exceeds max_output_bytes".to_string(),
                            error_payload: None,
                        },
                    );
                    return emit_or_return(emit_json, envelope, err, 2);
                }
            }
            if emit_json {
                let envelope = newton_core::workflow::io::CompletionEnvelope::success(
                    summary.execution_id,
                    summary.result.clone(),
                );
                println!("{}", serde_json::to_string(&envelope).unwrap_or_default());
            } else {
                println!(
                    "Workflow completed in {} iterations",
                    summary.total_iterations
                );
            }
            Ok(())
        }
        Err(app_error) => {
            if emit_json {
                let is_workflow_failure = matches!(
                    app_error.code.as_str(),
                    "WFG-EXEC-001"
                        | "WFG-GATE-001"
                        | "WFG-ITER-001"
                        | "WFG-ITER-002"
                        | "WFG-TIME-001"
                );
                let envelope = if is_workflow_failure {
                    newton_core::workflow::io::CompletionEnvelope::failure(
                        None,
                        newton_core::workflow::io::CompletionError {
                            code: Some(app_error.code.clone()),
                            category: app_error.category.to_string(),
                            message: app_error.message.clone(),
                            error_payload: None,
                        },
                    )
                } else {
                    newton_core::workflow::io::CompletionEnvelope::internal_error(
                        newton_core::workflow::io::CompletionError {
                            code: Some(app_error.code.clone()),
                            category: app_error.category.to_string(),
                            message: app_error.message.clone(),
                            error_payload: None,
                        },
                    )
                };
                println!("{}", serde_json::to_string(&envelope).unwrap_or_default());
                let exit_code = if is_workflow_failure { 2 } else { 1 };
                return Err(CliExit::new(exit_code, app_error.to_string()).into());
            }
            Err(app_error.into())
        }
    }
}

pub async fn workflow_run(args: RunArgs) -> anyhow::Result<()> {
    execute_run_command(&args).await
}

pub fn validate(args: ValidateArgs) -> StdResult<(), AppError> {
    let workflow_path = args.workflow.clone();
    let document = workflow_schema::load_workflow(&workflow_path)?;
    let unreachable = workflow_dot::reachability_warnings(&document);
    for id in &unreachable {
        eprintln!("warning: task '{id}' is not reachable from entry_task");
    }
    println!("Workflow definition is valid");
    Ok(())
}

pub fn dot(args: DotArgs) -> StdResult<(), AppError> {
    let workflow_path = args.workflow.clone();
    let document = workflow_schema::load_workflow(&workflow_path)?;
    let dot = workflow_dot::workflow_to_dot(&document);
    if let Some(path) = args.output {
        fs::write(path, dot).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to write DOT: {err}"),
            )
        })?;
    } else {
        println!("{dot}");
    }
    Ok(())
}

pub fn lint(args: LintArgs) -> StdResult<(), AppError> {
    let workflow_path = args.workflow.clone();
    let raw_document = workflow_schema::parse_workflow(&workflow_path)?;
    let document = workflow_transform::apply_default_pipeline(raw_document)?;
    let results = LintRegistry::new().run(&document);
    match args.format {
        OutputFormat::Json => super::print_lint_results_json(&results)?,
        OutputFormat::Text => {
            if results.is_empty() {
                println!("No lint issues");
            } else {
                super::print_lint_results_text(&results)?;
            }
        }
        OutputFormat::Prose => {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "prose format is not supported for lint command; use text or json",
            ));
        }
    }
    let error_count = results
        .iter()
        .filter(|result| result.severity == LintSeverity::Error)
        .count();
    if error_count > 0 {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!("workflow lint found {error_count} error(s)"),
        ));
    }
    Ok(())
}

pub fn explain(args: ExplainArgs) -> StdResult<(), AppError> {
    let workflow_path = args.workflow.clone();
    let _workspace = super::resolve_workflow_workspace(args.workspace)?;
    let raw_document = workflow_schema::parse_workflow(&workflow_path)?;
    let source_tasks = raw_document.workflow.tasks.len();
    let source_macro_invocations = raw_document.workflow.macro_invocation_count();
    let source_macro_names = raw_document.workflow.macro_names_referenced();
    let mut document = workflow_transform::apply_default_pipeline(raw_document)?;
    let overrides = super::parse_set_overrides(&args.context);
    let trigger_payload = super::build_trigger_payload(&args.parameters_json, &args.trigger)?
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    if !trigger_payload.is_null() {
        document.triggers = Some(workflow_schema::WorkflowTrigger::manual(
            trigger_payload.clone(),
        ));
    }
    let outcome = explain::build_explain_outcome(&document, &overrides, &trigger_payload)?;
    match args.format {
        OutputFormat::Json => super::print_explain_json(&outcome.output)?,
        OutputFormat::Text => super::print_explain_text(
            &outcome.output,
            Some((
                source_tasks,
                source_macro_invocations,
                source_macro_names.clone(),
            )),
        )?,
        OutputFormat::Prose => super::print_explain_prose(&outcome.output)?,
    }
    for diagnostic in &outcome.diagnostics {
        if let Some(location) = &diagnostic.location {
            eprintln!("explain diagnostic ({}): {}", location, diagnostic.message);
        } else {
            eprintln!("explain diagnostic: {}", diagnostic.message);
        }
    }
    if outcome.has_blocking_diagnostics() {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "workflow explain found blocking expression diagnostics",
        ));
    }
    Ok(())
}

pub async fn resume(args: ResumeArgs) -> StdResult<(), AppError> {
    let workspace = super::resolve_workflow_workspace(args.workspace)?;
    let state_dir = resolve_state_dir(&workspace, args.state_dir.as_deref());
    let execution =
        checkpoint::load_execution_from_base(&state_checkpoints_dir(&state_dir), &args.run_id)?;
    let settings = execution.settings_effective.clone();
    let registry =
        super::build_operator_registry(workspace.clone(), &state_dir, &settings, None).await;
    let summary = workflow_executor::resume_workflow(
        registry,
        workspace.clone(),
        args.run_id,
        args.allow_workflow_change,
    )
    .await?;
    println!(
        "Workflow resumed (execution {}) in {} iterations",
        summary.execution_id, summary.total_iterations
    );
    Ok(())
}

/// In-process (no subprocess) coverage of `emit_or_return`'s two branches
/// (spec 074, PR-1 / B3): non-`--emit-completion-json` invocations return a
/// plain `Err`, not a `CliExit`; `--emit-completion-json` on an actual
/// workflow-execution failure returns a `CliExit` with exit code 2. Calls
/// `workflow_run` directly rather than spawning `newton` — mirrors the seam
/// `mcp_data_malformed_call_no_exit.rs` and `data.rs`'s own in-crate tests
/// use for the same "handler no longer calls `std::process::exit`" family of
/// coverage. `test_e2e_io_contract.rs` (assert_cmd, subprocess) already pins
/// the `--emit-completion-json` exit codes end-to-end; these are the
/// same-crate complement for the `emit_json == false` branch that no
/// existing `--emit-completion-json`-named test exercises.
#[cfg(test)]
mod emit_or_return_tests {
    use super::*;

    const MAX_INPUT_BYTES_YAML: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 1
    max_workflow_iterations: 5
    io_settings:
      max_input_bytes: 1
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      terminal: success
"#;

    const FAILING_TASK_YAML: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: fail
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 1
    max_workflow_iterations: 5
    command_operator:
      allow_shell: true
  tasks:
    - id: fail
      operator: CommandOperator
      params:
        cmd: "false"
        shell: true
      terminal: success
"#;

    fn base_run_args(workflow: std::path::PathBuf, workspace: std::path::PathBuf) -> RunArgs {
        RunArgs {
            workflow,
            input_file: None,
            workspace: Some(workspace),
            trigger: vec![],
            context: vec![],
            parameters_json: None,
            emit_completion_json: false,
            parallel_limit: None,
            timeout_seconds: None,
            verbose: false,
            server: None,
            state_dir: None,
        }
    }

    /// Line 40 (`Err(err.into())`): without `--emit-completion-json`, a
    /// `max_input_bytes` violation must surface as a plain error, NOT a
    /// `CliExit` — only a direct-CLI-with-the-flag invocation gets the
    /// stdout-envelope-then-CliExit treatment.
    #[tokio::test]
    async fn without_emit_json_max_input_bytes_violation_is_a_plain_error() {
        let ws = tempfile::tempdir().expect("tempdir");
        let wf_path = ws.path().join("wf.yaml");
        std::fs::write(&wf_path, MAX_INPUT_BYTES_YAML).expect("write workflow");
        let params_file = ws.path().join("params.json");
        std::fs::write(&params_file, r#"{"repo":"my-repo"}"#).expect("write params");

        let mut args = base_run_args(wf_path, ws.path().to_path_buf());
        args.parameters_json = Some(params_file);
        args.emit_completion_json = false;

        let err = workflow_run(args)
            .await
            .expect_err("payload exceeding max_input_bytes must fail");
        assert!(
            err.downcast_ref::<CliExit>().is_none(),
            "emit_json=false must not produce a CliExit; got: {err:?}"
        );
        assert!(err.to_string().contains("max_input_bytes"), "err={err}");
    }

    /// Line 211 (`return Err(CliExit::new(exit_code, ...))`): with
    /// `--emit-completion-json`, an actual workflow execution failure
    /// (WFG-EXEC-001, a task failing with `continue_on_error: false`) must
    /// surface as a `CliExit` with exit code 2 (the `is_workflow_failure`
    /// branch), after printing the JSON envelope to stdout.
    #[tokio::test]
    async fn emit_json_workflow_execution_failure_returns_cli_exit_code_2() {
        let ws = tempfile::tempdir().expect("tempdir");
        let wf_path = ws.path().join("wf.yaml");
        std::fs::write(&wf_path, FAILING_TASK_YAML).expect("write workflow");

        let mut args = base_run_args(wf_path, ws.path().to_path_buf());
        args.emit_completion_json = true;

        let err = workflow_run(args)
            .await
            .expect_err("a failing task must fail the run");
        let exit = err
            .downcast::<CliExit>()
            .unwrap_or_else(|e| panic!("expected a CliExit, got: {e}"));
        assert_eq!(exit.code, 2, "WFG-EXEC-001 is a workflow failure (exit 2)");
    }
}
