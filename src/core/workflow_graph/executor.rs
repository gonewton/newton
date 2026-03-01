#![allow(clippy::result_large_err)] // Executor returns AppError to preserve full diagnostic context; boxing would discard run-time state.
use serde::Serialize;

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::artifacts::ArtifactStore;
use crate::core::workflow_graph::checkpoint;
use crate::core::workflow_graph::expression::{EvaluationContext, ExpressionEngine};
use crate::core::workflow_graph::operator::{
    ExecutionContext as OperatorContext, OperatorRegistry, StateView,
};
use crate::core::workflow_graph::schema::{
    self, GoalGateFailureBehavior, TerminalKind, WorkflowDocument, WorkflowTask,
};
use crate::core::workflow_graph::state::{
    canonicalize_workflow_path, compute_sha256_hex, redact_value, summarize_error, AppErrorSummary,
    GraphSettings, WorkflowCheckpoint, WorkflowExecution, WorkflowExecutionStatus,
    WorkflowTaskRunRecord, WorkflowTaskRunSummary, WorkflowTaskStatus,
    WORKFLOW_EXECUTION_FORMAT_VERSION,
};
use chrono::{DateTime, Utc};
use futures::future::join_all;
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde_json::{Map, Number, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::convert::TryFrom;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};
use tracing;
use uuid::Uuid;

/// Optional overrides supplied by CLI flags.
#[derive(Clone, Debug)]
pub struct ExecutionOverrides {
    pub parallel_limit: Option<usize>,
    pub max_time_seconds: Option<u64>,
    pub checkpoint_base_path: Option<PathBuf>,
    pub artifact_base_path: Option<PathBuf>,
    pub verbose: bool,
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
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionSummary {
    pub execution_id: Uuid,
    pub total_iterations: usize,
    pub completed_tasks: BTreeMap<String, TaskRunRecord>,
}

/// Record describing the last completed run of a task.
#[derive(Clone, Debug, Serialize)]
pub struct TaskRunRecord {
    pub status: TaskStatus,
    pub output: Value,
    pub error_code: Option<String>,
    pub duration_ms: u64,
    pub run_seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
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
    triggers: Value,
}

struct WorkflowRuntime {
    workspace_root: PathBuf,
    checkpoint_root: PathBuf,
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
    triggers: Value,
    redact_keys: Arc<Vec<String>>,
    last_checkpoint: Instant,
    start_time: Instant,
    verbose: bool,
}

impl ExecutionState {
    fn snapshot(&self) -> StateView {
        StateView::new(
            self.context.clone(),
            build_tasks_value(&self.completed),
            self.triggers.clone(),
        )
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
        tracing::info!(
            execution_id = %self.workflow_execution.execution_id,
            entry_task = %self.graph_settings.entry_task,
            "workflow starting"
        );
        self.save_execution()?;
        let mut terminal_stop_triggered = false;
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

            let mut frontier: Vec<TaskOutcome> = join_all(futures)
                .await
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;

            // Sort frontier by task_id alphabetically for deterministic ordering.
            frontier.sort_by(|a, b| a.task_id.cmp(&b.task_id));

            // Detect terminal tasks in this tick before processing.
            let mut tick_terminal_ids: Vec<String> = Vec::new();
            if self.graph_settings.completion.stop_on_terminal {
                for outcome in &frontier {
                    if let Some(task) = self.tasks_by_id.get(&outcome.task_id) {
                        if task.terminal.is_some() {
                            tick_terminal_ids.push(outcome.task_id.clone());
                        }
                    }
                }
            }

            let frontier_len = frontier.len();
            if let Err(err) = self.process_frontier(frontier).await {
                self.workflow_execution.status = WorkflowExecutionStatus::Failed;
                self.workflow_execution.completed_at = Some(Utc::now());
                self.persist_checkpoint_force().await?;
                return Err(err);
            }

            // Handle terminal stop after processing so all outcomes are recorded.
            if !tick_terminal_ids.is_empty() {
                if tick_terminal_ids.len() > 1 {
                    // WFG-TERM-001: multiple terminal tasks in same tick (informational).
                    let affected = tick_terminal_ids[1..].to_vec();
                    let warning = serde_json::json!({
                        "code": "WFG-TERM-001",
                        "message": format!(
                            "multiple terminal tasks completed in the same tick; \
                             tie-broken by task-id alphabetical order; \
                             first terminal: {}",
                            tick_terminal_ids[0]
                        ),
                        "affected_tasks": affected
                    });
                    self.workflow_execution.warnings.push(warning);
                }
                terminal_stop_triggered = true;
                self.persist_checkpoint_force().await?;
                break;
            }

            self.maybe_checkpoint(frontier_len).await?;
        }

