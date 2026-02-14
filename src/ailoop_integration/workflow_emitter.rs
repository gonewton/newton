use crate::ailoop_integration::config::CommandContext;
use chrono::{DateTime, Utc};
use serde_json::json;
use uuid::Uuid;

const SCHEMA_VERSION: &str = "1.0";

/// Event kinds emitted by the orchestrator.
#[derive(Debug, Clone)]
pub enum WorkflowEventType {
    ExecutionStarted,
    IterationStarted,
    IterationCompleted,
    ExecutionFailed,
    ExecutionCompleted,
}

impl WorkflowEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowEventType::ExecutionStarted => "execution_started",
            WorkflowEventType::IterationStarted => "iteration_started",
            WorkflowEventType::IterationCompleted => "iteration_completed",
            WorkflowEventType::ExecutionFailed => "execution_failed",
            WorkflowEventType::ExecutionCompleted => "execution_completed",
        }
    }
}

/// Structured data representing an event emitted to ailoop.
#[derive(Debug, Clone)]
pub struct WorkflowEvent {
    pub event_type: WorkflowEventType,
    pub execution_id: Uuid,
    pub iteration_number: Option<usize>,
    pub phase: Option<String>,
    pub status: String,
    pub message: Option<String>,
    pub progress_percent: Option<u8>,
    pub timestamp: DateTime<Utc>,
    pub workspace_identifier: String,
    pub command_context: CommandContext,
}

impl WorkflowEvent {
    /// Build a JSON payload suitable for sending to ailoop.
    pub fn to_payload(&self) -> serde_json::Value {
        json!({
            "schema_version": SCHEMA_VERSION,
            "event_type": self.event_type.as_str(),
            "execution_id": self.execution_id.to_string(),
            "iteration_number": self.iteration_number,
            "phase": self.phase,
            "status": self.status,
            "message": self.message,
            "progress_percent": self.progress_percent,
            "timestamp": self.timestamp.to_rfc3339(),
            "workspace_identifier": self.workspace_identifier,
            "command_context": self.command_context,
        })
    }
}
