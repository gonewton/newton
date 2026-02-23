#![allow(clippy::result_large_err)] // Workflow schema APIs return AppError to preserve structured validation context without boxing.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::expression::ExpressionEngine;
use crate::core::workflow_graph::transform;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

const SUPPORTED_VERSION: &str = "2.0";
const SUPPORTED_MODE: &str = "workflow_graph";

fn default_context_value() -> Value {
    Value::Object(Map::new())
}

fn default_params_value() -> Value {
    Value::Object(Map::new())
}

fn default_priority() -> i32 {
    100
}

fn default_classes() -> Vec<String> {
    Vec::new()
}

fn default_transitions() -> Vec<Transition> {
    Vec::new()
}

/// Root document for a workflow graph definition.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowDocument {
    pub version: String,
    pub mode: String,
    #[serde(default)]
    pub macros: Option<Vec<MacroDefinition>>,
    #[serde(default)]
    pub triggers: Option<WorkflowTrigger>,
    #[serde(default)]
    pub metadata: Option<WorkflowMetadata>,
    pub workflow: WorkflowDefinition,
}

/// Metadata embedded with a workflow document.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// Workflow-level configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowDefinition {
    #[serde(default = "default_context_value")]
    pub context: Value,
    pub settings: WorkflowSettings,
    pub tasks: Vec<TaskOrMacro>,
}

/// Execution settings for a workflow graph.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowSettings {
    pub entry_task: String,
    pub max_time_seconds: u64,
    pub parallel_limit: usize,
    pub continue_on_error: bool,
    pub max_task_iterations: usize,
    pub max_workflow_iterations: usize,
    #[serde(default)]
    pub artifact_storage: ArtifactStorageSettings,
    #[serde(default)]
    pub checkpoint: CheckpointSettings,
    #[serde(default)]
    pub redaction: RedactionSettings,
    #[serde(default = "default_command_operator_settings")]
    pub command_operator: CommandOperatorSettings,
    #[serde(default)]
    pub required_triggers: Vec<String>,
    #[serde(default)]
    pub human: HumanSettings,
    #[serde(default)]
    pub webhook: WebhookSettings,
    #[serde(default)]
    pub completion: CompletionSettings,
}

/// Terminal task kind — determines how the workflow outcome is affected by a terminal task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalKind {
    Success,
    Failure,
}

/// Controls whether a reached-but-failed goal gate causes the workflow to fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalGateFailureBehavior {
    #[default]
    Fail,
    Allow,
}

fn default_stop_on_terminal() -> bool {
    true
}

fn default_require_goal_gates() -> bool {
    true
}

fn default_success_requires_no_task_failures() -> bool {
    true
}

/// Completion policy configuration for workflow graphs.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompletionSettings {
    #[serde(default = "default_stop_on_terminal")]
    pub stop_on_terminal: bool,
    #[serde(default = "default_require_goal_gates")]
    pub require_goal_gates: bool,
    #[serde(default)]
    pub goal_gate_failure_behavior: GoalGateFailureBehavior,
    #[serde(default = "default_success_requires_no_task_failures")]
    pub success_requires_no_task_failures: bool,
}

impl Default for CompletionSettings {
    fn default() -> Self {
        Self {
            stop_on_terminal: true,
            require_goal_gates: true,
            goal_gate_failure_behavior: GoalGateFailureBehavior::Fail,
            success_requires_no_task_failures: true,
        }
    }
}

/// Artifact storage configuration embedded in workflow settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArtifactStorageSettings {
    pub base_path: PathBuf,
    pub max_inline_bytes: usize,
    pub max_artifact_bytes: usize,
    pub max_total_bytes: u64,
    pub retention_hours: u64,
    pub cleanup_policy: ArtifactCleanupPolicy,
}

/// Command operator specific settings embedded in workflow settings.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CommandOperatorSettings {
    pub allow_shell: bool,
}

fn default_command_operator_settings() -> CommandOperatorSettings {
    CommandOperatorSettings::default()
}

