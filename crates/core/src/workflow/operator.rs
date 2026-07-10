#![allow(clippy::result_large_err)] // Operator trait and registry return AppError directly for structured diagnostics without boxing.

use crate::core::error::AppError;
use crate::workflow::executor::ExecutionOverrides;
use crate::workflow::executor::GraphHandle;
use crate::workflow::expression::EvaluationContext;
use async_trait::async_trait;
use schemars::Schema;
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
    pub graph: GraphHandle,
    /// Canonical path to the workflow file currently being executed.
    pub workflow_file: PathBuf,
    /// Workflow nesting depth (0 = root workflow).
    pub nesting_depth: u32,
    /// Execution overrides inherited from the workflow runner.
    pub execution_overrides: ExecutionOverrides,
    /// Operator registry used for the current workflow execution.
    pub operator_registry: OperatorRegistry,
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

    /// JSON Schema for the operator's params object.
    fn params_schema(&self) -> Schema;

    /// JSON Schema for the operator's output object.
    fn output_schema(&self) -> Schema;
}

/// Store-independent description of an operator: name, params schema, output
/// schema. See ADR-0014 (`docs/adr/0014-operator-descriptor-execution-split.md`).
///
/// Every operator contributes exactly one Descriptor to the registry,
/// unconditionally — regardless of whether the runtime dependencies needed to
/// actually *execute* it (e.g. a `BackendStore`) are wired. Anything that
/// *describes* the operator vocabulary (`newton schema export`,
/// `workflow.schema.json` / `output_schemas.json` generation, DSL codegen,
/// lint/validate) iterates Descriptors, never executable registrations, so an
/// operator can never silently vanish from the schema just because its
/// runtime deps are absent in the calling context.
#[derive(Debug, Clone)]
pub struct Descriptor {
    /// Operator name as it appears in workflow YAML `operator:` fields (and
    /// in the composed schema's `operator` enum). Matches [`Operator::name`]
    /// for the same operator.
    pub name: &'static str,
    /// JSON Schema for the operator's `params` object. Matches
    /// [`Operator::params_schema`] for the same operator — operators should
    /// derive both from one `schema_for!` call (e.g. via
    /// `Self::descriptor().params_schema` in the trait impl) so the two can
    /// never drift apart.
    pub params_schema: Schema,
    /// JSON Schema for the value the operator's `execute` resolves to.
    /// Matches [`Operator::output_schema`] for the same operator, for the
    /// same reason as `params_schema` above.
    pub output_schema: Schema,
}

impl Descriptor {
    /// Builds a Descriptor by calling `name()`/`params_schema()`/
    /// `output_schema()` on an already-constructed operator instance. Used
    /// by [`OperatorRegistryBuilder::register`] for operators with no gated
    /// runtime deps, where an executable instance is always available to
    /// derive the Descriptor from — see `register_descriptor` /
    /// `register_executable_only` for operators that need the two paths
    /// split (ADR-0014).
    pub fn from_operator<T: Operator>(operator: &T) -> Self {
        Self {
            name: operator.name(),
            params_schema: operator.params_schema(),
            output_schema: operator.output_schema(),
        }
    }
}

/// Builder used to register operators before execution.
#[derive(Default)]
pub struct OperatorRegistryBuilder {
    operators: HashMap<String, Arc<dyn Operator>>,
    descriptors: HashMap<String, Descriptor>,
}

impl OperatorRegistryBuilder {
    pub fn new() -> Self {
        Self {
            operators: HashMap::new(),
            descriptors: HashMap::new(),
        }
    }

    /// Register an operator's Descriptor and its executable instance together.
    /// This is the normal path for operators with no gated runtime deps: the
    /// two can never drift apart because both are derived from the same
    /// constructed instance.
    pub fn register<T: Operator>(&mut self, operator: T) -> &mut Self {
        let name = operator.name();
        if self.operators.contains_key(name) {
            panic!("duplicate operator registered: {name}");
        }
        self.register_descriptor(Descriptor::from_operator(&operator));
        self.operators.insert(name.to_string(), Arc::new(operator));
        self
    }

    /// Register a store-independent Descriptor with no executable instance.
    /// Used by operators whose runtime deps (e.g. `BackendStore`) may not be
    /// wired in the calling context (schema export, DSL codegen, lint) — the
    /// operator stays part of the described vocabulary even when it cannot
    /// execute here. Pair with `register_executable_only` when the deps ARE
    /// available. See ADR-0014.
    pub fn register_descriptor(&mut self, descriptor: Descriptor) -> &mut Self {
        if self.descriptors.contains_key(descriptor.name) {
            panic!(
                "duplicate operator descriptor registered: {}",
                descriptor.name
            );
        }
        self.descriptors
            .insert(descriptor.name.to_string(), descriptor);
        self
    }

    /// Register an executable operator instance whose Descriptor was already
    /// registered separately via `register_descriptor`. Used for operators
    /// with runtime deps gated behind a caller-supplied context: the
    /// Descriptor is always present; the executable half is conditional.
    pub fn register_executable_only<T: Operator>(&mut self, operator: T) -> &mut Self {
        let name = operator.name();
        if self.operators.contains_key(name) {
            panic!("duplicate operator registered: {name}");
        }
        debug_assert!(
            self.descriptors.contains_key(name),
            "operator '{name}' registered as executable without a prior Descriptor; \
             call register_descriptor first (see ADR-0014)"
        );
        self.operators.insert(name.to_string(), Arc::new(operator));
        self
    }

    pub fn build(self) -> OperatorRegistry {
        OperatorRegistry {
            operators: Arc::new(self.operators),
            descriptors: Arc::new(self.descriptors),
        }
    }
}

/// Immutable registry available during workflow execution.
#[derive(Clone)]
pub struct OperatorRegistry {
    operators: Arc<HashMap<String, Arc<dyn Operator>>>,
    descriptors: Arc<HashMap<String, Descriptor>>,
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

    /// Look up the executable operator instance. `None` when the name is
    /// entirely unknown OR when it is a described-but-unwired operator (see
    /// `is_described`) — callers that need to distinguish the two should
    /// check `is_described` for the clearer diagnostic.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Operator>> {
        self.operators.get(name).cloned()
    }

    pub fn list_operators(&self) -> Vec<Arc<dyn Operator>> {
        self.operators.values().cloned().collect()
    }

    pub fn operator_names(&self) -> Vec<String> {
        self.operators.keys().cloned().collect()
    }

    /// All Descriptors in the registry — the full described operator
    /// vocabulary, independent of whether each is executable in this
    /// context. This is what `newton schema export` and DSL codegen iterate.
    pub fn descriptors(&self) -> Vec<Descriptor> {
        self.descriptors.values().cloned().collect()
    }

    /// True when `name` is part of the described operator vocabulary,
    /// whether or not an executable instance is currently wired.
    pub fn is_described(&self, name: &str) -> bool {
        self.descriptors.contains_key(name)
    }
}
