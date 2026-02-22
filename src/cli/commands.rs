#![allow(clippy::result_large_err)] // CLI command handlers return AppError directly to preserve diagnostic context without boxing.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::{
    ailoop_integration,
    cli::args::{
        BatchArgs, ErrorArgs, KeyValuePair, MonitorArgs, OutputFormat, ReportArgs, RunArgs,
        StatusArgs, StepArgs, WorkflowArgs, WorkflowArtifactCommand, WorkflowArtifactsArgs,
        WorkflowCheckpointCommand, WorkflowCheckpointsArgs, WorkflowCommand, WorkflowDotArgs,
        WorkflowExplainArgs, WorkflowLintArgs, WorkflowResumeArgs, WorkflowRunArgs,
        WorkflowValidateArgs,
    },
    cli::Command as CliCommand,
    core::{
        batch_config::{find_workspace_root, BatchProjectConfig},
        entities::ExecutionStatus,
        workflow_graph::{
            artifacts, checkpoint, dot as workflow_dot,
            executor::{self as workflow_executor, ExecutionOverrides},
            explain,
            lint::{LintRegistry, LintResult, LintSeverity},
            operator::OperatorRegistry,
            operators as workflow_operators, schema as workflow_schema,
        },
        ConfigLoader, ConfigValidator, DefaultErrorReporter, OptimizationOrchestrator,
        SuccessPolicy,
    },
    monitor,
    utils::serialization::{FileUtils, JsonSerializer},
    Result,
};
use anyhow::{anyhow, Error};
use chrono::Utc;
use humantime::{format_duration, parse_duration};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::{
    collections::HashMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    result::Result as StdResult,
    time::Duration,
};
use tokio::time::sleep;

/// Context struct to hold batch task information, reducing function argument counts
struct BatchTaskContext {
    batch_config: BatchProjectConfig,
    project_id: String,
    task_id: String,
    goal_file: PathBuf,
    workspace_root: PathBuf,
    state_dir: PathBuf,
    control_file: PathBuf,
    branch_name: String,
    base_branch: String,
}

/// Build additional environment variables for the run, returns (goal_file, env map)
fn build_run_additional_env(
    args: &RunArgs,
    workspace_path: &Path,
) -> Result<(Option<PathBuf>, HashMap<String, String>)> {
    let goal_file = prepare_goal_file(args, workspace_path)?;

    let user_feedback = args
        .feedback
        .clone()
        .or_else(|| env::var("NEWTON_USER_FEEDBACK").ok());

    let mut additional_env = HashMap::new();
    if let Some(ref path) = goal_file {
        additional_env.insert("NEWTON_GOAL_FILE".to_string(), path.display().to_string());
    }
    if let Some(ref feedback) = user_feedback {
        additional_env.insert("NEWTON_USER_FEEDBACK".to_string(), feedback.clone());
    }

    let control_file_path = args
        .control_file
        .clone()
        .unwrap_or_else(|| workspace_path.join("newton_control.json"));
    additional_env.insert(
        "NEWTON_CONTROL_FILE".to_string(),
        control_file_path.display().to_string(),
    );

    for key in &[
        "NEWTON_STATE_DIR",
        "NEWTON_WS_ROOT",
        "NEWTON_CODER_CMD",
        "NEWTON_BRANCH_NAME",
    ] {
        if let Ok(value) = env::var(key) {
            additional_env.insert(key.to_string(), value);
        }
    }

    Ok((goal_file, additional_env))
}

/// Build the execution configuration for the run
fn build_run_exec_config(
    args: &RunArgs,
    workspace_path: &Path,
) -> crate::core::entities::ExecutionConfiguration {
    let evaluator_cmd = args
        .evaluator_cmd
        .clone()
        .or_else(|| default_workspace_script(workspace_path, "evaluator.sh"));
    let advisor_cmd = args
        .advisor_cmd
        .clone()
        .or_else(|| default_workspace_script(workspace_path, "advisor.sh"));
    let executor_cmd = args
        .executor_cmd
        .clone()
        .or_else(|| default_workspace_script(workspace_path, "executor.sh"));
    let strict_toolchain_mode =
        args.evaluator_cmd.is_some() || args.advisor_cmd.is_some() || args.executor_cmd.is_some();

    crate::core::entities::ExecutionConfiguration {
        evaluator_cmd,
        advisor_cmd,
        executor_cmd,
        max_iterations: Some(args.max_iterations),
        max_time_seconds: Some(args.max_time),
        evaluator_timeout_ms: args.evaluator_timeout.map(|t| t * 1000),
        advisor_timeout_ms: args.advisor_timeout.map(|t| t * 1000),
        executor_timeout_ms: args.executor_timeout.map(|t| t * 1000),
        global_timeout_ms: Some(args.max_time * 1000),
        strict_toolchain_mode,
        resource_monitoring: false,
        verbose: args.verbose,
    }
}

/// Report the result of the run execution
fn report_run_result(execution: crate::core::entities::OptimizationExecution) -> Result<()> {
    if execution.status != ExecutionStatus::Completed
        && execution.status != ExecutionStatus::MaxIterationsReached
    {
        return Err(anyhow!(
            "Optimization ended with status {:?}",
            execution.status
        ));
    }
    tracing::info!("Optimization completed successfully");
    println!(
        "Optimization completed with {} iterations",
        execution.total_iterations_completed
    );
    if let Some(completed_at) = execution.completed_at {
        println!(
            "Duration: {}",
            completed_at.signed_duration_since(execution.started_at)
        );
    }
    Ok(())
}

