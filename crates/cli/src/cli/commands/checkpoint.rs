#![allow(clippy::result_large_err)]

use crate::cli::args::{CheckpointArgs, CheckpointCommand};
use crate::cli::workspace_paths::{resolve_state_dir, state_checkpoints_dir};
use humantime::format_duration;
use newton_core::core::error::AppError;
use newton_core::core::types::ErrorCategory;
use newton_core::workflow::checkpoint;
use serde_json::{json, Value};
use std::{path::PathBuf, result::Result as StdResult};

pub fn checkpoints(args: CheckpointArgs) -> StdResult<(), AppError> {
    match args.command {
        CheckpointCommand::List {
            workspace,
            state_dir,
            json,
        } => workflow_checkpoints_list(workspace, state_dir, json),
        CheckpointCommand::Clean {
            workspace,
            state_dir,
            older_than,
        } => workflow_checkpoints_clean(workspace, state_dir, older_than),
    }
}

fn workflow_checkpoints_list(
    workspace: Option<PathBuf>,
    state_dir: Option<PathBuf>,
    format_json: bool,
) -> StdResult<(), AppError> {
    let workspace = super::resolve_workflow_workspace(workspace)?;
    let state_dir = resolve_state_dir(&workspace, state_dir.as_deref());
    let mut entries = checkpoint::list_checkpoints_at(&state_checkpoints_dir(&state_dir))?;

    entries.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    if format_json {
        let items: Vec<Value> = entries
            .iter()
            .map(|summary| {
                json!({
                    "execution_id": summary.execution_id.to_string(),
                    "status": summary.status.as_str(),
                    "started_at": summary.started_at.to_rfc3339(),
                    "checkpoint_age": format!("{} ago", format_duration(summary.checkpoint_age)),
                    "size": summary.checkpoint_size,
                })
            })
            .collect();
        let serialized = serde_json::to_string_pretty(&items).map_err(|err| {
            AppError::new(
                ErrorCategory::SerializationError,
                format!("failed to serialize checkpoint list: {err}"),
            )
        })?;
        println!("{serialized}");
        return Ok(());
    }

    println!(
        "{:<36} {:<10} {:<16} {:<14} {:>7}",
        "EXECUTION ID", "STATUS", "STARTED AT", "CHECKPOINT AGE", "SIZE"
    );
    println!("{}", "-".repeat(93));

    for summary in entries {
        println!(
            "{:<36} {:<10} {:<16} {:<14} {:>7}",
            summary.execution_id,
            summary.status.as_str(),
            super::log::format_datetime_short(&summary.started_at),
            format!(
                "{} ago",
                super::log::format_duration_short(summary.checkpoint_age)
            ),
            super::log::format_bytes(summary.checkpoint_size),
        );
    }
    Ok(())
}

fn workflow_checkpoints_clean(
    workspace: Option<PathBuf>,
    state_dir: Option<PathBuf>,
    older_than: String,
) -> StdResult<(), AppError> {
    let workspace = super::resolve_workflow_workspace(workspace)?;
    let state_dir = resolve_state_dir(&workspace, state_dir.as_deref());
    let duration = super::log::parse_duration_arg(&older_than)?;
    checkpoint::clean_checkpoints_at(&state_checkpoints_dir(&state_dir), duration)?;
    println!("Removed checkpoints older than {older_than}");
    Ok(())
}
