#![allow(clippy::result_large_err)] // Executor returns AppError to preserve full diagnostic context; boxing would discard run-time state.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::artifacts::ArtifactStore;
use crate::core::workflow_graph::checkpoint;
use crate::core::workflow_graph::expression::{EvaluationContext, ExpressionEngine};
use crate::core::workflow_graph::operator::{
    ExecutionContext as OperatorContext, OperatorRegistry, StateView,
};
use crate::core::workflow_graph::schema::{self, Condition, WorkflowDocument, WorkflowTask};
use crate::core::workflow_graph::state::{
    canonicalize_workflow_path, compute_sha256_hex, redact_value, summarize_error, AppErrorSummary,
    GraphSettings, WorkflowCheckpoint, WorkflowExecution, WorkflowExecutionStatus,
    WorkflowTaskRunRecord, WorkflowTaskRunSummary, WorkflowTaskStatus,
    WORKFLOW_EXECUTION_FORMAT_VERSION,
};
use chrono::{DateTime, Utc};
use futures::future::join_all;
use rand::Rng;
use serde_json::{Map, Number, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::TryFrom;
use std::fs;
use std::path::{Path, PathBuf};
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
    pub execution_id: Uuid,
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

impl From<WorkflowTaskStatus> for TaskStatus {
    fn from(status: WorkflowTaskStatus) -> Self {
        match status {
            WorkflowTaskStatus::Success => TaskStatus::Success,
            WorkflowTaskStatus::Failed => TaskStatus::Failed,
            WorkflowTaskStatus::Skipped => TaskStatus::Skipped,
        }
    }
}

struct ExecutionState {
    context: Value,
    completed: HashMap<String, TaskRunRecord>,
    checkpoint_records: HashMap<String, WorkflowTaskRunRecord>,
}

struct WorkflowRuntime {
    workspace_root: PathBuf,
    registry: OperatorRegistry,
    tasks_by_id: Arc<HashMap<String, WorkflowTask>>,
    engine: Arc<ExpressionEngine>,
    graph_settings: GraphSettings,
    config: ExecutionConfig,
    artifact_store: ArtifactStore,
    state: Arc<tokio::sync::RwLock<ExecutionState>>,
    ready_queue: VecDeque<String>,
    task_iterations: HashMap<String, usize>,
    total_iterations: usize,
    workflow_execution: WorkflowExecution,
    redact_keys: Arc<Vec<String>>,
    last_checkpoint: Instant,
    start_time: Instant,
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
    started_at: DateTime<Utc>,
    completed_at: DateTime<Utc>,
    error_summary: Option<AppErrorSummary>,
}

impl WorkflowRuntime {
    async fn run(mut self) -> Result<ExecutionSummary, AppError> {
        self.save_execution()?;
        while !self.ready_queue.is_empty() {
            if self.start_time.elapsed().as_secs() >= self.config.max_time_seconds {
                self.workflow_execution.status = WorkflowExecutionStatus::Failed;
                self.workflow_execution.completed_at = Some(Utc::now());
                self.persist_checkpoint_force().await?;
                return Err(AppError::new(
                    ErrorCategory::TimeoutError,
                    "workflow exceeded max_time_seconds",
                )
                .with_code("WFG-TIME-001"));
            }

            let mut tick_tasks = Vec::new();
            while tick_tasks.len() < self.config.parallel_limit {
                if let Some(task_id) = self.ready_queue.pop_front() {
                    if self.total_iterations >= self.config.max_workflow_iterations {
                        self.workflow_execution.status = WorkflowExecutionStatus::Failed;
                        self.workflow_execution.completed_at = Some(Utc::now());
                        self.persist_checkpoint_force().await?;
                        return Err(AppError::new(
                            ErrorCategory::ValidationError,
                            "workflow exceeded max_workflow_iterations",
                        )
                        .with_code("WFG-ITER-001"));
                    }
                    self.total_iterations += 1;

                    let limit = self
                        .tasks_by_id
                        .get(&task_id)
                        .map(|task| task.iteration_limit(self.config.max_task_iterations))
                        .unwrap_or(self.config.max_task_iterations);
                    let entry = self.task_iterations.entry(task_id.clone()).or_insert(0);
                    if *entry >= limit {
                        self.workflow_execution.status = WorkflowExecutionStatus::Failed;
                        self.workflow_execution.completed_at = Some(Utc::now());
                        self.persist_checkpoint_force().await?;
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

            let snapshot = { self.state.read().await.snapshot() };
            let tick_tasks_owned = tick_tasks.clone();
            let mut futures = Vec::new();
            for (task_id, run_seq) in tick_tasks_owned {
                let task = self.tasks_by_id.get(&task_id).unwrap().clone();
                let registry = self.registry.clone();
                let engine = Arc::clone(&self.engine);
                let workspace = self.workspace_root.clone();
                let snapshot = snapshot.clone();
                let execution_id = self.workflow_execution.execution_id.to_string();
                futures.push(run_task(
                    task,
                    registry,
                    engine,
                    workspace,
                    snapshot,
                    execution_id,
                    run_seq,
                    Arc::clone(&self.redact_keys),
                ));
            }

            let mut frontier = Vec::new();
            for result in join_all(futures).await {
                frontier.push(result?);
            }

            let frontier_len = frontier.len();
            if let Err(err) = self.process_frontier(frontier).await {
                self.workflow_execution.status = WorkflowExecutionStatus::Failed;
                self.workflow_execution.completed_at = Some(Utc::now());
                self.persist_checkpoint_force().await?;
                return Err(err);
            }
            self.maybe_checkpoint(frontier_len).await?;
        }

        self.workflow_execution.status = WorkflowExecutionStatus::Completed;
        self.workflow_execution.completed_at = Some(Utc::now());
        if self.graph_settings.checkpoint.checkpoint_enabled {
            self.persist_checkpoint().await?;
        } else {
            self.save_execution()?;
        }
        let final_state = self.state.read().await;
        Ok(ExecutionSummary {
            execution_id: self.workflow_execution.execution_id,
            total_iterations: self.total_iterations,
            completed_tasks: final_state.completed.clone(),
        })
    }

    async fn process_frontier(&mut self, frontier: Vec<TaskOutcome>) -> Result<(), AppError> {
        let mut guard = self.state.write().await;
        for outcome in &frontier {
            guard
                .completed
                .insert(outcome.task_id.clone(), outcome.record.clone());
            if let Some(patch) = &outcome.context_patch {
                apply_patch(&mut guard.context, patch);
            }
            if outcome.failed && !self.config.continue_on_error {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("task {} failed", outcome.task_id),
                )
                .with_code("WFG-EXEC-001"));
            }
            let record = build_workflow_task_run_record(
                outcome,
                &mut self.artifact_store,
                &self.graph_settings,
                &self.workflow_execution.execution_id,
            )?;
            guard
                .checkpoint_records
                .insert(outcome.task_id.clone(), record.clone());
            self.workflow_execution
                .task_runs
                .push(WorkflowTaskRunSummary::from(record));
        }
        let snapshot = guard.snapshot();
        drop(guard);

        let mut seen = HashSet::new();
        for outcome in frontier {
            if let Some(task) = self.tasks_by_id.get(&outcome.task_id) {
                let mut transitions = task.transitions.clone();
                transitions.sort_by_key(|t| t.priority);
                for transition in transitions {
                    if evaluate_transition(&transition, self.engine.as_ref(), &snapshot)? {
                        if seen.insert(transition.to.clone()) {
                            self.ready_queue.push_back(transition.to.clone());
                        }
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    fn should_checkpoint(&self, frontier_len: usize) -> bool {
        if self.graph_settings.checkpoint.checkpoint_on_task_complete && frontier_len > 0 {
            return true;
        }
        let interval_secs = self.graph_settings.checkpoint.checkpoint_interval_seconds;
        if interval_secs > 0 {
            return self.last_checkpoint.elapsed() >= Duration::from_secs(interval_secs);
        }
        false
    }

    async fn maybe_checkpoint(&mut self, frontier_len: usize) -> Result<(), AppError> {
        if self.graph_settings.checkpoint.checkpoint_enabled {
            if self.should_checkpoint(frontier_len) {
                self.persist_checkpoint().await
            } else {
                self.save_execution()
            }
        } else {
            self.save_execution()
        }
    }

    async fn persist_checkpoint(&mut self) -> Result<(), AppError> {
        let guard = self.state.read().await;
        let mut redacted_context = guard.context.clone();
        redact_value(&mut redacted_context, &self.redact_keys);
        let ready_queue = self.ready_queue.iter().cloned().collect::<Vec<_>>();
        let checkpoint_records = guard.checkpoint_records.clone();
        drop(guard);
        let checkpoint = WorkflowCheckpoint::new(
            self.workflow_execution.execution_id,
            self.workflow_execution.workflow_hash.clone(),
            redacted_context,
            ready_queue,
            self.task_iterations.clone(),
            self.total_iterations,
            checkpoint_records,
        );
        checkpoint::save_checkpoint(
            &self.workspace_root,
            &self.workflow_execution.execution_id,
            &checkpoint,
            self.graph_settings.checkpoint.checkpoint_keep_history,
        )?;
        self.save_execution()?;
        self.last_checkpoint = Instant::now();
        Ok(())
    }

    async fn persist_checkpoint_force(&mut self) -> Result<(), AppError> {
        if self.graph_settings.checkpoint.checkpoint_enabled {
            self.persist_checkpoint().await
        } else {
            self.save_execution()
        }
    }

    fn save_execution(&self) -> Result<(), AppError> {
        checkpoint::save_execution(
            &self.workspace_root,
            &self.workflow_execution.execution_id,
            &self.workflow_execution,
        )
    }
}

/// Execute a workflow document with the provided overrides.
pub async fn execute_workflow(
    document: WorkflowDocument,
    workflow_path: PathBuf,
    registry: OperatorRegistry,
    workspace_root: PathBuf,
    overrides: ExecutionOverrides,
) -> Result<ExecutionSummary, AppError> {
    let mut graph_settings = document.workflow.settings;
    if let Some(parallel) = overrides.parallel_limit {
        graph_settings.parallel_limit = parallel;
    }
    if let Some(max_time) = overrides.max_time_seconds {
        graph_settings.max_time_seconds = max_time;
    }
    let workflow_file = canonicalize_workflow_path(&workflow_path)?;
    let workflow_bytes = fs::read(&workflow_file).map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!(
                "failed to read workflow file {}: {}",
                workflow_file.display(),
                err
            ),
        )
    })?;
    let workflow_hash = compute_sha256_hex(&workflow_bytes);
    let tasks_by_id = Arc::new(
        document
            .workflow
            .tasks
            .into_iter()
            .map(|task| (task.id.clone(), task))
            .collect::<HashMap<_, _>>(),
    );

    let config = ExecutionConfig {
        parallel_limit: graph_settings.parallel_limit,
        max_time_seconds: graph_settings.max_time_seconds,
        continue_on_error: graph_settings.continue_on_error,
        max_task_iterations: graph_settings.max_task_iterations,
        max_workflow_iterations: graph_settings.max_workflow_iterations,
    };

    #[allow(clippy::arc_with_non_send_sync)]
    let engine = Arc::new(ExpressionEngine::default());
    let context = resolve_initial_context(&document.workflow.context, engine.as_ref())?;
    let execution_uuid = Uuid::new_v4();
    let state = Arc::new(tokio::sync::RwLock::new(ExecutionState {
        context,
        completed: HashMap::new(),
        checkpoint_records: HashMap::new(),
    }));
    let workflow_execution = WorkflowExecution {
        format_version: WORKFLOW_EXECUTION_FORMAT_VERSION.to_string(),
        execution_id: execution_uuid,
        workflow_file: workflow_file.display().to_string(),
        workflow_version: document.version.clone(),
        workflow_hash: workflow_hash.clone(),
        started_at: Utc::now(),
        completed_at: None,
        status: WorkflowExecutionStatus::Running,
        settings_effective: graph_settings.clone(),
        task_runs: Vec::new(),
    };
    let artifact_store =
        ArtifactStore::new(workspace_root.clone(), &graph_settings.artifact_storage);
    let ready_queue = {
        let mut queue = VecDeque::new();
        queue.push_back(graph_settings.entry_task.clone());
        queue
    };
    let runtime = WorkflowRuntime {
        workspace_root: workspace_root.clone(),
        registry,
        tasks_by_id,
        engine,
        graph_settings: graph_settings.clone(),
        config,
        artifact_store,
        state,
        ready_queue,
        task_iterations: HashMap::new(),
        total_iterations: 0,
        workflow_execution,
        redact_keys: Arc::new(graph_settings.redaction.redact_keys.clone()),
        last_checkpoint: Instant::now(),
        start_time: Instant::now(),
    };
    runtime.run().await
}

/// Resume a workflow execution from the latest checkpoint.
pub async fn resume_workflow(
    registry: OperatorRegistry,
    workspace_root: PathBuf,
    execution_id: Uuid,
    allow_workflow_change: bool,
) -> Result<ExecutionSummary, AppError> {
    let execution = checkpoint::load_execution(&workspace_root, &execution_id)?;
    let checkpoint_data = checkpoint::load_checkpoint(&workspace_root, &execution_id)?;
    let workflow_path = PathBuf::from(&execution.workflow_file);
    let document = schema::load_workflow(&workflow_path)?;
    if document.version != execution.workflow_version {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "workflow schema version does not match checkpoint",
        )
        .with_code("WFG-CKPT-001"));
    }
    let current_bytes = fs::read(&workflow_path).map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!(
                "failed to read workflow file {}: {}",
                workflow_path.display(),
                err
            ),
        )
    })?;
    let current_hash = compute_sha256_hex(&current_bytes);
    if current_hash != execution.workflow_hash && !allow_workflow_change {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "workflow hash does not match checkpoint",
        )
        .with_code("WFG-CKPT-001"));
    }

    let graph_settings = execution.settings_effective.clone();
    let config = ExecutionConfig {
        parallel_limit: graph_settings.parallel_limit,
        max_time_seconds: graph_settings.max_time_seconds,
        continue_on_error: graph_settings.continue_on_error,
        max_task_iterations: graph_settings.max_task_iterations,
        max_workflow_iterations: graph_settings.max_workflow_iterations,
    };

    let tasks_by_id = Arc::new(
        document
            .workflow
            .tasks
            .into_iter()
            .map(|task| (task.id.clone(), task))
            .collect::<HashMap<_, _>>(),
    );

    #[allow(clippy::arc_with_non_send_sync)]
    let engine = Arc::new(ExpressionEngine::default());
    let completed_records = hydrate_completed_records(&checkpoint_data.completed, &workspace_root)?;
    let state = Arc::new(tokio::sync::RwLock::new(ExecutionState {
        context: checkpoint_data.context.clone(),
        completed: completed_records,
        checkpoint_records: checkpoint_data.completed.clone(),
    }));

    let mut workflow_execution = execution.clone();
    workflow_execution.status = WorkflowExecutionStatus::Running;
    workflow_execution.completed_at = None;
    let ready_queue = VecDeque::from(checkpoint_data.ready_queue.clone());
    let artifact_store =
        ArtifactStore::new(workspace_root.clone(), &graph_settings.artifact_storage);
    let runtime = WorkflowRuntime {
        workspace_root: workspace_root.clone(),
        registry,
        tasks_by_id,
        engine,
        graph_settings: graph_settings.clone(),
        config,
        artifact_store,
        state,
        ready_queue,
        task_iterations: checkpoint_data.task_iterations.clone(),
        total_iterations: checkpoint_data.total_iterations,
        workflow_execution,
        redact_keys: Arc::new(graph_settings.redaction.redact_keys.clone()),
        last_checkpoint: Instant::now(),
        start_time: Instant::now(),
    };
    runtime.run().await
}

