#![allow(clippy::result_large_err)] // Workflow schema APIs return AppError to preserve structured validation context without boxing.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::expression::ExpressionEngine;
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
    pub tasks: Vec<WorkflowTask>,
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
    #[serde(default = "default_transitions")]
    pub transitions: Vec<Transition>,
}

impl WorkflowTask {
    /// Return the max iterations for this task, falling back to the global default.
    pub fn iteration_limit(&self, global: usize) -> usize {
        self.max_iterations.unwrap_or(global)
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
    /// Load and validate a workflow document from a YAML file.
    pub fn load_from_file(path: &Path) -> Result<Self, AppError> {
        let text = fs::read_to_string(path).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to read {}: {}", path.display(), err),
            )
        })?;
        let doc: WorkflowDocument = serde_yaml::from_str(&text).map_err(|err| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("failed to parse {}: {}", path.display(), err),
            )
        })?;
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

        if self.workflow.tasks.is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "workflow must define at least one task",
            ));
        }

        let mut ids = HashSet::new();
        for task in &self.workflow.tasks {
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

        let mut exprs = Vec::new();
        collect_expression_strings(&self.workflow.context, &mut exprs);
        for task in &self.workflow.tasks {
            collect_expression_strings(&task.params, &mut exprs);
            for transition in &task.transitions {
                if !ids.contains(&transition.to) {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        format!("transition 'to' references unknown task: {}", transition.to),
                    ));
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
