#![allow(clippy::result_large_err)] // CLI command handlers return AppError directly to preserve diagnostic context without boxing.

use crate::cli::args::{
    ArtifactCommand, ArtifactsArgs, BatchArgs, CheckpointCommand, CheckpointsArgs, DotArgs,
    ExplainArgs, KeyValuePair, LintArgs, MonitorArgs, OutputFormat, ResumeArgs, RunArgs, ServeArgs,
    ValidateArgs, WebhookArgs, WebhookCommand, WebhookServeArgs, WebhookStatusArgs,
};
use crate::core::batch_config::BatchProjectConfig;
use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::monitor;
use crate::workflow::operator::OperatorRegistry;
use crate::workflow::{
    artifacts, checkpoint, dot as workflow_dot,
    executor::{self as workflow_executor, ExecutionOverrides},
    explain,
    expression::ExpressionEngine,
    lint::{LintRegistry, LintResult, LintSeverity},
    operators as workflow_operators, schema as workflow_schema,
    server_notifier::ServerNotifier,
    transform as workflow_transform, webhook,
};
use crate::Result;
use anyhow::anyhow;
use clap::CommandFactory;
use humantime::{format_duration, parse_duration};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
    result::Result as StdResult,
    time::Duration,
};
use tokio::time::sleep;

/// Print help for a specific command by name
/// For nested commands (e.g., "webhook serve"), displays the parent help
fn print_help_for_command(command_name: &str) {
    let mut cmd = crate::cli::Args::command();

    // Get the first part of the command name (parent command)
    let parts: Vec<&str> = command_name.split_whitespace().collect();
    let main_command = parts.first().unwrap_or(&command_name);

    // Find and print help for the main command
    // For nested commands, this will show the parent help which includes subcommand list
    if let Some(subcommand) = cmd.find_subcommand_mut(main_command) {
        let _ = subcommand.print_long_help();
    }
}

/// Load and validate a workflow document from the given arguments
fn load_and_validate_workflow(
    args: &RunArgs,
) -> Result<(workflow_schema::WorkflowDocument, PathBuf)> {
    let workflow_path = args.resolved_workflow_path().ok_or_else(|| {
        print_help_for_command("run");
        anyhow!("missing workflow file; pass WORKFLOW or --file PATH")
    })?;
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
            "workflow lint detected {} error(s); fix before running",
            error_count
        ));
    }

    apply_context_overrides(&mut document.workflow.context, &args.set);
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
        build_trigger_payload(&args.trigger_json, &args.arg)?.unwrap_or_else(|| json!({}));

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
        max_time_seconds: args.max_time_seconds,
        checkpoint_base_path: None,
        artifact_base_path: None,
        verbose: args.verbose,
        server_notifier,
        pre_seed_nodes: true,
    };

    let mut builder = OperatorRegistry::builder();
    workflow_operators::register_builtins(&mut builder, workspace.to_path_buf(), settings.clone());
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

    let (overrides, registry) =
        setup_workflow_execution(&args, &workspace, &document.workflow.settings);

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

pub async fn workflow_run(args: RunArgs) -> StdResult<(), AppError> {
    let workflow_path = args.resolved_workflow_path().ok_or_else(|| {
        print_help_for_command("run");
        AppError::new(
            ErrorCategory::ValidationError,
            "missing workflow file; pass WORKFLOW or --file PATH",
        )
    })?;
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
            format!(
                "workflow lint detected {} error(s); fix before running",
                error_count
            ),
        ));
    }
    apply_context_overrides(&mut document.workflow.context, &args.set);
    document.validate(&ExpressionEngine::default())?;

    if let Some(payload) = build_trigger_payload(&args.trigger_json, &args.arg)? {
        document.triggers = Some(workflow_schema::WorkflowTrigger {
            trigger_type: workflow_schema::TriggerType::Manual,
            schema_version: "1".to_string(),
            payload,
        });
    }

    let server_notifier = args
        .server
        .as_ref()
        .map(|url| std::sync::Arc::new(ServerNotifier::new(url.clone())));

    let overrides = ExecutionOverrides {
        parallel_limit: args.parallel_limit,
        max_time_seconds: args.max_time_seconds,
        checkpoint_base_path: None,
        artifact_base_path: None,
        verbose: false,
        server_notifier,
        pre_seed_nodes: true,
    };

    let mut builder = OperatorRegistry::builder();
    workflow_operators::register_builtins(
        &mut builder,
        workspace.clone(),
        document.workflow.settings.clone(),
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
    let workflow_path = args.resolved_workflow_path().ok_or_else(|| {
        print_help_for_command("validate");
        AppError::new(
            ErrorCategory::ValidationError,
            "missing workflow file; pass WORKFLOW or --file PATH",
        )
    })?;
    let document = workflow_schema::load_workflow(&workflow_path)?;
    let unreachable = workflow_dot::reachability_warnings(&document);
    for id in &unreachable {
        eprintln!("warning: task '{}' is not reachable from entry_task", id);
    }
    println!("Workflow definition is valid");
    Ok(())
}