fn resolve_initial_context(context: &Value, engine: &ExpressionEngine) -> Result<Value, AppError> {
    let eval = EvaluationContext::new(
        context.clone(),
        Value::Object(Map::new()),
        Value::Object(Map::new()),
    );
    resolve_value(context, engine, &eval)
}

#[allow(clippy::too_many_arguments)]
async fn run_task(
    task: WorkflowTask,
    registry: OperatorRegistry,
    engine: Arc<ExpressionEngine>,
    workspace_root: PathBuf,
    snapshot: StateView,
    execution_id: String,
    run_seq: u64,
    redact_keys: Arc<Vec<String>>,
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
                    started_at,
                    completed_at,
                    error_summary: None,
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

fn build_workflow_task_run_record(
    outcome: &TaskOutcome,
    artifact_store: &mut ArtifactStore,
    graph_settings: &GraphSettings,
    execution_id: &Uuid,
) -> Result<WorkflowTaskRunRecord, AppError> {
    let run_seq = usize::try_from(outcome.record.run_seq).map_err(|_| {
        AppError::new(
            ErrorCategory::ValidationError,
            "task run_seq value overflowed usize",
        )
        .with_code("WFG-EXEC-004")
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
        status: WorkflowTaskStatus::from_execution(outcome.record.status.clone()),
        output_ref,
        error: outcome.error_summary.clone(),
    })
}

fn hydrate_completed_records(
    records: &HashMap<String, WorkflowTaskRunRecord>,
    workspace_root: &Path,
) -> Result<HashMap<String, TaskRunRecord>, AppError> {
    let mut map = HashMap::new();
    for (task_id, record) in records {
        let output = record.output_ref.materialize(workspace_root)?;
        let duration_ms = record
            .completed_at
            .signed_duration_since(record.started_at)
            .num_milliseconds() as u64;
        map.insert(
            task_id.clone(),
            TaskRunRecord {
                status: TaskStatus::from(record.status),
                output,
                error_code: record.error.as_ref().map(|err| err.code.clone()),
                duration_ms,
                run_seq: record.run_seq as u64,
            },
        );
    }
    Ok(map)
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
