use crate::cli::args::{RunsArgs, RunsCommand};
use humantime::parse_duration;
use newton_core::core::error::AppError;
use newton_core::core::types::ErrorCategory;
use newton_core::workflow::checkpoint::WorkflowStatePaths;
use newton_core::workflow::state::{
    OutputRef, WorkflowCheckpoint, WorkflowExecution, WorkflowTaskRunRecord, WorkflowTaskStatus,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    result::Result as StdResult,
    time::Duration,
};

pub(super) fn format_duration_short(duration: Duration) -> String {
    let mut remaining = duration.as_secs();
    let mut parts = Vec::new();

    if remaining == 0 {
        return "0s".to_string();
    }

    const SECONDS_PER_DAY: u64 = 86400;
    const SECONDS_PER_HOUR: u64 = 3600;
    const SECONDS_PER_MINUTE: u64 = 60;

    if remaining >= SECONDS_PER_DAY {
        let days = remaining / SECONDS_PER_DAY;
        parts.push(format!("{days}d"));
        remaining %= SECONDS_PER_DAY;
    }

    if remaining >= SECONDS_PER_HOUR && parts.len() < 2 {
        let hours = remaining / SECONDS_PER_HOUR;
        parts.push(format!("{hours}h"));
        remaining %= SECONDS_PER_HOUR;
    }

    if remaining >= SECONDS_PER_MINUTE && parts.len() < 2 {
        let minutes = remaining / SECONDS_PER_MINUTE;
        parts.push(format!("{minutes}m"));
        remaining %= SECONDS_PER_MINUTE;
    }

    if parts.is_empty() && parts.len() < 2 {
        parts.push(format!("{remaining}s"));
    }

    parts.join(" ")
}

pub(super) fn format_datetime_short(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M").to_string()
}

pub(super) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut index = 0;
    while size >= 1024.0 && index < UNITS.len() - 1 {
        size /= 1024.0;
        index += 1;
    }
    if index == 0 {
        format!("{} {}", bytes, UNITS[index])
    } else {
        format!("{:.1} {}", size, UNITS[index])
    }
}

pub(super) fn parse_duration_arg(value: &str) -> StdResult<Duration, AppError> {
    parse_duration(value).map_err(|err| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("failed to parse duration {value}: {err}"),
        )
    })
}

pub fn log(args: RunsArgs) -> StdResult<(), AppError> {
    match args.command {
        RunsCommand::List {
            workspace,
            last,
            json,
        } => log_list(workspace, last, json),
        RunsCommand::Show {
            run_id,
            workspace,
            task,
            verbose,
            json,
        } => log_show(run_id, workspace, task, verbose, json),
    }
}

