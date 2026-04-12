#![allow(clippy::result_large_err)] // Operator functions return AppError for rich diagnostics without boxing.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::child_run::{ChildRunInput, ChildWorkflowRunner};
use crate::workflow::operator::{ExecutionContext, Operator};
use crate::workflow::state::canonicalize_workflow_path;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

pub struct WorkflowOperator {
    runner: Arc<dyn ChildWorkflowRunner>,
}

impl WorkflowOperator {
    #[must_use]
    pub fn new(runner: Arc<dyn ChildWorkflowRunner>) -> Self {
        Self { runner }
    }
}

#[async_trait]
impl Operator for WorkflowOperator {
    fn name(&self) -> &'static str {
        "WorkflowOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let obj = params.as_object().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ValidationError,
                "WorkflowOperator params must be an object",
            )
        })?;

        let workflow_path = obj
            .get("workflow_path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if workflow_path.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "workflow_path parameter is required",
            )
            .with_code("WFG-NEST-003"));
        }

        validate_optional_object(obj, "context")?;
        validate_optional_object(obj, "triggers")?;
        Ok(())
    }

    async fn execute(&self, params: Value, ctx: ExecutionContext) -> Result<Value, AppError> {
        self.validate_params(&params)?;
        let obj = params.as_object().expect("validate_params ensures object");

        let workflow_path_str = obj
            .get("workflow_path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        let child_path = resolve_and_sandbox_child_path(
            &workflow_path_str,
            &ctx.workflow_file,
            &ctx.workspace_path,
        )?;

        let parent_execution_id = Uuid::parse_str(&ctx.execution_id).map_err(|_| {
            AppError::new(
                ErrorCategory::InternalError,
                "execution_id is not a valid UUID",
            )
        })?;

        let explicit_context = obj.get("context").cloned();
        let explicit_triggers = obj.get("triggers").cloned();

        let context_merge = Some(merge_objects_with_optional(
            &ctx.state_view.context,
            explicit_context.as_ref(),
        ));
        let triggers_merge = Some(merge_objects_with_optional(
            &ctx.state_view.triggers,
            explicit_triggers.as_ref(),
        ));

        let summary = self
            .runner
            .run(ChildRunInput {
                workflow_path: child_path,
                workspace_root: ctx.workspace_path.clone(),
                operator_registry: ctx.operator_registry.clone(),
                execution_overrides: ctx.execution_overrides.clone(),
                context_merge,
                triggers_merge,
                parent_execution_id,
                parent_task_id: ctx.task_id.clone(),
                parent_nesting_depth: ctx.nesting_depth,
            })
            .await?;

        Ok(json!({
            "child_execution_id": summary.execution_id.to_string(),
            "child_workflow_file": summary.workflow_file,
            "child_status": "Completed",
            "child_total_iterations": summary.total_iterations,
            "child_completed_task_count": summary.completed_task_count,
        }))
    }
}

fn validate_optional_object(
    map: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<(), AppError> {
    let value = map.get(key);
    if let Some(value) = value {
        if !value.is_object() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("{key} must be an object"),
            )
            .with_code("WFG-NEST-004"));
        }
    }
    Ok(())
}

fn merge_objects_with_optional(base: &Value, overlay: Option<&Value>) -> Value {
    let mut merged = base.as_object().cloned().unwrap_or_default();
    if let Some(overlay) = overlay.and_then(Value::as_object) {
        for (key, value) in overlay {
            merged.insert(key.clone(), value.clone());
        }
    }
    Value::Object(merged)
}

