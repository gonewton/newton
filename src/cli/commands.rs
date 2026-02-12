use crate::core::config::ExecutorConfig;
use crate::{
    cli::args::{
        BatchArgs, ErrorArgs, InitArgs, MonitorArgs, ReportArgs, RunArgs, StatusArgs, StepArgs,
    },
    core::{
        batch_config::{find_workspace_root, BatchProjectConfig},
        entities::ExecutionStatus,
        ConfigLoader, ConfigValidator, DefaultErrorReporter, OptimizationOrchestrator,
        SuccessPolicy,
    },
    monitor,
    utils::serialization::{FileUtils, JsonSerializer},
    Result,
};
use aikit_sdk::{
    fetch::TemplateSource,
    install::{install_template_from_source, InstallError, InstallTemplateFromSourceOptions},
};
use anyhow::{anyhow, Error};
use chrono::Utc;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    collections::HashMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tokio::time::sleep;

const DEFAULT_TEMPLATE_SOURCE: &str = "gonewton/newton-templates";

pub async fn run(args: RunArgs) -> Result<()> {
    tracing::info!("Starting Newton Loop optimization run");

    let workspace_path = args.path.clone();

    let newton_config = if let Some(ref path) = args.config {
        ConfigLoader::load_from_file(path)?.unwrap_or_default()
    } else {
        ConfigLoader::load_from_workspace(&workspace_path)?
    };

    ConfigValidator::validate(&newton_config)?;

    let goal_file = prepare_goal_file(&args, &workspace_path)?;

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

    let evaluator_cmd = args
        .evaluator_cmd
        .clone()
        .or_else(|| default_workspace_script(&workspace_path, "evaluator.sh"));
    let advisor_cmd = args
        .advisor_cmd
        .clone()
        .or_else(|| default_workspace_script(&workspace_path, "advisor.sh"));
    let executor_cmd = args
        .executor_cmd
        .clone()
        .or_else(|| default_workspace_script(&workspace_path, "executor.sh"));
    let strict_toolchain_mode =
        args.evaluator_cmd.is_some() || args.advisor_cmd.is_some() || args.executor_cmd.is_some();

    let exec_config = crate::core::entities::ExecutionConfiguration {
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
    };

    let control_file_path = args
        .control_file
        .clone()
        .unwrap_or_else(|| workspace_path.join("newton_control.json"));
    additional_env.insert(
        "NEWTON_CONTROL_FILE".to_string(),
        control_file_path.display().to_string(),
    );
    let success_policy = SuccessPolicy::from_path(control_file_path.clone());

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

    let reporter = Box::new(DefaultErrorReporter);
    let orchestrator = OptimizationOrchestrator::new(JsonSerializer, FileUtils, reporter);

    let execution = orchestrator
        .run_optimization_with_policy(
            &workspace_path,
            exec_config,
            &additional_env,
            Some(&success_policy),
        )
        .await;

    match execution {
        Ok(execution) => {
            if execution.status != ExecutionStatus::Completed {
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
        Err(e) => {
            tracing::error!("Optimization failed: {}", e);
            eprintln!("Optimization failed: {}", e);
            Err(e.into())
        }
    }
}

pub async fn init(args: InitArgs) -> Result<()> {
    tracing::info!("Initializing Newton workspace");

    let workspace_root = if let Some(path) = args.path.clone() {
        path
    } else {
        env::current_dir()?
    };

    if !workspace_root.exists() {
        return Err(anyhow!(
            "Target path {} does not exist",
            workspace_root.display()
        ));
    }

    if !workspace_root.is_dir() {
        return Err(anyhow!(
            "Target path {} is not a directory",
            workspace_root.display()
        ));
    }

    let newton_dir = workspace_root.join(".newton");
    if newton_dir.exists() {
        return Err(anyhow!(
            "{} already contains a .newton directory; remove it or choose another location",
            workspace_root.display()
        ));
    }

    fs::create_dir_all(&newton_dir)?;
    fs::create_dir_all(newton_dir.join("configs"))?;
    fs::create_dir_all(newton_dir.join("tasks"))?;
    fs::create_dir_all(newton_dir.join("state"))?;
    let plan_default = newton_dir.join("plan").join("default");
    for stage in &["todo", "completed", "failed", "draft"] {
        fs::create_dir_all(plan_default.join(stage))?;
    }
    let scripts_dir = newton_dir.join("scripts");
    fs::create_dir_all(&scripts_dir)?;

    let source_str = args
        .template_source
        .as_deref()
        .unwrap_or(DEFAULT_TEMPLATE_SOURCE);
    let template_source = TemplateSource::parse(source_str).map_err(|e: InstallError| {
        anyhow!("Failed to parse template source '{}': {}", source_str, e)
    })?;

    install_template_from_source(InstallTemplateFromSourceOptions {
        source: template_source,
        project_root: workspace_root.clone(),
        packages_dir: None,
    })
    .map_err(|e: InstallError| {
        anyhow!("Failed to install template from '{}': {}", source_str, e)
    })?;

    let executor_script = scripts_dir.join("executor.sh");
    if !executor_script.exists() {
        fs::write(
            &executor_script,
            "#!/bin/sh\nset -euo pipefail\n\necho \"Executor script placeholder.\"\n",
        )?;
        #[cfg(unix)]
        {
            fs::set_permissions(&executor_script, PermissionsExt::from_mode(0o755))?;
        }
    }

    let executor_defaults = ExecutorConfig::default();
    let mut config_lines = vec![
        "project_root=.".to_string(),
        format!("coding_agent={}", executor_defaults.coding_agent),
        format!("coding_model={}", executor_defaults.coding_agent_model),
    ];

    if scripts_dir.join("post-success.sh").is_file() {
        config_lines.push("post_success_script=.newton/scripts/post-success.sh".to_string());
    }
    if scripts_dir.join("post-failure.sh").is_file() {
        config_lines.push("post_fail_script=.newton/scripts/post-failure.sh".to_string());
    }

    let config_path = newton_dir.join("configs").join("default.conf");
    fs::write(config_path, format!("{}\n", config_lines.join("\n")))?;

    println!(
        "Initialized Newton workspace at {}",
        workspace_root.display()
    );
    println!("Run: newton run");

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

pub async fn batch(args: BatchArgs) -> Result<()> {
    tracing::info!("Starting batch runner for project {}", args.project_id);

    let workspace_root = resolve_workspace_root(args.workspace)?;
    let configs_dir = workspace_root.join(".newton").join("configs");
    if !configs_dir.is_dir() {
        return Err(anyhow!(
            "Workspace {} must contain .newton/configs",
            workspace_root.display()
        ));
    }

    let plan_root = workspace_root.join(".newton").join("plan");
    if !plan_root.is_dir() {
        return Err(anyhow!(
            "Workspace {} must contain .newton/plan",
            workspace_root.display()
        ));
    }

    let batch_config = BatchProjectConfig::load(&workspace_root, &args.project_id)?;
    let plan_project_dir = plan_root.join(&args.project_id);
    let todo_dir = plan_project_dir.join("todo");
    let completed_dir = plan_project_dir.join("completed");
    let failed_dir = plan_project_dir.join("failed");
    let draft_dir = plan_project_dir.join("draft");
    fs::create_dir_all(&todo_dir)?;
    fs::create_dir_all(&completed_dir)?;
    fs::create_dir_all(&draft_dir)?;
    fs::create_dir_all(&failed_dir)?;

    loop {
        let plan_file = match next_plan_file(&todo_dir)? {
            Some(path) => path,
            None => {
                if args.once {
                    tracing::info!("Queue empty; exiting after --once");
                    return Ok(());
                }
                sleep_if_needed(args.sleep).await;
                continue;
            }
        };

        let task_id =
            sanitize_task_id(plan_file.file_name().and_then(|n| n.to_str()).unwrap_or(""));
        let task_input_dir = batch_config
            .project_root
            .join(".newton")
            .join("tasks")
            .join(&task_id)
            .join("input");
        fs::create_dir_all(&task_input_dir)?;
        let spec_path = task_input_dir.join("spec.md");
        if spec_path.exists() {
            fs::remove_file(&spec_path)?;
        }
        fs::copy(&plan_file, &spec_path)?;

        let state_dir = batch_config
            .project_root
            .join(".newton")
            .join("tasks")
            .join(&task_id)
            .join("state");
        let project_state_dir = batch_config.project_root.join(".newton").join("state");
        if batch_config.resume {
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

        let branch_name = derive_branch_name(&spec_path, &task_id);
        let base_branch = detect_base_branch(&batch_config.project_root);
        let coder_cmd = workspace_root
            .join(".newton")
            .join("scripts")
            .join("coder.sh");

        let project_root_str = batch_config.project_root.display().to_string();
        let workspace_root_str = workspace_root.display().to_string();
        let state_dir_str = state_dir.display().to_string();
        let coder_cmd_str = coder_cmd.display().to_string();
        let control_file_str = control_file_path.display().to_string();

        let env_pairs = [
            ("CODING_AGENT", batch_config.coding_agent.as_str()),
            ("CODING_AGENT_MODEL", batch_config.coding_model.as_str()),
            (
                "NEWTON_EXECUTOR_CODING_AGENT",
                batch_config.coding_agent.as_str(),
            ),
            (
                "NEWTON_EXECUTOR_CODING_AGENT_MODEL",
                batch_config.coding_model.as_str(),
            ),
            ("NEWTON_PROJECT_ROOT", project_root_str.as_str()),
            ("NEWTON_PROJECT_ID", args.project_id.as_str()),
            ("NEWTON_TASK_ID", task_id.as_str()),
            ("NEWTON_WS_ROOT", workspace_root_str.as_str()),
            ("NEWTON_STATE_DIR", state_dir_str.as_str()),
            ("NEWTON_CODER_CMD", coder_cmd_str.as_str()),
            ("NEWTON_BRANCH_NAME", branch_name.as_str()),
            ("NEWTON_CONTROL_FILE", control_file_str.as_str()),
        ];
        let overrides = apply_env_overrides(&env_pairs);

        let mut pre_run_error: Option<Error> = None;
        if let Some(pre_script) = &batch_config.pre_run_script {
            let pre_env = build_pre_run_env(
                &batch_config,
                args.project_id.as_str(),
                task_id.as_str(),
                &spec_path,
                &workspace_root,
                &state_dir,
                &control_file_path,
                &branch_name,
            );
            let status = run_batch_hook_script(&batch_config.project_root, pre_script, &pre_env)?;
            if !status.success() {
                let code = status.code().unwrap_or(-1);
                tracing::warn!(
                    "pre_run_script exited {} for {}; skipping run",
                    code,
                    plan_file.display()
                );
                write_run_log(
                    &state_dir,
                    &format!("pre-run script failed with exit code {}", code),
                )?;
                pre_run_error = Some(anyhow!("pre-run script failed with exit code {}", code));
            }
        }

        let run_result = if let Some(err) = pre_run_error {
            Err(err)
        } else {
            write_run_log(&state_dir, "Starting Newton run")?;
            let run_args = RunArgs::for_batch_with_tools(
                batch_config.project_root.clone(),
                Some(spec_path.clone()),
                batch_config.evaluator_cmd.clone(),
                batch_config.advisor_cmd.clone(),
                batch_config.executor_cmd.clone(),
                batch_config.max_iterations,
                batch_config.max_time,
                batch_config.verbose,
                Some(control_file_path.clone()),
            );
            let result = run(run_args).await;
            write_run_log(
                &state_dir,
                &format!(
                    "Newton run finished: {}",
                    if result.is_ok() { "success" } else { "failure" }
                ),
            )?;
            result
        };

        if run_result.is_ok() {
            let script_env = build_batch_hook_env(
                &batch_config,
                args.project_id.as_str(),
                task_id.as_str(),
                &spec_path,
                "success",
                &branch_name,
                &base_branch,
                &workspace_root,
                &state_dir,
                &control_file_path,
            );

            let mut final_destination = completed_dir.clone();
            if let Some(success_script) = &batch_config.post_success_script {
                let status =
                    run_batch_hook_script(&batch_config.project_root, success_script, &script_env)?;
                if !status.success() {
                    tracing::warn!(
                        "post_success_script exited {} for {}; moving plan to failed",
                        status.code().unwrap_or(-1),
                        plan_file.display()
                    );
                    final_destination = failed_dir.clone();
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
            fs::rename(&plan_file, &destination)?;
            restore_env_vars(overrides);
            if args.once {
                return Ok(());
            }
            continue;
        }

        let error = run_result.unwrap_err();
        tracing::error!(
            "Batch processing failed for {}: {}",
            plan_file.display(),
            error
        );
        let script_env = build_batch_hook_env(
            &batch_config,
            args.project_id.as_str(),
            task_id.as_str(),
            &spec_path,
            "failure",
            &branch_name,
            &base_branch,
            &workspace_root,
            &state_dir,
            &control_file_path,
        );
        if let Some(fail_script) = &batch_config.post_fail_script {
            match run_batch_hook_script(&batch_config.project_root, fail_script, &script_env) {
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
        fs::rename(&plan_file, &destination)?;
        restore_env_vars(overrides);
        if args.once {
            return Err(anyhow!("Batch run failed for {}", plan_file.display()));
        }

        sleep_if_needed(args.sleep).await;
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
        fs::create_dir_all(path.parent().unwrap())?;
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

#[allow(clippy::too_many_arguments)]
fn build_batch_hook_env(
    batch_config: &BatchProjectConfig,
    project_id: &str,
    task_id: &str,
    goal_file: &Path,
    result: &str,
    branch_name: &str,
    base_branch: &str,
    workspace_root: &Path,
    state_dir: &Path,
    control_file: &Path,
) -> HashMap<String, String> {
    let mut env_vars = HashMap::new();
    env_vars.insert(
        "CODING_AGENT".to_string(),
        batch_config.coding_agent.clone(),
    );
    env_vars.insert(
        "CODING_AGENT_MODEL".to_string(),
        batch_config.coding_model.clone(),
    );
    env_vars.insert(
        "NEWTON_EXECUTOR_CODING_AGENT".to_string(),
        batch_config.coding_agent.clone(),
    );
    env_vars.insert(
        "NEWTON_EXECUTOR_CODING_AGENT_MODEL".to_string(),
        batch_config.coding_model.clone(),
    );
    env_vars.insert(
        "NEWTON_GOAL_FILE".to_string(),
        goal_file.display().to_string(),
    );
    env_vars.insert("NEWTON_PROJECT_ID".to_string(), project_id.to_string());
    env_vars.insert("NEWTON_TASK_ID".to_string(), task_id.to_string());
    env_vars.insert(
        "NEWTON_PROJECT_ROOT".to_string(),
        batch_config.project_root.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_WS_ROOT".to_string(),
        workspace_root.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_STATE_DIR".to_string(),
        state_dir.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_CONTROL_FILE".to_string(),
        control_file.display().to_string(),
    );
    env_vars.insert("NEWTON_RESULT".to_string(), result.to_string());
    env_vars.insert("NEWTON_BASE_BRANCH".to_string(), base_branch.to_string());
    env_vars.insert("NEWTON_BRANCH_NAME".to_string(), branch_name.to_string());
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

#[allow(clippy::too_many_arguments)]
fn build_pre_run_env(
    batch_config: &BatchProjectConfig,
    project_id: &str,
    task_id: &str,
    goal_file: &Path,
    workspace_root: &Path,
    state_dir: &Path,
    control_file: &Path,
    branch_name: &str,
) -> HashMap<String, String> {
    let mut env_vars = HashMap::new();
    env_vars.insert(
        "CODING_AGENT".to_string(),
        batch_config.coding_agent.clone(),
    );
    env_vars.insert(
        "CODING_AGENT_MODEL".to_string(),
        batch_config.coding_model.clone(),
    );
    env_vars.insert(
        "NEWTON_PROJECT_ROOT".to_string(),
        batch_config.project_root.display().to_string(),
    );
    env_vars.insert("NEWTON_PROJECT_ID".to_string(), project_id.to_string());
    env_vars.insert("NEWTON_TASK_ID".to_string(), task_id.to_string());
    env_vars.insert(
        "NEWTON_GOAL_FILE".to_string(),
        goal_file.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_WS_ROOT".to_string(),
        workspace_root.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_STATE_DIR".to_string(),
        state_dir.display().to_string(),
    );
    env_vars.insert(
        "NEWTON_CONTROL_FILE".to_string(),
        control_file.display().to_string(),
    );
    env_vars.insert("NEWTON_BRANCH_NAME".to_string(), branch_name.to_string());
    env_vars.insert(
        "NEWTON_RESUME".to_string(),
        if batch_config.resume { "1" } else { "0" }.to_string(),
    );
    env_vars
}

fn derive_branch_name(goal_file: &Path, task_id: &str) -> String {
    if let Ok(content) = fs::read_to_string(goal_file) {
        let mut lines = content.lines();
        if lines.next().map(str::trim) == Some("---") {
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
        }
    }
    format!("feature/{}", task_id.replace('_', "-"))
}

fn detect_base_branch(project_root: &Path) -> String {
    let default_branch = "main".to_string();
    if let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("origin/HEAD")
        .output()
    {
        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Some(stripped) = branch.strip_prefix("origin/") {
                if !stripped.is_empty() {
                    return stripped.to_string();
                }
            }
            if !branch.is_empty() {
                return branch;
            }
        }
    }
    default_branch
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
