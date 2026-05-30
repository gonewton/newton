#![allow(clippy::result_large_err)]

use crate::cli::args::{ArtifactArgs, ArtifactCommand};
use crate::cli::workspace_paths::{resolve_state_dir, state_artifacts_dir, state_checkpoints_dir};
use newton_core::core::error::AppError;
use newton_core::workflow::artifacts;
use std::{path::PathBuf, result::Result as StdResult};

pub fn artifacts(args: ArtifactArgs) -> StdResult<(), AppError> {
    match args.command {
        ArtifactCommand::Clean {
            workspace,
            state_dir,
            older_than,
        } => workflow_artifacts_clean(workspace, state_dir, older_than),
    }
}

fn workflow_artifacts_clean(
    workspace: Option<PathBuf>,
    state_dir: Option<PathBuf>,
    older_than: String,
) -> StdResult<(), AppError> {
    let workspace = super::resolve_workflow_workspace(workspace)?;
    let state_dir = resolve_state_dir(&workspace, state_dir.as_deref());
    let duration = super::log::parse_duration_arg(&older_than)?;
    artifacts::ArtifactStore::clean_artifacts_at(
        &state_artifacts_dir(&state_dir),
        &state_checkpoints_dir(&state_dir),
        duration,
    )?;
    println!("Cleaned artifacts older than {older_than}");
    Ok(())
}
