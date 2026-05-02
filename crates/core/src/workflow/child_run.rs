//! Types for executing nested child workflows in-process.

use crate::core::error::AppError;
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use uuid::Uuid;

/// Input required to run a child workflow from within a parent workflow task.
#[derive(Clone)]
pub struct ChildRunInput {
    /// Resolved, sandboxed path to the child workflow file.
    pub workflow_path: PathBuf,
    /// Workspace root inherited from the parent workflow execution.
    pub workspace_root: PathBuf,
    /// Operator registry instance shared with the parent workflow.
    pub operator_registry: crate::workflow::operator::OperatorRegistry,
    /// Execution overrides inherited from the parent, with nesting metadata applied by the runner.
    pub execution_overrides: crate::workflow::executor::ExecutionOverrides,
    /// Optional shallow merge data applied onto the child workflow's context.
    pub context_merge: Option<Value>,
    /// Optional shallow merge data applied onto the child workflow's trigger payload.
    pub triggers_merge: Option<Value>,
    /// Parent workflow execution id for auditability.
    pub parent_execution_id: Uuid,
    /// Parent task id that spawned the child workflow.
    pub parent_task_id: String,
    /// Parent workflow nesting depth; child depth is `parent_nesting_depth + 1`.
    pub parent_nesting_depth: u32,
}

impl std::fmt::Debug for ChildRunInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildRunInput")
            .field("workflow_path", &self.workflow_path)
            .field("workspace_root", &self.workspace_root)
            .field("operator_registry", &"<operator_registry>")
            .field("execution_overrides", &self.execution_overrides)
            .field("context_merge", &self.context_merge)
            .field("triggers_merge", &self.triggers_merge)
            .field("parent_execution_id", &self.parent_execution_id)
            .field("parent_task_id", &self.parent_task_id)
            .field("parent_nesting_depth", &self.parent_nesting_depth)
            .finish()
    }
}

/// Summary returned to the parent task after the child workflow completes successfully.
#[derive(Debug, Clone)]
pub struct ChildWorkflowRunSummary {
    /// Child workflow execution id.
    pub execution_id: Uuid,
    /// Canonical path to the executed workflow file.
    pub workflow_file: String,
    /// Total task frontier iterations executed by the child workflow.
    pub total_iterations: usize,
    /// Number of tasks completed by the child workflow.
    pub completed_task_count: usize,
}

/// Runner responsible for executing a child workflow for [`crate::workflow::operators::workflow::WorkflowOperator`].
#[async_trait]
pub trait ChildWorkflowRunner: Send + Sync + 'static {
    /// Run the child workflow and return a summary on success.
    async fn run(&self, input: ChildRunInput) -> Result<ChildWorkflowRunSummary, AppError>;
}
