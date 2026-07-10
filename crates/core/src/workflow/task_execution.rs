//! Task execution logic with retry and timeout handling for workflow execution.
#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::artifacts::ArtifactStore;
use crate::workflow::operator::{ExecutionContext as OperatorContext, OperatorRegistry, StateView};
use crate::workflow::schema::WorkflowTask;
use crate::workflow::state::{
    redact_value, summarize_error, GraphSettings, TaskRunRecord, TaskStatus, WorkflowTaskRunRecord,
};
use crate::workflow::value_resolve as context;
use chrono::Utc;
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use uuid::Uuid;

use crate::workflow::executor::{ExecutionOverrides, GraphHandle, TaskOutcome};

pub const RESOLVED_PARAMS_SNAPSHOT_LIMIT_BYTES: usize = 65_536;

/// Cap on a single backoff sleep, before jitter. Mirrors `gh.rs::MAX_RETRY_DELAY_MS`.
pub(crate) const MAX_TASK_BACKOFF_MS: u64 = 300_000;

/// Returns true if an `AppError` represents a failure the engine retry loop is
/// permitted to retry.
///
/// Used by the engine retry loop to short-circuit on non-retryable errors —
/// but note `prepare_retry_state` (below) already defaults `max_attempts` to
/// `1` for any task without an explicit `retry:` block, so this function is
/// only ever consulted for tasks whose author *positively opted in* to
/// retries. That opt-in is itself the transience claim; this function is a
/// **permanent-error veto** over it, not a transience allow-list. It returns
/// `false` only for errors that are positively known to be permanent —
/// retrying them cannot succeed and would just double-apply
/// side-effecting work (`git push`, agent runs, gh mutations) — and `true`
/// for everything else, honoring the author's explicit retry configuration.
///
/// (Implementation note 2026-07-10: an earlier version of this function
/// inverted the default — unknown codes were treated as non-retryable —
/// on the premise that retry was implicitly on for every task. That premise
/// was wrong: retry is opt-in per task, so the flip silently killed every
/// explicit `retry:` block on an unclassified error code. See spec
/// `074-audit-remediation.md` S14 for the corrected decision record.)
///
/// Veto classification, in order:
/// 1. `ValidationError` category → always vetoed (bad input/config; retrying
///    without a state change can't succeed).
/// 2. `WFG-GH-*` codes → unchanged from pre-tranche-2 behavior (see match arms below);
///    several are vetoed outright, `WFG-GH-004` is vetoed unless its message matches a
///    known-transient pattern.
/// 3. `WFG-RECONCILE-ADJ-001` → explicitly NOT vetoed (kept as an explicit truth,
///    documenting intent rather than relying on the general default). Per
///    CONTEXT.md's "fuzziness is not failure tolerance" (decision 1 / B2), a
///    transient LLM adjudication outage keeps retrying with backoff; only
///    exhausting `max_attempts` fails the reconcile task (and therefore the
///    cycle) closed. Pinned by
///    `retry_classification_tests::reconcile_adjudication_failure_is_retryable`.
/// 4. `TimeoutError` category → explicitly NOT vetoed (redundant with the
///    default now, kept as an explicit truth): the operation didn't fail
///    semantically, it ran out of time budget (network stall, slow subprocess, human
///    response window). Covers `WFG-TIME-001/002`, `WFG-AGENT-005`,
///    `WFG-HUMAN-103/105`, etc. without needing to enumerate every timeout code.
/// 5. `ResourceError` category → explicitly NOT vetoed (redundant with the
///    default now, kept as an explicit truth): today this is exclusively
///    `WFG-AGENT-008` (agent SDK quota-exceeded) — the "rate-limit" case decision 7
///    calls out by name; the provider is asking the caller to back off, not
///    rejecting the request outright.
/// 6. Everything else → NOT vetoed (retryable), because the task's explicit
///    `retry:` configuration is the author's positive transience claim.
pub(crate) fn is_retryable(err: &AppError) -> bool {
    if matches!(err.category, ErrorCategory::ValidationError) {
        return false;
    }

    // gh CLI classification (WFG-GH-*): decided per-code, independent of category,
    // and preserved unchanged from pre-tranche-2 behavior — checked before the
    // general category rules below so e.g. WFG-GH-AUTH-002 (TimeoutError category)
    // stays non-retryable rather than picking up the generic timeout allowance.
    match err.code.as_str() {
        "WFG-GH-001" | "WFG-GH-002" | "WFG-GH-005" | "WFG-GH-006" | "WFG-GH-007" | "WFG-GH-008"
        | "WFG-GH-AUTH-001" | "WFG-GH-AUTH-002" | "WFG-GH-AUTH-003" => return false,
        "WFG-GH-003" => return true,
        "WFG-GH-004" => {
            let msg = err.message.to_lowercase();
            const TRANSIENT_PATTERNS: &[&str] = &[
                "tls handshake timeout",
                "dial tcp",
                "i/o timeout",
                "connection reset",
                "eof",
                "http 5",
            ];
            return TRANSIENT_PATTERNS.iter().any(|p| msg.contains(p));
        }
        "WFG-RECONCILE-ADJ-001" => return true,
        _ => {}
    }

    if matches!(err.category, ErrorCategory::TimeoutError) {
        return true;
    }
    if matches!(err.category, ErrorCategory::ResourceError) {
        return true;
    }

    // Not positively vetoed: honor the task's explicit `retry:` opt-in.
    true
}

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
    engine: Arc<crate::workflow::expression::ExpressionEngine>,
    workspace_root: PathBuf,
    snapshot: StateView,
    execution_id: String,
    run_seq: u64,
    redact_keys: Arc<Vec<String>>,
    runtime_graph: GraphHandle,
    workflow_file: PathBuf,
    nesting_depth: u32,
    execution_overrides: ExecutionOverrides,
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
            &workflow_file,
            nesting_depth,
            registry.clone(),
            execution_overrides.clone(),
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
                return Ok(build_success_outcome(
                    task.id,
                    output,
                    duration_ms,
                    run_seq,
                    started_at,
                    completed_at,
                    resolved_params.clone(),
                ));
            }
            Err(err) => {
                if retry_state.attempts >= retry_state.max_attempts || !is_retryable(&err) {
                    return Ok(build_failure_outcome(
                        task.id,
                        &err,
                        duration_ms,
                        run_seq,
                        started_at,
                        completed_at,
                        redact_keys.as_ref(),
                        resolved_params.clone(),
                    ));
                }
                let delay_ms = apply_backoff_and_retry(&mut retry_state, &mut rng).await;
                tracing::warn!(
                    task_id = %task.id,
                    operator = %task.operator,
                    attempt = retry_state.attempts,
                    max_attempts = retry_state.max_attempts,
                    delay_ms = delay_ms,
                    error_code = %err.code,
                    error_message = %err.message,
                    "transient failure on operator '{}' ({}); retrying after backoff",
                    task.operator,
                    err.code,
                );
            }
        }
    }
}

