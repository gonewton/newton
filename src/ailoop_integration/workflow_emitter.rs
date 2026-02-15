use crate::ailoop_integration::AiloopContext;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

const WORKFLOW_SCHEMA_VERSION: &str = "1.0.0";

/// Workflow progress event with phase and status information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEvent {
    /// Execution identifier.
    pub execution_id: Uuid,
    /// Phase name (e.g., "evaluator", "advisor", "executor").
    pub phase: String,
    /// Status of the phase (e.g., "started", "completed", "failed").
    pub status: String,
    /// Optional progress percentage (0-100).
    pub progress: Option<u8>,
    /// Optional message providing context.
    pub message: Option<String>,
    /// Timestamp of the event.
    pub timestamp: DateTime<Utc>,
    /// Schema version for forward compatibility.
    pub schema_version: String,
}

impl WorkflowEvent {
    /// Validate that progress is in valid range if present.
    fn validate(&self) -> Result<(), ValidationError> {
        if let Some(progress) = self.progress {
            if progress > 100 {
                return Err(ValidationError::InvalidProgress(progress));
            }
        }
        Ok(())
    }
}

/// Emitter for workflow progress events.
#[derive(Clone)]
pub struct WorkflowEmitter {
    #[allow(dead_code)]
    context: Arc<AiloopContext>,
    event_tx: mpsc::UnboundedSender<WorkflowEvent>,
}

impl WorkflowEmitter {
    /// Create a new workflow emitter.
    /// Spawns a background task to handle event emission.
    pub fn new(context: Arc<AiloopContext>) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let sender_context = context.clone();
        tokio::spawn(async move {
            Self::emitter_loop(sender_context, event_rx).await;
        });

        Self { context, event_tx }
    }

    /// Emit a workflow event.
    pub fn emit(
        &self,
        execution_id: Uuid,
        phase: String,
        status: String,
        progress: Option<u8>,
        message: Option<String>,
    ) -> Result<(), EmitError> {
        let event = WorkflowEvent {
            execution_id,
            phase,
            status,
            progress,
            message,
            timestamp: Utc::now(),
            schema_version: WORKFLOW_SCHEMA_VERSION.to_string(),
        };

        // Validate before sending
        event
            .validate()
            .map_err(|e| EmitError::ValidationError(e.to_string()))?;

        self.event_tx.send(event).map_err(|_| EmitError::QueueFull)
    }

    /// Emit a phase started event.
    pub fn phase_started(&self, execution_id: Uuid, phase: String) -> Result<(), EmitError> {
        self.emit(execution_id, phase, "started".to_string(), Some(0), None)
    }

    /// Emit a phase progress event.
    pub fn phase_progress(
        &self,
        execution_id: Uuid,
        phase: String,
        progress: u8,
        message: Option<String>,
    ) -> Result<(), EmitError> {
        self.emit(
            execution_id,
            phase,
            "in_progress".to_string(),
            Some(progress),
            message,
        )
    }

    /// Emit a phase completed event.
    pub fn phase_completed(&self, execution_id: Uuid, phase: String) -> Result<(), EmitError> {
        self.emit(
            execution_id,
            phase,
            "completed".to_string(),
            Some(100),
            None,
        )
    }

    /// Emit a phase failed event.
    pub fn phase_failed(
        &self,
        execution_id: Uuid,
        phase: String,
        message: String,
    ) -> Result<(), EmitError> {
        self.emit(
            execution_id,
            phase,
            "failed".to_string(),
            None,
            Some(message),
        )
    }

    /// Background task loop that emits events to ailoop.
    async fn emitter_loop(
        context: Arc<AiloopContext>,
        mut event_rx: mpsc::UnboundedReceiver<WorkflowEvent>,
    ) {
        while let Some(event) = event_rx.recv().await {
            if let Err(e) = Self::emit_event_once(&context, &event).await {
                tracing::warn!(
                    event = ?event,
                    error = %e,
                    "Failed to emit workflow event to ailoop"
                );
            }
        }
    }

    /// Emit a single event to ailoop HTTP endpoint.
    async fn emit_event_once(
        context: &AiloopContext,
        event: &WorkflowEvent,
    ) -> Result<(), EmitError> {
        let client = reqwest::Client::new();
        let endpoint = format!("{}/workflow/{}", context.http_url(), context.channel());

        let payload = serde_json::to_value(event)
            .map_err(|e| EmitError::SerializationError(e.to_string()))?;

        let response = client
            .post(&endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| EmitError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(EmitError::ServerError(format!(
                "Server returned status: {}",
                response.status()
            )));
        }

        Ok(())
    }
}

