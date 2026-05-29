#![allow(clippy::result_large_err)]

pub mod batch;
pub mod data;
pub mod import;
pub mod log;
pub mod serve;

use crate::cli::args::{
    ArtifactArgs, ArtifactCommand, CheckpointArgs, CheckpointCommand, DotArgs, ExplainArgs,
    KeyValuePair, LintArgs, OutputFormat, ResumeArgs, RunArgs, ValidateArgs, WebhookArgs,
    WebhookCommand, WebhookServeArgs, WebhookStatusArgs,
};
use crate::cli::workspace_paths::{
    resolve_state_dir, state_artifacts_dir, state_backend_sqlite_url, state_checkpoints_dir,
};
use crate::Result;
use anyhow::anyhow;
use humantime::format_duration;
use log::format_bytes;
use log::format_datetime_short;
use log::format_duration_short;
use log::parse_duration_arg;
use newton_backend::SqliteBackendStore;
use newton_core::core::error::AppError;
use newton_core::core::types::ErrorCategory;
use newton_core::workflow::operator::OperatorRegistry;
use newton_core::workflow::{
    artifacts, checkpoint, dot as workflow_dot,
    executor::{self as workflow_executor, ExecutionOverrides},
    explain,
    expression::ExpressionEngine,
    lint::{LintRegistry, LintResult, LintSeverity},
    operators as workflow_operators, schema as workflow_schema,
    server_notifier::ServerNotifier,
    transform as workflow_transform, webhook,
    workflow_sink::{DbSink, FanoutSink, WorkflowSink},
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

pub use batch::batch;
pub use data::data;
pub use import::workflow_import;
pub use log::log;
pub use serve::serve;

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

fn build_operator_registry(
    workspace: PathBuf,
    settings: &workflow_schema::WorkflowSettings,
    ailoop_ctx: Option<newton_core::integrations::ailoop::AiloopContext>,
) -> OperatorRegistry {
    let mut builder = OperatorRegistry::builder();
    let interviewer = newton_core::workflow::human::lazy_interviewer_provider(
        ailoop_ctx,
        Duration::from_secs(settings.human.default_timeout_seconds),
    );
    workflow_operators::register_builtins_with_deps(
        &mut builder,
        workspace,
        settings.clone(),
        workflow_operators::BuiltinOperatorDeps {
            interviewer: Some(interviewer),
            ..Default::default()
        },
    );
    builder.build()
}

#[allow(dead_code)]
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

#[allow(dead_code)]
fn build_comprehensive_trigger_payload(
    args: &RunArgs,
    workspace: &std::path::Path,
) -> Result<Option<Value>> {
    let mut trigger_payload =
        build_trigger_payload(&args.parameters_json, &args.trigger)?.unwrap_or_else(|| json!({}));

    if let Some(input_file) = &args.input_file {
        let input_file_path = if input_file.is_absolute() {
            input_file.clone()
        } else {
            std::env::current_dir()?.join(input_file)
        };
        trigger_payload["input_file"] = json!(input_file_path.display().to_string());
    }

    trigger_payload["workspace"] = json!(workspace.display().to_string());

    if trigger_payload.as_object().unwrap().is_empty() {
        Ok(None)
    } else {
        Ok(Some(trigger_payload))
    }
}

#[allow(dead_code)]
fn setup_workflow_execution(
    args: &RunArgs,
    workspace: &std::path::Path,
    settings: &workflow_schema::WorkflowSettings,
    state_dir: &std::path::Path,
    sink: Option<Arc<dyn WorkflowSink>>,
) -> (ExecutionOverrides, OperatorRegistry) {
    let overrides = ExecutionOverrides {
        parallel_limit: args.parallel_limit,
        max_time_seconds: args.timeout_seconds,
        checkpoint_base_path: Some(state_checkpoints_dir(state_dir)),
        artifact_base_path: Some(state_artifacts_dir(state_dir)),
        max_nesting_depth: None,
        verbose: args.verbose,
        sink,
        pre_seed_nodes: true,
    };

    let ailoop_ctx =
        newton_core::integrations::ailoop::init_context_for_command_name(workspace, "run")
            .ok()
            .flatten();
    let registry = build_operator_registry(workspace.to_path_buf(), settings, ailoop_ctx);

    (overrides, registry)
}

async fn execute_run_command(args: &RunArgs) -> StdResult<(), AppError> {
    let emit_json = args.emit_completion_json;
    let workflow_path = args.workflow.clone();
    let workspace = resolve_workflow_workspace(args.workspace.clone())?;
    let state_dir = resolve_state_dir(&workspace, args.state_dir.as_deref());
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
                if emit_json {
                    let envelope = newton_core::workflow::io::CompletionEnvelope::internal_error(
                        newton_core::workflow::io::CompletionError {
                            code: Some("WFG-IO-001".to_string()),
                            category: ErrorCategory::ValidationError.to_string(),
                            message: err.message.clone(),
                            error_payload: None,
                        },
                    );
                    println!("{}", serde_json::to_string(&envelope).unwrap_or_default());
                    std::process::exit(1);
                }
                return Err(err);
            }
        }
        if let Some(schema) = &settings.io.input_schema {
            if let Err(e) = newton_core::workflow::io::validate_input_schema(schema, payload) {
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
                return Err(e);
            }
        }
    }
    let io_settings = document.workflow.settings.io_settings.clone();
    let io_block = document.workflow.settings.io.clone();

    if state_dir.exists() && !state_dir.is_dir() {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "STATE-DIR-001: --state-dir path exists but is not a directory: {}",
                state_dir.display()
            ),
        )
        .with_code("STATE-DIR-001"));
    }
    fs::create_dir_all(state_checkpoints_dir(&state_dir)).map_err(|e| {
        AppError::new(
            ErrorCategory::IoError,
            format!("STATE-DIR-002: failed to create state directory: {}", e),
        )
        .with_code("STATE-DIR-002")
    })?;
    fs::create_dir_all(state_artifacts_dir(&state_dir)).map_err(|e| {
        AppError::new(
            ErrorCategory::IoError,
            format!("STATE-DIR-002: failed to create artifacts directory: {}", e),
        )
        .with_code("STATE-DIR-002")
    })?;

    let backend = SqliteBackendStore::new(&state_backend_sqlite_url(&state_dir))
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("STATE-DIR-003: backend store init failed: {}", e.message),
            )
            .with_code("STATE-DIR-003")
        })?;
    let backend_arc: Arc<dyn newton_backend::BackendStore> = Arc::new(backend);
    let db_sink = Arc::new(DbSink::new(backend_arc));
    let sink: Option<Arc<dyn WorkflowSink>> = if let Some(url) = &args.server {
        Some(Arc::new(FanoutSink(vec![
            db_sink as Arc<dyn WorkflowSink>,
            Arc::new(ServerNotifier::new(url.clone())),
        ])))
    } else {
        Some(db_sink as Arc<dyn WorkflowSink>)
    };

    let overrides = ExecutionOverrides {
        parallel_limit: args.parallel_limit,
        max_time_seconds: args.timeout_seconds,
        checkpoint_base_path: Some(state_checkpoints_dir(&state_dir)),
        artifact_base_path: Some(state_artifacts_dir(&state_dir)),
        max_nesting_depth: None,
        verbose: false,
        sink,
        pre_seed_nodes: true,
    };

    let settings = document.workflow.settings.clone();
    let ailoop_ctx =
        newton_core::integrations::ailoop::init_context_for_command_name(&workspace, "run")
            .ok()
            .flatten();
    let registry = build_operator_registry(workspace.clone(), &settings, ailoop_ctx);

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
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        e.message.clone(),
                    ));
                }
            }
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
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        "output exceeds max_output_bytes: WFG-IO-003".to_string(),
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
                std::process::exit(exit_code);
            }
            Err(app_error)
        }
    }
}

