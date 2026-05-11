#![allow(clippy::result_large_err)] // CLI command handlers return AppError directly to preserve diagnostic context without boxing.

use crate::cli::args::{
    ArtifactArgs, ArtifactCommand, BatchArgs, CheckpointArgs, CheckpointCommand, DataArgs,
    DataVerb, DotArgs, ExplainArgs, KeyValuePair, LintArgs, OutputFormat, ResumeArgs, RunArgs,
    RunsArgs, RunsCommand, ServeArgs, ValidateArgs, WebhookArgs, WebhookCommand, WebhookServeArgs,
    WebhookStatusArgs,
};
use crate::Result;
use anyhow::anyhow;
use humantime::{format_duration, parse_duration};
use newton_backend::BackendStore;
use newton_core::core::batch_config::BatchProjectConfig;
use newton_core::core::error::AppError;
use newton_core::core::types::ErrorCategory;
use newton_core::workflow::checkpoint::WorkflowStatePaths;
use newton_core::workflow::operator::OperatorRegistry;
use newton_core::workflow::state::{
    OutputRef, WorkflowCheckpoint, WorkflowExecution, WorkflowTaskRunRecord, WorkflowTaskStatus,
};
use newton_core::workflow::{
    artifacts, checkpoint, dot as workflow_dot,
    executor::{self as workflow_executor, ExecutionOverrides},
    explain,
    expression::ExpressionEngine,
    lint::{LintRegistry, LintResult, LintSeverity},
    operators as workflow_operators, schema as workflow_schema,
    server_notifier::ServerNotifier,
    transform as workflow_transform, webhook,
};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
    result::Result as StdResult,
    sync::Arc,
    time::Duration,
};
use tokio::time::sleep;

/// Load and validate a workflow document from the given arguments
fn load_and_validate_workflow(
    args: &RunArgs,
) -> Result<(workflow_schema::WorkflowDocument, PathBuf)> {
    let workflow_path = args.workflow.clone();
    let raw_document = workflow_schema::parse_workflow(&workflow_path)?;
    let mut document = workflow_transform::apply_default_pipeline(raw_document)?;

    let lint_results = LintRegistry::new().run(&document);
    if !lint_results.is_empty() {
        print_lint_results_text(&lint_results);
    }
    let error_count = lint_results
        .iter()
        .filter(|result| result.severity == LintSeverity::Error)
        .count();
    if error_count > 0 {
        return Err(anyhow!(
            "workflow lint detected {error_count} error(s); fix before running"
        ));
    }

    apply_context_overrides(&mut document.workflow.context, &args.context);
    document.validate(&ExpressionEngine::default())?;

    Ok((document, workflow_path))
}

/// Build comprehensive trigger payload including input file and workspace context
fn build_comprehensive_trigger_payload(
    args: &RunArgs,
    workspace: &std::path::Path,
) -> Result<Option<Value>> {
    // Start with base trigger payload from args
    let mut trigger_payload =
        build_trigger_payload(&args.parameters_json, &args.trigger)?.unwrap_or_else(|| json!({}));

    // Add input_file to payload if provided
    if let Some(input_file) = &args.input_file {
        let input_file_path = if input_file.is_absolute() {
            input_file.clone()
        } else {
            std::env::current_dir()?.join(input_file)
        };
        trigger_payload["input_file"] = json!(input_file_path.display().to_string());
    }

    // Add workspace to payload
    trigger_payload["workspace"] = json!(workspace.display().to_string());

    // Only return payload if it contains data
    if trigger_payload.as_object().unwrap().is_empty() {
        Ok(None)
    } else {
        Ok(Some(trigger_payload))
    }
}

/// Create execution overrides and operator registry for workflow execution
fn setup_workflow_execution(
    args: &RunArgs,
    workspace: &std::path::Path,
    settings: &workflow_schema::WorkflowSettings,
) -> (ExecutionOverrides, OperatorRegistry) {
    let server_notifier = args
        .server
        .as_ref()
        .map(|url| std::sync::Arc::new(ServerNotifier::new(url.clone())));

    let overrides = ExecutionOverrides {
        parallel_limit: args.parallel_limit,
        max_time_seconds: args.timeout_seconds,
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: args.verbose,
        server_notifier,
        pre_seed_nodes: true,
    };

    let ailoop_ctx =
        newton_core::integrations::ailoop::init_context_for_command_name(workspace, "run")
            .ok()
            .flatten();
    let mut builder = OperatorRegistry::builder();
    let interviewer = newton_core::workflow::human::lazy_interviewer_provider(
        ailoop_ctx,
        Duration::from_secs(settings.human.default_timeout_seconds),
    );
    workflow_operators::register_builtins_with_deps(
        &mut builder,
        workspace.to_path_buf(),
        settings.clone(),
        workflow_operators::BuiltinOperatorDeps {
            interviewer: Some(interviewer),
            ..Default::default()
        },
    );
    let registry = builder.build();

    (overrides, registry)
}

