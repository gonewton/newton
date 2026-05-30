#![allow(clippy::result_large_err)]
use serde::Serialize;

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::artifacts::ArtifactStore;
use crate::workflow::checkpoint;
use crate::workflow::child_run::{ChildRunInput, ChildWorkflowRunSummary, ChildWorkflowRunner};
use crate::workflow::expression::ExpressionEngine;
use crate::workflow::io::{evaluate_result_map, validate_output_schema};
use crate::workflow::lint::{LintRegistry, LintSeverity};
use crate::workflow::operator::{OperatorRegistry, StateView};
use crate::workflow::schema::{
    self, BarrierParams, GoalGateFailureBehavior, TerminalKind, WorkflowDocument, WorkflowTask,
};
use crate::workflow::state::{
    canonicalize_workflow_path, compute_sha256_hex, redact_value, GraphSettings, TaskRunRecord,
    WorkflowCheckpoint, WorkflowExecution, WorkflowExecutionStatus, WorkflowTaskRunRecord,
    WorkflowTaskRunSummary, WORKFLOW_EXECUTION_FORMAT_VERSION,
};
use crate::workflow::task_execution;
use crate::workflow::transform;
use crate::workflow::value_resolve as context;
use crate::workflow::workflow_sink::WorkflowSink;
use async_trait::async_trait;
use chrono::Utc;
use futures::future::join_all;
use newton_types::{NodeState, NodeStatus, WorkflowInstance, WorkflowStatus};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing;
use uuid::Uuid;

mod diagnosis;
mod graph_handle;
mod helpers;

pub use crate::workflow::state::TaskStatus;
pub use diagnosis::TaskOutcome;
pub use graph_handle::GraphHandle;

use diagnosis::{FailureDiagnosisInput::Outcome, FailureDiagnosisInput::Record};
use helpers::{
    extract_trigger_payload, hydrate_completed_records, shallow_merge_objects,
    validate_required_triggers,
};

#[derive(Clone, Debug)]
pub struct ExecutionOverrides {
    pub parallel_limit: Option<usize>,
    pub max_time_seconds: Option<u64>,
    pub checkpoint_base_path: Option<PathBuf>,
    pub artifact_base_path: Option<PathBuf>,
    pub max_nesting_depth: Option<u32>,
    pub verbose: bool,
    pub sink: Option<Arc<dyn WorkflowSink>>,
    pub pre_seed_nodes: bool,
}

#[derive(Clone, Debug)]
pub struct ExecutionConfig {
    pub parallel_limit: usize,
    pub max_time_seconds: u64,
    pub continue_on_error: bool,
    pub max_task_iterations: usize,
    pub max_workflow_iterations: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionSummary {
    pub execution_id: Uuid,
    pub total_iterations: usize,
    pub completed_tasks: BTreeMap<String, TaskRunRecord>,
    pub result: Option<Value>,
    pub output_valid: bool,
}

#[derive(Debug, Clone)]
pub struct ParentRunLink {
    pub parent_execution_id: Uuid,
    pub parent_task_id: String,
    pub nesting_depth: u32,
}

pub struct ExecutionState {
    context: Value,
    completed: HashMap<String, TaskRunRecord>,
    checkpoint_records: HashMap<String, WorkflowTaskRunRecord>,
    triggers: Value,
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
    current_tick_tasks: Vec<String>,
    sink: Option<Arc<dyn WorkflowSink>>,
    workflow_definition_json: Option<serde_json::Value>,
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

impl WorkflowRuntime {
    async fn fail_workflow(&mut self, err: AppError) -> Result<(), AppError> {
        self.workflow_execution.status = WorkflowExecutionStatus::Failed;
        self.workflow_execution.completed_at = Some(Utc::now());
        self.persist_checkpoint_force().await?;
        self.notify_completion(WorkflowStatus::Failed);
        Err(err)
    }

