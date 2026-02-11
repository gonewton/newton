use crate::{
    cli::args::{
        BatchArgs, ErrorArgs, InitArgs, MonitorArgs, ReportArgs, RunArgs, StatusArgs, StepArgs,
        DEFAULT_TEMPLATE_SOURCE,
    },
    core::{
        batch_config::{find_workspace_root, BatchProjectConfig},
        ConfigLoader, ConfigValidator, DefaultErrorReporter, OptimizationOrchestrator,
        SuccessPolicy,
    },
    monitor,
    utils::serialization::{FileUtils, JsonSerializer},
    Result,
};
use anyhow::anyhow;
use chrono::Utc;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tokio::time::sleep;

struct TemplateAsset {
    relative_path: &'static str,
    content: &'static [u8],
    executable: bool,
}

const BUILTIN_TEMPLATE_ASSETS: &[TemplateAsset] = &[
    TemplateAsset {
        relative_path: "README.md",
        content: include_bytes!("../../resources/newton-template/newton/README.md"),
        executable: false,
    },
    TemplateAsset {
        relative_path: "scripts/advisor.sh",
        content: include_bytes!("../../resources/newton-template/newton/scripts/advisor.sh"),
        executable: true,
    },
    TemplateAsset {
        relative_path: "scripts/evaluator.sh",
        content: include_bytes!("../../resources/newton-template/newton/scripts/evaluator.sh"),
        executable: true,
    },
    TemplateAsset {
        relative_path: "scripts/post-success.sh",
        content: include_bytes!("../../resources/newton-template/newton/scripts/post-success.sh"),
        executable: true,
    },
    TemplateAsset {
        relative_path: "scripts/post-failure.sh",
        content: include_bytes!("../../resources/newton-template/newton/scripts/post-failure.sh"),
        executable: true,
    },
];

const BUILTIN_EXECUTOR_STUB: &str = "\
#!/bin/sh\n\
# Executor placeholder created by newton init\n\
echo \"Executor not provided in template\"\n\
exit 0\n";

pub async fn init(args: InitArgs) -> Result<()> {
    tracing::info!("Initializing Newton workspace");

    let workspace_path = resolve_workspace_path(args.path)?;
    let newton_dir = workspace_path.join(".newton");
    if newton_dir.exists() {
        return Err(anyhow!(
            "{} already contains .newton. Remove it or choose another path.",
            workspace_path.display()
        ));
    }

    create_workspace_layout(&workspace_path)?;
    install_template(&workspace_path, args.template_source.as_deref())?;
    write_default_config(&workspace_path)?;

    println!(
        "Initialized Newton workspace at {}",
        workspace_path.display()
    );
    println!("Run: newton run");

    Ok(())
}

