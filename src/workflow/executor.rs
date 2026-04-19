#![allow(clippy::result_large_err)] // Executor returns AppError to preserve full diagnostic context; boxing would discard run-time state.
use serde::Serialize;

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::artifacts::ArtifactStore;
use crate::workflow::checkpoint;
use crate::workflow::child_run::{ChildRunInput, ChildWorkflowRunSummary, ChildWorkflowRunner};
use crate::workflow::expression::ExpressionEngine;
use crate::workflow::lint::{LintRegistry, LintSeverity};
use crate::workflow::operator::{OperatorRegistry, StateView};
use crate::workflow::schema::{
    self, BarrierParams, GoalGateFailureBehavior, TerminalKind, WorkflowDocument, WorkflowTask,
};
use crate::workflow::server_notifier::ServerNotifier;
use crate::workflow::state::{
    canonicalize_workflow_path, compute_sha256_hex, redact_value, AppErrorSummary, GraphSettings,
    TaskRunRecord, WorkflowCheckpoint, WorkflowExecution, WorkflowExecutionStatus,
    WorkflowTaskRunRecord, WorkflowTaskRunSummary, WORKFLOW_EXECUTION_FORMAT_VERSION,
};
use crate::workflow::task_execution;
use crate::workflow::transform;
use crate::workflow::value_resolve as context;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::future::join_all;
use newton_types::{NodeState, NodeStatus, WorkflowInstance, WorkflowStatus};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing;
use uuid::Uuid;

// Re-export TaskStatus for backward compatibility with existing tests
pub use crate::workflow::state::TaskStatus;

/// Optional overrides supplied by CLI flags.
#[derive(Clone, Debug)]
pub struct ExecutionOverrides {
    pub parallel_limit: Option<usize>,
    pub max_time_seconds: Option<u64>,
    pub checkpoint_base_path: Option<PathBuf>,
    pub artifact_base_path: Option<PathBuf>,
    /// Maximum allowed workflow nesting depth for `WorkflowOperator` (default: 16 when None).
    pub max_nesting_depth: Option<u32>,
    pub verbose: bool,
    /// Optional server notifier for registering with a newton serve instance.
    pub server_notifier: Option<Arc<ServerNotifier>>,
    /// Whether to pre-seed workflow nodes with Pending status on start. Defaults to true.
    pub pre_seed_nodes: bool,
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

/// Link metadata used to record a parent-child relationship for nested workflow runs.
#[derive(Debug, Clone)]
pub struct ParentRunLink {
    pub parent_execution_id: Uuid,
    pub parent_task_id: String,
    /// Nesting depth for the workflow being built (0 = root workflow).
    pub nesting_depth: u32,
}

pub struct ExecutionState {
    context: Value,
    completed: HashMap<String, TaskRunRecord>,
    checkpoint_records: HashMap<String, WorkflowTaskRunRecord>,
    triggers: Value,
}

/// Handle providing controlled access to the runtime workflow graph.
#[derive(Clone)]
pub struct GraphHandle(Arc<RwLock<HashMap<String, WorkflowTask>>>);

impl GraphHandle {
    pub fn new(tasks: HashMap<String, WorkflowTask>) -> Self {
        GraphHandle(Arc::new(RwLock::new(tasks)))
    }