pub fn dot(args: DotArgs) -> StdResult<(), AppError> {
    let workflow_path = args.resolved_workflow_path().ok_or_else(|| {
        print_help_for_command("dot");
        AppError::new(
            ErrorCategory::ValidationError,
            "missing workflow file; pass WORKFLOW or --file PATH",
        )
    })?;
    let document = workflow_schema::load_workflow(&workflow_path)?;
    let dot = workflow_dot::workflow_to_dot(&document);
    if let Some(path) = args.out {
        fs::write(path, dot).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to write DOT: {}", err),
            )
        })?;
    } else {
        println!("{}", dot);
    }
    Ok(())
}

pub fn lint(args: LintArgs) -> StdResult<(), AppError> {
    let workflow_path = args.resolved_workflow_path().ok_or_else(|| {
        print_help_for_command("lint");
        AppError::new(
            ErrorCategory::ValidationError,
            "missing workflow file; pass WORKFLOW or --file PATH",
        )
    })?;
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
            format!("workflow lint found {} error(s)", error_count),
        ));
    }
    Ok(())
}

pub fn explain(args: ExplainArgs) -> StdResult<(), AppError> {
    let workflow_path = args.resolved_workflow_path().ok_or_else(|| {
        print_help_for_command("explain");
        AppError::new(
            ErrorCategory::ValidationError,
            "missing workflow file; pass WORKFLOW or --file PATH",
        )
    })?;
    let _workspace = resolve_workflow_workspace(args.workspace)?;
    let raw_document = workflow_schema::parse_workflow(&workflow_path)?;
    let source_tasks = raw_document.workflow.tasks.len();
    let source_macro_invocations = raw_document.workflow.macro_invocation_count();
    let source_macro_names = raw_document.workflow.macro_names_referenced();
    let mut document = workflow_transform::apply_default_pipeline(raw_document)?;
    let overrides = parse_set_overrides(&args.set);
    let trigger_payload = build_trigger_payload(&args.trigger_json, &args.arg)?
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
    let execution = checkpoint::load_execution(&workspace, &args.execution_id)?;
    let mut builder = OperatorRegistry::builder();
    workflow_operators::register_builtins(
        &mut builder,
        workspace.clone(),
        execution.settings_effective.clone(),
    );
    let registry = builder.build();
    let summary = workflow_executor::resume_workflow(
        registry,
        workspace.clone(),
        args.execution_id,
        args.allow_workflow_change,
    )
    .await?;
    println!(
        "Workflow resumed (execution {}) in {} iterations",
        summary.execution_id, summary.total_iterations
    );
    Ok(())
}

pub fn checkpoints(args: CheckpointsArgs) -> StdResult<(), AppError> {
    match args.command {
        CheckpointCommand::List {
            workspace,
            format_json,
        } => workflow_checkpoints_list(workspace, format_json),
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
        parts.push(format!("{}d", days));
        remaining %= SECONDS_PER_DAY;
    }

    if remaining >= SECONDS_PER_HOUR && parts.len() < 2 {
        let hours = remaining / SECONDS_PER_HOUR;
        parts.push(format!("{}h", hours));
        remaining %= SECONDS_PER_HOUR;
    }

    if remaining >= SECONDS_PER_MINUTE && parts.len() < 2 {
        let minutes = remaining / SECONDS_PER_MINUTE;
        parts.push(format!("{}m", minutes));
        remaining %= SECONDS_PER_MINUTE;
    }

    if parts.is_empty() && parts.len() < 2 {
        parts.push(format!("{}s", remaining));
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
                format!("failed to serialize checkpoint list: {}", err),
            )
        })?;
        println!("{}", serialized);
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
    println!("Removed checkpoints older than {}", older_than);
    Ok(())
}

pub fn artifacts(args: ArtifactsArgs) -> StdResult<(), AppError> {
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
    println!("Cleaned artifacts older than {}", older_than);
    Ok(())
}

pub async fn webhook(args: WebhookArgs) -> StdResult<(), AppError> {
    match args.command {
        WebhookCommand::Serve(serve_args) => workflow_webhook_serve(serve_args).await,
        WebhookCommand::Status(status_args) => workflow_webhook_status(status_args),
    }
}

