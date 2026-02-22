use crate::ailoop_integration::AiloopContext;
use crate::core::types::ExecutionStatus;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

const EVENT_SCHEMA_VERSION: &str = "1.0.0";
const MAX_RETRY_ATTEMPTS: usize = 3;
const RETRY_DELAY_MS: u64 = 100;

/// Orchestrator lifecycle event types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum OrchestratorEvent {
    /// Execution started event.
    ExecutionStarted {
        execution_id: Uuid,
        workspace_path: String,
        command: String,
        timestamp: DateTime<Utc>,
        schema_version: String,
    },
    /// Iteration started event.
    IterationStarted {
        execution_id: Uuid,
        iteration_number: usize,
        timestamp: DateTime<Utc>,
        schema_version: String,
    },
    /// Iteration completed event.
    IterationCompleted {
        execution_id: Uuid,
        iteration_number: usize,
        timestamp: DateTime<Utc>,
        schema_version: String,
    },
    /// Execution failed event.
    ExecutionFailed {
        execution_id: Uuid,
        error_message: String,
        timestamp: DateTime<Utc>,
        schema_version: String,
    },
    /// Execution completed event.
    ExecutionCompleted {
        execution_id: Uuid,
        status: ExecutionStatus,
        total_iterations: usize,
        timestamp: DateTime<Utc>,
        schema_version: String,
    },
}

/// Notifier for sending orchestrator lifecycle events to ailoop.
#[derive(Clone)]
pub struct OrchestratorNotifier {
    context: Arc<AiloopContext>,
    event_tx: mpsc::UnboundedSender<OrchestratorEvent>,
}

impl OrchestratorNotifier {
    /// Create a new orchestrator notifier.
    /// Spawns a background task to handle event sending with retry logic.
    pub fn new(context: Arc<AiloopContext>) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let sender_context = context.clone();
        tokio::spawn(async move {
            Self::event_sender_loop(sender_context, event_rx).await;
        });

