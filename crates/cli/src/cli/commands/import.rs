use crate::cli::args::ImportArgs;
use crate::cli::workspace_paths::{
    resolve_state_dir, state_backend_sqlite_url, state_checkpoints_dir,
};
use newton_backend::SqliteBackendStore;
use newton_core::core::error::AppError;
use newton_core::core::types::ErrorCategory;
use newton_core::workflow::checkpoint;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub async fn workflow_import(args: ImportArgs) -> StdResult<(), AppError> {
    let workspace = super::resolve_workflow_workspace(args.workspace)?;
    let state_dir = resolve_state_dir(&workspace, args.state_dir.as_deref());
    let checkpoints_dir = state_checkpoints_dir(&state_dir);

    if !checkpoints_dir.exists() && !args.recursive {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "IMPORT-002: resolved state dir has no workflows/ subdirectory and --recursive was not supplied: {}",
                checkpoints_dir.display()
            ),
        )
        .with_code("IMPORT-002"));
    }

    if state_dir.exists() && !state_dir.is_dir() {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "STATE-DIR-001: --state-dir path is not a directory: {}",
                state_dir.display()
            ),
        )
        .with_code("STATE-DIR-001"));
    }
    fs::create_dir_all(&state_dir).map_err(|e| {
        AppError::new(ErrorCategory::IoError, format!("STATE-DIR-002: {e}"))
            .with_code("STATE-DIR-002")
    })?;

    let db_url = state_backend_sqlite_url(&state_dir);
    let backend = SqliteBackendStore::new(&db_url).await.map_err(|e| {
        AppError::new(
            ErrorCategory::IoError,
            format!("STATE-DIR-003: {}", e.message),
        )
        .with_code("STATE-DIR-003")
    })?;
    let backend_arc: Arc<dyn newton_backend::BackendStore> = Arc::new(backend);

    let mut uuid_dirs: Vec<(PathBuf, PathBuf)> = Vec::new();

    if !args.recursive {
        if checkpoints_dir.exists() {
            for entry in fs::read_dir(&checkpoints_dir)
                .map_err(|e| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!("failed to read workflows dir: {e}"),
                    )
                })?
                .flatten()
            {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    uuid_dirs.push((checkpoints_dir.clone(), entry.path()));
                }
            }
        }
    } else {
        walk_workspace_for_state_dirs(&workspace, &mut uuid_dirs);
    }

    let mut found = 0usize;
    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut errors = 0usize;

    for (base, uuid_dir) in &uuid_dirs {
        let file_name = uuid_dir.file_name().unwrap_or_default().to_string_lossy();
        let uuid = match uuid::Uuid::parse_str(&file_name) {
            Ok(u) => u,
            Err(_) => continue,
        };
        found += 1;

        let instance_id_str = uuid.to_string();
        if let Ok(existing) = backend_arc.get_workflow_instance(&instance_id_str).await {
            let is_terminal = matches!(
                existing.status,
                newton_types::WorkflowStatus::Succeeded
                    | newton_types::WorkflowStatus::Failed
                    | newton_types::WorkflowStatus::Cancelled
            );
            if is_terminal {
                skipped += 1;
                continue;
            }
        }

        let execution = match checkpoint::load_execution_from_base(base, &uuid) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(
                    code = "IMPORT-001",
                    error = %e.message,
                    uuid = %uuid,
                    "failed to read execution.json"
                );
                errors += 1;
                continue;
            }
        };

        // S11: single conversion point, `From<WorkflowExecutionStatus> for
        // WorkflowStatus` (crates/core/src/workflow/state.rs).
        let status: newton_types::WorkflowStatus = execution.status.into();

        let instance = newton_types::WorkflowInstance {
            instance_id: instance_id_str.clone(),
            workflow_id: execution.workflow_file.clone(),
            status: status.clone(),
            nodes: vec![],
            started_at: execution.started_at,
            ended_at: execution.completed_at,
            definition: None,
            linked_plan_id: None,
        };

        if let Err(e) = backend_arc.upsert_workflow_instance(&instance).await {
            tracing::warn!(
                code = "IMPORT-001",
                error = %e.message,
                uuid = %uuid,
                "failed to upsert workflow instance"
            );
            errors += 1;
            continue;
        }

        if let Ok(cp) = checkpoint::load_checkpoint_from_base(base, &uuid) {
            for task_id in cp.completed.keys() {
                let node = newton_types::NodeState {
                    node_id: task_id.clone(),
                    status: newton_types::NodeStatus::Succeeded,
                    started_at: None,
                    ended_at: None,
                    operator_type: None,
                };
                if let Err(e) = backend_arc.upsert_node_state(&instance_id_str, &node).await {
                    tracing::warn!(
                        code = "IMPORT-001",
                        error = %e.message,
                        "failed to upsert node state"
                    );
                }
            }
            for task_id in &cp.ready_queue {
                let node = newton_types::NodeState {
                    node_id: task_id.clone(),
                    status: newton_types::NodeStatus::Pending,
                    started_at: None,
                    ended_at: None,
                    operator_type: None,
                };
                if let Err(e) = backend_arc.upsert_node_state(&instance_id_str, &node).await {
                    tracing::warn!(
                        code = "IMPORT-001",
                        error = %e.message,
                        "failed to upsert node state for ready_queue entry"
                    );
                }
            }
        }

        imported += 1;
    }

    println!(
        "Import complete: {} found, {} imported, {} skipped (already present), {} errors",
        found, imported, skipped, errors
    );
    Ok(())
}

fn walk_workspace_for_state_dirs(workspace: &Path, result: &mut Vec<(PathBuf, PathBuf)>) {
    let skip_dirs = ["node_modules", "target"];
    if let Ok(entries) = fs::read_dir(workspace) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if skip_dirs.contains(&name.as_str()) {
                continue;
            }
            if path.is_dir() {
                let workflows_dir = path.join(".newton").join("state").join("workflows");
                if workflows_dir.is_dir() {
                    if let Ok(uuid_entries) = fs::read_dir(&workflows_dir) {
                        for uuid_entry in uuid_entries.flatten() {
                            if uuid_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                                result.push((workflows_dir.clone(), uuid_entry.path()));
                            }
                        }
                    }
                }
                walk_workspace_for_state_dirs(&path, result);
            }
        }
    }
}

use std::result::Result as StdResult;
