use crate::ailoop_integration::AiloopContext;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// Response from an ask question request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionResponse {
    /// The answer provided by the user.
    pub answer: Option<String>,
    /// Whether the request timed out.
    pub timed_out: bool,
}

/// Response from an authorization request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationResponse {
    /// Whether the request was authorized.
    pub authorized: bool,
    /// Whether the request timed out.
    pub timed_out: bool,
    /// Optional reason for denial.
    pub reason: Option<String>,
}

/// Client for tool scripts to interact with ailoop.
pub struct ToolClient {
    context: Arc<AiloopContext>,
    client: reqwest::Client,
}

impl ToolClient {
    /// Create a new tool client from environment variables.
    /// Returns None if ailoop integration is not enabled.
    pub fn from_env() -> Option<Self> {
        let enabled = std::env::var("NEWTON_AILOOP_ENABLED")
            .ok()
            .map(|v| v == "1")
            .unwrap_or(false);

        if !enabled {
            return None;
        }

        let http_url = std::env::var("NEWTON_AILOOP_HTTP_URL").ok()?;
        let ws_url = std::env::var("NEWTON_AILOOP_WS_URL").ok()?;
        let channel = std::env::var("NEWTON_AILOOP_CHANNEL").ok()?;

        let config = crate::ailoop_integration::config::AiloopConfig {
            http_url: url::Url::parse(&http_url).ok()?,
            ws_url: url::Url::parse(&ws_url).ok()?,
            channel,
            enabled: true,
            fail_fast: false,
        };

        let context = Arc::new(AiloopContext::new(
            config,
            std::path::PathBuf::from("."),
            "tool".to_string(),
        ));

        Some(Self::new(context))
    }

    /// Create a new tool client with the given context.
    pub fn new(context: Arc<AiloopContext>) -> Self {
        Self {
            context,
            client: reqwest::Client::new(),
        }
    }

    /// Ask a question and wait for a response with timeout.
    /// Returns QuestionResponse with timed_out=true if timeout expires.
    pub async fn ask_question(
        &self,
        question: String,
        timeout: Duration,
    ) -> Result<QuestionResponse, ClientError> {
        let endpoint = format!(
            "{}/questions/{}",
            self.context.http_url(),
            self.context.channel()
        );

        let payload = serde_json::json!({
            "question": question,
            "timeout_ms": timeout.as_millis() as u64,
        });

        let response = self
            .client
            .post(&endpoint)
            .json(&payload)
            .timeout(timeout + Duration::from_secs(5)) // Add buffer to HTTP timeout
            .send()
            .await
            .map_err(|e| ClientError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ClientError::ServerError(format!(
                "Server returned status: {}",
                response.status()
            )));
        }

        let result: QuestionResponse = response
            .json()
            .await
            .map_err(|e| ClientError::DeserializationError(e.to_string()))?;

        Ok(result)
    }

    /// Request authorization and wait for a response with timeout.
    /// Returns AuthorizationResponse with timed_out=true if timeout expires.
    pub async fn request_authorization(
        &self,
        action: String,
        details: String,
        timeout: Duration,
    ) -> Result<AuthorizationResponse, ClientError> {
        let endpoint = format!(
            "{}/authorization/{}",
            self.context.http_url(),
            self.context.channel()
        );

        let payload = serde_json::json!({
            "action": action,
            "details": details,
            "timeout_ms": timeout.as_millis() as u64,
        });

        let response = self
            .client
            .post(&endpoint)
            .json(&payload)
            .timeout(timeout + Duration::from_secs(5)) // Add buffer to HTTP timeout
            .send()
            .await
            .map_err(|e| ClientError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ClientError::ServerError(format!(
                "Server returned status: {}",
                response.status()
            )));
        }

        let result: AuthorizationResponse = response
            .json()
            .await
            .map_err(|e| ClientError::DeserializationError(e.to_string()))?;

        Ok(result)
    }

    /// Send a generic notification to ailoop.
    pub async fn send_notification(
        &self,
        message: String,
        level: NotificationLevel,
    ) -> Result<(), ClientError> {
        let endpoint = format!(
            "{}/notifications/{}",
            self.context.http_url(),
            self.context.channel()
        );

        let payload = serde_json::json!({
            "message": message,
            "level": level,
            "timestamp": chrono::Utc::now(),
        });

        let response = self
            .client
            .post(&endpoint)
            .json(&payload)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| ClientError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ClientError::ServerError(format!(
                "Server returned status: {}",
                response.status()
            )));
        }

        Ok(())
    }
}

/// Notification level for generic notifications.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationLevel {
    Info,
    Warning,
    Error,
}

/// Error types for tool client operations.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Server error: {0}")]
    ServerError(String),
    #[error("Deserialization error: {0}")]
    DeserializationError(String),
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
            "tool".to_string(),
        ))
    }

    #[test]
    fn test_client_creation() {
        let context = create_test_context();
        let _client = ToolClient::new(context);
        // Just verify it doesn't panic
    }

    #[test]
    #[serial_test::serial]
    fn test_from_env_not_enabled() {
        // Without NEWTON_AILOOP_ENABLED, should return None
        std::env::remove_var("NEWTON_AILOOP_ENABLED");
        let client = ToolClient::from_env();
        assert!(client.is_none());
    }

    #[test]
    #[serial_test::serial]
    fn test_from_env_incomplete() {
        // With partial env vars, should return None
        std::env::set_var("NEWTON_AILOOP_ENABLED", "1");
        std::env::remove_var("NEWTON_AILOOP_HTTP_URL");
        let client = ToolClient::from_env();
        std::env::remove_var("NEWTON_AILOOP_ENABLED");
        assert!(client.is_none());
    }

    #[test]
    #[serial_test::serial]
    fn test_from_env_complete() {
        // With complete env vars, should create client
        std::env::set_var("NEWTON_AILOOP_ENABLED", "1");
        std::env::set_var("NEWTON_AILOOP_HTTP_URL", "http://localhost:8080");
        std::env::set_var("NEWTON_AILOOP_WS_URL", "ws://localhost:8080");
        std::env::set_var("NEWTON_AILOOP_CHANNEL", "test");

        let client = ToolClient::from_env();

        std::env::remove_var("NEWTON_AILOOP_ENABLED");
        std::env::remove_var("NEWTON_AILOOP_HTTP_URL");
        std::env::remove_var("NEWTON_AILOOP_WS_URL");
        std::env::remove_var("NEWTON_AILOOP_CHANNEL");

        assert!(client.is_some());
    }

    #[test]
    fn test_notification_level_serialization() {
        let info = NotificationLevel::Info;
        let warning = NotificationLevel::Warning;
        let error = NotificationLevel::Error;

        assert_eq!(serde_json::to_string(&info).unwrap(), "\"info\"");
        assert_eq!(serde_json::to_string(&warning).unwrap(), "\"warning\"");
        assert_eq!(serde_json::to_string(&error).unwrap(), "\"error\"");
    }

    #[test]
    fn test_question_response_deserialization() {
        let json = r#"{"answer":"test answer","timed_out":false}"#;
        let response: QuestionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.answer, Some("test answer".to_string()));
        assert!(!response.timed_out);
    }

    #[test]
    fn test_authorization_response_deserialization() {
        let json = r#"{"authorized":true,"timed_out":false,"reason":null}"#;
        let response: AuthorizationResponse = serde_json::from_str(json).unwrap();
        assert!(response.authorized);
        assert!(!response.timed_out);
        assert!(response.reason.is_none());
    }
}
