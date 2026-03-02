//! Task execution logic with retry and timeout handling for workflow execution.
#![allow(clippy::result_large_err)] // Task execution returns AppError to preserve full diagnostic context; boxing would discard run-time state.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::artifacts::ArtifactStore;
use crate::core::workflow_graph::context;
use crate::core::workflow_graph::operator::{
    ExecutionContext as OperatorContext, OperatorRegistry, StateView,
};
use crate::core::workflow_graph::schema::WorkflowTask;
use crate::core::workflow_graph::state::{
    redact_value, summarize_error, GraphSettings, TaskRunRecord, TaskStatus, WorkflowTaskRunRecord,
};
use chrono::Utc;
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use uuid::Uuid;

use crate::core::workflow_graph::executor::{GraphHandle, TaskOutcome};

/// Executes a single workflow task with retry logic, timeout handling, and context patching.
///
/// This function handles the complete lifecycle of task execution including:
/// - Operator resolution and parameter validation
/// - Retry loop with exponential backoff and jitter
/// - Timeout enforcement per task
/// - Error handling and TaskOutcome construction
/// - Context patching support
#[allow(clippy::too_many_arguments)]
pub async fn run_task(
    task: WorkflowTask,
    registry: OperatorRegistry,
    engine: Arc<crate::core::workflow_graph::expression::ExpressionEngine>,
    workspace_root: PathBuf,
    snapshot: StateView,
    execution_id: String,
    run_seq: u64,
    redact_keys: Arc<Vec<String>>,
    runtime_graph: GraphHandle,
) -> Result<TaskOutcome, AppError> {
    let operator = registry.get(&task.operator).ok_or_else(|| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("operator '{}' is not registered", task.operator),
        )
        .with_code("WFG-OP-001")
    })?;

    let eval_ctx = snapshot.evaluation_context();
    let resolved_params = context::resolve_value(&task.params, engine.as_ref(), &eval_ctx)?;
    operator.validate_params(&resolved_params)?;

    let mut attempts = 0usize;
    let max_attempts = task.retry.as_ref().map(|r| r.max_attempts).unwrap_or(1);
    let mut backoff_ms = task.retry.as_ref().map(|r| r.backoff_ms).unwrap_or(0);
    let multiplier = task
        .retry
        .as_ref()
        .and_then(|r| r.backoff_multiplier)
        .unwrap_or(1.0);
    let jitter_ms = task.retry.as_ref().and_then(|r| r.jitter_ms).unwrap_or(0);
    let mut rng = StdRng::from_entropy();

    loop {
        attempts += 1;
        tracing::info!(
            task_id = %task.id,
            task_name = task.name.as_deref().unwrap_or("-"),
            operator = %task.operator,
            attempt = attempts,
            max_attempts = max_attempts,
            timeout_ms = task.timeout_ms.map(|t| t.to_string()).as_deref().unwrap_or("-"),
            "task starting"
        );
        let ctx = OperatorContext {
            workspace_path: workspace_root.clone(),
            execution_id: execution_id.clone(),
            task_id: task.id.clone(),
            iteration: run_seq,
            state_view: snapshot.clone(),
            graph: runtime_graph.clone(),
        };
        let params = resolved_params.clone();
        let started_at = Utc::now();
        let execution = operator.execute(params, ctx);
        let execution_result = if let Some(timeout_ms) = task.timeout_ms {
            match timeout(Duration::from_millis(timeout_ms), execution).await {
                Ok(res) => res,
                Err(_) => Err(AppError::new(
                    ErrorCategory::TimeoutError,
                    format!("task {} timed out", task.id),
                )
                .with_code("WFG-TIME-002")),
            }
        } else {
            execution.await
        };
        let completed_at = Utc::now();
        let duration_ms = completed_at
            .signed_duration_since(started_at)
            .num_milliseconds() as u64;

        match execution_result {
            Ok(output) => {
                tracing::info!(
                    task_id = %task.id,
                    duration_ms = duration_ms,
                    "task completed"
                );
                let patch = context::extract_context_patch(&output);
                return Ok(TaskOutcome {
                    task_id: task.id.clone(),
                    record: TaskRunRecord {
                        status: TaskStatus::Success,
                        output,
                        error_code: None,
                        duration_ms,
                        run_seq,
                    },
                    context_patch: patch,
                    failed: false,
                    started_at,
                    completed_at,
                    error_summary: None,
                });
            }
            Err(err) => {
                if attempts >= max_attempts {
                    tracing::warn!(
                        task_id = %task.id,
                        error_code = %err.code,
                        error = %err.message,
                        "task failed"
                    );
                    return Ok(TaskOutcome {
                        task_id: task.id.clone(),
                        record: TaskRunRecord {
                            status: TaskStatus::Failed,
                            output: Value::String(err.message.clone()),
                            error_code: Some(err.code.clone()),
                            duration_ms,
                            run_seq,
                        },
                        context_patch: None,
                        failed: true,
                        started_at,
                        completed_at,
                        error_summary: Some(summarize_error(&err, redact_keys.as_ref())),
                    });
                }
                let sleep_ms = backoff_ms.saturating_add(if jitter_ms > 0 {
                    rng.gen_range(0..=jitter_ms)
                } else {
                    0
                });
                if sleep_ms > 0 {
                    sleep(Duration::from_millis(sleep_ms)).await;
                }
                backoff_ms = ((backoff_ms as f32) * multiplier).max(1.0) as u64;
                continue;
            }
        }
    }
}

/// Builds a workflow task run record for persistence from a task outcome.
///
/// This function transforms the in-memory task execution result into a
/// persistent record that can be saved to checkpoints and execution logs.
pub fn build_workflow_task_run_record(
    outcome: &TaskOutcome,
    goal_gate_group: Option<String>,
    artifact_store: &mut ArtifactStore,
    graph_settings: &GraphSettings,
    execution_id: &Uuid,
) -> Result<WorkflowTaskRunRecord, AppError> {
    let run_seq = usize::try_from(outcome.record.run_seq).map_err(|_| {
        AppError::new(
            ErrorCategory::ValidationError,
            "run_seq overflow during conversion to usize",
        )
        .with_code("WFG-EXEC-002")
    })?;
    let mut persisted_output = outcome.record.output.clone();
    redact_value(&mut persisted_output, &graph_settings.redaction.redact_keys);
    let output_ref =
        artifact_store.route_output(execution_id, &outcome.task_id, run_seq, persisted_output)?;
    Ok(WorkflowTaskRunRecord {
        task_id: outcome.task_id.clone(),
        run_seq,
        started_at: outcome.started_at,
        completed_at: outcome.completed_at,
        status: outcome.record.status,
        goal_gate_group,
        output_ref,
        error: outcome.error_summary.clone(),
    })
}
