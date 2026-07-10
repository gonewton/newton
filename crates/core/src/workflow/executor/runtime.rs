#![allow(clippy::result_large_err)]
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use futures::future::join_all;
use newton_types::{NodeState, NodeStatus, WorkflowInstance, WorkflowStatus};
use serde_json::Value;

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::artifacts::ArtifactStore;
use crate::workflow::checkpoint;
use crate::workflow::expression::ExpressionEngine;
use crate::workflow::io::{evaluate_result_map, validate_output_schema};
use crate::workflow::operator::{OperatorRegistry, StateView};
use crate::workflow::schema::{
    self, BarrierParams, GoalGateFailureBehavior, TerminalKind, WorkflowTask,
};
use crate::workflow::state::{
    redact_value, TaskRunRecord, TaskStatus, WorkflowCheckpoint, WorkflowExecution,
    WorkflowExecutionStatus, WorkflowTaskRunSummary,
};
use crate::workflow::task_execution;
use crate::workflow::value_resolve as context;
use crate::workflow::workflow_sink::WorkflowSink;

use super::diagnosis;
use super::diagnosis::FailureDiagnosisInput::{Outcome, Record};
use super::graph_handle::GraphHandle;
use super::types::{ExecutionConfig, ExecutionOverrides, ExecutionState, ExecutionSummary};

pub(super) struct WorkflowRuntime {
    pub(super) workspace_root: PathBuf,
    pub(super) workflow_file: PathBuf,
    pub(super) checkpoint_root: PathBuf,
    pub(super) registry: OperatorRegistry,
    pub(super) runtime_graph: GraphHandle,
    pub(super) engine: Arc<ExpressionEngine>,
    pub(super) graph_settings: crate::workflow::state::GraphSettings,
    pub(super) config: ExecutionConfig,
    pub(super) execution_overrides: ExecutionOverrides,
    pub(super) artifact_store: ArtifactStore,
    pub(super) state: Arc<tokio::sync::RwLock<ExecutionState>>,
    pub(super) ready_queue: VecDeque<String>,
    pub(super) task_iterations: HashMap<String, usize>,
    pub(super) total_iterations: usize,
    pub(super) workflow_execution: WorkflowExecution,
    pub(super) triggers: Value,
    pub(super) redact_keys: Arc<Vec<String>>,
    pub(super) last_checkpoint: Instant,
    pub(super) start_time: Instant,
    pub(super) verbose: bool,
    pub(super) current_tick_tasks: Vec<String>,
    pub(super) sink: Option<Arc<dyn WorkflowSink>>,
    pub(super) workflow_definition_json: Option<serde_json::Value>,
    pub(super) pre_seed_nodes: bool,
}

impl WorkflowRuntime {
    pub(super) async fn fail_workflow(&mut self, err: AppError) -> Result<(), AppError> {
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
                let run_seq = *self.task_iterations.get(&task_id).ok_or_else(|| {
                    AppError::new(
                        ErrorCategory::InternalError,
                        format!("task '{task_id}' iteration count missing after increment"),
                    )
                })? as u64;
                tick_tasks.push((task_id.clone(), run_seq));
                self.current_tick_tasks.push(task_id);
            } else {
                break;
            }
        }
        Ok(tick_tasks)
    }

    async fn handle_terminal_tasks(
        &mut self,
        frontier: &[diagnosis::TaskOutcome],
    ) -> Result<bool, AppError> {
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

    fn notify_task_completions(&self, frontier: &[diagnosis::TaskOutcome]) {
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

    pub(super) async fn run(mut self) -> Result<ExecutionSummary, AppError> {
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
                let task = self.runtime_graph.get_task(&task_id).ok_or_else(|| {
                    AppError::new(
                        ErrorCategory::InternalError,
                        format!("task '{task_id}' not found in runtime graph during execution"),
                    )
                })?;
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

            let frontier_result: Result<Vec<diagnosis::TaskOutcome>, AppError> =
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
            .filter(|(_, r)| r.status == TaskStatus::Failed)
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
                let json = serde_json::to_string_pretty(&failure_envelope).map_err(|err| {
                    AppError::new(
                        ErrorCategory::SerializationError,
                        format!("failed to serialize failure completion envelope: {err}"),
                    )
                    .with_context(format!("original run failure: {e}"))
                })?;
                crate::fs_util::atomic_write(&completion_path, json.as_bytes()).map_err(|err| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!(
                            "failed to persist failure completion envelope {}: {}",
                            completion_path.display(),
                            err
                        ),
                    )
                    .with_code("WFG-COMPLETION-001")
                    .with_context(format!("original run failure: {e}"))
                })?;
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
            let json = serde_json::to_string_pretty(&envelope).map_err(|err| {
                AppError::new(
                    ErrorCategory::SerializationError,
                    format!("failed to serialize success completion envelope: {err}"),
                )
            })?;
            // A run whose result cannot be durably persisted must not be
            // reported as succeeded: "succeeded" only ever means the result
            // is actually on disk (spec 074, PR-3 / S1).
            crate::fs_util::atomic_write(&completion_path, json.as_bytes()).map_err(|err| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!(
                        "failed to persist success completion envelope {}: {}",
                        completion_path.display(),
                        err
                    ),
                )
                .with_code("WFG-COMPLETION-001")
            })?;
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
                    let passed = record.status == TaskStatus::Success;
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
                .any(|r| r.status == TaskStatus::Failed)
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

    async fn process_frontier(
        &mut self,
        frontier: Vec<diagnosis::TaskOutcome>,
    ) -> Result<(), AppError> {
        let mut guard = self.state.write().await;
        let mut failed_outcomes: Vec<&diagnosis::TaskOutcome> = Vec::new();
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
        // Fail the checkpoint outright on serialization failure instead of
        // silently recording `Null` — a swallowed failure here would make
        // resume think the workflow has no `io` block and silently drop
        // `result_map`/schemas (spec 074, B10).
        checkpoint.io_snapshot =
            Some(serde_json::to_value(&self.graph_settings.io).map_err(|e| {
                AppError::new(
                    ErrorCategory::SerializationError,
                    format!("failed to serialize io settings for checkpoint: {e}"),
                )
                .with_code("WFG-CKPT-004")
            })?);
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
