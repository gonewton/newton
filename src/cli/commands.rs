use crate::{
    cli::args::{ErrorArgs, ReportArgs, RunArgs, StatusArgs, StepArgs},
    core::{
        ConfigLoader, ConfigValidator, DefaultErrorReporter, GitManager, OptimizationOrchestrator,
        SuccessPolicy,
    },
    utils::serialization::{FileUtils, JsonSerializer},
    Result,
};
use std::collections::HashMap;

pub async fn run(args: RunArgs) -> Result<()> {
    tracing::info!("Starting Newton Loop optimization run");

    // 1. Load config
    let newton_config = if let Some(ref path) = args.config {
        ConfigLoader::load_from_file(path)?.unwrap_or_default()
    } else {
        ConfigLoader::load_from_workspace(&args.path)?
    };

    ConfigValidator::validate(&newton_config)?;

    // 2. Setup goal file if --goal provided
    let goal_file = if let Some(ref goal_text) = args.goal {
        let path = args.path.join(".newton/state/goal.txt");
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, goal_text)?;
        Some(path)
    } else {
        None
    };

    // 3. Resolve user feedback (CLI > env var)
    let user_feedback = args
        .feedback
        .clone()
        .or_else(|| std::env::var("NEWTON_USER_FEEDBACK").ok());

    // 4. Git: record original branch
    let git_manager = GitManager::new(&args.path);
    let original_branch = if git_manager.is_git_repo() {
        Some(git_manager.current_branch()?)
    } else {
        None
    };

    // 5. Branch: create or checkout if requested
    let branch_created = if args.branch_from_goal {
        if let Some(ref goal_text) = args.goal {
            let branch_manager = git_manager.branch_manager();
            if let Some(branch_name) = generate_branch_from_goal(goal_text, &args.path)? {
                if branch_manager.branch_exists(&branch_name)? {
                    branch_manager.checkout_branch(&branch_name)?;
                } else {
                    branch_manager.create_branch(&branch_name)?;
                }
                Some(branch_name)
            } else {
                None
            }
        } else {
            None
        }
    } else if let Some(ref branch_name) = args.branch {
        let branch_manager = git_manager.branch_manager();
        if branch_manager.branch_exists(branch_name)? {
            branch_manager.checkout_branch(branch_name)?;
        } else {
            branch_manager.create_branch(branch_name)?;
        }
        Some(branch_name.clone())
    } else {
        None
    };

    // 6. Create execution configuration (CLI args only per PRD)
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

    // 7. Create success policy (use default or from args)
    let control_file = args
        .control_file
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "newton_control.json".to_string());
    let success_policy = SuccessPolicy::new(&args.path, &control_file);

    // 8. Build additional environment variables
    let mut additional_env = HashMap::new();
    if let Some(ref path) = goal_file {
        additional_env.insert("NEWTON_GOAL_FILE".to_string(), path.display().to_string());
    }
    if let Some(ref feedback) = user_feedback {
        additional_env.insert("NEWTON_USER_FEEDBACK".to_string(), feedback.clone());
    }

    // 9. Run optimization with success policy
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

    // 10. On success: git operations if requested
    let success = match result {
        Ok(execution) => {
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

            // Perform git operations if requested
            if args.create_pr && branch_created.is_some() {
                println!("Branch created: {:?}", branch_created);
                if original_branch.is_some() {
                    println!("Ready to create PR from branch {:?}", branch_created);
                }
            }

            true
        }
        Err(e) => {
            tracing::error!("Optimization failed: {}", e);
            eprintln!("Optimization failed: {}", e);
            false
        }
    };

    // 11. Restore original branch if configured
    if args.restore_branch && original_branch.is_some() {
        if let Some(ref orig_branch) = original_branch {
            tracing::info!("Restoring original branch: {}", orig_branch);
            git_manager.branch_manager().checkout_branch(orig_branch)?;
        }
    }

    if !success {
        return Err(anyhow::anyhow!("Optimization failed"));
    }

    Ok(())
}

fn generate_branch_from_goal(
    goal: &str,
    workspace_path: &std::path::Path,
) -> Result<Option<String>> {
    let state_dir = workspace_path.join(".newton/state");
    std::fs::create_dir_all(&state_dir)?;

    // Simple branch name generation (hash of goal)
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    goal.hash(&mut hasher);
    let hash = hasher.finish();

    let branch_name = format!("goal-{:x}", hash);

    Ok(Some(branch_name))
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
        return Err(anyhow::anyhow!("Execution {} not found", args.execution_id));
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

    let execution_dir = std::path::Path::new(".")
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
