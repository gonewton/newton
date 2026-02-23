#![allow(clippy::result_large_err)] // Operator trait and registry return AppError directly for structured diagnostics without boxing.

use crate::core::error::AppError;
use crate::core::workflow_graph::expression::EvaluationContext;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Snapshot of global state exposed to operators.
#[derive(Clone)]
pub struct StateView {
    pub context: Value,
    pub tasks: Value,
    pub triggers: Value,
}

impl StateView {
    pub fn new(context: Value, tasks: Value, triggers: Value) -> Self {
        Self {
            context,
            tasks,
            triggers,
        }
    }

    pub fn evaluation_context(&self) -> EvaluationContext {
        EvaluationContext::new(
            self.context.clone(),
            self.tasks.clone(),
            self.triggers.clone(),
        )
    }
}

/// Execution context provided to each operator run.
#[derive(Clone)]
pub struct ExecutionContext {
    pub workspace_path: PathBuf,
    pub execution_id: String,
    pub task_id: String,
    pub iteration: u64,
    pub state_view: StateView,
}

/// Trait implemented by workflow graph operators.
#[async_trait]
pub trait Operator: Send + Sync + 'static {
    /// Operator name used in workflow definitions.
    fn name(&self) -> &'static str;

    /// Validate params ahead of execution.
    fn validate_params(&self, params: &Value) -> Result<(), AppError>;

    /// Execute the operator with resolved params.
    async fn execute(&self, params: Value, ctx: ExecutionContext) -> Result<Value, AppError>;
}

/// Builder used to register operators before execution.
pub struct OperatorRegistryBuilder {
    operators: HashMap<String, Arc<dyn Operator>>,
}

impl Default for OperatorRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl OperatorRegistryBuilder {
    pub fn new() -> Self {
        Self {
            operators: HashMap::new(),
        }
    }

    pub fn register<T: Operator>(&mut self, operator: T) -> &mut Self {
        let name = operator.name();
        if self.operators.contains_key(name) {
            panic!("duplicate operator registered: {}", name);
        }
        self.operators.insert(name.to_string(), Arc::new(operator));
        self
    }

    pub fn build(self) -> OperatorRegistry {
        OperatorRegistry {
            inner: Arc::new(self.operators),
        }
    }
}

/// Immutable registry available during workflow execution.
#[derive(Clone)]
pub struct OperatorRegistry {
    inner: Arc<HashMap<String, Arc<dyn Operator>>>,
}

impl Default for OperatorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl OperatorRegistry {
    pub fn new() -> Self {
        OperatorRegistryBuilder::new().build()
    }

    pub fn builder() -> OperatorRegistryBuilder {
        OperatorRegistryBuilder::new()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Operator>> {
        self.inner.get(name).cloned()
    }
}
