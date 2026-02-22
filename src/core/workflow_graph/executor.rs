#![allow(clippy::result_large_err)] // Executor returns AppError to preserve full diagnostic context; boxing would discard run-time state.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::expression::{EvaluationContext, ExpressionEngine};
use crate::core::workflow_graph::operator::{
    ExecutionContext as OperatorContext, OperatorRegistry, StateView,
};
use crate::core::workflow_graph::schema::{Condition, WorkflowDocument, WorkflowTask};
use futures::future::join_all;
use rand::Rng;
use serde_json::{Map, Number, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

/// Optional overrides supplied by CLI flags.
#[derive(Clone, Debug)]
pub struct ExecutionOverrides {
    pub parallel_limit: Option<usize>,
    pub max_time_seconds: Option<u64>,
}

/// Resolved execution configuration used by the runner.
#[derive(Clone, Debug)]
pub struct ExecutionConfig {
    pub parallel_limit: usize,
    pub max_time_seconds: u64,
    pub continue_on_error: bool,
    pub max_task_iterations: usize,
    pub max_workflow_iterations: usize,
}

/// Summary of a workflow execution run.
#[derive(Debug, Clone)]
pub struct ExecutionSummary {
    pub total_iterations: usize,
    pub completed_tasks: HashMap<String, TaskRunRecord>,
}

/// Record describing the last completed run of a task.
#[derive(Clone, Debug)]
pub struct TaskRunRecord {
    pub status: TaskStatus,
    pub output: Value,
    pub error_code: Option<String>,
    pub duration_ms: u64,
    pub run_seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    Success,
    Failed,
    Skipped,
}

impl TaskStatus {
    fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Success => "success",
            TaskStatus::Failed => "failed",
            TaskStatus::Skipped => "skipped",
        }
    }
}

struct ExecutionState {
    context: Value,
    completed: HashMap<String, TaskRunRecord>,
}

impl ExecutionState {
    fn snapshot(&self) -> StateView {
        StateView::new(self.context.clone(), build_tasks_value(&self.completed))
    }
}

#[derive(Clone)]
struct TaskOutcome {
    task_id: String,
    record: TaskRunRecord,
    context_patch: Option<Value>,
    failed: bool,
}