        // Compute final status per completion policy.
        let final_state = self.state.read().await;
        let (final_exec_status, maybe_err) =
            self.compute_final_status(&final_state, terminal_stop_triggered);
        drop(final_state);

        self.workflow_execution.status = final_exec_status;
        self.workflow_execution.completed_at = Some(Utc::now());
        if let Some(err) = maybe_err {
            self.persist_checkpoint_force().await?;
            return Err(err);
        }
        if self.graph_settings.checkpoint.checkpoint_enabled {
            self.persist_checkpoint().await?;
        } else {
            self.save_execution()?;
        }
        let final_state = self.state.read().await;
        tracing::info!(
            execution_id = %self.workflow_execution.execution_id,
            iterations = self.total_iterations,
            status = self.workflow_execution.status.as_str(),
            "workflow finished"
        );
        let mut completed_tasks = BTreeMap::from_iter(final_state.completed.clone());
        for record in completed_tasks.values_mut() {
            redact_value(
                &mut record.output,
                &self.graph_settings.redaction.redact_keys,
            );
        }

        Ok(ExecutionSummary {
            execution_id: self.workflow_execution.execution_id,
            total_iterations: self.total_iterations,
            completed_tasks,
        })
    }

    /// Compute the final workflow status according to the completion policy.
    /// Returns (status, optional error) where error is Some when the workflow fails.
    fn compute_final_status(
        &self,
        state: &ExecutionState,
        _terminal_stop: bool,
    ) -> (WorkflowExecutionStatus, Option<AppError>) {
        let completion = &self.graph_settings.completion;

        // Collect all goal gate tasks.
        let goal_gate_tasks: Vec<&WorkflowTask> =
            self.tasks_by_id.values().filter(|t| t.goal_gate).collect();

        // Rules 2a and 2b: evaluate goal gates.
        if !goal_gate_tasks.is_empty() {
            let mut failing_gates: Vec<String> = Vec::new();

            for gate in &goal_gate_tasks {
                if let Some(record) = state.completed.get(&gate.id) {
                    // Gate was reached — check if it passed.
                    let passed = record.status == TaskStatus::Success;
                    if !passed
                        && completion.goal_gate_failure_behavior == GoalGateFailureBehavior::Fail
                    {
                        // Rule 2b: reached but not passed.
                        let status_str = record.status.as_str();
                        let entry = if let Some(group) = &gate.goal_gate_group {
                            format!("{}(group={})={}", gate.id, group, status_str)
                        } else {
                            format!("{}={}", gate.id, status_str)
                        };
                        failing_gates.push(entry);
                    }
                } else if completion.require_goal_gates {
                    // Rule 2a: gate not reached.
                    let entry = if let Some(group) = &gate.goal_gate_group {
                        format!("{}(group={})=not_reached", gate.id, group)
                    } else {
                        format!("{}=not_reached", gate.id)
                    };
                    failing_gates.push(entry);
                }
            }

            if !failing_gates.is_empty() {
                failing_gates.sort();
                let err = AppError::new(
                    ErrorCategory::ValidationError,
                    format!("goal gates not passed: {}", failing_gates.join(", ")),
                )
                .with_code("WFG-GATE-001");
                return (WorkflowExecutionStatus::Failed, Some(err));
            }
        }

        // Rule 3: success_requires_no_task_failures.
        if completion.success_requires_no_task_failures
            && state
                .completed
                .values()
                .any(|r| r.status == TaskStatus::Failed)
        {
            let err = AppError::new(
                ErrorCategory::ValidationError,
                "workflow failed: one or more tasks failed",
            )
            .with_code("WFG-EXEC-001");
            return (WorkflowExecutionStatus::Failed, Some(err));
        }

        // Rule 4: any completed terminal:failure task causes failure.
        let mut terminal_failure_task: Option<&str> = None;
        for task_id in state.completed.keys() {
            if let Some(task) = self.tasks_by_id.get(task_id) {
                if task.terminal == Some(TerminalKind::Failure) {
                    terminal_failure_task = Some(task_id.as_str());
                    break;
                }
            }
        }
        if let Some(task_id) = terminal_failure_task {
            let err = AppError::new(
                ErrorCategory::ValidationError,
                format!("workflow terminated at failure terminal task '{}'", task_id),
            )
            .with_code("WFG-EXEC-002");
            return (WorkflowExecutionStatus::Failed, Some(err));
        }

        // Rule 5: Completed.
        (WorkflowExecutionStatus::Completed, None)
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

            // Verbose output: print task stdout/stderr after completion
            if self.verbose {
                self.print_task_verbose_output(outcome);
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
                self.tasks_by_id
                    .get(&outcome.task_id)
                    .and_then(|task| task.goal_gate_group.clone()),
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

                // If any transition has a `when` condition, the task uses exclusive/priority-
                // selection mode: transitions are evaluated in priority order and the first
                // matching one wins (including unconditional fallbacks).
                //
                // If no transitions have a `when` condition, the task fans out: all
                // unconditional transitions that pass `include_if` fire in parallel.
                let has_conditional = transitions.iter().any(|t| t.when.is_some());
                if has_conditional {
                    for transition in transitions {
                        if evaluate_transition(&transition, self.engine.as_ref(), &snapshot)? {
                            if seen.insert(transition.to.clone()) {
                                self.ready_queue.push_back(transition.to.clone());
                            }
                            break;
                        }
                    }
                } else {
                    for transition in transitions {
                        if evaluate_transition(&transition, self.engine.as_ref(), &snapshot)?
                            && seen.insert(transition.to.clone())
                        {
                            self.ready_queue.push_back(transition.to.clone());
                        }
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
            self.triggers.clone(),
            ready_queue,
            self.task_iterations.clone(),
            self.total_iterations,
            checkpoint_records,
        );
        checkpoint::save_checkpoint_at(
            &self.checkpoint_root,
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
        checkpoint::save_execution_at(
            &self.checkpoint_root,
            &self.workflow_execution.execution_id,
            &self.workflow_execution,
        )
    }

    /// Print task stdout/stderr for verbose mode
    fn print_task_verbose_output(&self, outcome: &TaskOutcome) {
        let output = &outcome.record.output;

        // For CommandOperator tasks, stdout/stderr are in the task output
        if let Value::Object(output_map) = output {
            if let Some(Value::String(stdout)) = output_map.get("stdout") {
                if !stdout.trim().is_empty() {
                    print!("{}", stdout);
                }
            }
            if let Some(Value::String(stderr)) = output_map.get("stderr") {
                if !stderr.trim().is_empty() {
                    eprint!("{}", stderr);
                }
            }
            // For AgentOperator tasks, print artifact paths instead
            if let Some(Value::String(artifact_path)) = output_map.get("stdout_artifact") {
                println!("stdout artifact: {}", artifact_path);
            }
            if let Some(Value::String(artifact_path)) = output_map.get("stderr_artifact") {
                eprintln!("stderr artifact: {}", artifact_path);
            }
        }
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
    let runtime =
        build_workflow_runtime(document, workflow_path, registry, workspace_root, overrides)?;
    runtime.run().await
}

pub fn spawn_workflow_execution(
    document: WorkflowDocument,
    workflow_path: PathBuf,
    registry: OperatorRegistry,
    workspace_root: PathBuf,
    overrides: ExecutionOverrides,
) -> Result<
    (
        Uuid,
        tokio::task::JoinHandle<Result<ExecutionSummary, AppError>>,
    ),
    AppError,
> {
    let runtime =
        build_workflow_runtime(document, workflow_path, registry, workspace_root, overrides)?;
    let execution_id = runtime.workflow_execution.execution_id;
    let handle = tokio::spawn(async move { runtime.run().await });
    Ok((execution_id, handle))
}

fn build_workflow_runtime(
    document: WorkflowDocument,
    workflow_path: PathBuf,
    registry: OperatorRegistry,
    workspace_root: PathBuf,
    overrides: ExecutionOverrides,
) -> Result<WorkflowRuntime, AppError> {
    let trigger_payload = extract_trigger_payload(&document);
    let mut graph_settings = document.workflow.settings;
    if let Some(parallel) = overrides.parallel_limit {
        graph_settings.parallel_limit = parallel;
    }
    if let Some(max_time) = overrides.max_time_seconds {
        graph_settings.max_time_seconds = max_time;
    }
    if let Some(artifact_base_path) = &overrides.artifact_base_path {
        graph_settings.artifact_storage.base_path = artifact_base_path.clone();
    }
    let checkpoint_root = overrides
        .checkpoint_base_path
        .as_ref()
        .map(|path| {
            if path.is_absolute() {
                path.clone()
            } else {
                workspace_root.join(path)
            }
        })
        .unwrap_or_else(|| {
            workspace_root
                .join(".newton")
                .join("state")
                .join("workflows")
        });
    validate_required_triggers(&graph_settings.required_triggers, &trigger_payload)?;
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

    let engine = Arc::new(ExpressionEngine::default());
    let eval_ctx = resolve_initial_evaluation_context(
        &document.workflow.context,
        engine.as_ref(),
        &trigger_payload,
    )?;

    let mut tasks_map = HashMap::new();
    for item in &document.workflow.tasks {
        match item {
            schema::TaskOrMacro::Task(task) => {
                let mut included = true;
                if let Some(ref guard) = task.include_if {
                    included = evaluate_condition(guard, engine.as_ref(), &eval_ctx)?;
                }
                if included {
                    let mut task_clone = task.clone();
                    task_clone.transitions = task
                        .transitions
                        .iter()
                        .filter(|t| {
                            if let Some(ref g) = t.include_if {
                                evaluate_condition(g, engine.as_ref(), &eval_ctx).unwrap_or(true)
                            } else {
                                true
                            }
                        })
                        .cloned()
                        .collect();
                    tasks_map.insert(task.id.clone(), task_clone);
                }
            }
            schema::TaskOrMacro::Macro(invocation) => {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "unexpanded macro invocation '{}' reached executor",
                        invocation.macro_name
                    ),
                )
                .with_code("WFG-MACRO-002"));
            }
        }
    }
    let tasks_by_id = Arc::new(tasks_map);

    let config = ExecutionConfig {
        parallel_limit: graph_settings.parallel_limit,
        max_time_seconds: graph_settings.max_time_seconds,
        continue_on_error: graph_settings.continue_on_error,
        max_task_iterations: graph_settings.max_task_iterations,
        max_workflow_iterations: graph_settings.max_workflow_iterations,
    };

    let context = eval_ctx.context.clone();
    let execution_uuid = Uuid::new_v4();
    let state = Arc::new(tokio::sync::RwLock::new(ExecutionState {
        context,
        completed: HashMap::new(),
        checkpoint_records: HashMap::new(),
        triggers: trigger_payload.clone(),
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
        trigger_payload: trigger_payload.clone(),
        task_runs: Vec::new(),
        warnings: Vec::new(),
    };
    let artifact_store =
        ArtifactStore::new(workspace_root.clone(), &graph_settings.artifact_storage);
    let ready_queue = {
        let mut queue = VecDeque::new();
        queue.push_back(graph_settings.entry_task.clone());
        queue
    };
    Ok(WorkflowRuntime {
        workspace_root: workspace_root.clone(),
        checkpoint_root,
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
        triggers: trigger_payload.clone(),
        redact_keys: Arc::new(graph_settings.redaction.redact_keys.clone()),
        last_checkpoint: Instant::now(),
        start_time: Instant::now(),
        verbose: overrides.verbose,
    })
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
            .map(|task| match task {
                schema::TaskOrMacro::Task(task) => Ok((task.id.clone(), task)),
                schema::TaskOrMacro::Macro(invocation) => Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "unexpanded macro invocation '{}' reached executor",
                        invocation.macro_name
                    ),
                )
                .with_code("WFG-MACRO-002")),
            })
            .collect::<Result<HashMap<_, _>, _>>()?,
    );

    let engine = Arc::new(ExpressionEngine::default());
    let completed_records = hydrate_completed_records(&checkpoint_data.completed, &workspace_root)?;
    let state = Arc::new(tokio::sync::RwLock::new(ExecutionState {
        context: checkpoint_data.context.clone(),
        completed: completed_records,
        checkpoint_records: checkpoint_data.completed.clone(),
        triggers: checkpoint_data.trigger_payload.clone(),
    }));

    let mut workflow_execution = execution.clone();
    workflow_execution.status = WorkflowExecutionStatus::Running;
    workflow_execution.completed_at = None;
    let ready_queue = VecDeque::from(checkpoint_data.ready_queue.clone());
    let artifact_store =
        ArtifactStore::new(workspace_root.clone(), &graph_settings.artifact_storage);
    let runtime = WorkflowRuntime {
        workspace_root: workspace_root.clone(),
        checkpoint_root: workspace_root
            .join(".newton")
            .join("state")
            .join("workflows"),
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
        triggers: checkpoint_data.trigger_payload.clone(),
        redact_keys: Arc::new(graph_settings.redaction.redact_keys.clone()),
        last_checkpoint: Instant::now(),
        start_time: Instant::now(),
        verbose: false, // Resume does not support verbose mode
    };
    runtime.run().await
}