/// Build additional environment variables for the run, returns (goal_file, env map)
pub async fn run(args: RunArgs) -> Result<()> {
    tracing::info!("Starting Newton workflow run");

    let workspace = resolve_workflow_workspace(args.workspace.clone())?;
    let (mut document, workflow_path) = load_and_validate_workflow(&args)?;

    if let Some(trigger_payload) = build_comprehensive_trigger_payload(&args, &workspace)? {
        document.triggers = Some(workflow_schema::WorkflowTrigger {
            trigger_type: workflow_schema::TriggerType::Manual,
            schema_version: "1".to_string(),
            payload: trigger_payload,
        });
    }

    let io_settings = document.workflow.settings.io_settings.clone();
    let io_block = document.workflow.settings.io.clone();
    let emit_json = args.emit_completion_json;

    // Input validation (size and schema) — must run before execute_workflow so that
    // pre-execution errors can still emit a JSON envelope when --emit-completion-json is set.
    if let Some(triggers) = &document.triggers {
        let payload = &triggers.payload;
        if let Some(max_bytes) = io_settings.max_input_bytes {
            let serialized = serde_json::to_string(payload).unwrap_or_default();
            if serialized.len() > max_bytes {
                let msg = format!(
                    "trigger payload exceeds max_input_bytes ({}): WFG-IO-001",
                    max_bytes
                );
                if emit_json {
                    let envelope = newton_core::workflow::io::CompletionEnvelope::internal_error(
                        newton_core::workflow::io::CompletionError {
                            code: Some("WFG-IO-001".to_string()),
                            category: "ValidationError".to_string(),
                            message: msg,
                            error_payload: None,
                        },
                    );
                    println!("{}", serde_json::to_string(&envelope).unwrap_or_default());
                    std::process::exit(1);
                }
                return Err(anyhow::anyhow!(
                    "trigger payload exceeds max_input_bytes ({}): WFG-IO-001",
                    max_bytes
                ));
            }
        }
        if let Some(schema) = &io_block.input_schema {
            use newton_core::workflow::io::validate_input_schema;
            if let Err(e) = validate_input_schema(schema, payload) {
                if emit_json {
                    let envelope = newton_core::workflow::io::CompletionEnvelope::internal_error(
                        newton_core::workflow::io::CompletionError {
                            code: Some(e.code.clone()),
                            category: e.category.to_string(),
                            message: e.message.clone(),
                            error_payload: None,
                        },
                    );
                    println!("{}", serde_json::to_string(&envelope).unwrap_or_default());
                    std::process::exit(1);
                }
                return Err(anyhow::Error::from(e));
            }
        }
    }

    let (overrides, registry) =
        setup_workflow_execution(&args, &workspace, &document.workflow.settings);

    let summary_result = workflow_executor::execute_workflow(
        document,
        workflow_path,
        registry,
        workspace.clone(),
        overrides,
    )
    .await;

    match summary_result {
        Ok(summary) => {
            // Check output_schema after successful execution
            if let (Some(schema), Some(ref result_val)) = (&io_block.output_schema, &summary.result)
            {
                use newton_core::workflow::io::validate_output_schema;
                if let Err(e) = validate_output_schema(schema, result_val) {
                    if emit_json {
                        let envelope = newton_core::workflow::io::CompletionEnvelope::failure(
                            Some(summary.execution_id),
                            newton_core::workflow::io::CompletionError {
                                code: Some("WFG-IO-003".to_string()),
                                category: "ValidationError".to_string(),
                                message: e.message.clone(),
                                error_payload: None,
                            },
                        );
                        println!("{}", serde_json::to_string(&envelope).unwrap_or_default());
                        std::process::exit(2);
                    }
                    return Err(anyhow::anyhow!("{}", e.message));
                }
            }
            // Check max_output_bytes
            if let (Some(max_bytes), Some(ref result_val)) =
                (io_settings.max_output_bytes, &summary.result)
            {
                let serialized = serde_json::to_string(result_val).unwrap_or_default();
                if serialized.len() > max_bytes {
                    if emit_json {
                        let envelope = newton_core::workflow::io::CompletionEnvelope::failure(
                            Some(summary.execution_id),
                            newton_core::workflow::io::CompletionError {
                                code: Some("WFG-IO-003".to_string()),
                                category: "ValidationError".to_string(),
                                message: "output exceeds max_output_bytes".to_string(),
                                error_payload: None,
                            },
                        );
                        println!("{}", serde_json::to_string(&envelope).unwrap_or_default());
                        std::process::exit(2);
                    }
                    return Err(anyhow::anyhow!(
                        "output exceeds max_output_bytes: WFG-IO-003"
                    ));
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
            let is_workflow_failure = matches!(
                app_error.code.as_str(),
                "WFG-EXEC-001" | "WFG-GATE-001" | "WFG-ITER-001" | "WFG-ITER-002" | "WFG-TIME-001"
            );
            let error_payload: Option<Value> = None;
            // WFG-IO-004: validate error_payload against error_schema (non-fatal warning).
            if let (Some(error_schema), Some(ref ep)) = (&io_block.error_schema, &error_payload) {
                use newton_core::workflow::io::validate_error_schema;
                if let Err(e) = validate_error_schema(error_schema, ep) {
                    tracing::warn!(
                        code = "WFG-IO-004",
                        message = %e.message,
                        "error_payload failed error_schema validation (non-fatal)"
                    );
                }
            }
            if emit_json {
                let status = if is_workflow_failure {
                    newton_core::workflow::io::CompletionStatus::Failure
                } else {
                    newton_core::workflow::io::CompletionStatus::InternalError
                };
                let error = newton_core::workflow::io::CompletionError {
                    code: if app_error.code.is_empty() {
                        None
                    } else {
                        Some(app_error.code.clone())
                    },
                    category: format!("{:?}", app_error.category),
                    message: app_error.message.clone(),
                    error_payload,
                };
                let envelope = newton_core::workflow::io::CompletionEnvelope {
                    schema_version: "1".to_string(),
                    execution_id: None,
                    status,
                    result: None,
                    error: Some(error),
                };
                println!("{}", serde_json::to_string(&envelope).unwrap_or_default());
                let exit_code = if is_workflow_failure { 2 } else { 1 };
                std::process::exit(exit_code);
            }
            Err(anyhow::anyhow!("{}", app_error.message))
        }
    }
}

pub async fn workflow_run(args: RunArgs) -> StdResult<(), AppError> {
    let workflow_path = args.workflow.clone();
    let workspace = resolve_workflow_workspace(args.workspace)?;
    let raw_document = workflow_schema::parse_workflow(&workflow_path)?;
    let mut document = workflow_transform::apply_default_pipeline(raw_document)?;
    let lint_results = LintRegistry::new().run(&document);
    if !lint_results.is_empty() {
        print_lint_results_text(&lint_results);
    }
    let error_count = lint_results
        .iter()
        .filter(|result| result.severity == LintSeverity::Error)
        .count();
    if error_count > 0 {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!("workflow lint detected {error_count} error(s); fix before running"),
        ));
    }
    apply_context_overrides(&mut document.workflow.context, &args.context);
    document.validate(&ExpressionEngine::default())?;

    if let Some(payload) = build_trigger_payload(&args.parameters_json, &args.trigger)? {
        document.triggers = Some(workflow_schema::WorkflowTrigger {
            trigger_type: workflow_schema::TriggerType::Manual,
            schema_version: "1".to_string(),
            payload,
        });
    }

    // Input validation (size and schema)
    if let Some(triggers) = &document.triggers {
        let settings = &document.workflow.settings;
        let payload = &triggers.payload;
        if let Some(max_bytes) = settings.io_settings.max_input_bytes {
            let serialized = serde_json::to_string(payload).unwrap_or_default();
            if serialized.len() > max_bytes {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("trigger payload exceeds max_input_bytes ({})", max_bytes),
                )
                .with_code("WFG-IO-001"));
            }
        }
        if let Some(schema) = &settings.io.input_schema {
            newton_core::workflow::io::validate_input_schema(schema, payload)?;
        }
    }

    let server_notifier = args
        .server
        .as_ref()
        .map(|url| std::sync::Arc::new(ServerNotifier::new(url.clone())));

    let overrides = ExecutionOverrides {
        parallel_limit: args.parallel_limit,
        max_time_seconds: args.timeout_seconds,
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        server_notifier,
        pre_seed_nodes: true,
    };

    let ailoop_ctx =
        newton_core::integrations::ailoop::init_context_for_command_name(&workspace, "run")
            .ok()
            .flatten();
    let mut builder = OperatorRegistry::builder();
    let settings = document.workflow.settings.clone();
    let interviewer = newton_core::workflow::human::lazy_interviewer_provider(
        ailoop_ctx,
        Duration::from_secs(settings.human.default_timeout_seconds),
    );
    workflow_operators::register_builtins_with_deps(
        &mut builder,
        workspace.clone(),
        settings,
        workflow_operators::BuiltinOperatorDeps {
            interviewer: Some(interviewer),
            ..Default::default()
        },
    );
    let registry = builder.build();

    let summary = workflow_executor::execute_workflow(
        document,
        workflow_path,
        registry,
        workspace.clone(),
        overrides,
    )
    .await?;
    println!(
        "Workflow completed in {} iterations",
        summary.total_iterations
    );
    Ok(())
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
        OutputFormat::Json => print_lint_results_json(&results)?,
        OutputFormat::Text => {
            if results.is_empty() {
                println!("No lint issues");
            } else {
                print_lint_results_text(&results);
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
    let _workspace = resolve_workflow_workspace(args.workspace)?;
    let raw_document = workflow_schema::parse_workflow(&workflow_path)?;
    let source_tasks = raw_document.workflow.tasks.len();
    let source_macro_invocations = raw_document.workflow.macro_invocation_count();
    let source_macro_names = raw_document.workflow.macro_names_referenced();
    let mut document = workflow_transform::apply_default_pipeline(raw_document)?;
    let overrides = parse_set_overrides(&args.context);
    let trigger_payload = build_trigger_payload(&args.parameters_json, &args.trigger)?
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
    if !trigger_payload.is_null() {
        document.triggers = Some(workflow_schema::WorkflowTrigger {
            trigger_type: workflow_schema::TriggerType::Manual,
            schema_version: "1".to_string(),
            payload: trigger_payload.clone(),
        });
    }
    let outcome = explain::build_explain_outcome(&document, &overrides, &trigger_payload)?;
    match args.format {
        OutputFormat::Json => print_explain_json(&outcome.output)?,
        OutputFormat::Text => print_explain_text(
            &outcome.output,
            Some((
                source_tasks,
                source_macro_invocations,
                source_macro_names.clone(),
            )),
        )?,
        OutputFormat::Prose => print_explain_prose(&outcome.output)?,
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
    let workspace = resolve_workflow_workspace(args.workspace)?;
    let execution = checkpoint::load_execution(&workspace, &args.run_id)?;
    let mut builder = OperatorRegistry::builder();
    let settings = execution.settings_effective.clone();
    let interviewer = newton_core::workflow::human::lazy_interviewer_provider(
        None,
        Duration::from_secs(settings.human.default_timeout_seconds),
    );
    workflow_operators::register_builtins_with_deps(
        &mut builder,
        workspace.clone(),
        settings,
        workflow_operators::BuiltinOperatorDeps {
            interviewer: Some(interviewer),
            ..Default::default()
        },
    );
    let registry = builder.build();
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

pub fn checkpoints(args: CheckpointArgs) -> StdResult<(), AppError> {
    match args.command {
        CheckpointCommand::List { workspace, json } => workflow_checkpoints_list(workspace, json),
        CheckpointCommand::Clean {
            workspace,
            older_than,
        } => workflow_checkpoints_clean(workspace, older_than),
    }
}

/// Format duration showing at most two significant time units
fn format_duration_short(duration: Duration) -> String {
    let mut remaining = duration.as_secs();
    let mut parts = Vec::new();

    if remaining == 0 {
        return "0s".to_string();
    }

    const SECONDS_PER_DAY: u64 = 86400;
    const SECONDS_PER_HOUR: u64 = 3600;
    const SECONDS_PER_MINUTE: u64 = 60;

    if remaining >= SECONDS_PER_DAY {
        let days = remaining / SECONDS_PER_DAY;
        parts.push(format!("{days}d"));
        remaining %= SECONDS_PER_DAY;
    }

    if remaining >= SECONDS_PER_HOUR && parts.len() < 2 {
        let hours = remaining / SECONDS_PER_HOUR;
        parts.push(format!("{hours}h"));
        remaining %= SECONDS_PER_HOUR;
    }

    if remaining >= SECONDS_PER_MINUTE && parts.len() < 2 {
        let minutes = remaining / SECONDS_PER_MINUTE;
        parts.push(format!("{minutes}m"));
        remaining %= SECONDS_PER_MINUTE;
    }

    if parts.is_empty() && parts.len() < 2 {
        parts.push(format!("{remaining}s"));
    }

    parts.join(" ")
}

/// Format datetime for compact human-readable display
fn format_datetime_short(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn workflow_checkpoints_list(
    workspace: Option<PathBuf>,
    format_json: bool,
) -> StdResult<(), AppError> {
    let workspace = resolve_workflow_workspace(workspace)?;
    let mut entries = checkpoint::list_checkpoints(&workspace)?;

    // Sort by started_at descending (newest first)
    entries.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    if format_json {
        let items: Vec<Value> = entries
            .iter()
            .map(|summary| {
                json!({
                    "execution_id": summary.execution_id.to_string(),
                    "status": summary.status.as_str(),
                    "started_at": summary.started_at.to_rfc3339(),
                    "checkpoint_age": format!("{} ago", format_duration(summary.checkpoint_age)),
                    "size": summary.checkpoint_size,
                })
            })
            .collect();
        let serialized = serde_json::to_string_pretty(&items).map_err(|err| {
            AppError::new(
                ErrorCategory::SerializationError,
                format!("failed to serialize checkpoint list: {err}"),
            )
        })?;
        println!("{serialized}");
        return Ok(());
    }

    // Table output with compact formatting
    println!(
        "{:<36} {:<10} {:<16} {:<14} {:>7}",
        "EXECUTION ID", "STATUS", "STARTED AT", "CHECKPOINT AGE", "SIZE"
    );
    println!("{}", "-".repeat(93));

    for summary in entries {
        println!(
            "{:<36} {:<10} {:<16} {:<14} {:>7}",
            summary.execution_id,
            summary.status.as_str(),
            format_datetime_short(&summary.started_at),
            format!("{} ago", format_duration_short(summary.checkpoint_age)),
            format_bytes(summary.checkpoint_size),
        );
    }
    Ok(())
}

fn workflow_checkpoints_clean(
    workspace: Option<PathBuf>,
    older_than: String,
) -> StdResult<(), AppError> {
    let workspace = resolve_workflow_workspace(workspace)?;
    let duration = parse_duration_arg(&older_than)?;
    checkpoint::clean_checkpoints(&workspace, duration)?;
    println!("Removed checkpoints older than {older_than}");
    Ok(())
}

pub fn artifacts(args: ArtifactArgs) -> StdResult<(), AppError> {
    match args.command {
        ArtifactCommand::Clean {
            workspace,
            older_than,
        } => workflow_artifacts_clean(workspace, older_than),
    }
}

fn workflow_artifacts_clean(
    workspace: Option<PathBuf>,
    older_than: String,
) -> StdResult<(), AppError> {
    let workspace = resolve_workflow_workspace(workspace)?;
    let duration = parse_duration_arg(&older_than)?;
    artifacts::ArtifactStore::clean_artifacts(&workspace, duration)?;
    println!("Cleaned artifacts older than {older_than}");
    Ok(())
}

pub async fn webhook(args: WebhookArgs) -> StdResult<(), AppError> {
    match args.command {
        WebhookCommand::Serve(serve_args) => workflow_webhook_serve(serve_args).await,
        WebhookCommand::Status(status_args) => workflow_webhook_status(status_args),
    }
}

async fn workflow_webhook_serve(args: WebhookServeArgs) -> StdResult<(), AppError> {
    let workflow_path = args.workflow.clone();
    let workspace = resolve_workflow_workspace(Some(args.workspace))?;
    let raw_document = workflow_schema::parse_workflow(&workflow_path)?;
    let document = workflow_transform::apply_default_pipeline(raw_document)?;
    let lint_results = LintRegistry::new().run(&document);
    if !lint_results.is_empty() {
        print_lint_results_text(&lint_results);
    }
    let error_count = lint_results
        .iter()
        .filter(|result| result.severity == LintSeverity::Error)
        .count();
    if error_count > 0 {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!("workflow lint detected {error_count} error(s); fix before starting webhook"),
        ));
    }
    document.validate(&ExpressionEngine::default())?;

    let mut builder = OperatorRegistry::builder();
    let settings = document.workflow.settings.clone();
    let interviewer = newton_core::workflow::human::lazy_interviewer_provider(
        None,
        Duration::from_secs(settings.human.default_timeout_seconds),
    );
    workflow_operators::register_builtins_with_deps(
        &mut builder,
        workspace.clone(),
        settings,
        workflow_operators::BuiltinOperatorDeps {
            interviewer: Some(interviewer),
            ..Default::default()
        },
    );
    let registry = builder.build();
    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        server_notifier: None,
        pre_seed_nodes: true,
    };

    webhook::serve_webhook(
        document,
        workflow_path,
        registry,
        workspace.clone(),
        overrides,
    )
    .await?;
    Ok(())
}

fn workflow_webhook_status(args: WebhookStatusArgs) -> StdResult<(), AppError> {
    let workspace = resolve_workflow_workspace(Some(args.workspace))?;
    let workflow_path = resolve_workspace_workflow_path(&workspace, args.workflow)?;
    let raw_document = workflow_schema::parse_workflow(&workflow_path)?;
    let document = workflow_transform::apply_default_pipeline(raw_document)?;
    let settings = document.workflow.settings.webhook;
    if !settings.enabled {
        println!("Webhook not configured.");
        return Ok(());
    }
    let token_set = env::var(&settings.auth_token_env)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    println!("{:<16} {}", "Bind address:", settings.bind);
    println!(
        "{:<16} {} (set: {})",
        "Auth token env:",
        settings.auth_token_env,
        if token_set { "yes" } else { "no" }
    );
    println!("{:<16} {}", "Max body bytes:", settings.max_body_bytes);
    Ok(())
}

fn parse_duration_arg(value: &str) -> StdResult<Duration, AppError> {
    parse_duration(value).map_err(|err| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("failed to parse duration {value}: {err}"),
        )
    })
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut index = 0;
    while size >= 1024.0 && index < UNITS.len() - 1 {
        size /= 1024.0;
        index += 1;
    }
    if index == 0 {
        format!("{} {}", bytes, UNITS[index])
    } else {
        format!("{:.1} {}", size, UNITS[index])
    }
}