    /// Add a single task to the runtime graph.
    pub fn add_task(
        &self,
        task: WorkflowTask,
        _enqueue: bool,
        if_absent: bool,
    ) -> Result<(), AppError> {
        let mut graph = self.0.write().unwrap();

        if let Some(existing_task) = graph.get(&task.id) {
            if !if_absent {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("Task '{}' already exists in runtime graph", task.id),
                )
                .with_code("WFG-DYN-001"));
            }
            // If if_absent is true and task exists with identical definition, it's a no-op
            if existing_task.operator != task.operator || existing_task.params != task.params {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "Task '{}' already exists with different definition",
                        task.id
                    ),
                )
                .with_code("WFG-DYN-001"));
            }
            return Ok(()); // No-op for identical task
        }

        // Validate required fields
        if task.id.trim().is_empty() {
            return Err(
                AppError::new(ErrorCategory::ValidationError, "Task ID cannot be empty")
                    .with_code("WFG-DYN-002"),
            );
        }
        if task.operator.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "Task operator cannot be empty",
            )
            .with_code("WFG-DYN-002"));
        }

        graph.insert(task.id.clone(), task);
        Ok(())
    }

    /// Add multiple tasks to the runtime graph.
    pub fn add_tasks(
        &self,
        tasks: Vec<WorkflowTask>,
        _enqueue: bool,
        if_absent: bool,
        barrier_task_id: Option<&str>,
    ) -> Result<(), AppError> {
        let mut task_ids = Vec::new();

        // Add all tasks first
        for task in tasks {
            self.add_task(task.clone(), false, if_absent)?; // Don't enqueue individual tasks
            task_ids.push(task.id);
        }

        // If barrier task is specified, register the added tasks with it
        if let Some(barrier_id) = barrier_task_id {
            self.register_barrier(barrier_id, &task_ids)?;
        }

        Ok(())
    }

    /// Register task IDs with a barrier operator.
    pub fn register_barrier(
        &self,
        barrier_task_id: &str,
        expected_ids: &[String],
    ) -> Result<(), AppError> {
        let mut graph = self.0.write().unwrap();

        let barrier_task = graph.get_mut(barrier_task_id).ok_or_else(|| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("Barrier task '{barrier_task_id}' not found in runtime graph"),
            )
            .with_code("WFG-DYN-004")
        })?;

        if barrier_task.operator != "barrier" {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("Task '{barrier_task_id}' is not a barrier operator"),
            )
            .with_code("WFG-DYN-004"));
        }

        // Parse current barrier params
        let mut barrier_params: BarrierParams = serde_json::from_value(barrier_task.params.clone())
            .unwrap_or_else(|_| BarrierParams { expected: vec![] });

        // Add new expected IDs
        barrier_params.expected.extend_from_slice(expected_ids);

        // Update the task params
        barrier_task.params = serde_json::to_value(&barrier_params).map_err(|err| {
            AppError::new(
                ErrorCategory::SerializationError,
                format!("Failed to serialize barrier params: {err}"),
            )
        })?;

        Ok(())
    }

    /// Get a task from the runtime graph (read-only access).
    pub fn get_task(&self, task_id: &str) -> Option<WorkflowTask> {
        let graph = self.0.read().unwrap();
        graph.get(task_id).cloned()
    }

    /// Get all tasks from the runtime graph (read-only access).
    pub fn get_all_tasks(&self) -> Vec<WorkflowTask> {
        let graph = self.0.read().unwrap();
        graph.values().cloned().collect()
    }

    /// Check if a task exists in the runtime graph.
    pub fn contains_task(&self, task_id: &str) -> bool {
        let graph = self.0.read().unwrap();
        graph.contains_key(task_id)
    }
}

struct WorkflowRuntime {
    workspace_root: PathBuf,
    workflow_file: PathBuf,
    checkpoint_root: PathBuf,
    registry: OperatorRegistry,
    runtime_graph: GraphHandle,
    engine: Arc<ExpressionEngine>,
    graph_settings: GraphSettings,
    config: ExecutionConfig,
    execution_overrides: ExecutionOverrides,
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
    /// Task IDs popped in the current tick, tracked for re-queue on hard batch failure.
    current_tick_tasks: Vec<String>,
    /// Optional server notifier for pushing lifecycle events to a newton serve instance.
    server_notifier: Option<Arc<ServerNotifier>>,
    /// Serialized workflow definition JSON for API exposure.
    workflow_definition_json: Option<serde_json::Value>,
    /// Whether to pre-seed workflow nodes with Pending status on start.
    pre_seed_nodes: bool,
}

impl ExecutionState {
    fn snapshot(&self) -> StateView {
        StateView::new(
            self.context.clone(),
            context::build_tasks_value(&self.completed),
            self.triggers.clone(),
        )
    }
}

#[derive(Clone)]
pub struct TaskOutcome {
    pub task_id: String,
    pub record: TaskRunRecord,
    pub context_patch: Option<Value>,
    pub failed: bool,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub error_summary: Option<AppErrorSummary>,
    /// Resolved operator parameters; passed through to WorkflowTaskRunRecord.
    pub resolved_params: Value,
}

impl WorkflowRuntime {
    /// Check if the workflow has exceeded the maximum allowed time.
    /// Returns an error if the timeout is exceeded.
    async fn check_timeout(&mut self) -> Result<(), AppError> {
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
        Ok(())
    }