/// Execute a workflow document with the provided overrides.
pub async fn execute_workflow(
    document: WorkflowDocument,
    registry: OperatorRegistry,
    workspace_root: PathBuf,
    overrides: ExecutionOverrides,
) -> Result<ExecutionSummary, AppError> {
    let settings = document.workflow.settings;
    let tasks_by_id = Arc::new(
        document
            .workflow
            .tasks
            .into_iter()
            .map(|task| (task.id.clone(), task))
            .collect::<HashMap<_, _>>(),
    );

    let config = ExecutionConfig {
        parallel_limit: overrides.parallel_limit.unwrap_or(settings.parallel_limit),
        max_time_seconds: overrides
            .max_time_seconds
            .unwrap_or(settings.max_time_seconds),
        continue_on_error: settings.continue_on_error,
        max_task_iterations: settings.max_task_iterations,
        max_workflow_iterations: settings.max_workflow_iterations,
    };

    // ExpressionEngine is not Send+Sync (Rhai Engine); tasks are joined in-place, never spawned.
    #[allow(clippy::arc_with_non_send_sync)]
    let engine = Arc::new(ExpressionEngine::default());
    let context = resolve_initial_context(&document.workflow.context, engine.as_ref())?;
    let execution_id = Uuid::new_v4().to_string();
    let state = Arc::new(tokio::sync::RwLock::new(ExecutionState {
        context,
        completed: HashMap::new(),
    }));

    let mut ready_queue = VecDeque::new();
    ready_queue.push_back(settings.entry_task.clone());
    let mut task_iterations: HashMap<String, usize> = HashMap::new();
    let mut total_iterations = 0usize;
    let start = Instant::now();

    while !ready_queue.is_empty() {
        if start.elapsed().as_secs() >= config.max_time_seconds {
            return Err(AppError::new(
                ErrorCategory::TimeoutError,
                "workflow exceeded max_time_seconds",
            )
            .with_code("WFG-TIME-001"));
        }

        let mut tick_tasks = Vec::new();
        while tick_tasks.len() < config.parallel_limit {
            if let Some(task_id) = ready_queue.pop_front() {
                if total_iterations >= config.max_workflow_iterations {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        "workflow exceeded max_workflow_iterations",
                    )
                    .with_code("WFG-ITER-001"));
                }
                total_iterations += 1;

                let limit = tasks_by_id
                    .get(&task_id)
                    .map(|task| task.iteration_limit(config.max_task_iterations))
                    .unwrap_or(config.max_task_iterations);
                let entry = task_iterations.entry(task_id.clone()).or_insert(0);
                if *entry >= limit {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        format!("task {} reached iteration cap", task_id),
                    )
                    .with_code("WFG-ITER-002"));
                }
                *entry += 1;
                tick_tasks.push((task_id, *entry as u64));
            } else {
                break;
            }
        }

        if tick_tasks.is_empty() {
            break;
        }

        let snapshot = { state.read().await.snapshot() };
        let tick_tasks_owned: Vec<(String, u64)> = tick_tasks.clone();
        let mut futures = Vec::new();
        for (task_id, run_seq) in tick_tasks_owned {
            let task = tasks_by_id.get(&task_id).unwrap().clone();
            let registry = registry.clone();
            let engine = Arc::clone(&engine);
            let workspace = workspace_root.clone();
            let snapshot = snapshot.clone();
            let execution_id = execution_id.clone();
            futures.push(run_task(
                task,
                registry,
                engine,
                workspace,
                snapshot,
                execution_id,
                run_seq,
            ));
        }

        let mut frontier = Vec::new();
        for result in join_all(futures).await {
            frontier.push(result?);
        }

        process_frontier(
            &mut ready_queue,
            frontier,
            &state,
            Arc::clone(&tasks_by_id),
            &config,
            Arc::clone(&engine),
        )
        .await?;
    }

    let final_state = state.read().await;
    Ok(ExecutionSummary {
        total_iterations,
        completed_tasks: final_state.completed.clone(),
    })
}

fn resolve_initial_context(context: &Value, engine: &ExpressionEngine) -> Result<Value, AppError> {
    let eval = EvaluationContext::new(
        context.clone(),
        Value::Object(Map::new()),
        Value::Object(Map::new()),
    );
    resolve_value(context, engine, &eval)
}

async fn run_task(
    task: WorkflowTask,
    registry: OperatorRegistry,
    engine: Arc<ExpressionEngine>,
    workspace_root: PathBuf,
    snapshot: StateView,
    execution_id: String,
    run_seq: u64,
) -> Result<TaskOutcome, AppError> {
    let operator = registry.get(&task.operator).ok_or_else(|| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("operator '{}' is not registered", task.operator),
        )
        .with_code("WFG-OP-001")
    })?;

    let eval_ctx = snapshot.evaluation_context();
    let resolved_params = resolve_value(&task.params, engine.as_ref(), &eval_ctx)?;
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
    let mut rng = rand::thread_rng();

    loop {
        attempts += 1;
        let ctx = OperatorContext {
            workspace_path: workspace_root.clone(),
            execution_id: execution_id.clone(),
            task_id: task.id.clone(),
            iteration: run_seq,
            state_view: snapshot.clone(),
        };
        let params = resolved_params.clone();
        let started = Instant::now();
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
        let duration_ms = started.elapsed().as_millis() as u64;

        match execution_result {
            Ok(output) => {
                let patch = extract_context_patch(&output);
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
                });
            }
            Err(err) => {
                if attempts >= max_attempts {
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

async fn process_frontier(
    ready_queue: &mut VecDeque<String>,
    frontier: Vec<TaskOutcome>,
    state: &tokio::sync::RwLock<ExecutionState>,
    tasks_by_id: Arc<HashMap<String, WorkflowTask>>,
    config: &ExecutionConfig,
    engine: Arc<ExpressionEngine>,
) -> Result<(), AppError> {
    let mut guard = state.write().await;
    for outcome in &frontier {
        guard
            .completed
            .insert(outcome.task_id.clone(), outcome.record.clone());
        if let Some(patch) = &outcome.context_patch {
            apply_patch(&mut guard.context, patch);
        }
        if outcome.failed && !config.continue_on_error {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("task {} failed", outcome.task_id),
            )
            .with_code("WFG-EXEC-001"));
        }
    }
    let snapshot = guard.snapshot();
    drop(guard);

    let mut seen = HashSet::new();
    for outcome in frontier {
        if let Some(task) = tasks_by_id.get(&outcome.task_id) {
            let mut transitions = task.transitions.clone();
            transitions.sort_by_key(|t| t.priority);
            for transition in transitions {
                if evaluate_transition(&transition, engine.as_ref(), &snapshot)? {
                    if seen.insert(transition.to.clone()) {
                        ready_queue.push_back(transition.to.clone());
                    }
                    break;
                }
            }
        }
    }

    Ok(())
}