pub fn log(args: RunsArgs) -> StdResult<(), AppError> {
    match args.command {
        RunsCommand::List {
            workspace,
            last,
            json,
        } => log_list(workspace, last, json),
        RunsCommand::Show {
            run_id,
            workspace,
            task,
            verbose,
            json,
        } => log_show(run_id, workspace, task, verbose, json),
    }
}

fn log_list(
    workspace: Option<PathBuf>,
    last: Option<usize>,
    emit_json: bool,
) -> StdResult<(), AppError> {
    if let Some(n) = last {
        if n == 0 {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "--last must be a positive integer (greater than zero)",
            )
            .with_code("LOG-003"));
        }
    }

    let workspace = resolve_workflow_workspace(workspace)?;
    let base = WorkflowStatePaths::workspace_root(&workspace);

    let mut entries: Vec<(WorkflowExecution, Option<usize>)> = Vec::new();

    if base.exists() {
        for entry in fs::read_dir(&base)
            .map_err(|err| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to list workflows state: {err}"),
                )
            })?
            .flatten()
        {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            if let Ok(uuid) = uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()) {
                let exec_file = base.join(uuid.to_string()).join("execution.json");
                if let Ok(bytes) = fs::read(&exec_file) {
                    if let Ok(execution) = serde_json::from_slice::<WorkflowExecution>(&bytes) {
                        // Try to get checkpoint task count.
                        let checkpoint_task_count = {
                            let ckpt_file = base.join(uuid.to_string()).join("checkpoint.json");
                            fs::read(&ckpt_file)
                                .ok()
                                .and_then(|b| serde_json::from_slice::<WorkflowCheckpoint>(&b).ok())
                                .map(|ckpt| ckpt.completed.len())
                        };
                        entries.push((execution, checkpoint_task_count));
                    }
                }
            }
        }
    }

    // Sort: newest first by started_at, tie-break by execution_id descending.
    entries.sort_by(|(a, _), (b, _)| {
        b.started_at
            .cmp(&a.started_at)
            .then_with(|| b.execution_id.to_string().cmp(&a.execution_id.to_string()))
    });

    // Apply --last filter.
    if let Some(n) = last {
        entries.truncate(n);
    }

    if emit_json {
        let items: Vec<Value> = entries
            .iter()
            .map(|(exec, ckpt_count)| {
                let task_count = ckpt_count.unwrap_or(exec.task_runs.len());
                let duration_ms = exec
                    .completed_at
                    .map(|completed| {
                        completed
                            .signed_duration_since(exec.started_at)
                            .num_milliseconds()
                    })
                    .filter(|&ms| ms >= 0)
                    .map(|ms| ms as u64);
                let failed_task_id = exec
                    .task_runs
                    .iter()
                    .find(|r| r.status == WorkflowTaskStatus::Failed)
                    .map(|r| r.task_id.clone());
                json!({
                    "execution_id": exec.execution_id.to_string(),
                    "workflow_file": exec.workflow_file,
                    "status": exec.status.as_str(),
                    "started_at": exec.started_at.to_rfc3339(),
                    "task_count": task_count,
                    "duration_ms": duration_ms,
                    "failed_task_id": failed_task_id,
                })
            })
            .collect();
        let serialized = serde_json::to_string_pretty(&items).map_err(|err| {
            AppError::new(
                ErrorCategory::SerializationError,
                format!("failed to serialize execution list: {err}"),
            )
        })?;
        println!("{serialized}");
        return Ok(());
    }

    // Text table output.
    println!(
        "{:<36}  {:<20}  {:<10}  {:<19}  {:>5}  DURATION",
        "EXECUTION ID", "WORKFLOW", "STATUS", "STARTED AT", "TASKS"
    );
    println!("{}", "-".repeat(102));
    for (exec, ckpt_count) in &entries {
        let task_count = ckpt_count.unwrap_or(exec.task_runs.len());
        let duration_str = exec
            .completed_at
            .map(|completed| {
                let ms = completed
                    .signed_duration_since(exec.started_at)
                    .num_milliseconds();
                if ms < 0 {
                    "-".to_string()
                } else {
                    format_duration_short(Duration::from_millis(ms as u64))
                }
            })
            .unwrap_or_else(|| "-".to_string());
        let workflow_short = {
            let wf = &exec.workflow_file;
            if wf.len() > 20 {
                format!("...{}", &wf[wf.len() - 17..])
            } else {
                wf.clone()
            }
        };
        println!(
            "{:<36}  {:<20}  {:<10}  {:<19}  {:>5}  {}",
            exec.execution_id,
            workflow_short,
            exec.status.as_str(),
            exec.started_at.format("%Y-%m-%d %H:%M:%S"),
            task_count,
            duration_str,
        );
    }
    Ok(())
}