pub async fn run(args: RunArgs) -> Result<()> {
    tracing::info!("Starting Newton Loop optimization run");

    match resolve_run_mode(&args) {
        Ok(RunMode::Workspace(workspace_path)) => run_workspace(args, workspace_path)
            .await
            .map_err(|err| anyhow!(err)),
        Ok(RunMode::Profile(context)) => run_profile_mode(args, *context)
            .await
            .map_err(|err| anyhow!(err)),
        Err(err) => Err(anyhow!(err)),
    }
}

async fn run_workspace(args: RunArgs, workspace_path: PathBuf) -> Result<()> {
    let newton_config = if let Some(ref path) = args.config {
        ConfigLoader::load_from_file(path)?.unwrap_or_default()
    } else {
        ConfigLoader::load_from_workspace(&workspace_path)?
    };

    ConfigValidator::validate(&newton_config)?;

    let (_goal_file, additional_env) = build_run_additional_env(&args, &workspace_path)?;
    let exec_config = build_run_exec_config(&args, &workspace_path);

    let control_file_path = args
        .control_file
        .clone()
        .unwrap_or_else(|| workspace_path.join("newton_control.json"));
    let success_policy = SuccessPolicy::from_path(control_file_path.clone());

    // Initialize ailoop context
    let ailoop_context =
        match ailoop_integration::init_context(&workspace_path, &CliCommand::Run(args.clone())) {
            Ok(ctx) => {
                if let Some(ref context) = ctx {
                    tracing::info!(
                        "Ailoop integration enabled for channel: {}",
                        context.channel()
                    );
                }
                ctx.map(Arc::new)
            }
            Err(e) => {
                tracing::warn!("Failed to initialize ailoop integration: {}", e);
                None
            }
        };

    let reporter = Box::new(DefaultErrorReporter);
    let orchestrator = OptimizationOrchestrator::with_ailoop_context(
        JsonSerializer,
        FileUtils,
        reporter,
        ailoop_context,
    );

    let execution = orchestrator
        .run_optimization_with_policy(
            &workspace_path,
            exec_config,
            &additional_env,
            Some(&success_policy),
        )
        .await;

    match execution {
        Ok(execution) => report_run_result(execution),
        Err(e) => {
            tracing::error!("Optimization failed: {}", e);
            eprintln!("Optimization failed: {}", e);
            Err(e.into())
        }
    }
}

async fn run_profile_mode(_args: RunArgs, context: ProfileRunContext) -> StdResult<(), AppError> {
    tracing::info!("Running Newton project profile '{}'", context.project_id);

    let env_pairs = [
        ("CODING_AGENT", context.batch_config.coding_agent.as_str()),
        (
            "CODING_AGENT_MODEL",
            context.batch_config.coding_model.as_str(),
        ),
        (
            "NEWTON_EXECUTOR_CODING_AGENT",
            context.batch_config.coding_agent.as_str(),
        ),
        (
            "NEWTON_EXECUTOR_CODING_AGENT_MODEL",
            context.batch_config.coding_model.as_str(),
        ),
    ];
    let overrides = apply_env_overrides(&env_pairs);

    let result = run_profile_inner(&context).await;
    restore_env_vars(overrides);

    result
}

async fn run_profile_inner(context: &ProfileRunContext) -> StdResult<(), AppError> {
    let hook_env = build_profile_hook_env(context);
    run_profile_pre_hook(context, &hook_env)?;

    let run_args = RunArgs::for_batch_with_tools(
        context.batch_config.project_root.clone(),
        None,
        context.batch_config.evaluator_cmd.clone(),
        context.batch_config.advisor_cmd.clone(),
        context.batch_config.executor_cmd.clone(),
        context.batch_config.max_iterations,
        context.batch_config.max_time,
        context.batch_config.verbose,
        Some(context.control_file_path.clone()),
    );
    if let Err(err) = run_workspace(run_args, context.batch_config.project_root.clone()).await {
        run_profile_post_fail_hook(context, &hook_env);
        return Err(err.into());
    }

    if let Some(script) = context.batch_config.post_success_script.as_ref() {
        let status = run_batch_hook_script(&context.batch_config.project_root, script, &hook_env)?;
        if !status.success() {
            run_profile_post_fail_hook(context, &hook_env);
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!(
                    "post_success_script '{}' exited {}",
                    script,
                    status.code().unwrap_or(-1)
                ),
            )
            .with_code("WFG-RUN-POST-001"));
        }
    }

    Ok(())
}

fn run_profile_pre_hook(
    context: &ProfileRunContext,
    hook_env: &HashMap<String, String>,
) -> StdResult<(), AppError> {
    if let Some(script) = context.batch_config.pre_run_script.as_ref() {
        let status = run_batch_hook_script(&context.batch_config.project_root, script, hook_env)?;
        if !status.success() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!(
                    "pre_run_script '{}' exited {}",
                    script,
                    status.code().unwrap_or(-1)
                ),
            )
            .with_code("WFG-RUN-PRE-001"));
        }
    }
    Ok(())
}