/// Resolves operator from registry and validates it exists.
///
/// Distinguishes two failure modes (ADR-0014): an entirely unknown operator
/// name (`WFG-OP-001`, e.g. a typo) vs. a *described* operator whose
/// executable half was never wired because its runtime deps (e.g. a
/// `BackendStore` for the optimization-loop operators) are absent in this
/// context (`WFG-OP-002`) — the operator never vanished from the vocabulary,
/// it just cannot run here.
fn resolve_operator(
    task: &WorkflowTask,
    registry: &OperatorRegistry,
) -> Result<rhai::Shared<dyn crate::workflow::operator::Operator>, AppError> {
    registry.get(&task.operator).ok_or_else(|| {
        if registry.is_described(&task.operator) {
            AppError::new(
                ErrorCategory::ValidationError,
                format!(
                    "operator '{}' requires a backend store; run within a command \
                     that provides one (e.g. `newton workflow run` / `newton optimize` \
                     with a resolved state directory)",
                    task.operator
                ),
            )
            .with_code("WFG-OP-002")
        } else {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("operator '{}' is not registered", task.operator),
            )
            .with_code("WFG-OP-001")
        }
    })
}

/// Resolves parameters and validates them against the operator.
fn resolve_and_validate_params(
    task: &WorkflowTask,
    engine: &crate::workflow::expression::ExpressionEngine,
    snapshot: &StateView,
    operator: &rhai::Shared<dyn crate::workflow::operator::Operator>,
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
        max_attempts: retry_config.map_or(1, |r| r.max_attempts),
        backoff_ms: retry_config.map_or(0, |r| r.backoff_ms),
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
#[allow(clippy::too_many_arguments)]
fn build_operator_context(
    workspace_root: &Path,
    execution_id: &str,
    task_id: &str,
    run_seq: u64,
    snapshot: &StateView,
    runtime_graph: &GraphHandle,
    workflow_file: &Path,
    nesting_depth: u32,
    operator_registry: OperatorRegistry,
    execution_overrides: ExecutionOverrides,
) -> OperatorContext {
    OperatorContext {
        workspace_path: workspace_root.to_path_buf(),
        execution_id: execution_id.to_string(),
        task_id: task_id.to_string(),
        iteration: run_seq,
        state_view: snapshot.clone(),
        graph: runtime_graph.clone(),
        workflow_file: workflow_file.to_path_buf(),
        nesting_depth,
        execution_overrides,
        operator_registry,
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
                format!("task {task_id} timed out"),
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
    resolved_params: Value,
) -> TaskOutcome {
    tracing::info!(
        task_id = %task_id,
        duration_ms = duration_ms,
        "task completed"
    );
    let patch = context::extract_context_patch(&output);
    TaskOutcome {
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
        resolved_params,
    }
}

/// Builds failure TaskOutcome from execution error.
#[allow(clippy::too_many_arguments)]
fn build_failure_outcome(
    task_id: String,
    err: &AppError,
    duration_ms: u64,
    run_seq: u64,
    started_at: chrono::DateTime<Utc>,
    completed_at: chrono::DateTime<Utc>,
    redact_keys: &[String],
    resolved_params: Value,
) -> TaskOutcome {
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
    TaskOutcome {
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
        resolved_params,
    }
}

/// Applies backoff delay and prepares for retry. Returns the actual delay in ms.
async fn apply_backoff_and_retry(retry_state: &mut RetryState, rng: &mut StdRng) -> u64 {
    let sleep_ms = calculate_backoff(retry_state, rng).min(MAX_TASK_BACKOFF_MS);
    if sleep_ms > 0 {
        sleep(Duration::from_millis(sleep_ms)).await;
    }
    retry_state.backoff_ms = (((retry_state.backoff_ms as f32) * retry_state.multiplier).max(1.0)
        as u64)
        .min(MAX_TASK_BACKOFF_MS);
    sleep_ms
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

    // Build resolved_params_snapshot: redact, serialize, apply size cap.
    let resolved_params_snapshot = {
        let mut snapshot = outcome.resolved_params.clone();
        redact_value(&mut snapshot, &graph_settings.redaction.redact_keys);
        match serde_json::to_vec(&snapshot) {
            Ok(bytes) if bytes.len() <= RESOLVED_PARAMS_SNAPSHOT_LIMIT_BYTES => Some(snapshot),
            Ok(bytes) => {
                let size_bytes = bytes.len();
                Some(serde_json::json!({"_truncated": true, "size_bytes": size_bytes}))
            }
            Err(_) => None,
        }
    };

    Ok(WorkflowTaskRunRecord {
        task_id: outcome.task_id.clone(),
        run_seq,
        started_at: outcome.started_at,
        completed_at: outcome.completed_at,
        status: outcome.record.status,
        goal_gate_group,
        output_ref,
        error: outcome.error_summary.clone(),
        resolved_params_snapshot,
    })
}

#[cfg(test)]
mod retry_classification_tests {
    use super::*;

    fn err(category: ErrorCategory, code: &str, msg: &str) -> AppError {
        AppError::new(category, msg.to_string()).with_code(code)
    }

    #[test]
    fn validation_errors_are_not_retryable() {
        let e = err(ErrorCategory::ValidationError, "ANY-CODE", "bad input");
        assert!(!is_retryable(&e));
    }

    #[test]
    fn gh_001_002_005_006_007_008_not_retryable() {
        for code in [
            "WFG-GH-001",
            "WFG-GH-002",
            "WFG-GH-005",
            "WFG-GH-006",
            "WFG-GH-007",
            "WFG-GH-008",
        ] {
            let e = err(ErrorCategory::ToolExecutionError, code, "x");
            assert!(!is_retryable(&e), "code {code} should not be retryable");
        }
    }

    #[test]
    fn gh_auth_codes_not_retryable() {
        for code in ["WFG-GH-AUTH-001", "WFG-GH-AUTH-002", "WFG-GH-AUTH-003"] {
            let e = err(ErrorCategory::ToolExecutionError, code, "x");
            assert!(!is_retryable(&e), "{code} should not be retryable");
        }
    }

    #[test]
    fn gh_003_is_retryable() {
        let e = err(
            ErrorCategory::ToolExecutionError,
            "WFG-GH-003",
            "spawn failed",
        );
        assert!(is_retryable(&e));
    }

    #[test]
    fn gh_004_retryable_only_on_transient_message() {
        let transient_msgs = [
            "gh failed: TLS handshake timeout",
            "dial tcp: connection failed",
            "i/o timeout reading body",
            "connection reset by peer",
            "unexpected EOF",
            "HTTP 503 Service Unavailable",
        ];
        for m in transient_msgs {
            let e = err(ErrorCategory::ToolExecutionError, "WFG-GH-004", m);
            assert!(is_retryable(&e), "msg {m:?} should be retryable");
        }
        let e = err(
            ErrorCategory::ToolExecutionError,
            "WFG-GH-004",
            "exit 1: not found",
        );
        assert!(!is_retryable(&e));
    }

    /// Implementation note 2026-07-10 (spec 074 S14 correction): unknown codes
    /// ARE retryable — explicit `retry:` config on a task is the author's
    /// positive classification of transience, and `is_retryable` is only a
    /// permanent-error veto over that opt-in (see the doc comment on
    /// `is_retryable`). An earlier version of this test asserted the
    /// opposite, on the mistaken premise that retry was implicitly on for
    /// every task; `prepare_retry_state` defaults `max_attempts` to `1`
    /// absent an explicit `retry:` block, so that premise was wrong and the
    /// flip would have silently killed every explicit retry config on an
    /// unclassified error code.
    #[test]
    fn unknown_codes_default_to_retryable() {
        let e = err(ErrorCategory::ToolExecutionError, "SOME-OTHER-CODE", "x");
        assert!(is_retryable(&e));
    }

    /// TimeoutError category is retryable regardless of code — decision 7 names
    /// "timeout" explicitly as a transient class. `WFG-GH-AUTH-002` is TimeoutError
    /// too but is carved out non-retryable by the gh-specific match (tested in
    /// `gh_auth_codes_not_retryable`), proving the gh code check runs first.
    #[test]
    fn timeout_category_is_retryable_for_unclassified_code() {
        let e = err(
            ErrorCategory::TimeoutError,
            "WFG-TIME-002",
            "task timed out",
        );
        assert!(is_retryable(&e));
        let e2 = err(
            ErrorCategory::TimeoutError,
            "SOME-NOVEL-TIMEOUT-CODE",
            "timed out",
        );
        assert!(is_retryable(&e2));
    }

    /// ResourceError category (today exclusively `WFG-AGENT-008` agent-quota-exceeded)
    /// is decision 7's "rate-limit" transient class.
    #[test]
    fn resource_error_quota_is_retryable() {
        let e = err(
            ErrorCategory::ResourceError,
            "WFG-AGENT-008",
            "agent quota exceeded",
        );
        assert!(is_retryable(&e));
    }

    /// Pins retryability for ReconcileOperator's adjudication-failure code
    /// (spec 074 PR-4 / B2 — "Reconciliation fails closed"). This now passes
    /// both because unknown codes default to retryable (see
    /// `unknown_codes_default_to_retryable` above) AND because of the
    /// explicit `WFG-RECONCILE-ADJ-001` arm in `is_retryable`; it is
    /// deliberately kept as its own test — not folded into that one — so the
    /// explicit arm's presence is independently pinned: a transient LLM
    /// adjudication outage must keep retrying with backoff, per
    /// CONTEXT.md's "Fuzziness is not failure tolerance" — it must NOT be
    /// treated as hard-non-retryable the way `ValidationError` is,
    /// regardless of how the *default* for unclassified codes is set.
    #[test]
    fn reconcile_adjudication_failure_is_retryable() {
        let e = err(
            ErrorCategory::ToolExecutionError,
            "WFG-RECONCILE-ADJ-001",
            "adjudication failed",
        );
        assert!(is_retryable(&e));
    }

    #[tokio::test(start_paused = true)]
    async fn backoff_clamped_to_max() {
        let mut state = RetryState {
            attempts: 0,
            max_attempts: 5,
            backoff_ms: 200_000,
            multiplier: 5.0,
            jitter_ms: 0,
        };
        let mut rng = StdRng::from_entropy();
        // First call sleeps 200_000 (under cap), then bumps state to clamped 300_000.
        let d1 = apply_backoff_and_retry(&mut state, &mut rng).await;
        assert_eq!(d1, 200_000);
        assert_eq!(state.backoff_ms, MAX_TASK_BACKOFF_MS);
        // Second call clamps the sleep to MAX_TASK_BACKOFF_MS.
        let d2 = apply_backoff_and_retry(&mut state, &mut rng).await;
        assert_eq!(d2, MAX_TASK_BACKOFF_MS);
    }

    #[tokio::test(start_paused = true)]
    async fn backoff_grows_exponentially() {
        let mut state = RetryState {
            attempts: 0,
            max_attempts: 5,
            backoff_ms: 100,
            multiplier: 2.0,
            jitter_ms: 0,
        };
        let mut rng = StdRng::from_entropy();
        let d1 = apply_backoff_and_retry(&mut state, &mut rng).await;
        let d2 = apply_backoff_and_retry(&mut state, &mut rng).await;
        let d3 = apply_backoff_and_retry(&mut state, &mut rng).await;
        assert_eq!(d1, 100);
        assert_eq!(d2, 200);
        assert_eq!(d3, 400);
    }
}
