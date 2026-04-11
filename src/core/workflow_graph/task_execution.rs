//! Task execution logic with retry and timeout handling for workflow execution.
#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::artifacts::ArtifactStore;
use crate::core::workflow_graph::operator::{
    ExecutionContext as OperatorContext, OperatorRegistry, StateView,
};
use crate::core::workflow_graph::schema::WorkflowTask;
use crate::core::workflow_graph::state::{
    redact_value, summarize_error, GraphSettings, TaskRunRecord, TaskStatus, WorkflowTaskRunRecord,
};
use crate::core::workflow_graph::value_resolve as context;
use chrono::Utc;
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde_json::Value;
use std::path::{Path, PathBuf};
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
    let operator = resolve_operator(&task, &registry)?;
    let resolved_params =
        resolve_and_validate_params(&task, engine.as_ref(), &snapshot, &operator)?;

    let mut retry_state = prepare_retry_state(&task);
    let mut rng = StdRng::from_entropy();

    loop {
        retry_state.attempts += 1;
        log_task_start(&task, retry_state.attempts, retry_state.max_attempts);

        let ctx = build_operator_context(
            &workspace_root,
            &execution_id,
            &task.id,
            run_seq,
            &snapshot,
            &runtime_graph,
        );

        let started_at = Utc::now();
        let execution = operator.execute(resolved_params.clone(), ctx);
        let execution_result = execute_with_timeout(execution, task.timeout_ms, &task.id).await;
        let completed_at = Utc::now();
        let duration_ms = completed_at
            .signed_duration_since(started_at)
            .num_milliseconds() as u64;

        match execution_result {
            Ok(output) => {
                return build_success_outcome(
                    task.id,
                    output,
                    duration_ms,
                    run_seq,
                    started_at,
                    completed_at,
                );
            }
            Err(err) => {
                if retry_state.attempts >= retry_state.max_attempts {
                    return build_failure_outcome(
                        task.id,
                        &err,
                        duration_ms,
                        run_seq,
                        started_at,
                        completed_at,
                        redact_keys.as_ref(),
                    );
                }
                apply_backoff_and_retry(&mut retry_state, &mut rng).await;
            }
        }
    }
}

/// Resolves operator from registry and validates it exists.
fn resolve_operator(
    task: &WorkflowTask,
    registry: &OperatorRegistry,
) -> Result<rhai::Shared<dyn crate::core::workflow_graph::operator::Operator>, AppError> {
    registry.get(&task.operator).ok_or_else(|| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("operator '{}' is not registered", task.operator),
        )
        .with_code("WFG-OP-001")
    })
}

/// Resolves parameters and validates them against the operator.
fn resolve_and_validate_params(
    task: &WorkflowTask,
    engine: &crate::core::workflow_graph::expression::ExpressionEngine,
    snapshot: &StateView,
    operator: &rhai::Shared<dyn crate::core::workflow_graph::operator::Operator>,
) -> Result<Value, AppError> {
    let eval_ctx = snapshot.evaluation_context();
    let resolved_params = context::resolve_value(&task.params, engine, &eval_ctx)?;
    operator.validate_params(&resolved_params)?;
    Ok(resolved_params)
}

/// Prepares retry configuration from task definition.
fn prepare_retry_state(task: &WorkflowTask) -> RetryState {
    let retry_config = task.retry.as_ref();

    RetryState {
        attempts: 0,
        max_attempts: retry_config.map(|r| r.max_attempts).unwrap_or(1),
        backoff_ms: retry_config.map(|r| r.backoff_ms).unwrap_or(0),
        multiplier: retry_config
            .and_then(|r| r.backoff_multiplier)
            .unwrap_or(1.0),
        jitter_ms: retry_config.and_then(|r| r.jitter_ms).unwrap_or(0),
    }
}

/// Retry configuration state.
struct RetryState {
    attempts: usize,
    max_attempts: usize,
    backoff_ms: u64,
    multiplier: f32,
    jitter_ms: u64,
}