fn run_profile_post_fail_hook(context: &ProfileRunContext, hook_env: &HashMap<String, String>) {
    if let Some(script) = context.batch_config.post_fail_script.as_ref() {
        match run_batch_hook_script(&context.batch_config.project_root, script, hook_env) {
            Ok(status) => {
                if !status.success() {
                    tracing::warn!(
                        "post_fail_script '{}' exited {}",
                        script,
                        status.code().unwrap_or(-1)
                    );
                }
            }
            Err(err) => {
                tracing::warn!("post_fail_script '{}' failed to start: {}", script, err);
            }
        }
    }
}

fn build_profile_hook_env(context: &ProfileRunContext) -> HashMap<String, String> {
    let mut env_vars = HashMap::new();
    env_vars.insert(
        "CODING_AGENT".to_string(),
        context.batch_config.coding_agent.clone(),
    );
    env_vars.insert(
        "CODING_AGENT_MODEL".to_string(),
        context.batch_config.coding_model.clone(),
    );
    env_vars.insert(
        "NEWTON_EXECUTOR_CODING_AGENT".to_string(),
        context.batch_config.coding_agent.clone(),
    );
    env_vars.insert(
        "NEWTON_EXECUTOR_CODING_AGENT_MODEL".to_string(),
        context.batch_config.coding_model.clone(),
    );
    env_vars.insert(
        "NEWTON_PROJECT_ROOT".to_string(),
        context.batch_config.project_root.display().to_string(),
    );
    env_vars.insert("NEWTON_PROJECT_ID".to_string(), context.project_id.clone());
    env_vars.insert(
        "NEWTON_WS_ROOT".to_string(),
        context.workspace_root.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_CONTROL_FILE".to_string(),
        context.control_file_path.display().to_string(),
    );
    env_vars
}

fn resolve_run_mode(args: &RunArgs) -> StdResult<RunMode, AppError> {
    let workspace_candidate = args.path.clone();
    let base_message = match fs::metadata(&workspace_candidate) {
        Ok(metadata) => {
            if metadata.is_dir() {
                return Ok(RunMode::Workspace(workspace_candidate));
            }
            format!(
                "workspace path {} is not a directory",
                workspace_candidate.display()
            )
        }
        Err(err) => format!("workspace path {}: {}", workspace_candidate.display(), err),
    };

    let current_dir = env::current_dir().map_err(|err| {
        AppError::with_source(
            ErrorCategory::IoError,
            "failed to determine current directory",
            Box::new(err),
        )
    })?;
    let workspace_root = find_workspace_root(&current_dir).map_err(|err| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("{}; workspace root discovery failed: {}", base_message, err),
        )
    })?;

    let project_id = workspace_candidate.display().to_string();
    let conf_path = workspace_root
        .join(".newton")
        .join("configs")
        .join(format!("{}.conf", project_id));
    if !conf_path.exists() {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "{}; workflow profile {} not found",
                base_message,
                conf_path.display()
            ),
        ));
    }

    let batch_config = BatchProjectConfig::load(&workspace_root, &project_id).map_err(|err| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("failed to load profile {}: {}", project_id, err),
        )
    })?;

    let control_file_name = batch_config
        .control_file
        .as_deref()
        .unwrap_or("newton_control.json");
    let control_file_path = if Path::new(control_file_name).is_absolute() {
        PathBuf::from(control_file_name)
    } else {
        batch_config.project_root.join(control_file_name)
    };

    if let Some(parent) = control_file_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            AppError::with_source(
                ErrorCategory::IoError,
                format!(
                    "failed to create control file directory {}",
                    parent.display()
                ),
                Box::new(err),
            )
        })?;
    }

    Ok(RunMode::Profile(Box::new(ProfileRunContext {
        project_id,
        workspace_root,
        batch_config,
        control_file_path,
    })))
}

enum RunMode {
    Workspace(PathBuf),
    Profile(Box<ProfileRunContext>),
}

struct ProfileRunContext {
    project_id: String,
    workspace_root: PathBuf,
    batch_config: BatchProjectConfig,
    control_file_path: PathBuf,
}

pub async fn workflow(args: WorkflowArgs) -> Result<()> {
    match args.command {
        WorkflowCommand::Run(run_args) => workflow_run(run_args).await.map_err(|err| anyhow!(err)),
        WorkflowCommand::Validate(validate_args) => {
            workflow_validate(validate_args).map_err(|err| anyhow!(err))
        }
        WorkflowCommand::Dot(dot_args) => workflow_dot(dot_args).map_err(|err| anyhow!(err)),
        WorkflowCommand::Lint(lint_args) => workflow_lint(lint_args).map_err(|err| anyhow!(err)),
        WorkflowCommand::Explain(explain_args) => {
            workflow_explain(explain_args).map_err(|err| anyhow!(err))
        }
        WorkflowCommand::Resume(resume_args) => workflow_resume(resume_args)
            .await
            .map_err(|err| anyhow!(err)),
        WorkflowCommand::Checkpoints(checkpoints_args) => {
            workflow_checkpoints(checkpoints_args).map_err(|err| anyhow!(err))
        }
        WorkflowCommand::Artifacts(artifacts_args) => {
            workflow_artifacts(artifacts_args).map_err(|err| anyhow!(err))
        }
    }
}

