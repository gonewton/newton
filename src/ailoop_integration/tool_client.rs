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
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use std::path::PathBuf;
    use url::Url;
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn create_context(http_url: &str, ws_url: &str, channel: &str) -> Arc<AiloopContext> {
        let config = AiloopConfig {
            http_url: Url::parse(http_url).unwrap(),
            ws_url: Url::parse(ws_url).unwrap(),
            channel: channel.to_string(),
            enabled: true,
            fail_fast: false,
        };
        Arc::new(AiloopContext::new(
            config,
            PathBuf::from("/test/workspace"),
            "tool".to_string(),
        ))
    }

    #[test]
    fn test_client_creation() {
        let context = create_context("http://localhost:8080", "ws://localhost:8080", "test");
        let _client = ToolClient::new(context);
    }

    #[test]
    #[serial]
    fn test_from_env_not_enabled() {
        env::remove_var("NEWTON_AILOOP_ENABLED");
        assert!(ToolClient::from_env().is_none());
    }

    #[test]
    #[serial]
    fn test_from_env_incomplete() {
        env::set_var("NEWTON_AILOOP_ENABLED", "1");
        env::remove_var("NEWTON_AILOOP_HTTP_URL");
        assert!(ToolClient::from_env().is_none());
        env::remove_var("NEWTON_AILOOP_ENABLED");
    }

    #[test]
    #[serial]
    fn test_from_env_complete() {
        env::set_var("NEWTON_AILOOP_ENABLED", "1");
        env::set_var("NEWTON_AILOOP_HTTP_URL", "http://localhost:8080");
        env::set_var("NEWTON_AILOOP_WS_URL", "ws://localhost:8080");
        env::set_var("NEWTON_AILOOP_CHANNEL", "test");

        let client = ToolClient::from_env();

        env::remove_var("NEWTON_AILOOP_ENABLED");
        env::remove_var("NEWTON_AILOOP_HTTP_URL");
        env::remove_var("NEWTON_AILOOP_WS_URL");
        env::remove_var("NEWTON_AILOOP_CHANNEL");

        assert!(client.is_some());
    }

    #[tokio::test]
    async fn test_ask_question_timeout_response() {
        let mock_server = MockServer::start().await;
        let channel = "test-channel";
        Mock::given(method("POST"))
            .and(path_regex(format!(r"^/+questions/{channel}$")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "answer": null,
                "timed_out": true
            })))
            .mount(&mock_server)
            .await;

        let context = create_context(&mock_server.uri(), "ws://localhost:8080", channel);
        let client = ToolClient::new(context);
        let result = client
            .ask_question("question".to_string(), Duration::from_secs(1))
            .await;
        let requests = mock_server
            .received_requests()
            .await
            .expect("should have received requests");
        assert!(
            requests[0]
                .url
                .path()
                .ends_with(&format!("/questions/{channel}")),
            "unexpected path: {}",
            requests[0].url.path()
        );
        let response = result.expect("should parse response");

        assert!(response.timed_out);
        assert!(response.answer.is_none());
    }

    #[tokio::test]
    async fn test_request_authorization_denied_response() {
        let mock_server = MockServer::start().await;
        let channel = "auth-channel";
        Mock::given(method("POST"))
            .and(path_regex(format!(r"^/+authorization/{channel}$")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "authorized": false,
                "timed_out": false,
                "reason": "denied"
            })))
            .mount(&mock_server)
            .await;

        let context = create_context(&mock_server.uri(), "ws://localhost:8080", channel);
        let client = ToolClient::new(context);
        let result = client
            .request_authorization(
                "action".to_string(),
                "details".to_string(),
                Duration::from_secs(1),
            )
            .await;
        let requests = mock_server
            .received_requests()
            .await
            .expect("should have received requests");
        assert!(
            requests[0]
                .url
                .path()
                .ends_with(&format!("/authorization/{channel}")),
            "unexpected path: {}",
            requests[0].url.path()
        );
        let response = result.expect("should parse response");

        assert!(!response.authorized);
        assert_eq!(response.reason.as_deref(), Some("denied"));
    }

    #[tokio::test]
    async fn test_ask_question_network_error_when_unreachable() {
        let context = create_context("http://127.0.0.1:1", "ws://localhost:8080", "unreachable");
        let client = ToolClient::new(context);
        let result = client
            .ask_question("hi".to_string(), Duration::from_millis(100))
            .await;

        assert!(matches!(result, Err(ClientError::NetworkError(_))));
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