fn resolve_value(
    value: &Value,
    engine: &ExpressionEngine,
    ctx: &EvaluationContext,
) -> Result<Value, AppError> {
    match value {
        Value::Object(map) => {
            if map.len() == 1 && map.contains_key("$expr") {
                if let Some(Value::String(expr)) = map.get("$expr") {
                    return engine.evaluate(expr, ctx);
                }
            }
            let mut resolved = Map::new();
            for (key, child) in map {
                resolved.insert(key.clone(), resolve_value(child, engine, ctx)?);
            }
            Ok(Value::Object(resolved))
        }
        Value::Array(items) => {
            let mut collection = Vec::new();
            for item in items {
                collection.push(resolve_value(item, engine, ctx)?);
            }
            Ok(Value::Array(collection))
        }
        other => Ok(other.clone()),
    }
}

fn evaluate_transition(
    transition: &crate::core::workflow_graph::schema::Transition,
    engine: &ExpressionEngine,
    snapshot: &StateView,
) -> Result<bool, AppError> {
    match &transition.when {
        None => Ok(true),
        Some(Condition::Bool(flag)) => Ok(*flag),
        Some(Condition::Expr { expr }) => {
            let ctx = EvaluationContext::new(
                snapshot.context.clone(),
                snapshot.tasks.clone(),
                Value::Object(Map::new()),
            );
            let result = engine.evaluate(expr, &ctx)?;
            if let Value::Bool(flag) = result {
                Ok(flag)
            } else {
                Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("transition expression '{}' did not return bool", expr),
                )
                .with_code("WFG-EXPR-002"))
            }
        }
    }
}

fn apply_patch(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(target_map), Value::Object(patch_map)) => {
            for (key, value) in patch_map {
                match target_map.get_mut(key) {
                    Some(existing) => apply_patch(existing, value),
                    None => {
                        target_map.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (target_value, patch_value) => {
            *target_value = patch_value.clone();
        }
    }
}

fn build_tasks_value(completed: &HashMap<String, TaskRunRecord>) -> Value {
    let mut map = Map::new();
    for (task_id, record) in completed {
        let mut entry = Map::new();
        entry.insert(
            "status".to_string(),
            Value::String(record.status.as_str().to_string()),
        );
        entry.insert("output".to_string(), record.output.clone());
        entry.insert(
            "error_code".to_string(),
            record
                .error_code
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        entry.insert(
            "duration_ms".to_string(),
            Value::Number(Number::from(record.duration_ms)),
        );
        entry.insert(
            "run_seq".to_string(),
            Value::Number(Number::from(record.run_seq)),
        );
        map.insert(task_id.clone(), Value::Object(entry));
    }
    Value::Object(map)
}

fn extract_context_patch(output: &Value) -> Option<Value> {
    if let Value::Object(map) = output {
        map.get("patch").cloned()
    } else {
        None
    }
}