fn log_show(
    execution_id: uuid::Uuid,
    workspace: Option<PathBuf>,
    task_filter: Option<String>,
    verbose: bool,
    emit_json: bool,
) -> StdResult<(), AppError> {
    let workspace = resolve_workflow_workspace(workspace)?;
    let paths = WorkflowStatePaths::new(&workspace, &execution_id);

    // Load execution.json — LOG-001 if not found.
    if !paths.execution_file.exists() {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "execution not found: no execution.json at {} (LOG-001)",
                paths.execution_file.display()
            ),
        )
        .with_code("LOG-001"));
    }
    let exec_bytes = fs::read(&paths.execution_file).map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to read execution.json: {err}"),
        )
    })?;
    let execution: WorkflowExecution = serde_json::from_slice(&exec_bytes).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to deserialize execution.json: {err}"),
        )
    })?;

    // Try to load checkpoint.json.
    let checkpoint_opt: Option<WorkflowCheckpoint> = if paths.checkpoint_file.exists() {
        fs::read(&paths.checkpoint_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
    } else {
        None
    };

    if emit_json {
        return log_show_json(
            execution_id,
            execution,
            checkpoint_opt,
            task_filter,
            &workspace,
        );
    }

    log_show_text(
        execution_id,
        execution,
        checkpoint_opt,
        task_filter,
        verbose,
        &workspace,
    )
}

/// Collect and sort task run records for replay ordering.
fn collect_sorted_records(checkpoint: &WorkflowCheckpoint) -> Vec<WorkflowTaskRunRecord> {
    let mut records: Vec<WorkflowTaskRunRecord> = checkpoint.completed.values().cloned().collect();
    records.sort_by(|a, b| {
        a.started_at
            .cmp(&b.started_at)
            .then_with(|| a.task_id.cmp(&b.task_id))
            .then_with(|| a.run_seq.cmp(&b.run_seq))
    });
    records
}

fn resolve_operator_str(task_id: &str, checkpoint: &WorkflowCheckpoint) -> String {
    if let Some(tasks) = &checkpoint.runtime_tasks {
        if let Some(task) = tasks.iter().find(|t| t.id == task_id) {
            return task.operator.clone();
        }
    }
    "(unknown)".to_string()
}

fn materialize_output(output_ref: &OutputRef, workspace: &Path) -> String {
    match output_ref.materialize(workspace) {
        Ok(val) => serde_json::to_string_pretty(&val).unwrap_or_else(|_| "(error)".to_string()),
        Err(err) => {
            // Show artifact missing if IoError (file deleted).
            if let OutputRef::Artifact { path, .. } = output_ref {
                format!("(artifact missing: {})", path.display())
            } else {
                format!("(error: {err})")
            }
        }
    }
}

fn log_show_text(
    _execution_id: uuid::Uuid,
    execution: WorkflowExecution,
    checkpoint_opt: Option<WorkflowCheckpoint>,
    task_filter: Option<String>,
    verbose: bool,
    workspace: &Path,
) -> StdResult<(), AppError> {
    // Print execution header.
    let duration_str = execution
        .completed_at
        .map(|c| {
            let ms = c
                .signed_duration_since(execution.started_at)
                .num_milliseconds();
            if ms < 0 {
                "-".to_string()
            } else {
                format_duration_short(Duration::from_millis(ms as u64))
            }
        })
        .unwrap_or_else(|| "-".to_string());
    println!("Execution: {}", execution.execution_id);
    println!("Workflow:  {}", execution.workflow_file);
    println!("Status:    {}", execution.status.as_str());
    println!(
        "Started:   {}",
        execution.started_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!("Duration:  {duration_str}");

    if let Some(checkpoint) = checkpoint_opt {
        let records = collect_sorted_records(&checkpoint);
        let filtered: Vec<WorkflowTaskRunRecord> = if let Some(ref filter) = task_filter {
            records
                .into_iter()
                .filter(|r| &r.task_id == filter)
                .collect()
        } else {
            records
        };

        // LOG-002: task filter matches nothing.
        if let Some(ref filter) = task_filter {
            if filtered.is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "task filter '{filter}' did not match any task in this execution (LOG-002)",
                    ),
                )
                .with_code("LOG-002"));
            }
        }

        let total = filtered.len();
        for (idx, record) in filtered.iter().enumerate() {
            let operator = resolve_operator_str(&record.task_id, &checkpoint);
            let is_failed = record.status == WorkflowTaskStatus::Failed;
            let status_label = if is_failed {
                "FAILED"
            } else {
                record.status.as_str()
            };

            if is_failed {
                println!(
                    "\n\u{2500}\u{2500}\u{2500} [FAILED] Task {} of {} {}",
                    idx + 1,
                    total,
                    "\u{2500}".repeat(40)
                );
            } else {
                println!(
                    "\n\u{2500}\u{2500}\u{2500} Task {} of {} {}",
                    idx + 1,
                    total,
                    "\u{2500}".repeat(40)
                );
            }

            let duration_ms = record
                .completed_at
                .signed_duration_since(record.started_at)
                .num_milliseconds();
            let duration_str = if duration_ms >= 0 {
                format_duration_short(Duration::from_millis(duration_ms as u64))
            } else {
                "-".to_string()
            };

            println!("  ID:       {}  (run {})", record.task_id, record.run_seq);
            println!("  Operator: {operator}");
            println!("  Status:   {status_label}");
            println!("  Duration: {duration_str}");

            if is_failed || verbose {
                if let Some(ref err) = record.error {
                    println!("\n  Error:");
                    println!("    Code:    {}", err.code);
                    println!("    Message: {}", err.message);
                }
            }

            // Show inputs (resolved params).
            match &record.resolved_params_snapshot {
                Some(params) => {
                    println!("\n  Inputs (resolved params):");
                    let pretty = serde_json::to_string_pretty(params)
                        .unwrap_or_else(|_| "(error)".to_string());
                    for line in pretty.lines() {
                        println!("  {line}");
                    }
                }
                None => {
                    println!("\n  Inputs (resolved params): (not available)");
                }
            }

            // Show output.
            println!("\n  Output:");
            let output_str = materialize_output(&record.output_ref, workspace);
            if output_str.starts_with("(artifact missing:") {
                println!("  {output_str}");
            } else {
                for line in output_str.lines() {
                    println!("  {line}");
                }
            }
        }
    } else {
        // No checkpoint — fall back to execution.json task_runs.
        println!("\n(full input replay requires completed checkpoint)\n");

        let filtered: Vec<_> = if let Some(ref filter) = task_filter {
            execution
                .task_runs
                .iter()
                .filter(|r| &r.task_id == filter)
                .collect()
        } else {
            execution.task_runs.iter().collect()
        };

        // LOG-002 check.
        if let Some(ref filter) = task_filter {
            if filtered.is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "task filter '{filter}' did not match any task in this execution (LOG-002)",
                    ),
                )
                .with_code("LOG-002"));
            }
        }

        let total = filtered.len();
        for (idx, record) in filtered.iter().enumerate() {
            let is_failed = record.status == WorkflowTaskStatus::Failed;
            if is_failed {
                println!(
                    "\u{2500}\u{2500}\u{2500} [FAILED] Task {} of {} {}",
                    idx + 1,
                    total,
                    "\u{2500}".repeat(40)
                );
            } else {
                println!(
                    "\u{2500}\u{2500}\u{2500} Task {} of {} {}",
                    idx + 1,
                    total,
                    "\u{2500}".repeat(40)
                );
            }
            println!("  ID:       {}  (run {})", record.task_id, record.run_seq);
            println!("  Status:   {}", record.status.as_str());
            println!("  Duration: {}ms", record.duration_ms);
            if let Some(ref code) = record.error_code {
                println!("  Error Code: {code}");
            }
        }
    }

    Ok(())
}