/// Human interaction configuration for workflows.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HumanSettings {
    pub default_timeout_seconds: u64,
    pub audit_path: PathBuf,
}

impl Default for HumanSettings {
    fn default() -> Self {
        Self {
            default_timeout_seconds: 86_400,
            audit_path: PathBuf::from(".newton/state/workflows"),
        }
    }
}

/// Webhook server configuration embedded in workflow settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebhookSettings {
    pub enabled: bool,
    pub bind: String,
    pub auth_token_env: String,
    pub max_body_bytes: usize,
}

impl Default for WebhookSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: "127.0.0.1:8787".to_string(),
            auth_token_env: "NEWTON_WEBHOOK_TOKEN".to_string(),
            max_body_bytes: 1_048_576,
        }
    }
}

/// Workflow trigger definition supporting manual and webhook workflows.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowTrigger {
    #[serde(rename = "type")]
    pub trigger_type: TriggerType,
    pub schema_version: String,
    #[serde(default = "default_trigger_payload")]
    pub payload: Value,
}

impl WorkflowTrigger {
    pub fn payload_object(&self) -> Option<&serde_json::Map<String, Value>> {
        self.payload.as_object()
    }

    pub fn to_value(&self) -> Value {
        let mut map = serde_json::Map::new();
        map.insert(
            "type".to_string(),
            Value::String(self.trigger_type.as_str().to_string()),
        );
        map.insert(
            "schema_version".to_string(),
            Value::String(self.schema_version.clone()),
        );
        map.insert("payload".to_string(), self.payload.clone());
        Value::Object(map)
    }
}

fn default_trigger_payload() -> Value {
    Value::Object(Map::new())
}

/// Allowed trigger types for workflow graphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TriggerType {
    Manual,
    Webhook,
}

impl TriggerType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TriggerType::Manual => "manual",
            TriggerType::Webhook => "webhook",
        }
    }
}

impl Default for ArtifactStorageSettings {
    fn default() -> Self {
        Self {
            base_path: PathBuf::from(".newton/artifacts"),
            max_inline_bytes: 65_536,
            max_artifact_bytes: 104_857_600,
            max_total_bytes: 1_073_741_824,
            retention_hours: 168,
            cleanup_policy: ArtifactCleanupPolicy::Lru,
        }
    }
}

/// Checkpointing configuration embedded in workflow settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CheckpointSettings {
    pub checkpoint_enabled: bool,
    pub checkpoint_interval_seconds: u64,
    pub checkpoint_on_task_complete: bool,
    pub checkpoint_keep_history: bool,
}

impl Default for CheckpointSettings {
    fn default() -> Self {
        Self {
            checkpoint_enabled: true,
            checkpoint_interval_seconds: 30,
            checkpoint_on_task_complete: true,
            checkpoint_keep_history: false,
        }
    }
}

/// Redaction configuration embedded in workflow settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RedactionSettings {
    #[serde(default = "default_redact_keys")]
    pub redact_keys: Vec<String>,
}

impl Default for RedactionSettings {
    fn default() -> Self {
        Self {
            redact_keys: default_redact_keys(),
        }
    }
}

fn default_redact_keys() -> Vec<String> {
    vec!["token".into(), "password".into(), "secret".into()]
}

/// Artifact cleanup policy.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactCleanupPolicy {
    #[default]
    Lru,
}

/// Task definition consumed by the workflow executor.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowTask {
    pub id: String,
    pub operator: String,
    #[serde(default = "default_params_value")]
    pub params: Value,
    pub name: Option<String>,
    #[serde(default = "default_classes")]
    pub classes: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub retry: Option<RetryPolicy>,
    pub max_iterations: Option<usize>,
    pub parallel_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_if: Option<Condition>,
    #[serde(default = "default_transitions")]
    pub transitions: Vec<Transition>,
    #[serde(default)]
    pub goal_gate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_gate_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalKind>,
}

impl WorkflowTask {
    /// Return the max iterations for this task, falling back to the global default.
    pub fn iteration_limit(&self, global: usize) -> usize {
        self.max_iterations.unwrap_or(global)
    }
}