pub async fn run(args: RunArgs) -> Result<()> {
    tracing::info!("Starting Newton Loop optimization run");

    let workspace_path = resolve_workspace_path(args.path.clone())?;

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

    let resolved_evaluator_cmd =
        resolve_tool_command(&workspace_path, args.evaluator_cmd.clone(), "evaluator.sh");
    let resolved_advisor_cmd =
        resolve_tool_command(&workspace_path, args.advisor_cmd.clone(), "advisor.sh");
    let resolved_executor_cmd =
        resolve_tool_command(&workspace_path, args.executor_cmd.clone(), "executor.sh");

    let exec_config = crate::core::entities::ExecutionConfiguration {
        evaluator_cmd: resolved_evaluator_cmd,
        advisor_cmd: resolved_advisor_cmd,
        executor_cmd: resolved_executor_cmd,
        max_iterations: Some(args.max_iterations),
        max_time_seconds: Some(args.max_time),
        evaluator_timeout_ms: args.evaluator_timeout.map(|t| t * 1000),
        advisor_timeout_ms: args.advisor_timeout.map(|t| t * 1000),
        executor_timeout_ms: args.executor_timeout.map(|t| t * 1000),
        global_timeout_ms: Some(args.max_time * 1000),
        strict_toolchain_mode: args.evaluator_cmd.is_some()
            || args.advisor_cmd.is_some()
            || args.executor_cmd.is_some(),
        resource_monitoring: false,
        verbose: args.verbose,
    };

    let control_file = args
        .control_file
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "newton_control.json".to_string());
    let success_policy = SuccessPolicy::new(&workspace_path, &control_file);

    let reporter = Box::new(DefaultErrorReporter);
    let orchestrator = OptimizationOrchestrator::new(JsonSerializer, FileUtils, reporter);

    let result = orchestrator
        .run_optimization_with_policy(
            &workspace_path,
            exec_config,
            &additional_env,
            Some(&success_policy),
        )
        .await;

    if let Ok(ref execution) = result {
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
    } else if let Err(ref e) = result {
        tracing::error!("Optimization failed: {}", e);
        eprintln!("Optimization failed: {}", e);
    }

    result?;
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
            ("NEWTON_PROJECT_ID", args.project_id.as_str()),
            ("NEWTON_TASK_ID", task_id.as_str()),
        ];
        let overrides = apply_env_overrides(&env_pairs);

        let run_args =
            RunArgs::for_batch(batch_config.project_root.clone(), Some(spec_path.clone()));
        let run_result = run(run_args).await;

        if run_result.is_ok() {
            let script_env = build_batch_hook_env(
                &batch_config,
                args.project_id.as_str(),
                task_id.as_str(),
                &spec_path,
                "success",
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

        tracing::error!(
            "Batch processing failed for {}: {}",
            plan_file.display(),
            run_result.as_ref().unwrap_err()
        );
        let script_env = build_batch_hook_env(
            &batch_config,
            args.project_id.as_str(),
            task_id.as_str(),
            &spec_path,
            "failure",
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

fn resolve_workspace_path(path_arg: Option<PathBuf>) -> Result<PathBuf> {
    let candidate = match path_arg {
        Some(p) => p,
        None => env::current_dir()?,
    };
    if !candidate.exists() {
        return Err(anyhow!(
            "Workspace path {} does not exist",
            candidate.display()
        ));
    }
    if !candidate.is_dir() {
        return Err(anyhow!(
            "Workspace path {} must be a directory",
            candidate.display()
        ));
    }
    let canonical = candidate.canonicalize()?;
    Ok(canonical)
}

fn create_workspace_layout(workspace: &Path) -> Result<()> {
    let newton_dir = workspace.join(".newton");
    fs::create_dir_all(newton_dir.join("configs"))?;
    fs::create_dir_all(newton_dir.join("tasks"))?;
    fs::create_dir_all(newton_dir.join("state"))?;
    fs::write(newton_dir.join("state/context.md"), "")?;
    fs::write(newton_dir.join("state/promise.txt"), "")?;
    create_plan_dirs(&newton_dir)?;
    Ok(())
}

fn create_plan_dirs(newton_dir: &Path) -> Result<()> {
    let plan_root = newton_dir.join("plan").join("default");
    for dir in ["todo", "completed", "failed", "draft"] {
        let target = plan_root.join(dir);
        fs::create_dir_all(&target)?;
        let gitkeep_path = target.join(".gitkeep");
        if !gitkeep_path.exists() {
            fs::write(gitkeep_path, "")?;
        }
    }
    Ok(())
}

fn install_template(workspace: &Path, template_source: Option<&str>) -> Result<()> {
    let newton_dir = workspace.join(".newton");
    let scripts_dir = ensure_scripts_dir(workspace)?;
    aikit_sdk::agent("newton")
        .ok_or_else(|| anyhow!("aikit-sdk does not register a Newton agent"))?;

    if let Some(source) = template_source {
        let trimmed = source.trim();
        if trimmed.is_empty() || trimmed == DEFAULT_TEMPLATE_SOURCE {
            install_builtin_template(workspace, &newton_dir, &scripts_dir)?;
            return Ok(());
        }
        let path = PathBuf::from(trimmed);
        if !path.is_dir() {
            return Err(anyhow!(
                "Template source {} is not a directory",
                path.display()
            ));
        }
        install_from_directory(workspace, &path, &scripts_dir)?;
        return Ok(());
    }

    install_builtin_template(workspace, &newton_dir, &scripts_dir)?;
    Ok(())
}

fn ensure_scripts_dir(workspace: &Path) -> Result<PathBuf> {
    let scripts_dir = aikit_sdk::scripts_dir(workspace, "newton")
        .ok_or_else(|| anyhow!("aikit-sdk does not expose scripts directory for newton"))?;
    fs::create_dir_all(&scripts_dir)?;
    Ok(scripts_dir)
}

fn install_builtin_template(_: &Path, newton_dir: &Path, scripts_dir: &Path) -> Result<()> {
    for asset in BUILTIN_TEMPLATE_ASSETS {
        let target = if asset.relative_path.starts_with("scripts/") {
            let relative = asset
                .relative_path
                .strip_prefix("scripts/")
                .unwrap_or(asset.relative_path);
            scripts_dir.join(relative)
        } else {
            newton_dir.join(asset.relative_path)
        };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, asset.content)?;
        if asset.executable {
            set_executable(&target)?;
        }
    }

    ensure_executor_script(scripts_dir)?;
    Ok(())
}

fn install_from_directory(workspace: &Path, template_dir: &Path, scripts_dir: &Path) -> Result<()> {
    let source_newton = template_dir.join("newton");
    if !source_newton.is_dir() {
        return Err(anyhow!(
            "Template directory {} lacks a newton/ folder",
            template_dir.display()
        ));
    }
    copy_dir_recursive(&source_newton, &workspace.join(".newton"))?;
    ensure_executor_script(scripts_dir)?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dest = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dest)?;
        } else if entry.file_type()?.is_file() {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &dest)?;
            if dest.extension().and_then(|e| e.to_str()) == Some("sh") {
                set_executable(&dest)?;
            }
        }
    }
    Ok(())
}

