use crate::{
    cli::args::{BatchArgs, ErrorArgs, ReportArgs, RunArgs, StatusArgs, StepArgs},
    core::{
        batch_config::{find_workspace_root, BatchProjectConfig},
        ConfigLoader, ConfigValidator, DefaultErrorReporter, OptimizationOrchestrator,
        SuccessPolicy,
    },
    utils::serialization::{FileUtils, JsonSerializer},
    Result,
};
use anyhow::anyhow;
use chrono::Utc;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::time::sleep;

pub async fn run(args: RunArgs) -> Result<()> {
    tracing::info!("Starting Newton Loop optimization run");

    let newton_config = if let Some(ref path) = args.config {
        ConfigLoader::load_from_file(path)?.unwrap_or_default()
    } else {
        ConfigLoader::load_from_workspace(&args.path)?
    };

    ConfigValidator::validate(&newton_config)?;

    let goal_file = prepare_goal_file(&args)?;

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

    let exec_config = crate::core::entities::ExecutionConfiguration {
        evaluator_cmd: args.evaluator_cmd.clone(),
        advisor_cmd: args.advisor_cmd.clone(),
        executor_cmd: args.executor_cmd.clone(),
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
    let success_policy = SuccessPolicy::new(&args.path, &control_file);

    let reporter = Box::new(DefaultErrorReporter);
    let orchestrator = OptimizationOrchestrator::new(JsonSerializer, FileUtils, reporter);

    let result = orchestrator
        .run_optimization_with_policy(
            &args.path,
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
    let draft_dir = plan_project_dir.join("draft");
    fs::create_dir_all(&todo_dir)?;
    fs::create_dir_all(&completed_dir)?;
    fs::create_dir_all(&draft_dir)?;

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

        restore_env_vars(overrides);

        if run_result.is_ok() {
            let destination = completed_dir.join(
                plan_file
                    .file_name()
                    .ok_or_else(|| anyhow!("Plan file missing name"))?,
            );
            if destination.exists() {
                fs::remove_file(&destination)?;
            }
            fs::rename(&plan_file, &destination)?;
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

fn prepare_goal_file(args: &RunArgs) -> Result<Option<PathBuf>> {
    if let Some(ref path) = args.goal_file {
        Ok(Some(path.clone()))
    } else if let Some(ref goal_text) = args.goal {
        let path = args.path.join(".newton/state/goal.txt");
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, goal_text)?;
        Ok(Some(path))
    } else {
        Ok(None)
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
