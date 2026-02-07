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

    // 1. Load and merge config
    let config_file = if let Some(ref path) = args.config {
        ConfigLoader::load_from_file(path)?
    } else {
        ConfigLoader::load_from_workspace(&args.path)?
    };

    let newton_config = ConfigLoader::merge_with_args(
        config_file,
        args.evaluator_cmd.clone(),
        args.advisor_cmd.clone(),
        args.executor_cmd.clone(),
        args.control_file.clone(),
        args.branch_from_goal,
        args.restore_branch,
        args.create_pr,
    );

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
    let branch_created =
        setup_branch(&args, &newton_config, &git_manager, goal_file.as_ref()).await?;

    // 6. Create execution configuration (merge CLI + config)
    let has_evaluator = args.evaluator_cmd.is_some() || newton_config.evaluator_cmd.is_some();
    let has_advisor = args.advisor_cmd.is_some() || newton_config.advisor_cmd.is_some();
    let has_executor = args.executor_cmd.is_some() || newton_config.executor_cmd.is_some();

    let exec_config = crate::core::entities::ExecutionConfiguration {
        evaluator_cmd: newton_config.evaluator_cmd,
        advisor_cmd: newton_config.advisor_cmd,
        executor_cmd: newton_config.executor_cmd,
        max_iterations: Some(args.max_iterations),
        max_time_seconds: Some(args.max_time),
        evaluator_timeout_ms: args.evaluator_timeout.map(|t| t * 1000),
        advisor_timeout_ms: args.advisor_timeout.map(|t| t * 1000),
        executor_timeout_ms: args.executor_timeout.map(|t| t * 1000),
        global_timeout_ms: Some(args.max_time * 1000),
        strict_toolchain_mode: has_evaluator || has_advisor || has_executor,
        resource_monitoring: false,
        verbose: args.verbose,
    };

    // 7. Create success policy
    let success_policy = SuccessPolicy::new(&args.path, &newton_config.control_file);

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

    // 10. On success: git operations if configured
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
            if (newton_config.git.create_pr_on_success || args.create_pr)
                && branch_created.is_some()
            {
                perform_git_ops(&git_manager, &branch_created, &original_branch)?;
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
    if (newton_config.git.restore_original_branch || args.restore_branch)
        && original_branch.is_some()
    {
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

pub async fn step(args: StepArgs) -> Result<()> {
    tracing::info!("Starting single step execution");

    // For step command, run a single iteration
    use crate::core::entities::ExecutionConfiguration as Config;
    let config = Config {
        evaluator_cmd: None,
        advisor_cmd: None,
        executor_cmd: None,
        max_iterations: Some(1),
        max_time_seconds: Some(300),
        evaluator_timeout_ms: None,
        advisor_timeout_ms: None,
        executor_timeout_ms: None,
        global_timeout_ms: Some(300000),
        strict_toolchain_mode: false,
        resource_monitoring: false,
        verbose: args.verbose,
    };

    // Create orchestrator
    let reporter = Box::new(DefaultErrorReporter);
    let orchestrator = OptimizationOrchestrator::new(JsonSerializer, FileUtils, reporter);

    // Run single iteration
    match orchestrator.run_optimization(&args.path, config).await {
        Ok(execution) => {
            tracing::info!("Step completed successfully");
            println!("Step completed");
            if !execution.iterations.is_empty() {
                let iteration = &execution.iterations[0];
                println!(
                    "Iteration {}: {:?}",
                    iteration.iteration_number, iteration.metadata.phase
                );
            }
        }
        Err(e) => {
            tracing::error!("Step failed: {}", e);
            eprintln!("Step failed: {}", e);
            return Err(e.into());
        }
    }

    Ok(())
}

/// Setup branch for optimization run
async fn setup_branch(
    args: &RunArgs,
    newton_config: &crate::core::config::NewtonConfig,
    git_manager: &GitManager,
    _goal_file: Option<&std::path::PathBuf>,
) -> Result<Option<String>> {
    if !git_manager.is_git_repo() {
        return Ok(None);
    }

    let branch_manager = git_manager.branch_manager();

    // If --branch provided, create or checkout that branch
    if let Some(ref branch_name) = args.branch {
        tracing::info!("Creating/checking out branch: {}", branch_name);
        if branch_manager.branch_exists(branch_name)? {
            branch_manager.checkout_branch(branch_name)?;
        } else {
            branch_manager.create_branch(branch_name)?;
        }
        return Ok(Some(branch_name.clone()));
    }

    // If --branch-from-goal or config.branch.create_from_goal, generate branch name
    if args.branch_from_goal || newton_config.branch.create_from_goal {
        if let Some(ref goal_text) = args.goal {
            if let Some(ref branch_namer_cmd) = newton_config.branch.branch_namer_cmd {
                let state_dir = args.path.join(".newton/state");
                let branch_name = crate::core::git::BranchManager::generate_branch_name(
                    goal_text,
                    branch_namer_cmd,
                    &state_dir,
                )?;

                tracing::info!("Generated branch name: {}", branch_name);

                if branch_manager.branch_exists(&branch_name)? {
                    branch_manager.checkout_branch(&branch_name)?;
                } else {
                    branch_manager.create_branch(&branch_name)?;
                }

                return Ok(Some(branch_name));
            }
        }
    }

    Ok(None)
}

/// Perform git operations: commit, push, create PR
fn perform_git_ops(
    git_manager: &GitManager,
    branch_created: &Option<String>,
    original_branch: &Option<String>,
) -> Result<()> {
    let commit_manager = git_manager.commit_manager();
    let pr_manager = git_manager.pr_manager();

    // Commit changes if any
    if commit_manager.has_changes()? {
        let commit_message =
            "Newton optimization completed\n\nCo-Authored-By: Newton <noreply@newton.ai>"
                .to_string();
        commit_manager.commit_all(&commit_message)?;
        tracing::info!("Committed changes");
    }

    // Push branch if created
    if let Some(ref branch_name) = branch_created {
        commit_manager.push(branch_name)?;
        tracing::info!("Pushed branch: {}", branch_name);

        // Create PR if gh is available and PR doesn't exist
        if pr_manager.is_gh_available() {
            if !pr_manager.pr_exists(branch_name)? {
                let base_branch = original_branch.as_deref().unwrap_or("main");
                let pr_title = format!("Newton: Optimization on {}", branch_name);
                let pr_body =
                    "This PR was automatically created by Newton after successful optimization.";

                let pr_url = pr_manager.create_pr(&pr_title, pr_body, base_branch)?;
                println!("Created PR: {}", pr_url);
                tracing::info!("Created PR: {}", pr_url);
            } else {
                tracing::info!("PR already exists for branch: {}", branch_name);
            }
        } else {
            tracing::warn!("gh CLI not available, skipping PR creation");
        }
    }

    Ok(())
}

pub async fn status(args: StatusArgs) -> Result<()> {
    tracing::info!("Checking execution status: {}", args.execution_id);

    // For now, implement basic status checking
    // In a full implementation, this would read execution state from path
    println!("Execution ID: {}", args.execution_id);
    println!("Path: {}", args.path.display());
    println!("Status: Unknown (execution tracking not yet implemented)");

    // TODO: Implement actual status retrieval from artifacts
    Ok(())
}

pub async fn report(args: ReportArgs) -> Result<()> {
    tracing::info!("Generating report for execution: {}", args.execution_id);

    // For now, implement basic report generation
    println!("Report for Execution: {}", args.execution_id);
    println!("Path: {}", args.path.display());
    println!("Format: {:?}", args.format);

    // TODO: Implement actual report generation from artifacts
    match args.format {
        crate::cli::args::ReportFormat::Text => {
            println!("Text report format not yet implemented");
        }
        crate::cli::args::ReportFormat::Json => {
            println!("JSON report format not yet implemented");
        }
    }

    Ok(())
}

pub async fn error(args: ErrorArgs) -> Result<()> {
    tracing::info!("Analyzing errors for execution: {}", args.execution_id);

    println!("Error Analysis for Execution: {}", args.execution_id);
    println!("Verbose: {}", args.verbose);

    // TODO: Implement actual error analysis from artifacts
    println!("Error analysis not yet implemented");

    Ok(())
}