fn log_show_json(
    _execution_id: uuid::Uuid,
    execution: WorkflowExecution,
    checkpoint_opt: Option<WorkflowCheckpoint>,
    task_filter: Option<String>,
    workspace: &Path,
) -> StdResult<(), AppError> {
    let tasks_array: Vec<Value>;

    if let Some(ref checkpoint) = checkpoint_opt {
        let records = collect_sorted_records(checkpoint);
        let filtered: Vec<WorkflowTaskRunRecord> = if let Some(ref filter) = task_filter {
            records
                .into_iter()
                .filter(|r| &r.task_id == filter)
                .collect()
        } else {
            records
        };

        // LOG-002 check.
        if let Some(ref filter) = task_filter {
            if filtered.is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "task filter '{filter}' did not match any task in this execution (LOG-002)",
                    ),
                )
                .with_code("LOG-002"));
            }
        }

        tasks_array = filtered
            .iter()
            .map(|record| {
                let operator = resolve_operator_str(&record.task_id, checkpoint);
                let duration_ms = record
                    .completed_at
                    .signed_duration_since(record.started_at)
                    .num_milliseconds();
                let output = match record.output_ref.materialize(workspace) {
                    Ok(v) => v,
                    Err(_) => {
                        if let OutputRef::Artifact { path, .. } = &record.output_ref {
                            json!(format!("(artifact missing: {})", path.display()))
                        } else {
                            json!(null)
                        }
                    }
                };
                let error_val = record.error.as_ref().map(|e| {
                    json!({
                        "code": e.code,
                        "category": e.category,
                        "message": e.message,
                    })
                });
                json!({
                    "task_id": record.task_id,
                    "run_seq": record.run_seq,
                    "operator": operator,
                    "status": record.status.as_str(),
                    "started_at": record.started_at.to_rfc3339(),
                    "completed_at": record.completed_at.to_rfc3339(),
                    "duration_ms": if duration_ms >= 0 { json!(duration_ms) } else { json!(null) },
                    "resolved_params": record.resolved_params_snapshot,
                    "output": output,
                    "error": error_val,
                })
            })
            .collect();
    } else {
        // Fallback to execution.json.
        let exec_records: Vec<_> = if let Some(ref filter) = task_filter {
            execution
                .task_runs
                .iter()
                .filter(|r| &r.task_id == filter)
                .collect()
        } else {
            execution.task_runs.iter().collect()
        };

        if let Some(ref filter) = task_filter {
            if exec_records.is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "task filter '{filter}' did not match any task in this execution (LOG-002)",
                    ),
                )
                .with_code("LOG-002"));
            }
        }

        tasks_array = exec_records
            .iter()
            .map(|record| {
                json!({
                    "task_id": record.task_id,
                    "run_seq": record.run_seq,
                    "operator": "(unknown)",
                    "status": record.status.as_str(),
                    "started_at": null,
                    "completed_at": null,
                    "duration_ms": record.duration_ms,
                    "resolved_params": null,
                    "output": null,
                    "error": record.error_code,
                })
            })
            .collect();
    }

    let exec_val = serde_json::to_value(&execution).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize execution: {err}"),
        )
    })?;

    let mut result = json!({
        "execution": exec_val,
        "tasks": tasks_array,
    });

    if let Some(filter) = task_filter {
        result
            .as_object_mut()
            .unwrap()
            .insert("task_filter".to_string(), json!(filter));
    }

    let serialized = serde_json::to_string_pretty(&result).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize log show output: {err}"),
        )
    })?;
    println!("{serialized}");
    Ok(())
}

fn resolve_workflow_workspace(path: Option<PathBuf>) -> StdResult<PathBuf, AppError> {
    match path {
        Some(p) => Ok(p),
        None => Ok(env::current_dir().map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to resolve workspace path: {err}"),
            )
        })?),
    }
}

fn resolve_workspace_workflow_path(
    workspace: &Path,
    override_path: Option<PathBuf>,
) -> StdResult<PathBuf, AppError> {
    if let Some(path) = override_path {
        return Ok(path);
    }
    for candidate in &["workflow.yaml", "workflow.yml"] {
        let candidate_path = workspace.join(candidate);
        if candidate_path.exists() {
            return Ok(candidate_path);
        }
    }
    Err(AppError::new(
        ErrorCategory::ValidationError,
        format!(
            "workflow file not found under {}; pass WORKFLOW or --file PATH to specify",
            workspace.display()
        ),
    ))
}

fn apply_context_overrides(context: &mut Value, overrides: &[KeyValuePair]) {
    if !context.is_object() {
        *context = Value::Object(Map::new());
    }
    if let Some(map) = context.as_object_mut() {
        for pair in overrides {
            let parsed = serde_json::from_str(&pair.value)
                .unwrap_or_else(|_| Value::String(pair.value.clone()));
            map.insert(pair.key.clone(), parsed);
        }
    }
}

fn print_lint_results_text(results: &[LintResult]) {
    for result in results {
        if let Some(location) = &result.location {
            println!(
                "{} {} ({}) : {}",
                result.severity, result.code, location, result.message
            );
        } else {
            println!("{} {} : {}", result.severity, result.code, result.message);
        }
        if let Some(suggestion) = &result.suggestion {
            println!("  Suggestion: {suggestion}");
        }
    }
}

fn print_lint_results_json(results: &[LintResult]) -> StdResult<(), AppError> {
    let payload = json!({ "results": results });
    let serialized = serde_json::to_string_pretty(&payload).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize lint results: {err}"),
        )
    })?;
    println!("{serialized}");
    Ok(())
}

fn print_explain_text(
    output: &explain::ExplainOutput,
    source_summary: Option<(usize, usize, Vec<String>)>,
) -> StdResult<(), AppError> {
    if let Some((task_count, macro_count, macro_names)) = source_summary {
        if macro_count > 0 {
            println!(
                "Source: {} tasks, {} macro invocations ({})",
                task_count,
                macro_count,
                macro_names.join(", ")
            );
        } else {
            println!("Source: {task_count} tasks, 0 macro invocations");
        }
        println!();
    }
    println!("Effective settings:");
    println!("{}", pretty_json(&output.settings)?);
    println!();
    println!("Initial context:");
    println!("{}", pretty_json(&output.context)?);
    println!();
    println!("Triggers:");
    println!("{}", pretty_json(&output.triggers)?);
    println!();
    println!("Tasks:");
    for task in &output.tasks {
        println!("  {} ({})", task.id, task.operator);
        println!("    Params:");
        println!("      {}", pretty_json(&task.params)?);
        println!("    Transitions:");
        for transition in &task.transitions {
            println!(
                "      - to={} priority={} when={}",
                transition.target, transition.priority, transition.when
            );
        }
    }
    Ok(())
}

fn print_explain_json(output: &explain::ExplainOutput) -> StdResult<(), AppError> {
    let serialized = serde_json::to_string_pretty(output).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize explain output: {err}"),
        )
    })?;
    println!("{serialized}");
    Ok(())
}

fn print_explain_prose(output: &explain::ExplainOutput) -> StdResult<(), AppError> {
    let prose = explain::format_explain_prose(output)?;
    println!("{prose}");
    Ok(())
}

fn pretty_json(value: &impl Serialize) -> StdResult<String, AppError> {
    serde_json::to_string_pretty(value).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize explain section: {err}"),
        )
    })
}

fn parse_set_overrides(pairs: &[KeyValuePair]) -> Vec<(String, Value)> {
    pairs
        .iter()
        .map(|pair| {
            let parsed = serde_json::from_str(&pair.value)
                .unwrap_or_else(|_| Value::String(pair.value.clone()));
            (pair.key.clone(), parsed)
        })
        .collect()
}

fn try_load_trigger_payload(path: &Option<PathBuf>) -> StdResult<Option<Value>, AppError> {
    match path {
        Some(path) => Ok(Some(load_trigger_payload(path)?)),
        None => Ok(None),
    }
}

pub fn build_trigger_payload(
    trigger_json: &Option<PathBuf>,
    args: &[KeyValuePair],
) -> StdResult<Option<Value>, AppError> {
    if trigger_json.is_none() && args.is_empty() {
        return Ok(None);
    }

    let mut payload =
        try_load_trigger_payload(trigger_json)?.unwrap_or_else(|| Value::Object(Map::new()));
    let map = payload.as_object_mut().ok_or_else(|| {
        AppError::new(
            ErrorCategory::ValidationError,
            "trigger JSON must be an object",
        )
    })?;

    for pair in args {
        map.insert(pair.key.clone(), resolve_trigger_arg_value(&pair.value)?);
    }

    Ok(Some(payload))
}

fn resolve_trigger_arg_value(value: &str) -> StdResult<Value, AppError> {
    if let Some(path) = value.strip_prefix("@@") {
        return Ok(Value::String(format!("@{path}")));
    }
    if let Some(path) = value.strip_prefix('@') {
        if path.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "trigger arg file path is empty",
            ));
        }
        let content = fs::read_to_string(path).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to read trigger arg file {path}: {err}"),
            )
        })?;
        return Ok(Value::String(content));
    }

    Ok(Value::String(value.to_string()))
}

fn load_trigger_payload(path: &Path) -> StdResult<Value, AppError> {
    // Strip leading @ if present (@path syntax)
    let path_str = path.to_str().unwrap_or("");
    let actual_path = if let Some(stripped) = path_str.strip_prefix('@') {
        std::path::Path::new(stripped)
    } else {
        path
    };
    let content = fs::read_to_string(actual_path).map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!(
                "failed to read trigger JSON {}: {}",
                actual_path.display(),
                err
            ),
        )
    })?;
    let value: Value = serde_json::from_str(&content).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to parse trigger JSON {}: {}", path.display(), err),
        )
    })?;
    if !value.is_object() {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "trigger JSON must be an object",
        ));
    }
    Ok(value)
}