    async fn check_timeout(&mut self) -> Result<(), AppError> {
        if self.start_time.elapsed().as_secs() >= self.config.max_time_seconds {
            return self
                .fail_workflow(
                    AppError::new(
                        ErrorCategory::TimeoutError,
                        "workflow exceeded max_time_seconds",
                    )
                    .with_code("WFG-TIME-001"),
                )
                .await;
        }
        Ok(())
    }

    async fn check_iteration_limits(&mut self, task_id: &str) -> Result<bool, AppError> {
        if self.total_iterations >= self.config.max_workflow_iterations {
            self.ready_queue.push_front(task_id.to_string());
            self.fail_workflow(
                AppError::new(
                    ErrorCategory::ValidationError,
                    "workflow exceeded max_workflow_iterations",
                )
                .with_code("WFG-ITER-001"),
            )
            .await?;
            unreachable!()
        }

        let limit = self
            .runtime_graph
            .get_task(task_id)
            .map_or(self.config.max_task_iterations, |task| {
                task.iteration_limit(self.config.max_task_iterations)
            });
        let entry = self.task_iterations.entry(task_id.to_string()).or_insert(0);
        if *entry >= limit {
            self.ready_queue.push_front(task_id.to_string());
            self.fail_workflow(
                AppError::new(
                    ErrorCategory::ValidationError,
                    format!("task {task_id} reached iteration cap"),
                )
                .with_code("WFG-ITER-002"),
            )
            .await?;
            unreachable!()
        }

        self.total_iterations += 1;
        *entry += 1;
        Ok(true)
    }

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

