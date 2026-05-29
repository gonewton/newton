use crate::cli::args::BatchArgs;
use crate::Result;
use anyhow::anyhow;
use newton_core::core::batch_config::BatchProjectConfig;
use newton_core::workflow::{
    executor::ExecutionOverrides, schema as workflow_schema, transform as workflow_transform,
};
use serde_json::json;
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::time::sleep;

async fn sleep_if_needed(duration_secs: u64) {
    sleep(Duration::from_secs(duration_secs)).await;
}

struct BatchDirs {
    todo_dir: PathBuf,
    completed_dir: PathBuf,
    failed_dir: PathBuf,
    #[allow(dead_code)]
    draft_dir: PathBuf,
}

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
        sink: None,
        pre_seed_nodes: true,
    };

    let settings = document.workflow.settings.clone();
    let ailoop_ctx =
        newton_core::integrations::ailoop::init_context_for_command_name(&workspace, "batch")
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