/// Newton REST route prefixes that the optional MCP mount MUST NOT shadow
/// (issue #294 §4.4). Kept in sync with `crates/core/src/api/mod.rs`.
const NEWTON_REST_ROUTE_PREFIXES: &[&str] = &[
    "/health",
    "/workflows",
    "/hil",
    "/streaming",
    "/operators",
    "/dashboard",
    "/portfolio",
    "/opportunities",
    "/requests",
    "/plans",
    "/persistence",
    "/testing",
];

/// Validate `--mcp-path` shape (issue #294 §4.4).
///
/// Returns `NEWTON-SERVE-MCP-001` when the value is empty, missing the leading
/// slash, equal to the bare root `/`, or ends with `/`.
fn validate_mcp_path(p: &str) -> StdResult<(), AppError> {
    let invalid =
        p.is_empty() || !p.starts_with('/') || p == "/" || (p.len() > 1 && p.ends_with('/'));
    if invalid {
        return Err(AppError::new(
            newton_core::core::types::ErrorCategory::ValidationError,
            format!(
                "NEWTON-SERVE-MCP-001: --mcp-path must start with '/' and must not be '/' or end with '/'; got {:?}",
                p
            ),
        )
        .with_code("NEWTON-SERVE-MCP-001"));
    }
    Ok(())
}