pub async fn run(args: RunArgs) -> Result<()> {
    execute_run_command(&args).await.map_err(|e| anyhow!("{e}"))
}

pub async fn workflow_run(args: RunArgs) -> StdResult<(), AppError> {
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
    let state_dir = resolve_state_dir(&workspace, args.state_dir.as_deref());
    let execution =
        checkpoint::load_execution_from_base(&state_checkpoints_dir(&state_dir), &args.run_id)?;
    let settings = execution.settings_effective.clone();
    let registry = build_operator_registry(workspace.clone(), &settings, None);
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
        CheckpointCommand::List {
            workspace,
            state_dir,
            json,
        } => workflow_checkpoints_list(workspace, state_dir, json),
        CheckpointCommand::Clean {
            workspace,
            state_dir,
            older_than,
        } => workflow_checkpoints_clean(workspace, state_dir, older_than),
    }
}

fn workflow_checkpoints_list(
    workspace: Option<PathBuf>,
    state_dir: Option<PathBuf>,
    format_json: bool,
) -> StdResult<(), AppError> {
    let workspace = resolve_workflow_workspace(workspace)?;
    let state_dir = resolve_state_dir(&workspace, state_dir.as_deref());
    let mut entries = checkpoint::list_checkpoints_at(&state_checkpoints_dir(&state_dir))?;

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
    state_dir: Option<PathBuf>,
    older_than: String,
) -> StdResult<(), AppError> {
    let workspace = resolve_workflow_workspace(workspace)?;
    let state_dir = resolve_state_dir(&workspace, state_dir.as_deref());
    let duration = parse_duration_arg(&older_than)?;
    checkpoint::clean_checkpoints_at(&state_checkpoints_dir(&state_dir), duration)?;
    println!("Removed checkpoints older than {older_than}");
    Ok(())
}

pub fn artifacts(args: ArtifactArgs) -> StdResult<(), AppError> {
    match args.command {
        ArtifactCommand::Clean {
            workspace,
            state_dir,
            older_than,
        } => workflow_artifacts_clean(workspace, state_dir, older_than),
    }
}

fn workflow_artifacts_clean(
    workspace: Option<PathBuf>,
    state_dir: Option<PathBuf>,
    older_than: String,
) -> StdResult<(), AppError> {
    let workspace = resolve_workflow_workspace(workspace)?;
    let state_dir = resolve_state_dir(&workspace, state_dir.as_deref());
    let duration = parse_duration_arg(&older_than)?;
    artifacts::ArtifactStore::clean_artifacts_at(
        &state_artifacts_dir(&state_dir),
        &state_checkpoints_dir(&state_dir),
        duration,
    )?;
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

    let settings = document.workflow.settings.clone();
    let registry = build_operator_registry(workspace.clone(), &settings, None);
    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        sink: None,
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