        Self { context, event_tx }
    }

    /// Emit an execution started event.
    pub fn execution_started(
        &self,
        execution_id: Uuid,
        workspace_path: String,
    ) -> Result<(), SendError> {
        let event = OrchestratorEvent::ExecutionStarted {
            execution_id,
            workspace_path,
            command: self.context.command_name.clone(),
            timestamp: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
        };
        self.send_event(event)
    }

    /// Emit an iteration started event.
    pub fn iteration_started(
        &self,
        execution_id: Uuid,
        iteration_number: usize,
    ) -> Result<(), SendError> {
        let event = OrchestratorEvent::IterationStarted {
            execution_id,
            iteration_number,
            timestamp: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
        };
        self.send_event(event)
    }

    /// Emit an iteration completed event.
    pub fn iteration_completed(
        &self,
        execution_id: Uuid,
        iteration_number: usize,
    ) -> Result<(), SendError> {
        let event = OrchestratorEvent::IterationCompleted {
            execution_id,
            iteration_number,
            timestamp: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
        };
        self.send_event(event)
    }

    /// Emit an execution failed event.
    pub fn execution_failed(
        &self,
        execution_id: Uuid,
        error_message: String,
    ) -> Result<(), SendError> {
        let event = OrchestratorEvent::ExecutionFailed {
            execution_id,
            error_message,
            timestamp: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
        };
        self.send_event(event)
    }

    /// Emit an execution completed event.
    pub fn execution_completed(
        &self,
        execution_id: Uuid,
        status: ExecutionStatus,
        total_iterations: usize,
    ) -> Result<(), SendError> {
        let event = OrchestratorEvent::ExecutionCompleted {
            execution_id,
            status,
            total_iterations,
            timestamp: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
        };
        self.send_event(event)
    }

    /// Send an event to the background sender task.
    fn send_event(&self, event: OrchestratorEvent) -> Result<(), SendError> {
        self.event_tx.send(event).map_err(|_| SendError::QueueFull)
    }

    /// Background task loop that sends events to ailoop with retry logic.
    async fn event_sender_loop(
        context: Arc<AiloopContext>,
        mut event_rx: mpsc::UnboundedReceiver<OrchestratorEvent>,
    ) {
        while let Some(event) = event_rx.recv().await {
            if let Err(e) = Self::send_event_with_retry(&context, &event).await {
                tracing::error!(
                    event = ?event,
                    error = %e,
                    "Failed to send orchestrator event to ailoop after retries"
                );
            }
        }
    }

    /// Send an event with retry logic.
    async fn send_event_with_retry(
        context: &AiloopContext,
        event: &OrchestratorEvent,
    ) -> Result<(), SendError> {
        for attempt in 0..MAX_RETRY_ATTEMPTS {
            match Self::send_event_once(context, event).await {
                Ok(()) => {
                    tracing::debug!(event = ?event, "Successfully sent orchestrator event");
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(
                        event = ?event,
                        attempt = attempt + 1,
                        max_attempts = MAX_RETRY_ATTEMPTS,
                        error = %e,
                        "Failed to send orchestrator event, will retry"
                    );
                    if attempt + 1 < MAX_RETRY_ATTEMPTS {
                        tokio::time::sleep(tokio::time::Duration::from_millis(
                            RETRY_DELAY_MS * (attempt as u64 + 1),
                        ))
                        .await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        Err(SendError::MaxRetriesExceeded)
    }

    /// Send an event once to ailoop HTTP endpoint.
    async fn send_event_once(
        context: &AiloopContext,
        event: &OrchestratorEvent,
    ) -> Result<(), SendError> {
        let client = reqwest::Client::new();
        let endpoint = format!("{}/events/{}", context.http_url(), context.channel());

        let payload = serde_json::to_value(event).map_err(|e| {
            SendError::SerializationError(format!("Failed to serialize event: {}", e))
        })?;

        let response = client
            .post(&endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| SendError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(SendError::ServerError(format!(
                "Server returned status: {}",
                response.status()
            )));
        }

        Ok(())
    }
}

/// Error types for event sending.
#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("Event queue is full")]
    QueueFull,
    #[error("Max retry attempts exceeded")]
    MaxRetriesExceeded,
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Server error: {0}")]
    ServerError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ailoop_integration::config::AiloopConfig;
    use serde_json::Value;
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
    async fn test_notifier_creation() {
        let context = create_test_context();
        let _notifier = OrchestratorNotifier::new(context);
        // Just verify it doesn't panic
    }

    #[tokio::test]
    async fn test_execution_started_event() {
        let context = create_test_context();
        let notifier = OrchestratorNotifier::new(context);
        let execution_id = Uuid::new_v4();

        let result = notifier.execution_started(execution_id, "/test/workspace".to_string());
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_iteration_started_event() {
        let context = create_test_context();
        let notifier = OrchestratorNotifier::new(context);
        let execution_id = Uuid::new_v4();

        let result = notifier.iteration_started(execution_id, 1);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_iteration_completed_event() {
        let context = create_test_context();
        let notifier = OrchestratorNotifier::new(context);
        let execution_id = Uuid::new_v4();

        let result = notifier.iteration_completed(execution_id, 1);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execution_failed_event() {
        let context = create_test_context();
        let notifier = OrchestratorNotifier::new(context);
        let execution_id = Uuid::new_v4();

        let result = notifier.execution_failed(execution_id, "Test error".to_string());
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execution_completed_event() {
        let context = create_test_context();
        let notifier = OrchestratorNotifier::new(context);
        let execution_id = Uuid::new_v4();

        let result = notifier.execution_completed(execution_id, ExecutionStatus::Completed, 5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_event_serialization() {
        let event = OrchestratorEvent::ExecutionStarted {
            execution_id: Uuid::new_v4(),
            workspace_path: "/test".to_string(),
            command: "run".to_string(),
            timestamp: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("execution_started"));
        assert!(json.contains("schema_version"));
    }

    fn assert_schema(value: &Value) {
        assert_eq!(
            value["schema_version"],
            Value::String(EVENT_SCHEMA_VERSION.to_string())
        );
        assert!(value["timestamp"].is_string());
    }

    #[test]
    fn execution_started_contains_required_fields() {
        let event = OrchestratorEvent::ExecutionStarted {
            execution_id: Uuid::new_v4(),
            workspace_path: "/test/workspace".to_string(),
            command: "run".to_string(),
            timestamp: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
        };

        let value = serde_json::to_value(&event).unwrap();
        assert_schema(&value);
        assert!(value["execution_id"].is_string());
        assert_eq!(
            value["workspace_path"],
            Value::String("/test/workspace".to_string())
        );
        assert_eq!(value["command"], Value::String("run".to_string()));
    }

    #[test]
    fn iteration_started_contains_required_fields() {
        let event = OrchestratorEvent::IterationStarted {
            execution_id: Uuid::new_v4(),
            iteration_number: 2,
            timestamp: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
        };

        let value = serde_json::to_value(&event).unwrap();
        assert_schema(&value);
        assert_eq!(
            value["iteration_number"],
            Value::Number(serde_json::Number::from(2))
        );
    }

    #[test]
    fn iteration_completed_contains_required_fields() {
        let event = OrchestratorEvent::IterationCompleted {
            execution_id: Uuid::new_v4(),
            iteration_number: 3,
            timestamp: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
        };

        let value = serde_json::to_value(&event).unwrap();
        assert_schema(&value);
        assert_eq!(
            value["iteration_number"],
            Value::Number(serde_json::Number::from(3))
        );
    }

    #[test]
    fn execution_failed_contains_required_fields() {
        let event = OrchestratorEvent::ExecutionFailed {
            execution_id: Uuid::new_v4(),
            error_message: "failure".to_string(),
            timestamp: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
        };

        let value = serde_json::to_value(&event).unwrap();
        assert_schema(&value);
        assert_eq!(value["error_message"], Value::String("failure".to_string()));
    }

    #[test]
    fn execution_completed_contains_required_fields() {
        let event = OrchestratorEvent::ExecutionCompleted {
            execution_id: Uuid::new_v4(),
            status: ExecutionStatus::Completed,
            total_iterations: 5,
            timestamp: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
        };

        let value = serde_json::to_value(&event).unwrap();
        assert_schema(&value);
        assert_eq!(
            value["total_iterations"],
            Value::Number(serde_json::Number::from(5))
        );
        let expected_status = serde_json::to_value(ExecutionStatus::Completed).unwrap();
        assert_eq!(value["status"], expected_status);
    }
}