fn log_list(
    workspace: Option<PathBuf>,
    last: Option<usize>,
    emit_json: bool,
) -> StdResult<(), AppError> {
    if let Some(n) = last {
        if n == 0 {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "--last must be a positive integer (greater than zero)",
            )
            .with_code("LOG-003"));
        }
    }

    let workspace = super::resolve_workflow_workspace(workspace)?;
    let base = WorkflowStatePaths::workspace_root(&workspace);

    let mut entries: Vec<(WorkflowExecution, Option<usize>)> = Vec::new();

    if base.exists() {
        for entry in fs::read_dir(&base)
            .map_err(|err| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to list workflows state: {err}"),
                )
            })?
            .flatten()
        {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            if let Ok(uuid) = uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()) {
                let exec_file = base.join(uuid.to_string()).join("execution.json");
                if let Ok(bytes) = fs::read(&exec_file) {
                    if let Ok(execution) = serde_json::from_slice::<WorkflowExecution>(&bytes) {
                        let checkpoint_task_count = {
                            let ckpt_file = base.join(uuid.to_string()).join("checkpoint.json");
                            fs::read(&ckpt_file)
                                .ok()
                                .and_then(|b| serde_json::from_slice::<WorkflowCheckpoint>(&b).ok())
                                .map(|ckpt| ckpt.completed.len())
                        };
                        entries.push((execution, checkpoint_task_count));
                    }
                }
            }
        }
    }

    entries.sort_by(|(a, _), (b, _)| {
        b.started_at
            .cmp(&a.started_at)
            .then_with(|| b.execution_id.to_string().cmp(&a.execution_id.to_string()))
    });

    if let Some(n) = last {
        entries.truncate(n);
    }

    if emit_json {
        let items: Vec<Value> = entries
            .iter()
            .map(|(exec, ckpt_count)| {
                let task_count = ckpt_count.unwrap_or(exec.task_runs.len());
                let duration_ms = exec
                    .completed_at
                    .map(|completed| {
                        completed
                            .signed_duration_since(exec.started_at)
                            .num_milliseconds()
                    })
                    .filter(|&ms| ms >= 0)
                    .map(|ms| ms as u64);
                let failed_task_id = exec
                    .task_runs
                    .iter()
                    .find(|r| r.status == WorkflowTaskStatus::Failed)
                    .map(|r| r.task_id.clone());
                json!({
                    "execution_id": exec.execution_id.to_string(),
                    "workflow_file": exec.workflow_file,
                    "status": exec.status.as_str(),
                    "started_at": exec.started_at.to_rfc3339(),
                    "task_count": task_count,
                    "duration_ms": duration_ms,
                    "failed_task_id": failed_task_id,
                })
            })
            .collect();
        let serialized = serde_json::to_string_pretty(&items).map_err(|err| {
            AppError::new(
                ErrorCategory::SerializationError,
                format!("failed to serialize execution list: {err}"),
            )
        })?;
        println!("{serialized}");
        return Ok(());
    }

    println!(
        "{:<36}  {:<20}  {:<10}  {:<19}  {:>5}  DURATION",
        "EXECUTION ID", "WORKFLOW", "STATUS", "STARTED AT", "TASKS"
    );
    println!("{}", "-".repeat(102));
    for (exec, ckpt_count) in &entries {
        let task_count = ckpt_count.unwrap_or(exec.task_runs.len());
        let duration_str = exec
            .completed_at
            .map(|completed| {
                let ms = completed
                    .signed_duration_since(exec.started_at)
                    .num_milliseconds();
                if ms < 0 {
                    "-".to_string()
                } else {
                    format_duration_short(Duration::from_millis(ms as u64))
                }
            })
            .unwrap_or_else(|| "-".to_string());
        let workflow_short = {
            let wf = &exec.workflow_file;
            if wf.len() > 20 {
                format!("...{}", &wf[wf.len() - 17..])
            } else {
                wf.clone()
            }
        };
        println!(
            "{:<36}  {:<20}  {:<10}  {:<19}  {:>5}  {}",
            exec.execution_id,
            workflow_short,
            exec.status.as_str(),
            exec.started_at.format("%Y-%m-%d %H:%M:%S"),
            task_count,
            duration_str,
        );
    }
    Ok(())
}

fn log_show(
    execution_id: uuid::Uuid,
    workspace: Option<PathBuf>,
    task_filter: Option<String>,
    verbose: bool,
    emit_json: bool,
) -> StdResult<(), AppError> {
    let workspace = super::resolve_workflow_workspace(workspace)?;
    let paths = WorkflowStatePaths::new(&workspace, &execution_id);

    if !paths.execution_file.exists() {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "execution not found: no execution.json at {} (LOG-001)",
                paths.execution_file.display()
            ),
        )
        .with_code("LOG-001"));
    }
    let exec_bytes = fs::read(&paths.execution_file).map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to read execution.json: {err}"),
        )
    })?;
    let execution: WorkflowExecution = serde_json::from_slice(&exec_bytes).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to deserialize execution.json: {err}"),
        )
    })?;

    let checkpoint_opt: Option<WorkflowCheckpoint> = if paths.checkpoint_file.exists() {
        fs::read(&paths.checkpoint_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
    } else {
        None
    };

    if emit_json {
        return log_show_json(
            execution_id,
            execution,
            checkpoint_opt,
            task_filter,
            &workspace,
        );
    }

    log_show_text(
        execution_id,
        execution,
        checkpoint_opt,
        task_filter,
        verbose,
        &workspace,
    )
}

