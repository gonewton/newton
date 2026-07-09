//! Shared execution-setup builder used by both `workflow run` and `optimize`.
//!
//! Consolidates state-directory validation, SQLite backend initialisation,
//! sink wiring, and `ExecutionOverrides` construction so that every driver
//! goes through the same code path.

use crate::cli::workspace_paths::{
    state_artifacts_dir, state_backend_sqlite_url, state_checkpoints_dir,
};
use newton_backend::SqliteBackendStore;
use newton_core::core::error::AppError;
use newton_core::core::types::ErrorCategory;
use newton_core::workflow::{
    executor::ExecutionOverrides,
    server_notifier::ServerNotifier,
    workflow_sink::{DbSink, FanoutSink, WorkflowSink},
};
use std::{fs, path::PathBuf, sync::Arc};

/// Everything needed to call `execute_workflow` through the shared path.
pub struct ExecutionSetup {
    pub state_dir: PathBuf,
    pub overrides: ExecutionOverrides,
}

/// Build the standard execution environment that every driver MUST use.
///
/// This function:
/// 1. Validates `state_dir` (must not be a non-directory path)
/// 2. Creates checkpoint and artifact subdirectories
/// 3. Initialises a `SqliteBackendStore`
/// 4. Wraps it in `DbSink`, optionally fanning out to `ServerNotifier`
/// 5. Returns `ExecutionOverrides` ready for `execute_workflow`
pub async fn build_execution_setup(
    state_dir: PathBuf,
    parallel_limit: Option<usize>,
    timeout_seconds: Option<u64>,
    server_url: Option<&str>,
) -> Result<ExecutionSetup, AppError> {
    if state_dir.exists() && !state_dir.is_dir() {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "STATE-DIR-001: --state-dir path exists but is not a directory: {}",
                state_dir.display()
            ),
        )
        .with_code("STATE-DIR-001"));
    }

    let checkpoints = state_checkpoints_dir(&state_dir);
    let artifacts = state_artifacts_dir(&state_dir);

    fs::create_dir_all(&checkpoints).map_err(|e| {
        AppError::new(
            ErrorCategory::IoError,
            format!("STATE-DIR-002: failed to create state directory: {}", e),
        )
        .with_code("STATE-DIR-002")
    })?;
    fs::create_dir_all(&artifacts).map_err(|e| {
        AppError::new(
            ErrorCategory::IoError,
            format!("STATE-DIR-002: failed to create artifacts directory: {}", e),
        )
        .with_code("STATE-DIR-002")
    })?;

    let backend = SqliteBackendStore::new(&state_backend_sqlite_url(&state_dir))
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("STATE-DIR-003: backend store init failed: {}", e.message),
            )
            .with_code("STATE-DIR-003")
        })?;

    let backend_arc: Arc<dyn newton_backend::BackendStore> = Arc::new(backend);
    let db_sink = Arc::new(DbSink::new(backend_arc));

    let sink: Option<Arc<dyn WorkflowSink>> = if let Some(url) = server_url {
        Some(Arc::new(FanoutSink(vec![
            db_sink as Arc<dyn WorkflowSink>,
            Arc::new(ServerNotifier::new(url.to_string())),
        ])))
    } else {
        Some(db_sink as Arc<dyn WorkflowSink>)
    };

    let overrides = ExecutionOverrides {
        parallel_limit,
        max_time_seconds: timeout_seconds,
        checkpoint_base_path: Some(checkpoints),
        artifact_base_path: Some(artifacts),
        sink,
        pre_seed_nodes: true,
        state_dir: Some(state_dir.clone()),
        ..Default::default()
    };

    Ok(ExecutionSetup {
        state_dir,
        overrides,
    })
}
