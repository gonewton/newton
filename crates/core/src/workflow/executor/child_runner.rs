#![allow(clippy::result_large_err)]
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::artifacts::ArtifactStore;
use crate::workflow::checkpoint;
use crate::workflow::child_run::{ChildRunInput, ChildWorkflowRunSummary, ChildWorkflowRunner};
use crate::workflow::expression::ExpressionEngine;
use crate::workflow::schema::{self, WorkflowDocument, WorkflowTask};
use crate::workflow::state::{
    canonicalize_workflow_path, compute_sha256_hex, WorkflowExecution, WorkflowExecutionStatus,
    WORKFLOW_EXECUTION_FORMAT_VERSION,
};
use crate::workflow::transform;
use crate::workflow::value_resolve as context;

use super::graph_handle::GraphHandle;
use super::helpers::{
    extract_trigger_payload, hydrate_completed_records, shallow_merge_objects,
    validate_required_triggers,
};
use super::runtime::WorkflowRuntime;
use super::types::{
    ExecutionConfig, ExecutionOverrides, ExecutionState, ExecutionSummary, ParentRunLink,
};

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
            document.triggers = Some(schema::WorkflowTrigger::manual(merged));
        }

        crate::workflow::loader::check_lint_errors_after_run(&document)?;

        let mut child_overrides = input.execution_overrides.clone();
        child_overrides.sink = None;

        let parent_link = ParentRunLink {
            parent_execution_id: input.parent_execution_id,
            parent_task_id: input.parent_task_id.clone(),
            nesting_depth: child_depth,
        };
        let runtime = build_workflow_runtime(
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

pub(super) fn build_workflow_runtime(
    document: WorkflowDocument,
    workflow_path: PathBuf,
    registry: crate::workflow::operator::OperatorRegistry,
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

    let engine = Arc::new(ExpressionEngine::new(graph_settings.allow_env_fn));
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
        terminal_stop: false,
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

fn tasks_to_graph(tasks: Vec<schema::TaskOrMacro>) -> Result<GraphHandle, AppError> {
    let map = tasks
        .into_iter()
        .map(|item| match item {
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
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok(GraphHandle::new(map))
}

/// Resumes a checkpointed workflow execution.
///
/// `overrides` MUST be the same `ExecutionOverrides` the caller would pass to
/// `execute_workflow`/`build_workflow_runtime` for a fresh run against the same
/// state root (spec 074, P6 — resume parity with run): `checkpoint_base_path`
/// and `artifact_base_path` relocate the resumed run's checkpoint/artifact
/// writes onto the resolved `--state-dir` root instead of silently falling back
/// to `<workspace>/.newton/state/workflows`; `sink` re-wires the resumed run to
/// the same `DbSink`/`ServerNotifier` fan-out `run` uses (without it, a resumed
/// grading workflow's writes never reach the backend store); `verbose` and
/// `parallel_limit`/`max_time_seconds` propagate the same way they do for `run`.
/// Pass `ExecutionOverrides::default()` only when the caller genuinely wants the
/// workspace-default state root and no sink (e.g. most unit tests below).
pub async fn resume_workflow(
    registry: crate::workflow::operator::OperatorRegistry,
    workspace_root: PathBuf,
    execution_id: Uuid,
    allow_workflow_change: bool,
    overrides: ExecutionOverrides,
) -> Result<ExecutionSummary, AppError> {
    // Same fallback as `build_workflow_runtime`: an explicit
    // `checkpoint_base_path` (from `--state-dir`) relocates the checkpoint
    // root; otherwise fall back to the workspace-default tree. This MUST be
    // resolved before the initial checkpoint load below — a caller-supplied
    // `--state-dir` override was previously silently ignored here (spec 074,
    // P6): the checkpoint was always re-read from `<workspace_root>/.newton/
    // state/workflows`, split-brained against wherever the run actually
    // checkpointed if `--state-dir` had been used for the original `run`.
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

    let execution = checkpoint::load_execution_from_base(&checkpoint_root, &execution_id)?;
    let checkpoint_data = checkpoint::load_checkpoint_from_base(&checkpoint_root, &execution_id)?;
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

    // A workflow whose definition declares an `io` block (input/output
    // schemas, result_map, error_schema) but whose checkpoint's io_snapshot
    // is null/absent is a corrupted-or-stale checkpoint: resuming it would
    // silently skip the io-contract comparison below instead of enforcing
    // it (spec 074, B10). Note this is deliberately narrower than "any empty
    // snapshot" — an *explicit* `{}` io_snapshot (a workflow whose io block
    // was genuinely empty at checkpoint time) still falls through to the
    // ordinary mismatch check below, which already reports a precise
    // WFG-CKPT-001 "io block has changed" error for that case.
    let workflow_has_io = !document.workflow.settings.io.is_empty();
    let snapshot_is_null_or_absent =
        matches!(&checkpoint_data.io_snapshot, None | Some(Value::Null));
    if workflow_has_io && snapshot_is_null_or_absent {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "checkpoint has no io_snapshot but the workflow definition declares an io block \
             (input_schema/output_schema/result_map/error_schema); resuming would silently \
             skip validating that contract. The checkpoint may predate the io block or be \
             corrupted — start a fresh execution instead of resuming.",
        )
        .with_code("WFG-CKPT-003"));
    }

    if let Some(io_snapshot) = &checkpoint_data.io_snapshot {
        let current_io = serde_json::to_value(&document.workflow.settings.io).map_err(|e| {
            AppError::new(
                ErrorCategory::SerializationError,
                format!("failed to serialize workflow io settings for checkpoint comparison: {e}"),
            )
            .with_code("WFG-CKPT-001")
        })?;
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
    // Parity with `build_workflow_runtime` (spec 074, P6): a caller-supplied
    // override wins over the checkpointed settings, exactly like `run`.
    if let Some(parallel) = overrides.parallel_limit {
        graph_settings.parallel_limit = parallel;
    }
    if let Some(max_time) = overrides.max_time_seconds {
        graph_settings.max_time_seconds = max_time;
    }
    if let Some(artifact_base_path) = &overrides.artifact_base_path {
        graph_settings.artifact_storage.base_path = artifact_base_path.clone();
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
            tasks_to_graph(document.workflow.tasks)?
        }
    } else {
        tasks_to_graph(document.workflow.tasks)?
    };

    let engine = Arc::new(ExpressionEngine::new(graph_settings.allow_env_fn));
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
        checkpoint_root,
        registry,
        runtime_graph,
        engine,
        graph_settings: graph_settings.clone(),
        config,
        execution_overrides: overrides.clone(),
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
        verbose: overrides.verbose,
        current_tick_tasks: Vec::new(),
        sink: overrides.sink.clone(),
        workflow_definition_json: None,
        pre_seed_nodes: false,
    };
    runtime.run().await
}