fn collect_sorted_records(checkpoint: &WorkflowCheckpoint) -> Vec<WorkflowTaskRunRecord> {
    let mut records: Vec<WorkflowTaskRunRecord> = checkpoint.completed.values().cloned().collect();
    records.sort_by(|a, b| {
        a.started_at
            .cmp(&b.started_at)
            .then_with(|| a.task_id.cmp(&b.task_id))
            .then_with(|| a.run_seq.cmp(&b.run_seq))
    });
    records
}

fn resolve_operator_str(task_id: &str, checkpoint: &WorkflowCheckpoint) -> String {
    if let Some(tasks) = &checkpoint.runtime_tasks {
        if let Some(task) = tasks.iter().find(|t| t.id == task_id) {
            return task.operator.clone();
        }
    }
    "(unknown)".to_string()
}

fn materialize_output(output_ref: &OutputRef, workspace: &Path) -> String {
    match output_ref.materialize(workspace) {
        Ok(val) => serde_json::to_string_pretty(&val).unwrap_or_else(|_| "(error)".to_string()),
        Err(err) => {
            if let OutputRef::Artifact { path, .. } = output_ref {
                format!("(artifact missing: {})", path.display())
            } else {
                format!("(error: {err})")
            }
        }
    }
}

fn log_show_text(
    _execution_id: uuid::Uuid,
    execution: WorkflowExecution,
    checkpoint_opt: Option<WorkflowCheckpoint>,
    task_filter: Option<String>,
    verbose: bool,
    workspace: &Path,
) -> StdResult<(), AppError> {
    let duration_str = execution
        .completed_at
        .map(|c| {
            let ms = c
                .signed_duration_since(execution.started_at)
                .num_milliseconds();
            if ms < 0 {
                "-".to_string()
            } else {
                format_duration_short(Duration::from_millis(ms as u64))
            }
        })
        .unwrap_or_else(|| "-".to_string());
    println!("Execution: {}", execution.execution_id);
    println!("Workflow:  {}", execution.workflow_file);
    println!("Status:    {}", execution.status.as_str());
    println!(
        "Started:   {}",
        execution.started_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!("Duration:  {duration_str}");

    if let Some(checkpoint) = checkpoint_opt {
        let records = collect_sorted_records(&checkpoint);
        let filtered: Vec<WorkflowTaskRunRecord> = if let Some(ref filter) = task_filter {
            records
                .into_iter()
                .filter(|r| &r.task_id == filter)
                .collect()
        } else {
            records
        };

        if let Some(ref filter) = task_filter {
            if filtered.is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "task filter '{filter}' did not match any task in this execution (LOG-002)",
                    ),
                )
                .with_code("LOG-002"));
            }
        }

        let total = filtered.len();
        for (idx, record) in filtered.iter().enumerate() {
            let operator = resolve_operator_str(&record.task_id, &checkpoint);
            let is_failed = record.status == WorkflowTaskStatus::Failed;
            let status_label = if is_failed {
                "FAILED"
            } else {
                record.status.as_str()
            };

            if is_failed {
                println!(
                    "\n\u{2500}\u{2500}\u{2500} [FAILED] Task {} of {} {}",
                    idx + 1,
                    total,
                    "\u{2500}".repeat(40)
                );
            } else {
                println!(
                    "\n\u{2500}\u{2500}\u{2500} Task {} of {} {}",
                    idx + 1,
                    total,
                    "\u{2500}".repeat(40)
                );
            }

            let duration_ms = record
                .completed_at
                .signed_duration_since(record.started_at)
                .num_milliseconds();
            let duration_str = if duration_ms >= 0 {
                format_duration_short(Duration::from_millis(duration_ms as u64))
            } else {
                "-".to_string()
            };

            println!("  ID:       {}  (run {})", record.task_id, record.run_seq);
            println!("  Operator: {operator}");
            println!("  Status:   {status_label}");
            println!("  Duration: {duration_str}");

            if is_failed || verbose {
                if let Some(ref err) = record.error {
                    println!("\n  Error:");
                    println!("    Code:    {}", err.code);
                    println!("    Message: {}", err.message);
                }
            }

            match &record.resolved_params_snapshot {
                Some(params) => {
                    println!("\n  Inputs (resolved params):");
                    let pretty = serde_json::to_string_pretty(params)
                        .unwrap_or_else(|_| "(error)".to_string());
                    for line in pretty.lines() {
                        println!("  {line}");
                    }
                }
                None => {
                    println!("\n  Inputs (resolved params): (not available)");
                }
            }

            println!("\n  Output:");
            let output_str = materialize_output(&record.output_ref, workspace);
            if output_str.starts_with("(artifact missing:") {
                println!("  {output_str}");
            } else {
                for line in output_str.lines() {
                    println!("  {line}");
                }
            }
        }
    } else {
        println!("\n(full input replay requires completed checkpoint)\n");

        let filtered: Vec<_> = if let Some(ref filter) = task_filter {
            execution
                .task_runs
                .iter()
                .filter(|r| &r.task_id == filter)
                .collect()
        } else {
            execution.task_runs.iter().collect()
        };

        if let Some(ref filter) = task_filter {
            if filtered.is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "task filter '{filter}' did not match any task in this execution (LOG-002)",
                    ),
                )
                .with_code("LOG-002"));
            }
        }

        let total = filtered.len();
        for (idx, record) in filtered.iter().enumerate() {
            let is_failed = record.status == WorkflowTaskStatus::Failed;
            if is_failed {
                println!(
                    "\u{2500}\u{2500}\u{2500} [FAILED] Task {} of {} {}",
                    idx + 1,
                    total,
                    "\u{2500}".repeat(40)
                );
            } else {
                println!(
                    "\u{2500}\u{2500}\u{2500} Task {} of {} {}",
                    idx + 1,
                    total,
                    "\u{2500}".repeat(40)
                );
            }
            println!("  ID:       {}  (run {})", record.task_id, record.run_seq);
            println!("  Status:   {}", record.status.as_str());
            println!("  Duration: {}ms", record.duration_ms);
            if let Some(ref code) = record.error_code {
                println!("  Error Code: {code}");
            }
        }
    }

    Ok(())
}