    /// Check if we can schedule a task without exceeding iteration limits.
    /// Returns Ok(true) if the task can be scheduled, Ok(false) if we should stop,
    /// or an error if limits are exceeded.
    async fn check_iteration_limits(&mut self, task_id: &str) -> Result<bool, AppError> {
        if self.total_iterations >= self.config.max_workflow_iterations {
            self.workflow_execution.status = WorkflowExecutionStatus::Failed;
            self.workflow_execution.completed_at = Some(Utc::now());
            self.ready_queue.push_front(task_id.to_string());
            self.persist_checkpoint_force().await?;
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "workflow exceeded max_workflow_iterations",
            )
            .with_code("WFG-ITER-001"));
        }

        let limit = self
            .runtime_graph
            .get_task(task_id)
            .map_or(self.config.max_task_iterations, |task| {
                task.iteration_limit(self.config.max_task_iterations)
            });
        let entry = self.task_iterations.entry(task_id.to_string()).or_insert(0);
        if *entry >= limit {
            self.workflow_execution.status = WorkflowExecutionStatus::Failed;
            self.workflow_execution.completed_at = Some(Utc::now());
            self.ready_queue.push_front(task_id.to_string());
            self.persist_checkpoint_force().await?;
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("task {task_id} reached iteration cap"),
            )
            .with_code("WFG-ITER-002"));
        }

        // Update iteration count
        self.total_iterations += 1;
        *entry += 1;
        Ok(true)
    }

    /// Prepare tasks for execution in the current tick.
    /// Returns a list of (task_id, run_sequence) pairs.
    async fn prepare_tick_tasks(&mut self) -> Result<Vec<(String, u64)>, AppError> {
        let mut tick_tasks = Vec::new();
        self.current_tick_tasks.clear();
        while tick_tasks.len() < self.config.parallel_limit {
            if let Some(task_id) = self.ready_queue.pop_front() {
                self.check_iteration_limits(&task_id).await?;
                let run_seq = *self.task_iterations.get(&task_id).unwrap() as u64;
                tick_tasks.push((task_id.clone(), run_seq));
                self.current_tick_tasks.push(task_id);
            } else {
                break;
            }
        }
        Ok(tick_tasks)
    }

    /// Detect and handle terminal tasks in the current frontier.
    /// Returns true if a terminal stop was triggered, false otherwise.
    async fn handle_terminal_tasks(&mut self, frontier: &[TaskOutcome]) -> Result<bool, AppError> {
        // Detect terminal tasks in this tick before processing.
        let mut tick_terminal_ids: Vec<String> = Vec::new();
        if self.graph_settings.completion.stop_on_terminal {
            for outcome in frontier {
                if let Some(task) = self.runtime_graph.get_task(&outcome.task_id) {
                    if task.terminal.is_some() {
                        tick_terminal_ids.push(outcome.task_id.clone());
                    }
                }
            }
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
            self.persist_checkpoint_force().await?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Build the list of pre-seeded node states sent to the server at startup.
    fn build_preseed_nodes(&self) -> Vec<NodeState> {
        if self.pre_seed_nodes {
            self.runtime_graph
                .get_all_tasks()
                .into_iter()
                .map(|task| NodeState {
                    node_id: task.id.clone(),
                    status: NodeStatus::Pending,
                    started_at: None,
                    ended_at: None,
                    operator_type: Some(task.operator.clone()),
                })
                .collect()
        } else {
            vec![]
        }
    }

    /// Notify the server that a set of tasks is now running.
    fn notify_task_starts(&self, tick_tasks: &[(String, u64)]) {
        if let Some(notifier) = &self.server_notifier {
            let instance_id = self.workflow_execution.execution_id.to_string();
            let now = Utc::now();
            for (task_id, _) in tick_tasks {
                let operator_type = self
                    .runtime_graph
                    .get_task(task_id)
                    .map(|t| t.operator.clone());
                let node = NodeState {
                    node_id: task_id.clone(),
                    status: NodeStatus::Running,
                    started_at: Some(now),
                    ended_at: None,
                    operator_type,
                };
                notifier.notify_node_updated(instance_id.clone(), node);
            }
        }
    }

    /// Notify the server of all task outcomes from the current tick.
    fn notify_task_completions(&self, frontier: &[TaskOutcome]) {
        if let Some(notifier) = &self.server_notifier {
            let instance_id = self.workflow_execution.execution_id.to_string();
            for outcome in frontier {
                let node_status = if outcome.failed {
                    NodeStatus::Failed
                } else {
                    NodeStatus::Succeeded
                };
                let operator_type = self
                    .runtime_graph
                    .get_task(&outcome.task_id)
                    .map(|t| t.operator.clone());
                let node = NodeState {
                    node_id: outcome.task_id.clone(),
                    status: node_status,
                    started_at: Some(outcome.started_at),
                    ended_at: Some(outcome.completed_at),
                    operator_type,
                };
                notifier.notify_node_updated(instance_id.clone(), node);
            }
        }
    }

    /// Notify the server that the workflow has completed with the given status.
    fn notify_completion(&self, status: WorkflowStatus) {
        if let Some(notifier) = &self.server_notifier {
            notifier.notify_workflow_completed(
                self.workflow_execution.execution_id.to_string(),
                status,
                self.workflow_execution
                    .completed_at
                    .unwrap_or_else(Utc::now),
            );
        }
    }

    async fn run(mut self) -> Result<ExecutionSummary, AppError> {
        tracing::info!(
            execution_id = %self.workflow_execution.execution_id,
            entry_task = %self.graph_settings.entry_task,
            "workflow starting"
        );
        self.save_execution()?;

        // Notify server that workflow has started.
        if let Some(notifier) = &self.server_notifier {
            let instance = WorkflowInstance {
                instance_id: self.workflow_execution.execution_id.to_string(),
                workflow_id: self.workflow_execution.workflow_file.clone(),
                status: WorkflowStatus::Running,
                nodes: self.build_preseed_nodes(),
                started_at: self.workflow_execution.started_at,
                ended_at: None,
                definition: self.workflow_definition_json.clone(),
            };
            notifier.notify_workflow_started(instance);
        }

        let mut terminal_stop_triggered = false;
        while !self.ready_queue.is_empty() {
            self.check_timeout().await?;

            let tick_tasks = self.prepare_tick_tasks().await?;

            if tick_tasks.is_empty() {
                break;
            }

            self.notify_task_starts(&tick_tasks);

            let snapshot = { self.state.read().await.snapshot() };
            let tick_tasks_owned = tick_tasks.clone();
            let mut futures = Vec::new();
            for (task_id, run_seq) in tick_tasks_owned {
                let task = self.runtime_graph.get_task(&task_id).unwrap();
                let registry = self.registry.clone();
                let engine = Arc::clone(&self.engine);
                let workspace = self.workspace_root.clone();
                let snapshot = snapshot.clone();
                let execution_id = self.workflow_execution.execution_id.to_string();
                futures.push(task_execution::run_task(
                    task,
                    registry,
                    engine,
                    workspace,
                    snapshot,
                    execution_id,
                    run_seq,
                    Arc::clone(&self.redact_keys),
                    self.runtime_graph.clone(),
                    self.workflow_file.clone(),
                    self.workflow_execution.nesting_depth,
                    self.execution_overrides.clone(),
                ));
            }

            let frontier_result: Result<Vec<TaskOutcome>, AppError> =
                join_all(futures).await.into_iter().collect();

            let mut frontier = match frontier_result {
                Ok(outcomes) => {
                    self.current_tick_tasks.clear();
                    outcomes
                }
                Err(err) => {
                    // Hard task error: re-queue aborted tasks in reverse order to
                    // preserve original execution priority.
                    for task_id in self.current_tick_tasks.drain(..).rev() {
                        self.ready_queue.push_front(task_id);
                    }
                    self.workflow_execution.status = WorkflowExecutionStatus::Failed;
                    self.workflow_execution.completed_at = Some(Utc::now());
                    self.persist_checkpoint_force().await?;
                    self.notify_completion(WorkflowStatus::Failed);
                    return Err(err);
                }
            };

            // Sort frontier by task_id alphabetically for deterministic ordering.
            frontier.sort_by(|a, b| a.task_id.cmp(&b.task_id));

            let frontier_len = frontier.len();
            if let Err(err) = self.process_frontier(frontier.clone()).await {
                self.workflow_execution.status = WorkflowExecutionStatus::Failed;
                self.workflow_execution.completed_at = Some(Utc::now());
                self.persist_checkpoint_force().await?;
                self.notify_completion(WorkflowStatus::Failed);
                return Err(err);
            }

            self.notify_task_completions(&frontier);

            // Handle terminal tasks - check if we should stop
            if self.handle_terminal_tasks(&frontier).await? {
                terminal_stop_triggered = true;
                break;
            }

            self.maybe_checkpoint(frontier_len).await?;
        }

        // Compute final status per completion policy.
        let final_state = self.state.read().await;
        let (final_exec_status, maybe_err) =
            self.compute_final_status(&final_state, terminal_stop_triggered);
        // Collect failed task ids for hint printing (ascending order).
        let mut final_failed_task_ids: Vec<String> = final_state
            .completed
            .iter()
            .filter(|(_, r)| r.status == crate::workflow::state::TaskStatus::Failed)
            .map(|(id, _)| id.clone())
            .collect();
        final_failed_task_ids.sort();
        drop(final_state);

        self.workflow_execution.status = final_exec_status;
        self.workflow_execution.completed_at = Some(Utc::now());
        if let Some(err) = maybe_err {
            // Print hint lines for task failures before propagating error.
            if err.code == "WFG-EXEC-001" && !final_failed_task_ids.is_empty() {
                for task_id in &final_failed_task_ids {
                    println!(
                        "newton: task failed execution_id={} task_id={} inspect: newton log show {} --task {}",
                        self.workflow_execution.execution_id,
                        task_id,
                        self.workflow_execution.execution_id,
                        task_id
                    );
                }
            }
            self.persist_checkpoint_force().await?;
            self.notify_completion(WorkflowStatus::Failed);
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

        // Notify server of workflow completion.
        let final_status = match self.workflow_execution.status {
            WorkflowExecutionStatus::Completed => WorkflowStatus::Succeeded,
            _ => WorkflowStatus::Failed,
        };
        self.notify_completion(final_status);

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
        let goal_gate_tasks: Vec<WorkflowTask> = self
            .runtime_graph
            .get_all_tasks()
            .into_iter()
            .filter(|t| t.goal_gate)
            .collect();

        // Rules 2a and 2b: evaluate goal gates.
        if !goal_gate_tasks.is_empty() {
            let mut failing_gates: Vec<String> = Vec::new();

            for gate in &goal_gate_tasks {
                if let Some(record) = state.completed.get(&gate.id) {
                    // Gate was reached — check if it passed.
                    let passed = record.status == crate::workflow::state::TaskStatus::Success;
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
                .any(|r| r.status == crate::workflow::state::TaskStatus::Failed)
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
            if let Some(task) = self.runtime_graph.get_task(task_id) {
                if task.terminal == Some(TerminalKind::Failure) {
                    terminal_failure_task = Some(task_id.as_str());
                    break;
                }
            }
        }
        if let Some(task_id) = terminal_failure_task {
            let err = AppError::new(
                ErrorCategory::ValidationError,
                format!("workflow terminated at failure terminal task '{task_id}'"),
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
                context::apply_patch(&mut guard.context, patch);
            }

            // Verbose output: print task stdout/stderr after completion
            if self.verbose {
                Self::print_task_verbose_output(outcome);
            }

            let record = task_execution::build_workflow_task_run_record(
                outcome,
                self.runtime_graph
                    .get_task(&outcome.task_id)
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

            if outcome.failed && !self.config.continue_on_error {
                if let Some(error) = outcome.error_summary.as_ref() {
                    if error.code == "WFG-NEST-005" {
                        return Err(AppError::new(
                            ErrorCategory::ValidationError,
                            error.message.clone(),
                        )
                        .with_code("WFG-NEST-005"));
                    }
                }
                println!(
                    "newton: task failed execution_id={} task_id={} inspect: newton log show {} --task {}",
                    self.workflow_execution.execution_id,
                    outcome.task_id,
                    self.workflow_execution.execution_id,
                    outcome.task_id
                );
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
            if let Some(task) = self.runtime_graph.get_task(&outcome.task_id) {
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
                        if context::evaluate_transition(
                            &transition,
                            self.engine.as_ref(),
                            &snapshot,
                        )? {
                            // WFG-DYN-003: Validate that transition target exists in runtime graph
                            if !self.runtime_graph.contains_task(&transition.to) {
                                return Err(AppError::new(
                                    ErrorCategory::ValidationError,
                                    format!(
                                        "Task '{}' transition references non-existent task '{}' in runtime graph",
                                        task.id, transition.to
                                    ),
                                ).with_code("WFG-DYN-003"));
                            }
                            if seen.insert(transition.to.clone()) {
                                self.ready_queue.push_back(transition.to.clone());
                            }
                            break;
                        }
                    }
                } else {
                    for transition in transitions {
                        if context::evaluate_transition(
                            &transition,
                            self.engine.as_ref(),
                            &snapshot,
                        )? {
                            // WFG-DYN-003: Validate that transition target exists in runtime graph
                            if !self.runtime_graph.contains_task(&transition.to) {
                                return Err(AppError::new(
                                    ErrorCategory::ValidationError,
                                    format!(
                                        "Task '{}' transition references non-existent task '{}' in runtime graph",
                                        task.id, transition.to
                                    ),
                                ).with_code("WFG-DYN-003"));
                            }
                            if seen.insert(transition.to.clone()) {
                                self.ready_queue.push_back(transition.to.clone());
                            }
                        }
                    }
                }
            }
        }

        // Evaluate barrier tasks: check if they can be enqueued
        self.evaluate_barrier_tasks().await?;

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
        let runtime_tasks = self.runtime_graph.get_all_tasks();
        let checkpoint = WorkflowCheckpoint::new_v2(
            self.workflow_execution.execution_id,
            self.workflow_execution.workflow_hash.clone(),
            redacted_context,
            self.triggers.clone(),
            ready_queue,
            self.task_iterations.clone(),
            self.total_iterations,
            checkpoint_records,
            runtime_tasks,
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

    /// Evaluate barrier tasks and enqueue those that are ready.
    async fn evaluate_barrier_tasks(&mut self) -> Result<(), AppError> {
        // Get the current state
        let guard = self.state.read().await;
        let completed_tasks = &guard.completed;

        // Get all barrier tasks from the runtime graph
        let all_tasks = self.runtime_graph.get_all_tasks();
        let barrier_tasks: Vec<WorkflowTask> = all_tasks
            .into_iter()
            .filter(|task| task.operator == "barrier")
            .collect();

        for barrier_task in barrier_tasks {
            // Skip if this barrier is already completed or already in the ready queue
            if completed_tasks.contains_key(&barrier_task.id)
                || self.ready_queue.contains(&barrier_task.id)
            {
                continue;
            }

            // Parse barrier parameters
            let barrier_params: BarrierParams =
                match serde_json::from_value(barrier_task.params.clone()) {
                    Ok(params) => params,
                    Err(e) => {
                        return Err(AppError::new(
                            ErrorCategory::ValidationError,
                            format!("barrier task '{}' has invalid params: {e}", barrier_task.id),
                        )
                        .with_code("WFG-BARRIER-001"));
                    }
                };

            // Check if all expected tasks are completed
            let all_expected_completed = barrier_params
                .expected
                .iter()
                .all(|task_id| completed_tasks.contains_key(task_id));

            if all_expected_completed && !barrier_params.expected.is_empty() {
                // All expected tasks are completed, enqueue the barrier
                self.ready_queue.push_back(barrier_task.id.clone());

                tracing::info!(
                    barrier_task_id = %barrier_task.id,
                    expected_count = barrier_params.expected.len(),
                    "barrier task ready: all expected tasks completed"
                );
            }
        }

        Ok(())
    }

    /// Print task stdout/stderr for verbose mode
    fn print_task_verbose_output(outcome: &TaskOutcome) {
        let output = &outcome.record.output;

        // For CommandOperator tasks, stdout/stderr are in the task output
        if let Value::Object(output_map) = output {
            if let Some(Value::String(stdout)) = output_map.get("stdout") {
                if !stdout.trim().is_empty() {
                    print!("{stdout}");
                }
            }
            if let Some(Value::String(stderr)) = output_map.get("stderr") {
                if !stderr.trim().is_empty() {
                    eprint!("{stderr}");
                }
            }
            // For AgentOperator tasks, print artifact paths instead
            if let Some(Value::String(artifact_path)) = output_map.get("stdout_artifact") {
                println!("stdout artifact: {artifact_path}");
            }
            if let Some(Value::String(artifact_path)) = output_map.get("stderr_artifact") {
                eprintln!("stderr artifact: {artifact_path}");
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

/// In-process runner for nested child workflow executions.
#[derive(Debug, Default)]
pub struct InProcessChildWorkflowRunner;

impl InProcessChildWorkflowRunner {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ChildWorkflowRunner for InProcessChildWorkflowRunner {
    async fn run(&self, input: ChildRunInput) -> Result<ChildWorkflowRunSummary, AppError> {
        let child_depth = input.parent_nesting_depth.saturating_add(1);
        let max_depth = input.execution_overrides.max_nesting_depth.unwrap_or(16);
        if child_depth > max_depth {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("nesting depth exceeded: requested {child_depth}, max {max_depth}"),
            )
            .with_code("WFG-NEST-002"));
        }

        let raw_document = schema::parse_workflow(&input.workflow_path)?;
        let mut document = transform::apply_default_pipeline(raw_document)?;

        if let Some(merge) = input.context_merge.as_ref() {
            document.workflow.context = shallow_merge_objects(&document.workflow.context, merge)?;
        }

        if let Some(merge) = input.triggers_merge.as_ref() {
            let current_payload = extract_trigger_payload(&document);
            let merged = shallow_merge_objects(&current_payload, merge)?;
            document.triggers = Some(schema::WorkflowTrigger {
                trigger_type: schema::TriggerType::Manual,
                schema_version: "1".to_string(),
                payload: merged,
            });
        }

        let lint_results = LintRegistry::new().run(&document);
        let error_count = lint_results
            .iter()
            .filter(|result| result.severity == LintSeverity::Error)
            .count();
        if error_count > 0 {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("workflow lint detected {error_count} error(s); fix before running"),
            ));
        }
        document.validate(&ExpressionEngine::default())?;

        let mut child_overrides = input.execution_overrides.clone();
        // Child executions should not notify `newton serve` by default.
        child_overrides.server_notifier = None;

        let parent_link = ParentRunLink {
            parent_execution_id: input.parent_execution_id,
            parent_task_id: input.parent_task_id.clone(),
            nesting_depth: child_depth,
        };
        let runtime = build_workflow_runtime_with_parent(
            document,
            input.workflow_path.clone(),
            input.operator_registry.clone(),
            input.workspace_root.clone(),
            child_overrides,
            Some(parent_link),
        )?;
        let workflow_file = canonicalize_workflow_path(&input.workflow_path)?
            .display()
            .to_string();
        let summary = runtime.run().await?;
        Ok(ChildWorkflowRunSummary {
            execution_id: summary.execution_id,
            workflow_file,
            total_iterations: summary.total_iterations,
            completed_task_count: summary.completed_tasks.len(),
        })
    }
}

fn build_workflow_runtime(
    document: WorkflowDocument,
    workflow_path: PathBuf,
    registry: OperatorRegistry,
    workspace_root: PathBuf,
    overrides: ExecutionOverrides,
) -> Result<WorkflowRuntime, AppError> {
    build_workflow_runtime_with_parent(
        document,
        workflow_path,
        registry,
        workspace_root,
        overrides,
        None,
    )
}

fn build_workflow_runtime_with_parent(
    document: WorkflowDocument,
    workflow_path: PathBuf,
    registry: OperatorRegistry,
    workspace_root: PathBuf,
    overrides: ExecutionOverrides,
    parent_link: Option<ParentRunLink>,
) -> Result<WorkflowRuntime, AppError> {
    let workflow_definition_json = serde_json::to_value(&document).map_err(|e| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("Failed to serialize workflow definition: {e}"),
        )
        .with_code("API-WORKFLOW-004")
    })?;
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
    let execution_overrides = overrides.clone();
    let checkpoint_root = overrides.checkpoint_base_path.as_ref().map_or_else(
        || {
            workspace_root
                .join(".newton")
                .join("state")
                .join("workflows")
        },
        |path| {
            if path.is_absolute() {
                path.clone()
            } else {
                workspace_root.join(path)
            }
        },
    );
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
    let eval_ctx = context::resolve_initial_evaluation_context(
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
                    included = context::evaluate_condition(guard, engine.as_ref(), &eval_ctx)?;
                }
                if included {
                    let mut task_clone = task.clone();
                    task_clone.transitions = task
                        .transitions
                        .iter()
                        .filter_map(|t| {
                            if let Some(ref g) = t.include_if {
                                match context::evaluate_condition(g, engine.as_ref(), &eval_ctx)
                                    .map_err(|e| e.with_code("WFG-GRAPH-001"))
                                {
                                    Ok(true) => Some(Ok(t.clone())),
                                    Ok(false) => None,
                                    Err(e) => Some(Err(e)),
                                }
                            } else {
                                Some(Ok(t.clone()))
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?;
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
    let runtime_graph = GraphHandle::new(tasks_map);

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
        parent_execution_id: parent_link.as_ref().map(|link| link.parent_execution_id),
        parent_task_id: parent_link.as_ref().map(|link| link.parent_task_id.clone()),
        nesting_depth: parent_link
            .as_ref()
            .map(|link| link.nesting_depth)
            .unwrap_or(0),
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
        workflow_file: workflow_file.clone(),
        checkpoint_root,
        registry,
        runtime_graph,
        engine,
        graph_settings: graph_settings.clone(),
        config,
        execution_overrides,
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
        current_tick_tasks: Vec::new(),
        server_notifier: overrides.server_notifier.clone(),
        workflow_definition_json: Some(workflow_definition_json),
        pre_seed_nodes: overrides.pre_seed_nodes,
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

    // GUARD: Detect old-format checkpoint where a hard task abort left the queue empty
    // but total_iterations exceeds completed task count — indicates an aborted task
    // was never re-queued (pre-fix checkpoint). Resuming would silently succeed with
    // no work done, so we fail fast with a clear error.
    if checkpoint_data.ready_queue.is_empty()
        && checkpoint_data.total_iterations > checkpoint_data.completed.len()
    {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "workflow cannot be resumed: {} tasks ran but only {} completed; \
                 the last task aborted without a transition. \
                 Re-run with a fresh execution or inspect the workflow.",
                checkpoint_data.total_iterations,
                checkpoint_data.completed.len()
            ),
        )
        .with_code("WFG-RESUME-002"));
    }

    let mut graph_settings = execution.settings_effective.clone();
    if allow_workflow_change {
        graph_settings.max_workflow_iterations = document.workflow.settings.max_workflow_iterations;
        graph_settings.max_task_iterations = document.workflow.settings.max_task_iterations;
    }

    let config = ExecutionConfig {
        parallel_limit: graph_settings.parallel_limit,
        max_time_seconds: graph_settings.max_time_seconds,
        continue_on_error: graph_settings.continue_on_error,
        max_task_iterations: graph_settings.max_task_iterations,
        max_workflow_iterations: graph_settings.max_workflow_iterations,
    };

    let runtime_graph = if checkpoint_data.version >= 2 && !allow_workflow_change {
        if let Some(runtime_tasks) = checkpoint_data.runtime_tasks {
            // Version 2+: Build runtime graph from checkpoint's runtime_tasks (unchanged workflow)
            let tasks_map: HashMap<String, WorkflowTask> = runtime_tasks
                .into_iter()
                .map(|task| (task.id.clone(), task))
                .collect();
            GraphHandle::new(tasks_map)
        } else {
            // Version 2+ but no runtime_tasks (fallback to document)
            GraphHandle::new(
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
            )
        }
    } else {
        // Version 1, or allow_workflow_change: use current workflow document (e.g. updated max_iterations)
        GraphHandle::new(
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
        )
    };

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
        workflow_file: workflow_path.clone(),
        checkpoint_root: workspace_root
            .join(".newton")
            .join("state")
            .join("workflows"),
        registry,
        runtime_graph,
        engine,
        graph_settings: graph_settings.clone(),
        config,
        execution_overrides: ExecutionOverrides {
            parallel_limit: None,
            max_time_seconds: None,
            checkpoint_base_path: None,
            artifact_base_path: None,
            max_nesting_depth: None,
            verbose: false,
            server_notifier: None,
            pre_seed_nodes: false,
        },
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
        current_tick_tasks: Vec::new(),
        server_notifier: None, // Resume does not support server notification
        workflow_definition_json: None,
        pre_seed_nodes: false,
    };
    runtime.run().await
}

fn extract_trigger_payload(document: &WorkflowDocument) -> Value {
    document.triggers.as_ref().map_or_else(
        || Value::Object(Map::new()),
        |trigger| trigger.payload.clone(),
    )
}

fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn shallow_merge_objects(base: &Value, overlay: &Value) -> Result<Value, AppError> {
    let overlay_obj = overlay.as_object().ok_or_else(|| {
        AppError::new(
            ErrorCategory::ValidationError,
            "merge value must be an object",
        )
    })?;
    let mut merged = base.as_object().cloned().ok_or_else(|| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "merge base must be a JSON object, got {}",
                json_type_name(base)
            ),
        )
        .with_code("WFG-NEST-005")
    })?;
    for (key, value) in overlay_obj {
        merged.insert(key.clone(), value.clone());
    }
    Ok(Value::Object(merged))
}

fn validate_required_triggers(required: &[String], payload: &Value) -> Result<(), AppError> {
    if required.is_empty() {
        return Ok(());
    }
    for key in required {
        if payload.as_object().and_then(|map| map.get(key)).is_none() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("trigger payload missing required key '{key}'"),
            )
            .with_code("WFG-TRIG-001"));
        }
    }
    Ok(())
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
                status: record.status,
                output,
                error_code: record.error.as_ref().map(|err| err.code.clone()),
                duration_ms,
                run_seq: record.run_seq as u64,
            },
        );
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::shallow_merge_objects;
    use serde_json::json;

    #[test]
    fn shallow_merge_non_object_base_returns_err() {
        let err = shallow_merge_objects(&json!("string"), &json!({}))
            .expect_err("non-object base must error");
        assert_eq!(err.code, "WFG-NEST-005");
    }
}