    async fn handle_terminal_tasks(&mut self, frontier: &[TaskOutcome]) -> Result<bool, AppError> {
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

        if !tick_terminal_ids.is_empty() {
            if tick_terminal_ids.len() > 1 {
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

    fn notify_task_starts(&self, tick_tasks: &[(String, u64)]) {
        if let Some(notifier) = &self.sink {
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

    fn notify_task_completions(&self, frontier: &[TaskOutcome]) {
        if let Some(notifier) = &self.sink {
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

    fn notify_completion(&self, status: WorkflowStatus) {
        if let Some(notifier) = &self.sink {
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

        let workflow_instance = WorkflowInstance {
            instance_id: self.workflow_execution.execution_id.to_string(),
            workflow_id: self.workflow_execution.workflow_file.clone(),
            status: WorkflowStatus::Running,
            nodes: self.build_preseed_nodes(),
            started_at: self.workflow_execution.started_at,
            ended_at: None,
            linked_plan_id: None,
            definition: self.workflow_definition_json.clone(),
        };

        if let Some(notifier) = &self.sink {
            notifier.notify_workflow_started(workflow_instance);
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
                    for task_id in self.current_tick_tasks.drain(..).rev() {
                        self.ready_queue.push_front(task_id);
                    }
                    self.fail_workflow(err).await?;
                    unreachable!()
                }
            };

            frontier.sort_by(|a, b| a.task_id.cmp(&b.task_id));

            let frontier_len = frontier.len();
            if let Err(err) = self.process_frontier(frontier.clone()).await {
                self.fail_workflow(err).await?;
            }

            self.notify_task_completions(&frontier);

            if self.handle_terminal_tasks(&frontier).await? {
                terminal_stop_triggered = true;
                break;
            }

            self.maybe_checkpoint(frontier_len).await?;
        }

        let final_state = self.state.read().await;
        let (final_exec_status, maybe_err) =
            self.compute_final_status(&final_state, terminal_stop_triggered);
        let mut final_failed_records: Vec<(String, TaskRunRecord)> = final_state
            .completed
            .iter()
            .filter(|(_, r)| r.status == crate::workflow::state::TaskStatus::Failed)
            .map(|(id, r)| (id.clone(), r.clone()))
            .collect();
        final_failed_records.sort_by(|a, b| a.0.cmp(&b.0));
        drop(final_state);

        self.workflow_execution.status = final_exec_status;
        self.workflow_execution.completed_at = Some(Utc::now());
        if let Some(err) = maybe_err {
            if err.code == "WFG-EXEC-001" && !final_failed_records.is_empty() {
                for (task_id, record) in &final_failed_records {
                    println!(
                        "newton: task failed execution_id={} task_id={} inspect: newton runs show {} --task {}",
                        self.workflow_execution.execution_id,
                        task_id,
                        self.workflow_execution.execution_id,
                        task_id
                    );
                    diagnosis::eprint_task_failure_diagnosis(
                        Record {
                            task_id: task_id.as_str(),
                            record,
                        },
                        false,
                    );
                }
            }
            let Err(e) = self.fail_workflow(err).await else {
                unreachable!()
            };
            if self.graph_settings.io.result_map.is_some() {
                let completion_path = self
                    .checkpoint_root
                    .join(self.workflow_execution.execution_id.to_string())
                    .join("completion.json");
                let failure_envelope = crate::workflow::io::CompletionEnvelope::failure(
                    Some(self.workflow_execution.execution_id),
                    crate::workflow::io::CompletionError {
                        code: if e.code.is_empty() {
                            None
                        } else {
                            Some(e.code.clone())
                        },
                        category: format!("{:?}", e.category),
                        message: e.message.clone(),
                        error_payload: None,
                    },
                );
                if let Ok(json) = serde_json::to_string_pretty(&failure_envelope) {
                    let _ = fs::write(&completion_path, json);
                }
            }
            return Err(e);
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

        let final_status = match self.workflow_execution.status {
            WorkflowExecutionStatus::Completed => WorkflowStatus::Succeeded,
            _ => WorkflowStatus::Failed,
        };
        self.notify_completion(final_status);

        let io = &self.graph_settings.io;
        let final_state_view = StateView::new(
            final_state.context.clone(),
            {
                let mut tasks_map = serde_json::Map::new();
                for (id, record) in &final_state.completed {
                    let result_val = record.output.get("result").cloned().unwrap_or(Value::Null);
                    tasks_map.insert(
                        id.clone(),
                        serde_json::json!({
                            "output": record.output,
                            "result": result_val,
                        }),
                    );
                }
                Value::Object(tasks_map)
            },
            final_state.triggers.clone(),
        );
        let result = if io.result_map.is_some() {
            match evaluate_result_map(io, &final_state_view, &self.engine) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("result_map evaluation failed: {}", e.message);
                    None
                }
            }
        } else {
            None
        };

        let output_valid =
            if let (Some(schema), Some(ref result_val)) = (&io.output_schema, &result) {
                match validate_output_schema(schema, result_val) {
                    Ok(()) => true,
                    Err(e) => {
                        tracing::warn!("output_schema validation failed: {}", e.message);
                        false
                    }
                }
            } else {
                true
            };

        if result.is_some() {
            let completion_path = self
                .checkpoint_root
                .join(self.workflow_execution.execution_id.to_string())
                .join("completion.json");
            let envelope = crate::workflow::io::CompletionEnvelope::success(
                self.workflow_execution.execution_id,
                result.clone(),
            );
            if let Ok(json) = serde_json::to_string_pretty(&envelope) {
                let _ = fs::write(&completion_path, json);
            }
        }

        Ok(ExecutionSummary {
            execution_id: self.workflow_execution.execution_id,
            total_iterations: self.total_iterations,
            completed_tasks,
            result,
            output_valid,
        })
    }

    fn compute_final_status(
        &self,
        state: &ExecutionState,
        _terminal_stop: bool,
    ) -> (WorkflowExecutionStatus, Option<AppError>) {
        let completion = &self.graph_settings.completion;

        let goal_gate_tasks: Vec<WorkflowTask> = self
            .runtime_graph
            .get_all_tasks()
            .into_iter()
            .filter(|t| t.goal_gate)
            .collect();

        if !goal_gate_tasks.is_empty() {
            let mut failing_gates: Vec<String> = Vec::new();

            for gate in &goal_gate_tasks {
                if let Some(record) = state.completed.get(&gate.id) {
                    let passed = record.status == crate::workflow::state::TaskStatus::Success;
                    if !passed
                        && completion.goal_gate_failure_behavior == GoalGateFailureBehavior::Fail
                    {
                        let status_str = record.status.as_str();
                        let entry = if let Some(group) = &gate.goal_gate_group {
                            format!("{}(group={})={}", gate.id, group, status_str)
                        } else {
                            format!("{}={}", gate.id, status_str)
                        };
                        failing_gates.push(entry);
                    }
                } else if completion.require_goal_gates {
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

        (WorkflowExecutionStatus::Completed, None)
    }

    async fn process_frontier(&mut self, frontier: Vec<TaskOutcome>) -> Result<(), AppError> {
        let mut guard = self.state.write().await;
        let mut failed_outcomes: Vec<&TaskOutcome> = Vec::new();
        for outcome in &frontier {
            guard
                .completed
                .insert(outcome.task_id.clone(), outcome.record.clone());
            if let Some(patch) = &outcome.context_patch {
                context::apply_patch(&mut guard.context, patch);
            }

            if self.verbose {
                diagnosis::print_task_verbose_output(outcome);
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
                failed_outcomes.push(outcome);
            }
        }
        if let Some(nested_error) = failed_outcomes.iter().find_map(|outcome| {
            outcome
                .error_summary
                .as_ref()
                .filter(|error| error.code == "WFG-NEST-005")
        }) {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                nested_error.message.clone(),
            )
            .with_code("WFG-NEST-005"));
        }
        if !failed_outcomes.is_empty() {
            let mut failed_task_ids: Vec<&str> = failed_outcomes
                .iter()
                .map(|outcome| outcome.task_id.as_str())
                .collect();
            failed_task_ids.sort_unstable();
            for task_id in &failed_task_ids {
                println!(
                    "newton: task failed execution_id={} task_id={} inspect: newton runs show {} --task {}",
                    self.workflow_execution.execution_id,
                    task_id,
                    self.workflow_execution.execution_id,
                    task_id
                );
                if let Some(outcome) = failed_outcomes
                    .iter()
                    .find(|o| o.task_id.as_str() == *task_id)
                {
                    diagnosis::eprint_task_failure_diagnosis(Outcome(outcome), self.verbose);
                }
            }
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("task {} failed", failed_task_ids[0]),
            )
            .with_code("WFG-EXEC-001"));
        }
        let snapshot = guard.snapshot();
        drop(guard);

        let mut seen = HashSet::new();
        for outcome in frontier {
            if let Some(task) = self.runtime_graph.get_task(&outcome.task_id) {
                let mut transitions = task.transitions.clone();
                transitions.sort_by_key(|t| t.priority);

                let has_conditional = transitions.iter().any(|t| t.when.is_some());
                self.evaluate_transitions(
                    &transitions,
                    &snapshot,
                    &mut seen,
                    &task.id,
                    has_conditional,
                )?;
            }
        }

        self.evaluate_barrier_tasks().await?;

        Ok(())
    }

    fn evaluate_transitions(
        &mut self,
        transitions: &[schema::Transition],
        snapshot: &StateView,
        seen: &mut HashSet<String>,
        task_id: &str,
        exclusive: bool,
    ) -> Result<(), AppError> {
        for transition in transitions {
            if context::evaluate_transition(transition, self.engine.as_ref(), snapshot)? {
                if !self.runtime_graph.contains_task(&transition.to) {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        format!(
                            "Task '{}' transition references non-existent task '{}' in runtime graph",
                            task_id, transition.to
                        ),
                    )
                    .with_code("WFG-DYN-003"));
                }
                if seen.insert(transition.to.clone()) {
                    self.ready_queue.push_back(transition.to.clone());
                }
                if exclusive {
                    break;
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
        let runtime_tasks = self.runtime_graph.get_all_tasks();
        let mut checkpoint = WorkflowCheckpoint::new_v2(
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
        checkpoint.io_snapshot =
            Some(serde_json::to_value(&self.graph_settings.io).unwrap_or(Value::Null));
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

    async fn evaluate_barrier_tasks(&mut self) -> Result<(), AppError> {
        let guard = self.state.read().await;
        let completed_tasks = &guard.completed;

        let all_tasks = self.runtime_graph.get_all_tasks();
        let barrier_tasks: Vec<WorkflowTask> = all_tasks
            .into_iter()
            .filter(|task| task.operator == "barrier")
            .collect();

        for barrier_task in barrier_tasks {
            if completed_tasks.contains_key(&barrier_task.id)
                || self.ready_queue.contains(&barrier_task.id)
            {
                continue;
            }

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

            let all_expected_completed = barrier_params
                .expected
                .iter()
                .all(|task_id| completed_tasks.contains_key(task_id));

            if all_expected_completed && !barrier_params.expected.is_empty() {
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
}

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
        child_overrides.sink = None;

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
            result: summary.result,
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
    let workflow_hash = {
        let json_bytes = serde_json::to_vec(&workflow_definition_json).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("failed to serialize workflow definition for hashing: {e}"),
            )
        })?;
        compute_sha256_hex(&json_bytes)
    };

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

    {
        let state_paths =
            checkpoint::WorkflowStatePaths::from_base(&checkpoint_root, &execution_uuid);
        if let Ok(snapshot_bytes) = serde_json::to_vec(&workflow_definition_json) {
            let _ =
                checkpoint::atomic_write(&state_paths.workflow_definition_file, &snapshot_bytes);
        }
    }

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
        sink: overrides.sink.clone(),
        workflow_definition_json: Some(workflow_definition_json),
        pre_seed_nodes: overrides.pre_seed_nodes,
    })
}

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
    let current_hash = {
        let doc_json = serde_json::to_value(&document).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("failed to serialize workflow definition for hash check: {e}"),
            )
        })?;
        let json_bytes = serde_json::to_vec(&doc_json).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("failed to serialize workflow definition for hash check: {e}"),
            )
        })?;
        compute_sha256_hex(&json_bytes)
    };
    if current_hash != execution.workflow_hash && !allow_workflow_change {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "workflow hash does not match checkpoint",
        )
        .with_code("WFG-CKPT-001"));
    }

    if let Some(io_snapshot) = &checkpoint_data.io_snapshot {
        let current_io =
            serde_json::to_value(&document.workflow.settings.io).unwrap_or(Value::Null);
        if io_snapshot != &current_io && !allow_workflow_change {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "workflow io block has changed since checkpoint was created",
            )
            .with_code("WFG-CKPT-001"));
        }
        if allow_workflow_change {
            if let Some(schema) = &document.workflow.settings.io.input_schema {
                let trigger_payload = &checkpoint_data.trigger_payload;
                use crate::workflow::io::validate_input_schema;
                validate_input_schema(schema, trigger_payload)?;
            }
        }
    }

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
            let tasks_map: HashMap<String, WorkflowTask> = runtime_tasks
                .into_iter()
                .map(|task| (task.id.clone(), task))
                .collect();
            GraphHandle::new(tasks_map)
        } else {
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
            sink: None,
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
        verbose: false,
        current_tick_tasks: Vec::new(),
        sink: None,
        workflow_definition_json: None,
        pre_seed_nodes: false,
    };
    runtime.run().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::state::{AppErrorSummary, TaskRunRecord, TaskStatus};
    use diagnosis::{
        tail_truncate_utf8, write_task_failure_diagnosis, FailureDiagnosisInput,
        FAILURE_DIAGNOSIS_STREAM_CAP_BYTES,
    };
    use serde_json::json;

    #[test]
    fn shallow_merge_non_object_base_returns_err() {
        let err = shallow_merge_objects(&json!("string"), &json!({}))
            .expect_err("non-object base must error");
        assert_eq!(err.code, "WFG-NEST-005");
    }

    fn make_failed_record(output: Value, error_code: Option<&str>) -> TaskRunRecord {
        TaskRunRecord {
            status: TaskStatus::Failed,
            output,
            error_code: error_code.map(str::to_string),
            duration_ms: 0,
            run_seq: 1,
        }
    }

    fn make_failed_outcome(
        task_id: &str,
        record: TaskRunRecord,
        summary: Option<AppErrorSummary>,
    ) -> TaskOutcome {
        let now = Utc::now();
        TaskOutcome {
            task_id: task_id.to_string(),
            record,
            context_patch: None,
            failed: true,
            started_at: now,
            completed_at: now,
            error_summary: summary,
            resolved_params: json!({}),
        }
    }

    fn diagnose_to_string(input: FailureDiagnosisInput<'_>, verbose: bool) -> String {
        let mut buf: Vec<u8> = Vec::new();
        write_task_failure_diagnosis(&mut buf, input, verbose).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn tail_truncate_utf8_under_cap_is_lossless() {
        let s = "hello";
        let (slice, len, trunc) = tail_truncate_utf8(s, 100);
        assert_eq!(slice, "hello");
        assert_eq!(len, 5);
        assert!(!trunc);
    }

    #[test]
    fn tail_truncate_utf8_ascii_returns_tail_at_exact_boundary() {
        let s = "abcdefghij";
        let (slice, len, trunc) = tail_truncate_utf8(s, 4);
        assert_eq!(len, 10);
        assert!(trunc);
        assert_eq!(slice, "ghij");
    }

    #[test]
    fn tail_truncate_utf8_never_splits_multibyte_codepoint() {
        let s: String = "é".repeat(10);
        assert_eq!(s.len(), 20);
        let (slice, len, trunc) = tail_truncate_utf8(&s, 5);
        assert_eq!(len, 20);
        assert!(trunc);
        assert!(slice.len() <= 5);
        assert!(slice.len() % 2 == 0);
        for ch in slice.chars() {
            assert_eq!(ch, 'é');
        }
    }

    #[test]
    fn diagnosis_uses_error_summary_when_present() {
        let rec = make_failed_record(json!({}), Some("WFG-EXEC-001"));
        let summary = AppErrorSummary {
            code: "WFG-EXEC-007".to_string(),
            category: "ValidationError".to_string(),
            message: "summary message".to_string(),
            context: std::collections::HashMap::new(),
        };
        let outcome = make_failed_outcome("t1", rec, Some(summary));
        let out = diagnose_to_string(FailureDiagnosisInput::Outcome(&outcome), false);
        assert!(out.contains("--- task failed: t1 ---"), "got: {out}");
        assert!(out.contains("code=WFG-EXEC-007"), "got: {out}");
        assert!(out.contains("message=summary message"), "got: {out}");
    }

    #[test]
    fn diagnosis_falls_back_to_record_error_code_and_output_message() {
        let rec = make_failed_record(
            json!({ "error": { "message": "from output" } }),
            Some("WFG-CMD-001"),
        );
        let out = diagnose_to_string(
            FailureDiagnosisInput::Record {
                task_id: "tx",
                record: &rec,
            },
            false,
        );
        assert!(out.contains("code=WFG-CMD-001"), "got: {out}");
        assert!(out.contains("message=from output"), "got: {out}");
    }

    #[test]
    fn diagnosis_emits_message_unavailable_when_no_source() {
        let rec = make_failed_record(json!({}), None);
        let out = diagnose_to_string(
            FailureDiagnosisInput::Record {
                task_id: "tx",
                record: &rec,
            },
            false,
        );
        assert!(out.contains("code=<unavailable>"), "got: {out}");
        assert!(out.contains("message=<unavailable>"), "got: {out}");
    }

    #[test]
    fn diagnosis_omits_empty_or_whitespace_streams() {
        let rec = make_failed_record(
            json!({
                "exit_code": 2,
                "stderr": "",
                "stdout": "   \n\n",
            }),
            Some("WFG-CMD-001"),
        );
        let out = diagnose_to_string(
            FailureDiagnosisInput::Record {
                task_id: "tx",
                record: &rec,
            },
            false,
        );
        assert!(out.contains("exit_code=2"), "got: {out}");
        assert!(!out.contains("--- stderr ("), "got: {out}");
        assert!(!out.contains("--- stdout ("), "got: {out}");
    }

    #[test]
    fn diagnosis_includes_command_streams_with_byte_headers() {
        let rec = make_failed_record(
            json!({
                "exit_code": 1,
                "stderr": "boom\n",
                "stdout": "ok-line",
            }),
            Some("WFG-CMD-001"),
        );
        let out = diagnose_to_string(
            FailureDiagnosisInput::Record {
                task_id: "tx",
                record: &rec,
            },
            false,
        );
        assert!(out.contains("exit_code=1"), "got: {out}");
        assert!(out.contains("--- stderr (5 bytes) ---"), "got: {out}");
        assert!(out.contains("boom"), "got: {out}");
        assert!(out.contains("--- stdout (7 bytes) ---"), "got: {out}");
        assert!(out.contains("ok-line"), "got: {out}");
    }

    #[test]
    fn diagnosis_truncates_oversized_stream_with_marker() {
        let big = "x".repeat(FAILURE_DIAGNOSIS_STREAM_CAP_BYTES + 100);
        let rec = make_failed_record(
            json!({ "exit_code": 1, "stderr": big.clone() }),
            Some("WFG-CMD-001"),
        );
        let out = diagnose_to_string(
            FailureDiagnosisInput::Record {
                task_id: "tx",
                record: &rec,
            },
            false,
        );
        assert!(
            out.contains(&format!(
                "truncated to {} bytes",
                FAILURE_DIAGNOSIS_STREAM_CAP_BYTES
            )),
            "got: {out}"
        );
        assert!(
            out.contains(&format!("({} bytes,", big.len())),
            "got: {out}"
        );
    }

    #[test]
    fn diagnosis_for_agent_output_prints_artifact_paths() {
        let rec = make_failed_record(
            json!({
                "stdout_artifact": "/tmp/agent.stdout",
                "stderr_artifact": "/tmp/agent.stderr",
            }),
            Some("WFG-AGENT-005"),
        );
        let out = diagnose_to_string(
            FailureDiagnosisInput::Record {
                task_id: "agent_t",
                record: &rec,
            },
            false,
        );
        assert!(
            out.contains("stderr artifact: /tmp/agent.stderr"),
            "got: {out}"
        );
        assert!(
            out.contains("stdout artifact: /tmp/agent.stdout"),
            "got: {out}"
        );
        assert!(!out.contains("--- stderr ("), "got: {out}");
        assert!(!out.contains("--- stdout ("), "got: {out}");
    }

    #[test]
    fn diagnosis_with_verbose_suppresses_stream_bodies_only() {
        let rec = make_failed_record(
            json!({
                "exit_code": 1,
                "stderr": "boom\n",
                "stdout": "ok\n",
                "stderr_artifact": "/a/err",
                "stdout_artifact": "/a/out",
            }),
            Some("WFG-CMD-001"),
        );
        let outcome = make_failed_outcome("tx", rec, None);
        let out = diagnose_to_string(FailureDiagnosisInput::Outcome(&outcome), true);
        assert!(out.contains("--- task failed: tx ---"), "got: {out}");
        assert!(out.contains("exit_code=1"), "got: {out}");
        assert!(out.contains("stderr artifact: /a/err"), "got: {out}");
        assert!(out.contains("stdout artifact: /a/out"), "got: {out}");
        assert!(!out.contains("--- stderr ("), "got: {out}");
        assert!(!out.contains("--- stdout ("), "got: {out}");
        assert!(!out.contains("boom"), "got: {out}");
    }
}
