use crate::cli::args::OptimizeArgs;
use crate::Result;
use anyhow::anyhow;
use newton_core::core::batch_config::PlanQueueConfig;
use newton_core::workflow::{
    executor::ExecutionOverrides, schema as workflow_schema, transform as workflow_transform,
};
use serde_json::json;
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};
struct OptimizeDirs {
    todo_dir: PathBuf,
    completed_dir: PathBuf,
    failed_dir: PathBuf,
}

fn ensure_optimize_dirs(workspace_root: &Path, project_id: &str) -> Result<OptimizeDirs> {
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

    fs::create_dir_all(&todo_dir)?;
    fs::create_dir_all(&completed_dir)?;
    fs::create_dir_all(plan_project_dir.join("draft"))?;
    fs::create_dir_all(&failed_dir)?;
    fs::create_dir_all(plan_project_dir.join("abandoned"))?;

    Ok(OptimizeDirs {
        todo_dir,
        completed_dir,
        failed_dir,
    })
}

pub async fn optimize(args: OptimizeArgs) -> Result<()> {
    tracing::info!("Starting optimization loop for project {}", args.project_id);

    let workspace_root = validate_optimize_workspace(args.workspace.clone())?;
    let plan_config = PlanQueueConfig::load(&workspace_root, &args.project_id)?;
    let dirs = ensure_optimize_dirs(&workspace_root, &args.project_id)?;

    loop {
        let Some(plan_file) =
            fetch_next_plan(&dirs.todo_dir, args.once, args.poll_interval_seconds).await?
        else {
            return Ok(());
        };

        let task_layout = prepare_task_layout(&plan_config, &plan_file)?;
        let run_result = execute_workflow_for_plan(&plan_config, &task_layout).await;

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
            tokio::time::sleep(Duration::from_secs(args.poll_interval_seconds)).await;
        }
    }
}

async fn execute_workflow_for_plan(
    plan_config: &PlanQueueConfig,
    task_layout: &TaskLayout,
) -> Result<()> {
    fs::create_dir_all(task_layout.state_dir.join("workflows"))?;
    fs::create_dir_all(task_layout.state_dir.join("artifacts").join("workflows"))?;

    let workspace = plan_config.project_root.clone();
    let workflow_path = plan_config.workflow_file.clone();
    let raw_document = workflow_schema::parse_workflow(&workflow_path)?;
    let mut document = workflow_transform::apply_default_pipeline(raw_document)?;
    document.triggers = Some(workflow_schema::WorkflowTrigger::manual(json!({
        "input_file": task_layout.input_file.display().to_string(),
        "workspace": plan_config.project_root.display().to_string(),
    })));

    let overrides = ExecutionOverrides {
        checkpoint_base_path: Some(task_layout.state_dir.join("workflows")),
        artifact_base_path: Some(task_layout.state_dir.join("artifacts").join("workflows")),
        pre_seed_nodes: true,
        ..Default::default()
    };

    let settings = document.workflow.settings.clone();
    let ailoop_ctx =
        newton_core::integrations::ailoop::init_context_for_command_name(&workspace, "optimize")
            .ok()
            .flatten();
    let registry = super::build_operator_registry(workspace.clone(), &settings, ailoop_ctx);

    let previous_state_dir = env::var_os("NEWTON_STATE_DIR");
    env::set_var("NEWTON_STATE_DIR", &task_layout.state_dir);

    let result = newton_core::workflow::executor::execute_workflow(
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

fn prepare_task_layout(plan_config: &PlanQueueConfig, plan_file: &Path) -> Result<TaskLayout> {
    let task_id = plan_file
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Plan file missing stem: {}", plan_file.display()))?;
    let task_root = plan_config
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

fn validate_optimize_workspace(workspace: Option<PathBuf>) -> Result<PathBuf> {
    let workspace_root = match workspace {
        Some(p) => p,
        None => std::env::current_dir()
            .map_err(|e| anyhow!("failed to resolve current directory: {e}"))?,
    };
    let configs_dir = workspace_root.join(".newton").join("configs");
    if !configs_dir.is_dir() {
        return Err(anyhow!(
            "Workspace {} must contain .newton/configs",
            workspace_root.display()
        ));
    }
    Ok(workspace_root)
}

async fn fetch_next_plan(
    todo_dir: &Path,
    once: bool,
    sleep_duration: u64,
) -> Result<Option<PathBuf>> {
    loop {
        let mut entries = fs::read_dir(todo_dir)?;
        if let Some(Ok(entry)) = entries.next() {
            let path = entry.path();
            if path.is_file() {
                return Ok(Some(path));
            }
        }

        if once {
            tracing::info!("Plan queue empty; exiting after --once");
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_secs(sleep_duration)).await;
    }
}
