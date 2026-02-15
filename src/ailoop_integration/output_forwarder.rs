use crate::ailoop_integration::AiloopContext;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

const FORWARDER_QUEUE_SIZE: usize = 10000;
const MAX_MESSAGE_LENGTH: usize = 8192;

/// Priority level for output messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessagePriority {
    /// Normal priority (stdout).
    Normal,
    /// High priority (stderr).
    High,
}

/// Output message to be forwarded to ailoop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputMessage {
    /// The output content.
    pub content: String,
    /// Priority level.
    pub priority: MessagePriority,
    /// Source of the output (stdout or stderr).
    pub source: String,
    /// Optional execution context.
    pub execution_id: Option<uuid::Uuid>,
}

/// Forwarder for streaming tool output to ailoop.
#[derive(Clone)]
pub struct OutputForwarder {
    #[allow(dead_code)]
    context: Arc<AiloopContext>,
    message_tx: mpsc::Sender<OutputMessage>,
}

impl OutputForwarder {
    /// Create a new output forwarder.
    /// Spawns a background task to handle message forwarding with bounded buffering.
    pub fn new(context: Arc<AiloopContext>) -> Self {
        let (message_tx, message_rx) = mpsc::channel(FORWARDER_QUEUE_SIZE);

        let sender_context = context.clone();
        tokio::spawn(async move {
            Self::forwarder_loop(sender_context, message_rx).await;
        });

        Self {
            context,
            message_tx,
        }
    }

    /// Forward a stdout line.
    pub async fn forward_stdout(
        &self,
        content: String,
        execution_id: Option<uuid::Uuid>,
    ) -> Result<(), ForwardError> {
        let message = OutputMessage {
            content: Self::truncate_if_needed(content),
            priority: MessagePriority::Normal,
            source: "stdout".to_string(),
            execution_id,
        };
        self.send_message(message).await
    }

    /// Forward a stderr line.
    pub async fn forward_stderr(
        &self,
        content: String,
        execution_id: Option<uuid::Uuid>,
    ) -> Result<(), ForwardError> {
        let message = OutputMessage {
            content: Self::truncate_if_needed(content),
            priority: MessagePriority::High,
            source: "stderr".to_string(),
            execution_id,
        };
        self.send_message(message).await
    }

    /// Send a message to the background forwarder task.
    async fn send_message(&self, message: OutputMessage) -> Result<(), ForwardError> {
        self.message_tx
            .send(message)
            .await
            .map_err(|_| ForwardError::QueueFull)
    }

    /// Truncate message content if it exceeds maximum length.
    fn truncate_if_needed(mut content: String) -> String {
        if content.len() > MAX_MESSAGE_LENGTH {
            content.truncate(MAX_MESSAGE_LENGTH - 20);
            content.push_str("\n... (truncated)");
        }
        content
    }

    /// Background task loop that forwards messages to ailoop.
    async fn forwarder_loop(
        context: Arc<AiloopContext>,
        mut message_rx: mpsc::Receiver<OutputMessage>,
    ) {
        while let Some(message) = message_rx.recv().await {
            if let Err(e) = Self::forward_message_once(&context, &message).await {
                // Log error but don't fail the tool process
                tracing::warn!(
                    priority = ?message.priority,
                    source = %message.source,
                    error = %e,
                    "Failed to forward output message to ailoop"
                );
            }
        }
    }

    /// Forward a single message to ailoop HTTP endpoint.
    async fn forward_message_once(
        context: &AiloopContext,
        message: &OutputMessage,
    ) -> Result<(), ForwardError> {
        let client = reqwest::Client::new();
        let endpoint = format!("{}/messages/{}", context.http_url(), context.channel());

        let payload = serde_json::json!({
            "content": message.content,
            "priority": message.priority,
            "source": message.source,
            "execution_id": message.execution_id,
            "timestamp": chrono::Utc::now(),
        });

        let response = client
            .post(&endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map_err(|e| ForwardError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ForwardError::ServerError(format!(
                "Server returned status: {}",
                response.status()
            )));
        }

        Ok(())
    }
}

/// Error types for output forwarding.
#[derive(Debug, thiserror::Error)]
pub enum ForwardError {
    #[error("Message queue is full")]
    QueueFull,
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Server error: {0}")]
    ServerError(String),
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
    async fn test_forwarder_creation() {
        let context = create_test_context();
        let _forwarder = OutputForwarder::new(context);
        // Just verify it doesn't panic
    }

    #[tokio::test]
    async fn test_forward_stdout() {
        let context = create_test_context();
        let forwarder = OutputForwarder::new(context);
        let execution_id = uuid::Uuid::new_v4();

        let result = forwarder
            .forward_stdout("Test output".to_string(), Some(execution_id))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_forward_stderr() {
        let context = create_test_context();
        let forwarder = OutputForwarder::new(context);
        let execution_id = uuid::Uuid::new_v4();

        let result = forwarder
            .forward_stderr("Test error".to_string(), Some(execution_id))
            .await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_truncate_if_needed_short() {
        let short_content = "Short message".to_string();
        let result = OutputForwarder::truncate_if_needed(short_content.clone());
        assert_eq!(result, short_content);
    }

    #[test]
    fn test_truncate_if_needed_long() {
        let long_content = "x".repeat(MAX_MESSAGE_LENGTH + 100);
        let result = OutputForwarder::truncate_if_needed(long_content);
        assert!(result.len() <= MAX_MESSAGE_LENGTH);
        assert!(result.ends_with("... (truncated)"));
    }

    #[test]
    fn test_message_priority_serialization() {
        let normal = MessagePriority::Normal;
        let high = MessagePriority::High;

        let normal_json = serde_json::to_string(&normal).unwrap();
        let high_json = serde_json::to_string(&high).unwrap();

        assert_eq!(normal_json, "\"normal\"");
        assert_eq!(high_json, "\"high\"");
    }

    #[test]
    fn test_output_message_serialization() {
        let message = OutputMessage {
            content: "Test content".to_string(),
            priority: MessagePriority::Normal,
            source: "stdout".to_string(),
            execution_id: Some(uuid::Uuid::new_v4()),
        };

        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("Test content"));
        assert!(json.contains("stdout"));
        assert!(json.contains("normal"));
    }
}