fn log_show_json(
    _execution_id: uuid::Uuid,
    execution: WorkflowExecution,
    checkpoint_opt: Option<WorkflowCheckpoint>,
    task_filter: Option<String>,
    workspace: &Path,
) -> StdResult<(), AppError> {
    let tasks_array: Vec<Value>;

    if let Some(ref checkpoint) = checkpoint_opt {
        let records = collect_sorted_records(checkpoint);
        let filtered: Vec<WorkflowTaskRunRecord> = if let Some(ref filter) = task_filter {
            records
                .into_iter()
                .filter(|r| &r.task_id == filter)
                .collect()
        } else {
            records
        };

        if let Some(ref filter) = task_filter {
            if filtered.is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "task filter '{filter}' did not match any task in this execution (LOG-002)",
                    ),
                )
                .with_code("LOG-002"));
            }
        }

        tasks_array = filtered
            .iter()
            .map(|record| {
                let operator = resolve_operator_str(&record.task_id, checkpoint);
                let duration_ms = record
                    .completed_at
                    .signed_duration_since(record.started_at)
                    .num_milliseconds();
                let output = match record.output_ref.materialize(workspace) {
                    Ok(v) => v,
                    Err(_) => {
                        if let OutputRef::Artifact { path, .. } = &record.output_ref {
                            json!(format!("(artifact missing: {})", path.display()))
                        } else {
                            json!(null)
                        }
                    }
                };
                let error_val = record.error.as_ref().map(|e| {
                    json!({
                        "code": e.code,
                        "category": e.category,
                        "message": e.message,
                    })
                });
                json!({
                    "task_id": record.task_id,
                    "run_seq": record.run_seq,
                    "operator": operator,
                    "status": record.status.as_str(),
                    "started_at": record.started_at.to_rfc3339(),
                    "completed_at": record.completed_at.to_rfc3339(),
                    "duration_ms": if duration_ms >= 0 { json!(duration_ms) } else { json!(null) },
                    "resolved_params": record.resolved_params_snapshot,
                    "output": output,
                    "error": error_val,
                })
            })
            .collect();
    } else {
        let exec_records: Vec<_> = if let Some(ref filter) = task_filter {
            execution
                .task_runs
                .iter()
                .filter(|r| &r.task_id == filter)
                .collect()
        } else {
            execution.task_runs.iter().collect()
        };

        if let Some(ref filter) = task_filter {
            if exec_records.is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "task filter '{filter}' did not match any task in this execution (LOG-002)",
                    ),
                )
                .with_code("LOG-002"));
            }
        }

        tasks_array = exec_records
            .iter()
            .map(|record| {
                json!({
                    "task_id": record.task_id,
                    "run_seq": record.run_seq,
                    "operator": "(unknown)",
                    "status": record.status.as_str(),
                    "started_at": null,
                    "completed_at": null,
                    "duration_ms": record.duration_ms,
                    "resolved_params": null,
                    "output": null,
                    "error": record.error_code,
                })
            })
            .collect();
    }

    let exec_val = serde_json::to_value(&execution).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize execution: {err}"),
        )
    })?;

    let mut result = json!({
        "execution": exec_val,
        "tasks": tasks_array,
    });

    if let Some(filter) = task_filter {
        result
            .as_object_mut()
            .unwrap()
            .insert("task_filter".to_string(), json!(filter));
    }

    let serialized = serde_json::to_string_pretty(&result).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize log show output: {err}"),
        )
    })?;
    println!("{serialized}");
    Ok(())
}