/// Reject `--mcp-path` values that collide with or are an ancestor of an
/// existing Newton REST route prefix (issue #294 §4.4). Returns
/// `NEWTON-SERVE-MCP-002`.
fn ensure_no_route_collision(mcp_path: &str) -> StdResult<(), AppError> {
    for prefix in NEWTON_REST_ROUTE_PREFIXES {
        if mcp_path == *prefix
            || prefix.starts_with(&format!("{}/", mcp_path))
            || mcp_path.starts_with(&format!("{}/", prefix))
        {
            return Err(AppError::new(
                newton_core::core::types::ErrorCategory::ValidationError,
                format!(
                    "NEWTON-SERVE-MCP-002: --mcp-path {:?} collides with Newton REST route prefix {:?}",
                    mcp_path, prefix
                ),
            )
            .with_code("NEWTON-SERVE-MCP-002"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod serve_mcp_validation_tests {
    use super::*;

    #[test]
    fn validate_path_accepts_normal_paths() {
        assert!(validate_mcp_path("/mcp").is_ok());
        assert!(validate_mcp_path("/api/mcp").is_ok());
    }

    #[test]
    fn validate_path_rejects_empty_or_root() {
        assert!(validate_mcp_path("").is_err());
        assert!(validate_mcp_path("/").is_err());
    }

    #[test]
    fn validate_path_rejects_missing_leading_slash() {
        let err = validate_mcp_path("mcp").unwrap_err();
        assert!(err.message.contains("NEWTON-SERVE-MCP-001"));
    }

    #[test]
    fn validate_path_rejects_trailing_slash() {
        assert!(validate_mcp_path("/mcp/").is_err());
    }

    #[test]
    fn collision_detects_health() {
        let err = ensure_no_route_collision("/health").unwrap_err();
        assert!(err.message.contains("NEWTON-SERVE-MCP-002"));
    }

    #[test]
    fn collision_detects_ancestor_of_health() {
        // /health is an existing prefix; mounting under it would shadow it.
        assert!(ensure_no_route_collision("/health/x").is_err());
    }

    #[test]
    fn collision_allows_unrelated_path() {
        assert!(ensure_no_route_collision("/mcp").is_ok());
        assert!(ensure_no_route_collision("/api/mcp").is_ok());
    }
}

/// Launch the Newton HTTP API server
pub async fn serve(args: ServeArgs) -> StdResult<(), AppError> {
    use newton_core::api::{self, state::AppState};
    use newton_core::workflow::operators;
    use std::net::SocketAddr;
    use tokio::net::TcpListener;
    use tracing::info;

    if args.with_mcp {
        validate_mcp_path(&args.mcp_path)?;
        ensure_no_route_collision(&args.mcp_path)?;
    }

    info!("Starting Newton API server on {}: {}", args.host, args.port);

    let mut builder = OperatorRegistry::builder();
    let serve_settings: workflow_schema::WorkflowSettings = Default::default();
    let interviewer = newton_core::workflow::human::lazy_interviewer_provider(
        None,
        Duration::from_secs(serve_settings.human.default_timeout_seconds),
    );
    operators::register_builtins_with_deps(
        &mut builder,
        std::path::PathBuf::from("."),
        serve_settings,
        operators::BuiltinOperatorDeps {
            interviewer: Some(interviewer),
            ..Default::default()
        },
    );
    let registry = builder.build();

    let operator_names = registry.operator_names();
    let operator_descriptors: Vec<newton_types::OperatorDescriptor> = operator_names
        .iter()
        .map(|name: &String| newton_types::OperatorDescriptor {
            operator_type: name.clone(),
            description: format!("{name} operator"),
            params_schema: serde_json::json!({}),
        })
        .collect();

    let db_path = std::path::PathBuf::from(".newton")
        .join("state")
        .join("backend.sqlite");
    if let Some(dir) = db_path.parent() {
        fs::create_dir_all(dir).map_err(|e| {
            AppError::new(
                newton_core::core::types::ErrorCategory::IoError,
                format!("failed to create backend state dir: {e}"),
            )
        })?;
    }
    let db_url = format!("sqlite:{}?mode=rwc", db_path.display());

    let store = newton_backend::SqliteBackendStore::new(&db_url)
        .await
        .map_err(|e| {
            AppError::new(
                newton_core::core::types::ErrorCategory::IoError,
                format!("backend store init failed: {}", e.message),
            )
        })?;
    info!("Backend store initialized at {}", db_path.display());
    let backend: Arc<dyn newton_backend::BackendStore> = Arc::new(store);

    let state = AppState::new(operator_descriptors, backend);

    let mut app = api::create_router(state, args.static_ui.clone());

    if args.with_mcp {
        let ctx = crate::cli::context::NewtonContext::new();
        let mcp_router =
            crate::cli::framework_setup::build_mcp_router_for_serve(ctx, &args.mcp_path).map_err(
                |err| {
                    AppError::new(
                        newton_core::core::types::ErrorCategory::InternalError,
                        format!("NEWTON-SERVE-MCP-004: failed to build MCP router: {err}"),
                    )
                    .with_code("NEWTON-SERVE-MCP-004")
                },
            )?;
        app = app.merge(mcp_router);
    }

    let addr = format!("{}:{}", args.host, args.port);
    let socket_addr: SocketAddr = addr.parse().map_err(|err| {
        AppError::new(
            newton_core::core::types::ErrorCategory::ValidationError,
            format!("invalid bind address: {err}"),
        )
    })?;

    let listener = TcpListener::bind(&socket_addr).await.map_err(|err| {
        AppError::new(
            newton_core::core::types::ErrorCategory::IoError,
            format!("failed to bind to {addr}: {err}"),
        )
    })?;

    info!("Newton API server listening on {}", socket_addr);

    if args.with_mcp {
        let bind_address = format!("{}:{}", args.host, args.port);
        let count = crate::cli::mcp::tool_count();
        tracing::info!(
            event = "mcp_serve_started",
            mcp_enabled = true,
            bind_address = %bind_address,
            mcp_path = %args.mcp_path,
            tool_count = count,
            "MCP router mounted on Newton serve listener"
        );
        // Mirror to stderr as a single JSON line so integration tests have a
        // deterministic surface (matches `cli/mcp.rs:166-169`).
        eprintln!(
            "{{\"event\":\"mcp_serve_started\",\"mcp_enabled\":true,\"bind_address\":\"{}\",\"mcp_path\":\"{}\",\"tool_count\":{}}}",
            bind_address, args.mcp_path, count
        );
    }

    axum::serve(listener, app.into_make_service())
        .await
        .map_err(|err| {
            AppError::new(
                newton_core::core::types::ErrorCategory::IoError,
                format!("server error: {err}"),
            )
        })?;

    Ok(())
}

async fn sleep_if_needed(duration_secs: u64) {
    sleep(Duration::from_secs(duration_secs)).await;
}

/// Holds the paths for batch processing directories
struct BatchDirs {
    todo_dir: PathBuf,
    completed_dir: PathBuf,
    failed_dir: PathBuf,
    #[allow(dead_code)]
    draft_dir: PathBuf,
}

/// Create and validate batch directories
fn ensure_batch_dirs(workspace_root: &Path, project_id: &str) -> Result<BatchDirs> {
    let plan_root = workspace_root.join(".newton").join("plan");
    if !plan_root.is_dir() {
        return Err(anyhow!(
            "Workspace {} must contain .newton/plan",
            workspace_root.display()
        ));
    }

    let plan_project_dir = plan_root.join(project_id);
    let todo_dir = plan_project_dir.join("todo");
    let completed_dir = plan_project_dir.join("completed");
    let failed_dir = plan_project_dir.join("failed");
    let draft_dir = plan_project_dir.join("draft");

    fs::create_dir_all(&todo_dir)?;
    fs::create_dir_all(&completed_dir)?;
    fs::create_dir_all(&draft_dir)?;
    fs::create_dir_all(&failed_dir)?;

    Ok(BatchDirs {
        todo_dir,
        completed_dir,
        failed_dir,
        draft_dir,
    })
}

pub async fn batch(args: BatchArgs) -> Result<()> {
    tracing::info!(
        "Starting workflow batch runner for project {}",
        args.project_id
    );

    let workspace_root = validate_batch_workspace(args.workspace.clone())?;
    let batch_config = BatchProjectConfig::load(&workspace_root, &args.project_id)?;
    let dirs = ensure_batch_dirs(&workspace_root, &args.project_id)?;

    loop {
        let plan_file =
            fetch_next_task(&dirs.todo_dir, args.once, args.poll_interval_seconds).await?;
        if plan_file.is_none() {
            return Ok(());
        }
        let plan_file = plan_file.unwrap();

        let task_layout = prepare_task_layout(&batch_config, &plan_file)?;
        let run_result = execute_workflow_for_plan(&batch_config, &task_layout).await;

        // Move plan file to completed or failed directory
        let destination_dir = if run_result.is_ok() {
            &dirs.completed_dir
        } else {
            &dirs.failed_dir
        };

        let destination = destination_dir.join(
            plan_file
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Plan file missing name"))?,
        );
        if destination.exists() {
            fs::remove_file(&destination)?;
        }
        fs::rename(&plan_file, &destination)?;

        if let Err(error) = run_result {
            tracing::error!(
                "Workflow execution failed for {}: {}",
                plan_file.display(),
                error
            );
            if args.once {
                return Err(error);
            }
        } else {
            tracing::info!("Workflow execution completed for {}", plan_file.display());
            if args.once {
                return Ok(());
            }
        }

        if !args.once {
            sleep_if_needed(args.poll_interval_seconds).await;
        }
    }
}

async fn execute_workflow_for_plan(
    batch_config: &BatchProjectConfig,
    task_layout: &TaskLayout,
) -> Result<()> {
    fs::create_dir_all(task_layout.state_dir.join("workflows"))?;
    fs::create_dir_all(task_layout.state_dir.join("artifacts").join("workflows"))?;

    let workspace = batch_config.project_root.clone();
    let workflow_path = batch_config.workflow_file.clone();
    let raw_document = workflow_schema::parse_workflow(&workflow_path)?;
    let mut document = workflow_transform::apply_default_pipeline(raw_document)?;
    document.triggers = Some(workflow_schema::WorkflowTrigger {
        trigger_type: workflow_schema::TriggerType::Manual,
        schema_version: "1".to_string(),
        payload: json!({
            "input_file": task_layout.input_file.display().to_string(),
            "workspace": batch_config.project_root.display().to_string(),
        }),
    });

    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: Some(task_layout.state_dir.join("workflows")),
        artifact_base_path: Some(task_layout.state_dir.join("artifacts").join("workflows")),
        max_nesting_depth: None,
        verbose: false,
        server_notifier: None,
        pre_seed_nodes: true,
    };

    let ailoop_ctx =
        newton_core::integrations::ailoop::init_context_for_command_name(&workspace, "batch")
            .ok()
            .flatten();
    let mut builder = OperatorRegistry::builder();
    let settings = document.workflow.settings.clone();
    let interviewer = newton_core::workflow::human::lazy_interviewer_provider(
        ailoop_ctx,
        Duration::from_secs(settings.human.default_timeout_seconds),
    );
    workflow_operators::register_builtins_with_deps(
        &mut builder,
        workspace.clone(),
        settings,
        workflow_operators::BuiltinOperatorDeps {
            interviewer: Some(interviewer),
            ..Default::default()
        },
    );
    let registry = builder.build();

    let previous_state_dir = env::var_os("NEWTON_STATE_DIR");
    env::set_var("NEWTON_STATE_DIR", &task_layout.state_dir);

    let result = workflow_executor::execute_workflow(
        document,
        workflow_path,
        registry,
        workspace,
        overrides,
    )
    .await;

    if let Some(previous) = previous_state_dir {
        env::set_var("NEWTON_STATE_DIR", previous);
    } else {
        env::remove_var("NEWTON_STATE_DIR");
    }

    result
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Workflow execution failed: {e}"))
}

#[derive(Debug)]
struct TaskLayout {
    state_dir: PathBuf,
    input_file: PathBuf,
}

fn prepare_task_layout(batch_config: &BatchProjectConfig, plan_file: &Path) -> Result<TaskLayout> {
    let task_id = plan_file
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Plan file missing stem: {}", plan_file.display()))?;
    let task_root = batch_config
        .project_root
        .join(".newton")
        .join("tasks")
        .join(task_id);
    let input_dir = task_root.join("input");
    let state_dir = task_root.join("state");
    fs::create_dir_all(&input_dir)?;
    fs::create_dir_all(&state_dir)?;
    let input_file = input_dir.join("spec.md");
    fs::copy(plan_file, &input_file)?;
    Ok(TaskLayout {
        state_dir,
        input_file,
    })
}

fn validate_batch_workspace(workspace: Option<PathBuf>) -> Result<PathBuf> {
    let workspace_root = workspace.unwrap_or_else(|| std::env::current_dir().unwrap());
    let configs_dir = workspace_root.join(".newton").join("configs");
    if !configs_dir.is_dir() {
        return Err(anyhow!(
            "Workspace {} must contain .newton/configs",
            workspace_root.display()
        ));
    }
    Ok(workspace_root)
}

async fn fetch_next_task(
    todo_dir: &Path,
    once: bool,
    sleep_duration: u64,
) -> Result<Option<PathBuf>> {
    loop {
        // Simple implementation to get first plan file from todo directory
        let mut entries = fs::read_dir(todo_dir)?;
        if let Some(Ok(entry)) = entries.next() {
            let path = entry.path();
            if path.is_file() {
                return Ok(Some(path));
            }
        }

        if once {
            tracing::info!("Queue empty; exiting after --once");
            return Ok(None);
        }
        sleep_if_needed(sleep_duration).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn build_trigger_payload_returns_none_without_inputs() {
        let payload = build_trigger_payload(&None, &[]).expect("build payload");
        assert!(payload.is_none());
    }

    #[test]
    fn build_trigger_payload_merges_json_and_args_last_wins() {
        let temp = tempdir().expect("tempdir");
        let json_path = temp.path().join("trigger.json");
        fs::write(&json_path, r#"{"prompt":"base","env":"dev"}"#).expect("write trigger json");

        let args = vec![
            KeyValuePair {
                key: "prompt".to_string(),
                value: "override".to_string(),
            },
            KeyValuePair {
                key: "new_key".to_string(),
                value: "new_value".to_string(),
            },
        ];
        let payload =
            build_trigger_payload(&Some(json_path), &args).expect("build trigger payload");
        assert_eq!(
            payload.expect("payload"),
            json!({"prompt":"override","env":"dev","new_key":"new_value"})
        );
    }

    #[test]
    fn build_trigger_payload_reads_arg_value_from_file() {
        let temp = tempdir().expect("tempdir");
        let prompt_path = temp.path().join("prompt.md");
        fs::write(&prompt_path, "line1\nline2\n").expect("write prompt");
        let arg_value = format!("@{}", prompt_path.display());
        let args = vec![KeyValuePair {
            key: "prompt".to_string(),
            value: arg_value,
        }];

        let payload = build_trigger_payload(&None, &args)
            .expect("build trigger payload")
            .expect("payload");
        assert_eq!(payload, json!({"prompt":"line1\nline2\n"}));
    }

    #[test]
    fn build_trigger_payload_supports_literal_at_escape() {
        let args = vec![KeyValuePair {
            key: "prompt".to_string(),
            value: "@@literal".to_string(),
        }];
        let payload = build_trigger_payload(&None, &args)
            .expect("build trigger payload")
            .expect("payload");
        assert_eq!(payload, json!({"prompt":"@literal"}));
    }

    #[test]
    fn build_trigger_payload_rejects_empty_file_path() {
        let args = vec![KeyValuePair {
            key: "prompt".to_string(),
            value: "@".to_string(),
        }];
        let err = build_trigger_payload(&None, &args).expect_err("expected error");
        assert!(
            err.to_string().contains("trigger arg file path is empty"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn build_trigger_payload_rejects_non_utf8_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("binary.bin");
        fs::write(&path, [0xff, 0xfe]).expect("write bytes");
        let arg_value = format!("@{}", path.display());
        let args = vec![KeyValuePair {
            key: "prompt".to_string(),
            value: arg_value,
        }];
        let err = build_trigger_payload(&None, &args).expect_err("expected error");
        assert!(
            err.to_string().contains("failed to read trigger arg file"),
            "unexpected error: {}",
            err
        );
    }
}

#[cfg(test)]
mod duration_formatting_tests {
    use super::*;

    #[test]
    fn format_duration_short_zero() {
        let duration = Duration::from_secs(0);
        assert_eq!(format_duration_short(duration), "0s");
    }

    #[test]
    fn format_duration_short_seconds() {
        let duration = Duration::from_secs(30);
        assert_eq!(format_duration_short(duration), "30s");
    }

    #[test]
    fn format_duration_short_minutes() {
        let duration = Duration::from_secs(150);
        assert_eq!(format_duration_short(duration), "2m");
    }

    #[test]
    fn format_duration_short_hours() {
        let duration = Duration::from_secs(7200);
        assert_eq!(format_duration_short(duration), "2h");
    }

    #[test]
    fn format_duration_short_days() {
        let duration = Duration::from_secs(172800);
        assert_eq!(format_duration_short(duration), "2d");
    }

    #[test]
    fn format_duration_short_hours_minutes() {
        let duration = Duration::from_secs(8100);
        assert_eq!(format_duration_short(duration), "2h 15m");
    }

    #[test]
    fn format_duration_short_days_hours() {
        let duration = Duration::from_secs(205200);
        assert_eq!(format_duration_short(duration), "2d 9h");
    }

    #[test]
    fn format_duration_short_large_duration() {
        let duration = Duration::from_secs(90061);
        assert_eq!(format_duration_short(duration), "1d 1h");
    }

    #[test]
    fn format_datetime_short_format() {
        use chrono::TimeZone;
        let dt = chrono::Utc
            .with_ymd_and_hms(2026, 3, 4, 16, 27, 43)
            .unwrap();
        let formatted = format_datetime_short(&dt);
        assert_eq!(formatted, "2026-03-04 16:27");
    }

    #[test]
    fn format_duration_short_two_units_max() {
        let duration = Duration::from_secs(93784);
        assert_eq!(format_duration_short(duration), "1d 2h");
    }
}

pub async fn data(args: DataArgs) -> anyhow::Result<()> {
    // DATA-001: mutually exclusive --file and --body
    if args.file.is_some() && args.body.is_some() {
        eprintln!("DATA-001: --file and --body are mutually exclusive; provide at most one");
        std::process::exit(1);
    }

    // Resolve workspace and open store
    let workspace = match args.workspace {
        Some(ref p) => p.clone(),
        None => std::env::current_dir()?,
    };
    let db_path = workspace
        .join(".newton")
        .join("state")
        .join("backend.sqlite");
    let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let store = match newton_backend::SqliteBackendStore::new(&db_url).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to open backend store: {}", e.message);
            std::process::exit(1);
        }
    };

    // Parse body if provided
    let body_value: Option<serde_json::Value> = if let Some(ref path) = args.file {
        let raw = if path.to_string_lossy() == "-" {
            use std::io::Read;
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        } else {
            fs::read_to_string(path)?
        };
        match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("DATA-004: invalid JSON in body: {e}");
                std::process::exit(1);
            }
        }
    } else if let Some(ref s) = args.body {
        match serde_json::from_str::<serde_json::Value>(s) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("DATA-004: invalid JSON in --body: {e}");
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    // Normalize resource token (plural -> singular for mutations)
    let resource = args.resource.as_str();

    // Validate resource
    let valid_resources = [
        "product",
        "products",
        "component",
        "components",
        "repo",
        "repos",
        "module",
        "modules",
        "module-dependency",
        "module-dependencies",
    ];
    if !valid_resources.contains(&resource) {
        eprintln!("DATA-003: unknown resource '{resource}'; must be one of: product, products, component, components, repo, repos, module, modules, module-dependency, module-dependencies");
        std::process::exit(1);
    }

    // Require body for POST/PUT/PATCH
    if matches!(args.verb, DataVerb::Post | DataVerb::Put | DataVerb::Patch) && body_value.is_none()
    {
        eprintln!("DATA-005: --file or --body is required for {}", args.verb);
        std::process::exit(1);
    }

    // Require id for single-item GET and all mutations except POST
    let needs_id = match args.verb {
        DataVerb::Get => !matches!(
            resource,
            "products" | "components" | "repos" | "modules" | "module-dependencies"
        ),
        DataVerb::Post => false,
        DataVerb::Put | DataVerb::Patch | DataVerb::Delete => true,
    };
    if needs_id && args.id.is_none() {
        eprintln!("DATA-002: ID is required for {} {}", args.verb, resource);
        std::process::exit(1);
    }

    // Dry-run mode: parse body and print, no DB write
    if args.dry_run {
        if let Some(ref v) = body_value {
            eprintln!("[dry-run] validated payload (no DB write):");
            println!("{}", serde_json::to_string_pretty(v)?);
        } else {
            eprintln!("[dry-run] no body to validate");
        }
        return Ok(());
    }

    // Dispatch
    match dispatch_data(&store, &args.verb, resource, args.id.as_deref(), body_value).await {
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    }
}

async fn dispatch_data(
    store: &newton_backend::SqliteBackendStore,
    verb: &DataVerb,
    resource: &str,
    id: Option<&str>,
    body: Option<serde_json::Value>,
) -> std::result::Result<serde_json::Value, String> {
    fn api_err(e: newton_types::ApiError) -> String {
        format!("{}: {}", e.code, e.message)
    }

    fn to_json<T: serde::Serialize>(v: T) -> std::result::Result<serde_json::Value, String> {
        serde_json::to_value(v).map_err(|e| format!("serialize error: {e}"))
    }

    fn parse_body<T: serde::de::DeserializeOwned>(
        body: Option<serde_json::Value>,
    ) -> std::result::Result<T, String> {
        match body {
            None => Err("body required".to_string()),
            Some(v) => {
                serde_json::from_value(v).map_err(|e| format!("DATA-004: body parse error: {e}"))
            }
        }
    }

    let id = id.unwrap_or("");

    match (verb, resource) {
        // -- Product -----------------------------------------------------------
        (DataVerb::Get, "products") => store
            .list_products()
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "product") => store
            .get_product(id)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Post, "product" | "products") => {
            let b = parse_body::<newton_backend::CreateProductBody>(body)?;
            store
                .create_product(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Put, "product" | "products") => {
            let b = parse_body::<newton_backend::PutProductBody>(body)?;
            store
                .put_product(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Patch, "product" | "products") => {
            let b = parse_body::<newton_backend::PatchProductBody>(body)?;
            store
                .patch_product(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Delete, "product" | "products") => store
            .delete_product(id)
            .await
            .map_err(api_err)
            .and_then(|deleted_id| to_json(serde_json::json!({"id": deleted_id}))),
        // -- Component ---------------------------------------------------------
        (DataVerb::Get, "components") => store
            .list_components()
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "component") => store
            .get_component(id)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Post, "component" | "components") => {
            let b = parse_body::<newton_backend::CreateComponentBody>(body)?;
            store
                .create_component(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Put, "component" | "components") => {
            let b = parse_body::<newton_backend::PutComponentBody>(body)?;
            store
                .put_component(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Patch, "component" | "components") => {
            let b = parse_body::<newton_backend::PatchComponentBody>(body)?;
            store
                .patch_component(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Delete, "component" | "components") => store
            .delete_component(id)
            .await
            .map_err(api_err)
            .and_then(|deleted_id| to_json(serde_json::json!({"id": deleted_id}))),
        // -- Repo --------------------------------------------------------------
        (DataVerb::Get, "repos") => store.list_repos().await.map_err(api_err).and_then(to_json),
        (DataVerb::Get, "repo") => store.get_repo(id).await.map_err(api_err).and_then(to_json),
        (DataVerb::Post, "repo" | "repos") => {
            let b = parse_body::<newton_backend::CreateRepoBody>(body)?;
            store
                .create_repo(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Put, "repo" | "repos") => {
            let b = parse_body::<newton_backend::PutRepoBody>(body)?;
            store
                .put_repo(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Patch, "repo" | "repos") => {
            let b = parse_body::<newton_backend::PatchRepoBody>(body)?;
            store
                .patch_repo(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Delete, "repo" | "repos") => store
            .delete_repo(id)
            .await
            .map_err(api_err)
            .and_then(|deleted_id| to_json(serde_json::json!({"id": deleted_id}))),
        // -- Module ------------------------------------------------------------
        (DataVerb::Get, "modules") => store
            .list_modules()
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "module") => store
            .get_module(id)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Post, "module" | "modules") => {
            let b = parse_body::<newton_backend::CreateModuleBody>(body)?;
            store
                .create_module(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Put, "module" | "modules") => {
            let b = parse_body::<newton_backend::PutModuleBody>(body)?;
            store
                .put_module(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Patch, "module" | "modules") => {
            let b = parse_body::<newton_backend::PatchModuleBody>(body)?;
            store
                .patch_module(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Delete, "module" | "modules") => store
            .delete_module(id)
            .await
            .map_err(api_err)
            .and_then(|deleted_id| to_json(serde_json::json!({"id": deleted_id}))),
        // -- ModuleDependency --------------------------------------------------
        (DataVerb::Get, "module-dependencies") => store
            .list_module_dependencies()
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "module-dependency") => store
            .get_module_dependency(id)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Patch, "module-dependency" | "module-dependencies") => {
            let b = parse_body::<newton_backend::PatchModuleDependencyBody>(body)?;
            store
                .patch_module_dependency(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Delete, "module-dependency" | "module-dependencies") => store
            .delete_module_dependency(id)
            .await
            .map_err(api_err)
            .and_then(|deleted_id| to_json(serde_json::json!({"id": deleted_id}))),
        (v, r) => Err(format!("unsupported combination: {v} {r}")),
    }
}
