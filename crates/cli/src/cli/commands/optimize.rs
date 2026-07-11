use crate::cli::args::OptimizeArgs;
use crate::Result;
use anyhow::anyhow;
use newton_core::core::plan_queue_config::PlanQueueConfig;
use newton_core::workflow::{schema as workflow_schema, transform as workflow_transform};
use serde_json::json;
use std::{
    fs,
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
    let workspace = plan_config.project_root.clone();
    let workflow_path = plan_config.workflow_file.clone();
    let raw_document = workflow_schema::parse_workflow(&workflow_path)?;
    // Live execution: honor the workflow's own opt-in (spec 074 S8) so
    // `env()` works in macro args / include_if / templates, not just
    // task `$expr` params.
    let allow_env_fn = raw_document.workflow.settings.allow_env_fn;
    let mut document = workflow_transform::apply_default_pipeline(raw_document, allow_env_fn)?;

    let trigger_payload = json!({
        "input_file": task_layout.input_file.display().to_string(),
        "workspace": plan_config.project_root.display().to_string(),
    });
    document.triggers = Some(workflow_schema::WorkflowTrigger::manual(
        trigger_payload.clone(),
    ));

    // Input validation: max_input_bytes
    let settings = &document.workflow.settings;
    if let Some(max_bytes) = settings.io_settings.max_input_bytes {
        let serialized = serde_json::to_string(&trigger_payload).unwrap_or_default();
        if serialized.len() > max_bytes {
            return Err(anyhow!(
                "WFG-IO-001: trigger payload exceeds max_input_bytes ({})",
                max_bytes
            ));
        }
    }

    // Input validation: input_schema
    if let Some(schema) = &settings.io.input_schema {
        if let Err(e) = newton_core::workflow::io::validate_input_schema(schema, &trigger_payload) {
            return Err(anyhow!(
                "{}: input schema validation failed: {}",
                e.code,
                e.message
            ));
        }
    }

    // Use the shared execution builder for backend + sink wiring
    let exec_setup = super::shared_execution::build_execution_setup(
        task_layout.state_dir.clone(),
        None,
        None,
        None,
    )
    .await
    .map_err(|e| anyhow!("{}: {}", e.code, e.message))?;

    let settings = document.workflow.settings.clone();
    let ailoop_ctx =
        newton_core::integrations::ailoop::init_context_for_command_name(&workspace, "optimize")
            .ok()
            .flatten();
    // Pass the resolved state root explicitly (the same one `build_execution_setup`
    // above just wired into the executor's DbSink) instead of mutating the
    // process-global NEWTON_STATE_DIR env var — that workaround let concurrent
    // executions race each other's state resolution.
    let registry = super::build_operator_registry(
        workspace.clone(),
        &task_layout.state_dir,
        &settings,
        ailoop_ctx,
    )
    .await;

    let result = newton_core::workflow::executor::execute_workflow(
        document,
        workflow_path,
        registry,
        workspace,
        exec_setup.overrides,
    )
    .await;

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

/// Pick the next plan file from `todo_dir`.
///
/// Ordering contract: **lexicographic FIFO by filename**. All file entries
/// (subdirectories are ignored) are collected and sorted by `file_name()`,
/// and the first one is returned — NOT by mtime, and NOT by whatever order
/// the OS happens to hand back from `read_dir` (which is arbitrary and can
/// vary by filesystem/platform). Plan producers that want strict ordering
/// MUST name files with a sortable prefix, e.g. `001-do-x.md`,
/// `002-do-y.md` (spec 074, B21).
///
/// When the queue is empty: returns `Ok(None)` immediately if `once` is set,
/// otherwise polls every `sleep_duration` seconds until a file appears.
async fn fetch_next_plan(
    todo_dir: &Path,
    once: bool,
    sleep_duration: u64,
) -> Result<Option<PathBuf>> {
    loop {
        let mut candidates: Vec<PathBuf> = fs::read_dir(todo_dir)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect();
        candidates.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        if let Some(path) = candidates.into_iter().next() {
            return Ok(Some(path));
        }

        if once {
            tracing::info!("Plan queue empty; exiting after --once");
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_secs(sleep_duration)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetch_next_plan_picks_lexicographically_first_filename() {
        let dir = tempfile::tempdir().unwrap();
        // Written in reverse creation/mtime order: b.md first, then a.md. A
        // naive "first entry from read_dir" or an mtime-based pick could
        // return b.md; the FIFO contract requires a.md (lexicographically
        // first) regardless of creation order.
        std::fs::write(dir.path().join("b.md"), "plan b").unwrap();
        std::fs::write(dir.path().join("a.md"), "plan a").unwrap();

        let picked = fetch_next_plan(dir.path(), true, 0)
            .await
            .unwrap()
            .expect("todo dir has files, must return Some");

        assert_eq!(
            picked.file_name().and_then(|n| n.to_str()),
            Some("a.md"),
            "must pick the lexicographically first filename, not mtime/OS order"
        );
    }

    #[tokio::test]
    async fn fetch_next_plan_numeric_prefix_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("002-second.md"), "second").unwrap();
        std::fs::write(dir.path().join("001-first.md"), "first").unwrap();

        let picked = fetch_next_plan(dir.path(), true, 0)
            .await
            .unwrap()
            .expect("todo dir has files, must return Some");

        assert_eq!(
            picked.file_name().and_then(|n| n.to_str()),
            Some("001-first.md")
        );
    }

    #[tokio::test]
    async fn fetch_next_plan_ignores_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("a-subdir")).unwrap();
        std::fs::write(dir.path().join("z.md"), "only file").unwrap();

        let picked = fetch_next_plan(dir.path(), true, 0)
            .await
            .unwrap()
            .expect("todo dir has one file, must return Some");

        assert_eq!(picked.file_name().and_then(|n| n.to_str()), Some("z.md"));
    }

    #[tokio::test]
    async fn fetch_next_plan_empty_dir_once_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let picked = fetch_next_plan(dir.path(), true, 0).await.unwrap();
        assert!(picked.is_none());
    }
}