/// Reusable macro definition containing one or more task templates.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MacroDefinition {
    pub name: String,
    pub tasks: Vec<WorkflowTask>,
}

/// Invocation of a named macro from the workflow task list.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MacroInvocation {
    #[serde(rename = "macro")]
    pub macro_name: String,
    #[serde(default)]
    pub with: Map<String, Value>,
}

/// Workflow task entries can be concrete tasks or macro invocations pre-transform.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[allow(clippy::large_enum_variant)] // Task variant intentionally carries full task payload pre-transform.
#[serde(untagged)]
pub enum TaskOrMacro {
    Task(WorkflowTask),
    Macro(MacroInvocation),
}

impl TaskOrMacro {
    pub fn as_task(&self) -> Option<&WorkflowTask> {
        match self {
            TaskOrMacro::Task(task) => Some(task),
            TaskOrMacro::Macro(_) => None,
        }
    }

    pub fn as_task_mut(&mut self) -> Option<&mut WorkflowTask> {
        match self {
            TaskOrMacro::Task(task) => Some(task),
            TaskOrMacro::Macro(_) => None,
        }
    }
}

impl WorkflowDefinition {
    pub fn tasks(&self) -> impl Iterator<Item = &WorkflowTask> {
        self.tasks.iter().filter_map(TaskOrMacro::as_task)
    }

    pub fn tasks_mut(&mut self) -> impl Iterator<Item = &mut WorkflowTask> {
        self.tasks.iter_mut().filter_map(TaskOrMacro::as_task_mut)
    }

    pub fn macro_invocation_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|item| matches!(item, TaskOrMacro::Macro(_)))
            .count()
    }

    pub fn macro_names_referenced(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .tasks
            .iter()
            .filter_map(|item| match item {
                TaskOrMacro::Macro(invocation) => Some(invocation.macro_name.clone()),
                TaskOrMacro::Task(_) => None,
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

/// Retry configuration for a task.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RetryPolicy {
    pub max_attempts: usize,
    pub backoff_ms: u64,
    #[serde(default)]
    pub backoff_multiplier: Option<f32>,
    #[serde(default)]
    pub jitter_ms: Option<u64>,
}

impl RetryPolicy {
    /// Ensure the retry policy is sane.
    pub fn validate(&self) -> Result<(), AppError> {
        if self.max_attempts == 0 {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "retry.max_attempts must be >= 1",
            ));
        }
        Ok(())
    }
}

/// Transition between tasks.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Transition {
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_if: Option<Condition>,
    #[serde(default)]
    pub when: Option<Condition>,
    #[serde(default = "default_priority")]
    pub priority: i32,
    pub label: Option<String>,
}

/// Condition used to guard transitions between tasks.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Condition {
    Expr {
        #[serde(rename = "$expr")]
        expr: String,
    },
    Bool(bool),
}

impl Condition {
    pub fn expression(&self) -> Option<&str> {
        match self {
            Condition::Expr { expr } => Some(expr.as_str()),
            Condition::Bool(_) => None,
        }
    }
}