#[cfg(test)]
mod duration_formatting_tests {
    use super::*;

    #[test]
    fn format_duration_short_zero() {
        let duration = Duration::from_secs(0);
        assert_eq!(format_duration_short(duration), "0s");
    }

    #[test]
    fn format_duration_short_seconds() {
        let duration = Duration::from_secs(30);
        assert_eq!(format_duration_short(duration), "30s");
    }

    #[test]
    fn format_duration_short_minutes() {
        let duration = Duration::from_secs(150);
        assert_eq!(format_duration_short(duration), "2m");
    }

    #[test]
    fn format_duration_short_hours() {
        let duration = Duration::from_secs(7200);
        assert_eq!(format_duration_short(duration), "2h");
    }

    #[test]
    fn format_duration_short_days() {
        let duration = Duration::from_secs(172800);
        assert_eq!(format_duration_short(duration), "2d");
    }

    #[test]
    fn format_duration_short_hours_minutes() {
        let duration = Duration::from_secs(8100);
        assert_eq!(format_duration_short(duration), "2h 15m");
    }

    #[test]
    fn format_duration_short_days_hours() {
        let duration = Duration::from_secs(205200);
        assert_eq!(format_duration_short(duration), "2d 9h");
    }

    #[test]
    fn format_duration_short_large_duration() {
        let duration = Duration::from_secs(90061);
        assert_eq!(format_duration_short(duration), "1d 1h");
    }

    #[test]
    fn format_datetime_short_format() {
        use chrono::TimeZone;
        let dt = chrono::Utc
            .with_ymd_and_hms(2026, 3, 4, 16, 27, 43)
            .unwrap();
        let formatted = format_datetime_short(&dt);
        assert_eq!(formatted, "2026-03-04 16:27");
    }

    #[test]
    fn format_duration_short_two_units_max() {
        let duration = Duration::from_secs(93784);
        assert_eq!(format_duration_short(duration), "1d 2h");
    }
}