async fn workflow_webhook_serve(args: WebhookServeArgs) -> StdResult<(), AppError> {
    let workflow_path = args.resolved_workflow_path().ok_or_else(|| {
        print_help_for_command("webhook");
        AppError::new(
            ErrorCategory::ValidationError,
            "missing workflow file; pass WORKFLOW or --file PATH",
        )
    })?;
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
            format!(
                "workflow lint detected {} error(s); fix before starting webhook",
                error_count
            ),
        ));
    }
    document.validate(&ExpressionEngine::default())?;

    let mut builder = OperatorRegistry::builder();
    workflow_operators::register_builtins(
        &mut builder,
        workspace.clone(),
        document.workflow.settings.clone(),
    );
    let registry = builder.build();
    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: None,
        artifact_base_path: None,
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
    let resolved_workflow = args.resolved_workflow_path();
    let workspace = resolve_workflow_workspace(Some(args.workspace))?;
    let workflow_path = resolve_workspace_workflow_path(&workspace, resolved_workflow)?;
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
            format!("failed to parse duration {}: {}", value, err),
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

fn resolve_workflow_workspace(path: Option<PathBuf>) -> StdResult<PathBuf, AppError> {
    match path {
        Some(p) => Ok(p),
        None => Ok(env::current_dir().map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to resolve workspace path: {}", err),
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
            println!("  Suggestion: {}", suggestion);
        }
    }
}

fn print_lint_results_json(results: &[LintResult]) -> StdResult<(), AppError> {
    let payload = json!({ "results": results });
    let serialized = serde_json::to_string_pretty(&payload).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize lint results: {}", err),
        )
    })?;
    println!("{}", serialized);
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
            println!("Source: {} tasks, 0 macro invocations", task_count);
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
            format!("failed to serialize explain output: {}", err),
        )
    })?;
    println!("{}", serialized);
    Ok(())
}

fn print_explain_prose(output: &explain::ExplainOutput) -> StdResult<(), AppError> {
    let prose = explain::format_explain_prose(output)?;
    println!("{}", prose);
    Ok(())
}

fn pretty_json(value: &impl Serialize) -> StdResult<String, AppError> {
    serde_json::to_string_pretty(value).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize explain section: {}", err),
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

fn build_trigger_payload(
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
        return Ok(Value::String(format!("@{}", path)));
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
                format!("failed to read trigger arg file {}: {}", path, err),
            )
        })?;
        return Ok(Value::String(content));
    }

    Ok(Value::String(value.to_string()))
}

fn load_trigger_payload(path: &Path) -> StdResult<Value, AppError> {
    let content = fs::read_to_string(path).map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to read trigger JSON {}: {}", path.display(), err),
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

/// Launch the Newton HTTP API server
pub async fn serve(args: ServeArgs) -> StdResult<(), AppError> {
    use crate::api::{self, state::AppState};
    use crate::workflow::operators;
    use std::net::SocketAddr;
    use tokio::net::TcpListener;
    use tracing::info;

    info!("Starting Newton API server on {}: {}", args.host, args.port);

    let mut builder = OperatorRegistry::builder();
    operators::register_builtins(
        &mut builder,
        std::path::PathBuf::from("."),
        Default::default(),
    );
    let registry = builder.build();

    let operator_names = registry.operator_names();
    let operator_descriptors: Vec<newton_types::OperatorDescriptor> = operator_names
        .iter()
        .map(|name: &String| newton_types::OperatorDescriptor {
            operator_type: name.clone(),
            description: format!("{} operator", name),
            params_schema: serde_json::json!({}),
        })
        .collect();

    let state = AppState::new(operator_descriptors);
    let app = api::create_router(state, args.ui_dir.clone());

    let addr = format!("{}:{}", args.host, args.port);
    let socket_addr: SocketAddr = addr.parse().map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::ValidationError,
            format!("invalid bind address: {}", err),
        )
    })?;

    let listener = TcpListener::bind(&socket_addr).await.map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::IoError,
            format!("failed to bind to {}: {}", addr, err),
        )
    })?;

    info!("Newton API server listening on {}", socket_addr);

    axum::serve(listener, app.into_make_service())
        .await
        .map_err(|err| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!("server error: {}", err),
            )
        })?;

    Ok(())
}

/// Launch the interactive Newton monitor TUI that watches ailoop channels.
pub async fn monitor(args: MonitorArgs) -> Result<()> {
    tracing::info!("Starting Newton monitor TUI");
    monitor::run(args).await
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
        let plan_file = fetch_next_task(&dirs.todo_dir, args.once, args.sleep).await?;
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
            sleep_if_needed(args.sleep).await;
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
        verbose: false,
        server_notifier: None,
        pre_seed_nodes: true,
    };

    let mut builder = OperatorRegistry::builder();
    workflow_operators::register_builtins(
        &mut builder,
        workspace.clone(),
        document.workflow.settings.clone(),
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
        .map_err(|e| anyhow::anyhow!("Workflow execution failed: {}", e))
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