async fn workflow_run(args: WorkflowRunArgs) -> StdResult<(), AppError> {
    let workspace = resolve_workflow_workspace(args.workspace)?;
    let workflow_path = args.workflow.clone();
    let mut document = workflow_schema::load_workflow(&workflow_path)?;
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

    let overrides = ExecutionOverrides {
        parallel_limit: args.parallel_limit,
        max_time_seconds: args.max_time_seconds,
    };

    let mut builder = OperatorRegistry::builder();
    workflow_operators::register_builtins(&mut builder, workspace.clone());
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

fn workflow_validate(args: WorkflowValidateArgs) -> StdResult<(), AppError> {
    let document = workflow_schema::load_workflow(&args.workflow)?;
    let unreachable = workflow_dot::reachability_warnings(&document);
    for id in &unreachable {
        eprintln!("warning: task '{}' is not reachable from entry_task", id);
    }
    println!("Workflow definition is valid");
    Ok(())
}

fn workflow_dot(args: WorkflowDotArgs) -> StdResult<(), AppError> {
    let document = workflow_schema::load_workflow(&args.workflow)?;
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

fn workflow_lint(args: WorkflowLintArgs) -> StdResult<(), AppError> {
    let document = workflow_schema::load_workflow(&args.workflow)?;
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

fn workflow_explain(args: WorkflowExplainArgs) -> StdResult<(), AppError> {
    let _workspace = resolve_workflow_workspace(args.workspace)?;
    let document = workflow_schema::load_workflow(&args.workflow)?;
    let overrides = parse_set_overrides(&args.set);
    let output = explain::build_explain_output(&document, &overrides);
    match args.format {
        OutputFormat::Json => print_explain_json(&output)?,
        OutputFormat::Text => print_explain_text(&output)?,
    }
    Ok(())
}

async fn workflow_resume(args: WorkflowResumeArgs) -> StdResult<(), AppError> {
    let workspace = resolve_workflow_workspace(args.workspace)?;
    let mut builder = OperatorRegistry::builder();
    workflow_operators::register_builtins(&mut builder, workspace.clone());
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

fn workflow_checkpoints(args: WorkflowCheckpointsArgs) -> StdResult<(), AppError> {
    match args.command {
        WorkflowCheckpointCommand::List {
            workspace,
            format_json,
        } => workflow_checkpoints_list(workspace, format_json),
        WorkflowCheckpointCommand::Clean {
            workspace,
            older_than,
        } => workflow_checkpoints_clean(workspace, older_than),
    }
}

fn workflow_checkpoints_list(
    workspace: Option<PathBuf>,
    format_json: bool,
) -> StdResult<(), AppError> {
    let workspace = resolve_workflow_workspace(workspace)?;
    let entries = checkpoint::list_checkpoints(&workspace)?;
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
    println!(
        "{:<36} {:<10} {:<24} {:<12} {:>7}",
        "EXECUTION ID", "STATUS", "STARTED AT", "CHECKPOINT AGE", "SIZE"
    );
    for summary in entries {
        println!(
            "{:<36} {:<10} {:<24} {:<12} {:>7}",
            summary.execution_id,
            summary.status.as_str(),
            summary.started_at.to_rfc3339(),
            format!("{} ago", format_duration(summary.checkpoint_age)),
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

fn workflow_artifacts(args: WorkflowArtifactsArgs) -> StdResult<(), AppError> {
    match args.command {
        WorkflowArtifactCommand::Clean {
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

fn apply_context_overrides(context: &mut Value, overrides: &[KeyValuePair]) {
    if !context.is_object() {
        *context = Value::Object(Map::new());
    }
    if let Some(map) = context.as_object_mut() {
        for pair in overrides {
            map.insert(pair.key.clone(), Value::String(pair.value.clone()));
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

fn print_explain_text(output: &explain::ExplainOutput) -> StdResult<(), AppError> {
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

/// Setup task directories and state, returns (spec_path, state_dir, control_file_path)
fn setup_task_dirs_and_state(
    batch_config: &BatchProjectConfig,
    task_id: &str,
    plan_file: &Path,
    resume: bool,
) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let task_input_dir = batch_config
        .project_root
        .join(".newton")
        .join("tasks")
        .join(task_id)
        .join("input");
    fs::create_dir_all(&task_input_dir)?;
    let spec_path = task_input_dir.join("spec.md");
    if spec_path.exists() {
        fs::remove_file(&spec_path)?;
    }
    fs::copy(plan_file, &spec_path)?;

    let state_dir = batch_config
        .project_root
        .join(".newton")
        .join("tasks")
        .join(task_id)
        .join("state");
    let project_state_dir = batch_config.project_root.join(".newton").join("state");

    if resume {
        fs::create_dir_all(&state_dir)?;
        fs::create_dir_all(&project_state_dir)?;
    } else {
        if state_dir.exists() {
            fs::remove_dir_all(&state_dir)?;
        }
        if project_state_dir.exists() {
            fs::remove_dir_all(&project_state_dir)?;
        }
        fs::create_dir_all(&state_dir)?;
        fs::create_dir_all(&project_state_dir)?;
    }

    let control_file_name = batch_config
        .control_file
        .as_deref()
        .unwrap_or("newton_control.json");
    let control_file_path = state_dir.join(control_file_name);

    Ok((spec_path, state_dir, control_file_path))
}

/// Run the pre-run hook if configured
fn run_pre_run_hook(
    batch_config: &BatchProjectConfig,
    context: &BatchTaskContext,
    plan_file: &Path,
) -> Result<()> {
    if let Some(pre_script) = &batch_config.pre_run_script {
        let pre_env = build_pre_run_env(context);
        let status = run_batch_hook_script(&batch_config.project_root, pre_script, &pre_env)?;
        if !status.success() {
            let code = status.code().unwrap_or(-1);
            tracing::warn!(
                "pre_run_script exited {} for {}; skipping run",
                code,
                plan_file.display()
            );
            write_run_log(
                &context.state_dir,
                &format!("pre-run script failed with exit code {}", code),
            )?;
            return Err(anyhow!("pre-run script failed with exit code {}", code));
        }
    }
    Ok(())
}

/// Handle successful run completion
fn handle_success(
    context: &BatchTaskContext,
    plan_file: &Path,
    completed_dir: &Path,
    failed_dir: &Path,
    overrides: Vec<(String, Option<String>)>,
    once: bool,
) -> Result<bool> {
    let script_env = build_batch_hook_env(context, "success");

    let mut final_destination = completed_dir.to_path_buf();
    if let Some(success_script) = &context.batch_config.post_success_script {
        let status = run_batch_hook_script(
            &context.batch_config.project_root,
            success_script,
            &script_env,
        )?;
        if !status.success() {
            tracing::warn!(
                "post_success_script exited {} for {}; moving plan to failed",
                status.code().unwrap_or(-1),
                plan_file.display()
            );
            final_destination = failed_dir.to_path_buf();
        }
    }

    let destination = final_destination.join(
        plan_file
            .file_name()
            .ok_or_else(|| anyhow!("Plan file missing name"))?,
    );
    if destination.exists() {
        fs::remove_file(&destination)?;
    }
    fs::rename(plan_file, &destination)?;
    restore_env_vars(overrides);

    Ok(once)
}

/// Handle failed run
fn handle_failure(
    context: &BatchTaskContext,
    plan_file: &Path,
    failed_dir: &Path,
    overrides: Vec<(String, Option<String>)>,
    once: bool,
    error: Error,
) -> Result<()> {
    tracing::error!(
        "Batch processing failed for {}: {}",
        plan_file.display(),
        error
    );

    let script_env = build_batch_hook_env(context, "failure");
    if let Some(fail_script) = &context.batch_config.post_fail_script {
        match run_batch_hook_script(&context.batch_config.project_root, fail_script, &script_env) {
            Ok(status) => {
                if !status.success() {
                    tracing::warn!(
                        "post_fail_script exited {} for {}",
                        status.code().unwrap_or(-1),
                        plan_file.display()
                    );
                }
            }
            Err(err) => tracing::error!(
                "Failed to run post_fail_script for {}: {}",
                plan_file.display(),
                err
            ),
        }
    }

    let destination = failed_dir.join(
        plan_file
            .file_name()
            .ok_or_else(|| anyhow!("Plan file missing name"))?,
    );
    if destination.exists() {
        fs::remove_file(&destination)?;
    }
    fs::rename(plan_file, &destination)?;
    restore_env_vars(overrides);

    if once {
        Err(anyhow!("Batch run failed for {}", plan_file.display()))
    } else {
        Ok(())
    }
}

pub async fn batch(args: BatchArgs) -> Result<()> {
    tracing::info!("Starting batch runner for project {}", args.project_id);

    let workspace_root = validate_batch_workspace(args.workspace.clone())?;
    let batch_config = BatchProjectConfig::load(&workspace_root, &args.project_id)?;
    let dirs = ensure_batch_dirs(&workspace_root, &args.project_id)?;

    loop {
        let plan_file = fetch_next_task(&dirs.todo_dir, args.once, args.sleep).await?;
        if plan_file.is_none() {
            return Ok(());
        }
        let plan_file = plan_file.unwrap();

        let task_id =
            sanitize_task_id(plan_file.file_name().and_then(|n| n.to_str()).unwrap_or(""));

        let (spec_path, state_dir, control_file_path) =
            setup_task_dirs_and_state(&batch_config, &task_id, &plan_file, batch_config.resume)?;

        let context = prepare_batch_context(
            &batch_config,
            &args.project_id,
            &task_id,
            &spec_path,
            &workspace_root,
            &state_dir,
            &control_file_path,
        );

        let overrides = setup_task_environment(&context, &workspace_root);

        let run_result = execute_batch_task(&context, &args, &workspace_root, &plan_file).await;

        if run_result.is_ok() {
            if handle_success(
                &context,
                &plan_file,
                &dirs.completed_dir,
                &dirs.failed_dir,
                overrides,
                args.once,
            )? {
                return Ok(());
            }
            continue;
        }

        let error = run_result.unwrap_err();
        handle_failure(
            &context,
            &plan_file,
            &dirs.failed_dir,
            overrides,
            args.once,
            error,
        )?;

        sleep_if_needed(args.sleep).await;
    }
}

fn validate_batch_workspace(workspace: Option<PathBuf>) -> Result<PathBuf> {
    let workspace_root = resolve_workspace_root(workspace)?;
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
        if let Some(path) = next_plan_file(todo_dir)? {
            return Ok(Some(path));
        }
        if once {
            tracing::info!("Queue empty; exiting after --once");
            return Ok(None);
        }
        sleep_if_needed(sleep_duration).await;
    }
}

fn prepare_batch_context(
    batch_config: &BatchProjectConfig,
    project_id: &str,
    task_id: &str,
    spec_path: &Path,
    workspace_root: &Path,
    state_dir: &Path,
    control_file: &Path,
) -> BatchTaskContext {
    let branch_name = derive_branch_name(spec_path, task_id);
    let base_branch = detect_base_branch(&batch_config.project_root);

    BatchTaskContext {
        batch_config: batch_config.clone(),
        project_id: project_id.to_string(),
        task_id: task_id.to_string(),
        goal_file: spec_path.to_path_buf(),
        workspace_root: workspace_root.to_path_buf(),
        state_dir: state_dir.to_path_buf(),
        control_file: control_file.to_path_buf(),
        branch_name,
        base_branch,
    }
}

fn setup_task_environment(
    context: &BatchTaskContext,
    workspace_root: &Path,
) -> Vec<(String, Option<String>)> {
    let coder_cmd = context
        .batch_config
        .coder_cmd
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            workspace_root
                .join(".newton")
                .join("scripts")
                .join("coder.sh")
        });

    let project_root_str = context.batch_config.project_root.display().to_string();
    let workspace_root_str = context.workspace_root.display().to_string();
    let state_dir_str = context.state_dir.display().to_string();
    let coder_cmd_str = coder_cmd.display().to_string();
    let control_file_str = context.control_file.display().to_string();

    let env_pairs = [
        ("CODING_AGENT", context.batch_config.coding_agent.as_str()),
        (
            "CODING_AGENT_MODEL",
            context.batch_config.coding_model.as_str(),
        ),
        (
            "NEWTON_EXECUTOR_CODING_AGENT",
            context.batch_config.coding_agent.as_str(),
        ),
        (
            "NEWTON_EXECUTOR_CODING_AGENT_MODEL",
            context.batch_config.coding_model.as_str(),
        ),
        ("NEWTON_PROJECT_ROOT", project_root_str.as_str()),
        ("NEWTON_PROJECT_ID", context.project_id.as_str()),
        ("NEWTON_TASK_ID", context.task_id.as_str()),
        ("NEWTON_WS_ROOT", workspace_root_str.as_str()),
        ("NEWTON_STATE_DIR", state_dir_str.as_str()),
        ("NEWTON_CODER_CMD", coder_cmd_str.as_str()),
        ("NEWTON_BRANCH_NAME", context.branch_name.as_str()),
        ("NEWTON_CONTROL_FILE", control_file_str.as_str()),
    ];
    apply_env_overrides(&env_pairs)
}

async fn execute_batch_task(
    context: &BatchTaskContext,
    args: &BatchArgs,
    workspace_root: &Path,
    plan_file: &Path,
) -> Result<()> {
    run_pre_run_hook(&context.batch_config, context, plan_file)?;

    write_run_log(&context.state_dir, "Starting Newton run")?;

    let ailoop_ctx = initialize_ailoop_context(workspace_root, args, &context.task_id)?;

    let run_args = RunArgs::for_batch_with_tools(
        context.batch_config.project_root.clone(),
        Some(context.goal_file.clone()),
        context.batch_config.evaluator_cmd.clone(),
        context.batch_config.advisor_cmd.clone(),
        context.batch_config.executor_cmd.clone(),
        context.batch_config.max_iterations,
        context.batch_config.max_time,
        context.batch_config.verbose,
        Some(context.control_file.clone()),
    );

    let newton_config = ConfigLoader::load_from_workspace(&context.batch_config.project_root)?;
    ConfigValidator::validate(&newton_config)?;

    let (_goal_file, additional_env) =
        build_run_additional_env(&run_args, &context.batch_config.project_root)?;
    let exec_config = build_run_exec_config(&run_args, &context.batch_config.project_root);
    let success_policy = SuccessPolicy::from_path(context.control_file.clone());

    let reporter = Box::new(DefaultErrorReporter);
    let orchestrator = OptimizationOrchestrator::with_ailoop_context(
        JsonSerializer,
        FileUtils,
        reporter,
        ailoop_ctx,
    );

    let result = orchestrator
        .run_optimization_with_policy(
            &context.batch_config.project_root,
            exec_config,
            &additional_env,
            Some(&success_policy),
        )
        .await;

    let result = match result {
        Ok(execution) => report_run_result(execution),
        Err(e) => Err(e.into()),
    };

    write_run_log(
        &context.state_dir,
        &format!(
            "Newton run finished: {}",
            if result.is_ok() { "success" } else { "failure" }
        ),
    )?;

    result
}

fn initialize_ailoop_context(
    workspace_root: &Path,
    args: &BatchArgs,
    task_id: &str,
) -> Result<Option<Arc<ailoop_integration::AiloopContext>>> {
    match ailoop_integration::init_context(workspace_root, &CliCommand::Batch(args.clone())) {
        Ok(ctx) => {
            if let Some(ref context) = ctx {
                tracing::info!(
                    "Ailoop integration enabled for batch task {} on channel: {}",
                    task_id,
                    context.channel()
                );
            }
            Ok(ctx.map(Arc::new))
        }
        Err(e) => {
            tracing::warn!("Failed to initialize ailoop integration for batch: {}", e);
            Ok(None)
        }
    }
}

pub async fn step(args: StepArgs) -> Result<()> {
    tracing::info!("Executing single step");

    let reporter = Box::new(DefaultErrorReporter);
    let orchestrator = OptimizationOrchestrator::new(JsonSerializer, FileUtils, reporter);

    let exec_config = crate::core::entities::ExecutionConfiguration {
        evaluator_cmd: None,
        advisor_cmd: None,
        executor_cmd: None,
        max_iterations: Some(1),
        max_time_seconds: Some(30),
        evaluator_timeout_ms: None,
        advisor_timeout_ms: None,
        executor_timeout_ms: None,
        global_timeout_ms: Some(30000),
        strict_toolchain_mode: false,
        resource_monitoring: false,
        verbose: args.verbose,
    };

    orchestrator
        .run_optimization(&args.path, exec_config)
        .await?;

    Ok(())
}

pub async fn status(args: StatusArgs) -> Result<()> {
    tracing::info!("Checking status for execution: {}", args.execution_id);

    let execution_dir = args
        .path
        .join(".newton")
        .join("executions")
        .join(&args.execution_id);
    let execution_file = execution_dir.join("execution.json");

    if !execution_file.exists() {
        println!("Execution {} not found", args.execution_id);
        return Ok(());
    }

    let content = std::fs::read_to_string(&execution_file)?;
    let execution: crate::core::entities::OptimizationExecution = serde_json::from_str(&content)?;

    println!("Execution ID: {}", execution.id);
    println!("Status: {:?}", execution.status);
    println!("Iterations: {}", execution.total_iterations_completed);
    println!("Started: {}", execution.started_at);
    if let Some(completed) = execution.completed_at {
        println!("Completed: {}", completed);
    }

    Ok(())
}

pub async fn report(args: ReportArgs) -> Result<()> {
    tracing::info!("Generating report for execution: {}", args.execution_id);

    let execution_dir = args
        .path
        .join(".newton")
        .join("executions")
        .join(&args.execution_id);
    let execution_file = execution_dir.join("execution.json");

    if !execution_file.exists() {
        return Err(anyhow!("Execution {} not found", args.execution_id));
    }

    let content = std::fs::read_to_string(&execution_file)?;
    let execution: crate::core::entities::OptimizationExecution = serde_json::from_str(&content)?;

    match args.format {
        crate::cli::args::ReportFormat::Text => {
            println!("Newton Optimization Report");
            println!("=========================\n");
            println!("Execution ID: {}", execution.id);
            println!("Status: {:?}", execution.status);
            println!("Iterations: {}", execution.total_iterations_completed);
            println!("Started: {}", execution.started_at);
            if let Some(completed) = execution.completed_at {
                let duration = completed.signed_duration_since(execution.started_at);
                println!("Duration: {}", duration);
            }
            println!(
                "\nTotal Iterations: {}",
                execution.total_iterations_completed
            );
        }
        crate::cli::args::ReportFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&execution)?);
        }
    }

    Ok(())
}

pub async fn error(args: ErrorArgs) -> Result<()> {
    tracing::info!("Analyzing errors for execution: {}", args.execution_id);

    let execution_dir = Path::new(".")
        .join(".newton")
        .join("executions")
        .join(&args.execution_id);

    let error_log = execution_dir.join("error.log");

    if error_log.exists() {
        let content = std::fs::read_to_string(&error_log)?;
        if args.verbose {
            println!("Full error details:\n{}", content);
        } else {
            let summary: String = content.lines().take(10).collect::<Vec<_>>().join("\n");
            println!("Error summary:\n{}", summary);
            if content.lines().count() > 10 {
                println!("\n... (truncated, use --verbose for full output)");
            }
        }
    } else {
        println!("No error log found for execution {}", args.execution_id);
    }

    Ok(())
}

fn prepare_goal_file(args: &RunArgs, workspace_path: &Path) -> Result<Option<PathBuf>> {
    if let Some(ref path) = args.goal_file {
        Ok(Some(path.clone()))
    } else if let Some(ref goal_text) = args.goal {
        let path = workspace_path.join(".newton/state/goal.txt");
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("goal path has no parent directory"))?;
        fs::create_dir_all(parent)?;
        fs::write(&path, goal_text)?;
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

fn default_workspace_script(workspace_path: &Path, script_name: &str) -> Option<String> {
    let script_path = workspace_path.join(".newton/scripts").join(script_name);
    if script_path.is_file() {
        Some(script_path.display().to_string())
    } else {
        None
    }
}

fn resolve_workspace_root(minimum_workspace: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(workspace) = minimum_workspace {
        if workspace.join(".newton").is_dir() {
            return Ok(workspace);
        }
        return Err(anyhow!(
            "Provided workspace {} is missing .newton",
            workspace.display()
        ));
    }

    let current_dir = env::current_dir()?;
    find_workspace_root(&current_dir)
}

fn next_plan_file(plan_dir: &Path) -> Result<Option<PathBuf>> {
    let mut entries: Vec<PathBuf> = fs::read_dir(plan_dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_file())
        .collect();

    entries.sort();
    Ok(entries.into_iter().next())
}

fn sanitize_task_id(raw_name: &str) -> String {
    let filtered: String = raw_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if filtered.is_empty() {
        format!("task-{}", Utc::now().timestamp_millis())
    } else {
        filtered
    }
}

fn apply_env_overrides(pairs: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
    pairs
        .iter()
        .map(|(key, value)| {
            let previous = env::var(key).ok();
            env::set_var(key, value);
            (key.to_string(), previous)
        })
        .collect()
}

fn restore_env_vars(overrides: Vec<(String, Option<String>)>) {
    for (key, previous) in overrides.into_iter().rev() {
        if let Some(value) = previous {
            env::set_var(&key, value);
        } else {
            env::remove_var(&key);
        }
    }
}

fn build_batch_hook_env(context: &BatchTaskContext, result: &str) -> HashMap<String, String> {
    let mut env_vars = HashMap::new();
    env_vars.insert(
        "CODING_AGENT".to_string(),
        context.batch_config.coding_agent.clone(),
    );
    env_vars.insert(
        "CODING_AGENT_MODEL".to_string(),
        context.batch_config.coding_model.clone(),
    );
    env_vars.insert(
        "NEWTON_EXECUTOR_CODING_AGENT".to_string(),
        context.batch_config.coding_agent.clone(),
    );
    env_vars.insert(
        "NEWTON_EXECUTOR_CODING_AGENT_MODEL".to_string(),
        context.batch_config.coding_model.clone(),
    );
    env_vars.insert(
        "NEWTON_GOAL_FILE".to_string(),
        context.goal_file.display().to_string(),
    );
    env_vars.insert("NEWTON_PROJECT_ID".to_string(), context.project_id.clone());
    env_vars.insert("NEWTON_TASK_ID".to_string(), context.task_id.clone());
    env_vars.insert(
        "NEWTON_PROJECT_ROOT".to_string(),
        context.batch_config.project_root.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_WS_ROOT".to_string(),
        context.workspace_root.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_STATE_DIR".to_string(),
        context.state_dir.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_CONTROL_FILE".to_string(),
        context.control_file.display().to_string(),
    );
    env_vars.insert("NEWTON_RESULT".to_string(), result.to_string());
    env_vars.insert(
        "NEWTON_BASE_BRANCH".to_string(),
        context.base_branch.clone(),
    );
    env_vars.insert(
        "NEWTON_BRANCH_NAME".to_string(),
        context.branch_name.clone(),
    );
    env_vars
}

fn run_batch_hook_script(
    project_root: &Path,
    script: &str,
    env_vars: &HashMap<String, String>,
) -> Result<std::process::ExitStatus> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(project_root)
        .envs(env_vars)
        .status()
        .map_err(|e| anyhow!("failed to execute hook script: {}", e))?;
    Ok(status)
}

fn build_pre_run_env(context: &BatchTaskContext) -> HashMap<String, String> {
    let mut env_vars = HashMap::new();
    env_vars.insert(
        "CODING_AGENT".to_string(),
        context.batch_config.coding_agent.clone(),
    );
    env_vars.insert(
        "CODING_AGENT_MODEL".to_string(),
        context.batch_config.coding_model.clone(),
    );
    env_vars.insert(
        "NEWTON_PROJECT_ROOT".to_string(),
        context.batch_config.project_root.display().to_string(),
    );
    env_vars.insert("NEWTON_PROJECT_ID".to_string(), context.project_id.clone());
    env_vars.insert("NEWTON_TASK_ID".to_string(), context.task_id.clone());
    env_vars.insert(
        "NEWTON_GOAL_FILE".to_string(),
        context.goal_file.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_WS_ROOT".to_string(),
        context.workspace_root.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_STATE_DIR".to_string(),
        context.state_dir.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_CONTROL_FILE".to_string(),
        context.control_file.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_BRANCH_NAME".to_string(),
        context.branch_name.clone(),
    );
    env_vars.insert(
        "NEWTON_RESUME".to_string(),
        if context.batch_config.resume {
            "1"
        } else {
            "0"
        }
        .to_string(),
    );
    env_vars
}

fn derive_branch_name(goal_file: &Path, task_id: &str) -> String {
    let content = match fs::read_to_string(goal_file) {
        Ok(content) => content,
        Err(_) => return format!("feature/{}", task_id.replace('_', "-")),
    };

    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return format!("feature/{}", task_id.replace('_', "-"));
    }

    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("branch:") {
            let branch = rest.trim().trim_matches('"');
            if !branch.is_empty() {
                return branch.to_string();
            }
        }
    }

    format!("feature/{}", task_id.replace('_', "-"))
}

fn detect_base_branch(project_root: &Path) -> String {
    let default_branch = "main".to_string();

    let output = match Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("origin/HEAD")
        .output()
    {
        Ok(output) => output,
        Err(_) => return default_branch,
    };

    if !output.status.success() {
        return default_branch;
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        return default_branch;
    }

    if let Some(stripped) = branch.strip_prefix("origin/") {
        if !stripped.is_empty() {
            return stripped.to_string();
        }
    }

    branch
}

fn write_run_log(state_dir: &Path, message: &str) -> Result<()> {
    let log_path = state_dir.join("newton_run.log");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    writeln!(file, "[{}] {}", Utc::now(), message)?;
    Ok(())
}