/// Logs task start information.
fn log_task_start(task: &WorkflowTask, attempt: usize, max_attempts: usize) {
    tracing::info!(
        task_id = %task.id,
        task_name = task.name.as_deref().unwrap_or("-"),
        operator = %task.operator,
        attempt = attempt,
        max_attempts = max_attempts,
        timeout_ms = task.timeout_ms.map(|t| t.to_string()).as_deref().unwrap_or("-"),
        "task starting"
    );
}

/// Builds operator execution context.
fn build_operator_context(
    workspace_root: &Path,
    execution_id: &str,
    task_id: &str,
    run_seq: u64,
    snapshot: &StateView,
    runtime_graph: &GraphHandle,
) -> OperatorContext {
    OperatorContext {
        workspace_path: workspace_root.to_path_buf(),
        execution_id: execution_id.to_string(),
        task_id: task_id.to_string(),
        iteration: run_seq,
        state_view: snapshot.clone(),
        graph: runtime_graph.clone(),
    }
}

/// Executes operator with optional timeout enforcement.
async fn execute_with_timeout(
    execution: impl std::future::Future<Output = Result<Value, AppError>>,
    timeout_ms: Option<u64>,
    task_id: &str,
) -> Result<Value, AppError> {
    if let Some(timeout_ms) = timeout_ms {
        match timeout(Duration::from_millis(timeout_ms), execution).await {
            Ok(res) => res,
            Err(_) => Err(AppError::new(
                ErrorCategory::TimeoutError,
                format!("task {} timed out", task_id),
            )
            .with_code("WFG-TIME-002")),
        }
    } else {
        execution.await
    }
}

/// Builds success TaskOutcome from execution result.
fn build_success_outcome(
    task_id: String,
    output: Value,
    duration_ms: u64,
    run_seq: u64,
    started_at: chrono::DateTime<Utc>,
    completed_at: chrono::DateTime<Utc>,
) -> Result<TaskOutcome, AppError> {
    tracing::info!(
        task_id = %task_id,
        duration_ms = duration_ms,
        "task completed"
    );
    let patch = context::extract_context_patch(&output);
    Ok(TaskOutcome {
        task_id,
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
    })
}

/// Builds failure TaskOutcome from execution error.
fn build_failure_outcome(
    task_id: String,
    err: &AppError,
    duration_ms: u64,
    run_seq: u64,
    started_at: chrono::DateTime<Utc>,
    completed_at: chrono::DateTime<Utc>,
    redact_keys: &[String],
) -> Result<TaskOutcome, AppError> {
    tracing::warn!(
        task_id = %task_id,
        error_code = %err.code,
        error = %err.message,
        "task failed"
    );
    let output = if let Some(output_str) = err.context.get("output") {
        serde_json::from_str::<Value>(output_str)
            .unwrap_or_else(|_| Value::String(err.message.clone()))
    } else {
        Value::String(err.message.clone())
    };
    Ok(TaskOutcome {
        task_id,
        record: TaskRunRecord {
            status: TaskStatus::Failed,
            output,
            error_code: Some(err.code.clone()),
            duration_ms,
            run_seq,
        },
        context_patch: None,
        failed: true,
        started_at,
        completed_at,
        error_summary: Some(summarize_error(err, redact_keys)),
    })
}

/// Applies backoff delay and prepares for retry.
async fn apply_backoff_and_retry(retry_state: &mut RetryState, rng: &mut StdRng) {
    let sleep_ms = calculate_backoff(retry_state, rng);
    if sleep_ms > 0 {
        sleep(Duration::from_millis(sleep_ms)).await;
    }
    retry_state.backoff_ms =
        ((retry_state.backoff_ms as f32) * retry_state.multiplier).max(1.0) as u64;
}

/// Calculates backoff duration with jitter.
fn calculate_backoff(retry_state: &RetryState, rng: &mut StdRng) -> u64 {
    let jitter = if retry_state.jitter_ms > 0 {
        rng.gen_range(0..=retry_state.jitter_ms)
    } else {
        0
    };
    retry_state.backoff_ms.saturating_add(jitter)
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