fn extract_trigger_payload(document: &WorkflowDocument) -> Value {
    document
        .triggers
        .as_ref()
        .map(|trigger| trigger.payload.clone())
        .unwrap_or_else(|| Value::Object(Map::new()))
}

fn validate_required_triggers(required: &[String], payload: &Value) -> Result<(), AppError> {
    if required.is_empty() {
        return Ok(());
    }
    for key in required {
        if payload.as_object().and_then(|map| map.get(key)).is_none() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("trigger payload missing required key '{}'", key),
            )
            .with_code("WFG-TRIG-001"));
        }
    }
    Ok(())
}

fn resolve_initial_context(
    context: &Value,
    engine: &ExpressionEngine,
    triggers: &Value,
) -> Result<Value, AppError> {
    let eval = EvaluationContext::new(context.clone(), Value::Object(Map::new()), triggers.clone());
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

fn build_workflow_task_run_record(
    outcome: &TaskOutcome,
    goal_gate_group: Option<String>,
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
        goal_gate_group,
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
    let ctx = snapshot.evaluation_context();

    // Check include_if (compile-time/init-time condition, but we evaluate here if not already filtered)
    if let Some(ref guard) = transition.include_if {
        if !evaluate_condition(guard, engine, &ctx)? {
            return Ok(false);
        }
    }

    match &transition.when {
        None => Ok(true),
        Some(cond) => evaluate_condition(cond, engine, &ctx),
    }
}

fn evaluate_condition(
    condition: &crate::core::workflow_graph::schema::Condition,
    engine: &ExpressionEngine,
    ctx: &EvaluationContext,
) -> Result<bool, AppError> {
    match condition {
        crate::core::workflow_graph::schema::Condition::Bool(flag) => Ok(*flag),
        crate::core::workflow_graph::schema::Condition::Expr { expr } => {
            let result = engine.evaluate(expr, ctx)?;
            if let Value::Bool(flag) = result {
                Ok(flag)
            } else {
                Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "expression in condition evaluated to a non-boolean value at runtime: {:?}",
                        result
                    ),
                )
                .with_code("WFG-EXPR-BOOL-001"))
            }
        }
    }
}

fn resolve_initial_evaluation_context(
    context: &Value,
    engine: &ExpressionEngine,
    triggers: &Value,
) -> Result<EvaluationContext, AppError> {
    let resolved_context = resolve_initial_context(context, engine, triggers)?;
    Ok(EvaluationContext::new(
        resolved_context,
        Value::Object(Map::new()),
        triggers.clone(),
    ))
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