/// Error types for workflow event emission.
#[derive(Debug, thiserror::Error)]
pub enum EmitError {
    #[error("Event queue is full")]
    QueueFull,
    #[error("Validation error: {0}")]
    ValidationError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Server error: {0}")]
    ServerError(String),
}

/// Validation errors for workflow events.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Invalid progress value: {0} (must be 0-100)")]
    InvalidProgress(u8),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ailoop_integration::config::AiloopConfig;
    use url::Url;

    fn create_test_context() -> Arc<AiloopContext> {
        let config = AiloopConfig {
            http_url: Url::parse("http://localhost:8080").unwrap(),
            ws_url: Url::parse("ws://localhost:8080").unwrap(),
            channel: "test-channel".to_string(),
            enabled: true,
            fail_fast: false,
        };
        Arc::new(AiloopContext::new(
            config,
            std::path::PathBuf::from("/test/workspace"),
            "run".to_string(),
        ))
    }

    #[tokio::test]
    async fn test_emitter_creation() {
        let context = create_test_context();
        let _emitter = WorkflowEmitter::new(context);
        // Just verify it doesn't panic
    }

    #[tokio::test]
    async fn test_phase_started() {
        let context = create_test_context();
        let emitter = WorkflowEmitter::new(context);
        let execution_id = Uuid::new_v4();

        let result = emitter.phase_started(execution_id, "evaluator".to_string());
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_phase_progress() {
        let context = create_test_context();
        let emitter = WorkflowEmitter::new(context);
        let execution_id = Uuid::new_v4();

        let result = emitter.phase_progress(
            execution_id,
            "advisor".to_string(),
            50,
            Some("Processing recommendations".to_string()),
        );
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_phase_completed() {
        let context = create_test_context();
        let emitter = WorkflowEmitter::new(context);
        let execution_id = Uuid::new_v4();

        let result = emitter.phase_completed(execution_id, "executor".to_string());
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_phase_failed() {
        let context = create_test_context();
        let emitter = WorkflowEmitter::new(context);
        let execution_id = Uuid::new_v4();

        let result = emitter.phase_failed(
            execution_id,
            "evaluator".to_string(),
            "Test failure".to_string(),
        );
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_invalid_progress() {
        let context = create_test_context();
        let emitter = WorkflowEmitter::new(context);
        let execution_id = Uuid::new_v4();

        let result = emitter.emit(
            execution_id,
            "test".to_string(),
            "started".to_string(),
            Some(150), // Invalid progress > 100
            None,
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_valid_progress_boundary() {
        let context = create_test_context();
        let emitter = WorkflowEmitter::new(context);
        let execution_id = Uuid::new_v4();

        // Test boundary values
        assert!(emitter
            .emit(
                execution_id,
                "test".to_string(),
                "started".to_string(),
                Some(0),
                None
            )
            .is_ok());

        assert!(emitter
            .emit(
                execution_id,
                "test".to_string(),
                "started".to_string(),
                Some(100),
                None
            )
            .is_ok());
    }

    #[test]
    fn test_event_serialization() {
        let event = WorkflowEvent {
            execution_id: Uuid::new_v4(),
            phase: "evaluator".to_string(),
            status: "started".to_string(),
            progress: Some(0),
            message: None,
            timestamp: Utc::now(),
            schema_version: WORKFLOW_SCHEMA_VERSION.to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("evaluator"));
        assert!(json.contains("started"));
        assert!(json.contains("schema_version"));
    }
}
