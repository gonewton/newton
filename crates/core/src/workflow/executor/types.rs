use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::workflow::operator::StateView;
use crate::workflow::state::{TaskRunRecord, WorkflowTaskRunRecord};
use crate::workflow::value_resolve as context;
use crate::workflow::workflow_sink::WorkflowSink;

#[derive(Clone, Debug, Default)]
pub struct ExecutionOverrides {
    pub parallel_limit: Option<usize>,
    pub max_time_seconds: Option<u64>,
    pub checkpoint_base_path: Option<PathBuf>,
    pub artifact_base_path: Option<PathBuf>,
    pub max_nesting_depth: Option<u32>,
    pub verbose: bool,
    pub sink: Option<Arc<dyn WorkflowSink>>,
    pub pre_seed_nodes: bool,
    /// Resolved state root, injected as `NEWTON_STATE_DIR` into operator
    /// subprocess environments so child `newton` invocations (e.g. `newton
    /// data get/post` shelled out from workflow YAML) resolve the same state
    /// root as the in-process executor (spec 074 decision 2: one state
    /// root).
    pub state_dir: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(super) struct ExecutionConfig {
    pub(super) parallel_limit: usize,
    pub(super) max_time_seconds: u64,
    pub(super) continue_on_error: bool,
    pub(super) max_task_iterations: usize,
    pub(super) max_workflow_iterations: usize,
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
pub(super) struct ParentRunLink {
    pub(super) parent_execution_id: Uuid,
    pub(super) parent_task_id: String,
    pub(super) nesting_depth: u32,
}

pub(super) struct ExecutionState {
    pub(super) context: Value,
    pub(super) completed: HashMap<String, TaskRunRecord>,
    pub(super) checkpoint_records: HashMap<String, WorkflowTaskRunRecord>,
    pub(super) triggers: Value,
}

impl ExecutionState {
    pub(super) fn snapshot(&self) -> StateView {
        StateView::new(
            self.context.clone(),
            context::build_tasks_value(&self.completed),
            self.triggers.clone(),
        )
    }
}
