use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::schema::{BarrierParams, WorkflowTask};

#[derive(Clone)]
pub struct GraphHandle(Arc<RwLock<HashMap<String, WorkflowTask>>>);

impl GraphHandle {
    pub fn new(tasks: HashMap<String, WorkflowTask>) -> Self {
        GraphHandle(Arc::new(RwLock::new(tasks)))
    }

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
            return Ok(());
        }

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

    pub fn add_tasks(
        &self,
        tasks: Vec<WorkflowTask>,
        _enqueue: bool,
        if_absent: bool,
        barrier_task_id: Option<&str>,
    ) -> Result<(), AppError> {
        let mut task_ids = Vec::new();

        for task in tasks {
            self.add_task(task.clone(), false, if_absent)?;
            task_ids.push(task.id);
        }

        if let Some(barrier_id) = barrier_task_id {
            self.register_barrier(barrier_id, &task_ids)?;
        }

        Ok(())
    }

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

        let mut barrier_params: BarrierParams = serde_json::from_value(barrier_task.params.clone())
            .unwrap_or_else(|_| BarrierParams { expected: vec![] });

        barrier_params.expected.extend_from_slice(expected_ids);

        barrier_task.params = serde_json::to_value(&barrier_params).map_err(|err| {
            AppError::new(
                ErrorCategory::SerializationError,
                format!("Failed to serialize barrier params: {err}"),
            )
        })?;

        Ok(())
    }

    pub fn get_task(&self, task_id: &str) -> Option<WorkflowTask> {
        let graph = self.0.read().unwrap();
        graph.get(task_id).cloned()
    }

    pub fn get_all_tasks(&self) -> Vec<WorkflowTask> {
        let graph = self.0.read().unwrap();
        graph.values().cloned().collect()
    }

    pub fn contains_task(&self, task_id: &str) -> bool {
        let graph = self.0.read().unwrap();
        graph.contains_key(task_id)
    }
}