impl WorkflowDocument {
    /// Parse a workflow document from a YAML file without semantic validation.
    pub fn parse_from_file(path: &Path) -> Result<Self, AppError> {
        let text = fs::read_to_string(path).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to read {}: {}", path.display(), err),
            )
        })?;
        serde_yaml::from_str(&text).map_err(|err| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("failed to parse {}: {}", path.display(), err),
            )
        })
    }

    /// Load and validate a workflow document from a YAML file.
    pub fn load_from_file(path: &Path) -> Result<Self, AppError> {
        let raw = Self::parse_from_file(path)?;
        let doc = transform::apply_default_pipeline(raw)?;
        let engine = ExpressionEngine::default();
        doc.validate(&engine)?;
        Ok(doc)
    }

    /// Validate the workflow document against schema requirements.
    pub fn validate(&self, engine: &ExpressionEngine) -> Result<(), AppError> {
        if self.version != SUPPORTED_VERSION {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!(
                    "unsupported workflow version {}, expected {}",
                    self.version, SUPPORTED_VERSION
                ),
            ));
        }
        if self.mode != SUPPORTED_MODE {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!(
                    "workflow mode must be {}, got {}",
                    SUPPORTED_MODE, self.mode
                ),
            ));
        }

        if self.workflow.tasks().next().is_none() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "workflow must define at least one task",
            ));
        }

        let mut ids = HashSet::new();
        for item in &self.workflow.tasks {
            let task = match item {
                TaskOrMacro::Task(task) => task,
                TaskOrMacro::Macro(invocation) => {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        format!(
                            "unexpanded macro invocation '{}' found during validation",
                            invocation.macro_name
                        ),
                    )
                    .with_code("WFG-MACRO-002"));
                }
            };
            if !ids.insert(task.id.clone()) {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("duplicate task id: {}", task.id),
                ));
            }
            if task.operator.trim().is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("task {} has empty operator", task.id),
                ));
            }
            if let Some(retry) = &task.retry {
                retry.validate()?;
            }
        }

        if !ids.contains(&self.workflow.settings.entry_task) {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!(
                    "entry_task '{}' is not present in workflow tasks",
                    self.workflow.settings.entry_task
                ),
            ));
        }

        if self.workflow.settings.parallel_limit == 0 {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "settings.parallel_limit must be >= 1",
            ));
        }
        if self.workflow.settings.max_task_iterations == 0 {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "settings.max_task_iterations must be >= 1",
            ));
        }
        if self.workflow.settings.max_workflow_iterations == 0 {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "settings.max_workflow_iterations must be >= 1",
            ));
        }
        if self.workflow.settings.max_time_seconds == 0 {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "settings.max_time_seconds must be >= 1",
            ));
        }

        if let Some(triggers) = &self.triggers {
            if triggers.schema_version.trim().is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "triggers.schema_version must be set",
                ));
            }
            if !triggers.payload.is_object() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "triggers.payload must be an object",
                ));
            }
        }

        let mut exprs = Vec::new();
        collect_expression_strings(&self.workflow.context, &mut exprs);
        for task in self.workflow.tasks() {
            collect_expression_strings(&task.params, &mut exprs);
            if let Some(include_if) = &task.include_if {
                if let Some(expr) = include_if.expression() {
                    exprs.push(expr.to_string());
                }
            }
            for transition in &task.transitions {
                if !ids.contains(&transition.to) {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        format!("transition 'to' references unknown task: {}", transition.to),
                    ));
                }
                if let Some(include_if) = &transition.include_if {
                    if let Some(expr) = include_if.expression() {
                        exprs.push(expr.to_string());
                    }
                }
                if let Some(condition) = &transition.when {
                    if let Some(expr) = condition.expression() {
                        exprs.push(expr.to_string());
                    }
                }
            }
        }

        for expr in exprs {
            engine.compile(&expr)?;
        }

        Ok(())
    }
}

fn collect_expression_strings(value: &Value, expressions: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if map.len() == 1 && map.contains_key("$expr") {
                if let Some(Value::String(expr)) = map.get("$expr") {
                    expressions.push(expr.clone());
                    return;
                }
            }
            for (_key, child) in map {
                collect_expression_strings(child, expressions);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_expression_strings(item, expressions);
            }
        }
        _ => {}
    }
}

pub fn load_workflow(path: &Path) -> Result<WorkflowDocument, AppError> {
    WorkflowDocument::load_from_file(path)
}

pub fn parse_workflow(path: &Path) -> Result<WorkflowDocument, AppError> {
    WorkflowDocument::parse_from_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collects_exprs_from_nested_values() {
        let value = json!({
            "foo": {"$expr": "1 + 2"},
            "bar": [
                {"baz": {"$expr": "true"}},
                42
            ]
        });
        let mut exprs = Vec::new();
        collect_expression_strings(&value, &mut exprs);
        assert_eq!(exprs.len(), 2);
    }
}