fn resolve_and_sandbox_child_path(
    workflow_path: &str,
    parent_workflow_file: &Path,
    workspace_root: &Path,
) -> Result<PathBuf, AppError> {
    let requested = PathBuf::from(workflow_path);
    let base_dir = parent_workflow_file.parent().ok_or_else(|| {
        AppError::new(
            ErrorCategory::InternalError,
            "parent workflow file has no parent directory",
        )
    })?;
    let resolved = if requested.is_absolute() {
        requested
    } else {
        base_dir.join(requested)
    };

    let canonical = canonicalize_workflow_path(&resolved)?;
    let workspace_canonical = workspace_root.canonicalize().map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!(
                "failed to canonicalize workspace root {}: {err}",
                workspace_root.display()
            ),
        )
    })?;

    if !canonical.starts_with(&workspace_canonical) {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "child workflow path {} is outside workspace sandbox",
                canonical.display()
            ),
        )
        .with_code("WFG-NEST-001"));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::child_run::ChildWorkflowRunSummary;
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[derive(Default)]
    struct NoopRunner;

    #[async_trait]
    impl ChildWorkflowRunner for NoopRunner {
        async fn run(&self, _input: ChildRunInput) -> Result<ChildWorkflowRunSummary, AppError> {
            Ok(ChildWorkflowRunSummary {
                execution_id: Uuid::new_v4(),
                workflow_file: "/tmp/child.yaml".to_string(),
                total_iterations: 1,
                completed_task_count: 1,
            })
        }
    }

    fn base_ctx(workspace: &TempDir, workflow_file: &Path) -> ExecutionContext {
        let state_view = crate::workflow::operator::StateView::new(json!({}), json!({}), json!({}));
        ExecutionContext {
            workspace_path: workspace.path().to_path_buf(),
            execution_id: Uuid::new_v4().to_string(),
            task_id: "task".to_string(),
            iteration: 1,
            state_view,
            graph: crate::workflow::executor::GraphHandle::new(HashMap::new()),
            workflow_file: workflow_file.to_path_buf(),
            nesting_depth: 0,
            execution_overrides: crate::workflow::executor::ExecutionOverrides {
                parallel_limit: None,
                max_time_seconds: None,
                checkpoint_base_path: None,
                artifact_base_path: None,
                max_nesting_depth: None,
                verbose: false,
                server_notifier: None,
                pre_seed_nodes: true,
            },
            operator_registry: crate::workflow::operator::OperatorRegistry::new(),
        }
    }

    #[test]
    fn validate_requires_workflow_path() {
        let op = WorkflowOperator::new(Arc::new(NoopRunner));
        let err = op
            .validate_params(&json!({}))
            .expect_err("missing workflow_path must error");
        assert_eq!(err.code, "WFG-NEST-003");
    }

    #[test]
    fn validate_rejects_non_object_context() {
        let op = WorkflowOperator::new(Arc::new(NoopRunner));
        let err = op
            .validate_params(&json!({"workflow_path": "child.yaml", "context": 1}))
            .expect_err("context scalar must error");
        assert_eq!(err.code, "WFG-NEST-004");
    }

    #[tokio::test]
    async fn execute_rejects_sandbox_escape() {
        let workspace = TempDir::new().expect("workspace");
        let parent_workflow_dir = workspace.path().join("workflows");
        std::fs::create_dir_all(&parent_workflow_dir).expect("mkdir");
        let parent_workflow_file = parent_workflow_dir.join("parent.yaml");
        std::fs::write(&parent_workflow_file, "version: \"2.0\"\nmode: workflow_graph\nworkflow:\n  settings:\n    entry_task: start\n  tasks: []\n").expect("write");

        let outside = workspace
            .path()
            .parent()
            .expect("workspace has parent")
            .join("outside.yaml");
        std::fs::write(&outside, "version: \"2.0\"\nmode: workflow_graph\nworkflow:\n  settings:\n    entry_task: start\n  tasks: []\n").expect("write outside");

        let op = WorkflowOperator::new(Arc::new(NoopRunner));
        let ctx = base_ctx(&workspace, &parent_workflow_file);
        let err = op
            .execute(json!({"workflow_path": "../../outside.yaml"}), ctx)
            .await
            .expect_err("sandbox escape must fail");
        assert_eq!(err.code, "WFG-NEST-001");
    }
}