fn ensure_executor_script(scripts_dir: &Path) -> Result<()> {
    let executor_path = scripts_dir.join("executor.sh");
    if !executor_path.exists() {
        fs::write(&executor_path, BUILTIN_EXECUTOR_STUB)?;
        set_executable(&executor_path)?;
    }
    Ok(())
}

fn set_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(permissions.mode() | 0o111);
        fs::set_permissions(path, permissions)?;
    }
    #[cfg(not(unix))]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn resolve_tool_command(
    workspace: &Path,
    command_override: Option<String>,
    script_name: &str,
) -> Option<String> {
    if let Some(cmd) = command_override {
        return Some(cmd);
    }
    aikit_sdk::scripts_dir(workspace, "newton").and_then(|scripts_dir| {
        let script_path = scripts_dir.join(script_name);
        if script_path.is_file() {
            Some(script_path.to_string_lossy().to_string())
        } else {
            None
        }
    })
}

fn write_default_config(workspace: &Path) -> Result<()> {
    let configs_dir = workspace.join(".newton/configs");
    fs::create_dir_all(&configs_dir)?;
    let config_path = configs_dir.join("default.conf");
    let mut lines = vec![
        "project_root=.".to_string(),
        "coding_agent=opencode".to_string(),
        "coding_model=zai-coding-plan/glm-4.7".to_string(),
    ];

    if let Some(scripts_dir) = aikit_sdk::scripts_dir(workspace, "newton") {
        if scripts_dir.join("post-success.sh").is_file() {
            lines.push("post_success_script=.newton/scripts/post-success.sh".to_string());
        }
        if scripts_dir.join("post-failure.sh").is_file() {
            lines.push("post_fail_script=.newton/scripts/post-failure.sh".to_string());
        }
    }

    fs::write(config_path, lines.join("\n"))?;
    Ok(())
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

fn build_batch_hook_env(
    batch_config: &BatchProjectConfig,
    project_id: &str,
    task_id: &str,
    goal_file: &Path,
    result: &str,
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
    env_vars.insert("NEWTON_RESULT".to_string(), result.to_string());
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
