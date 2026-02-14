use crate::ailoop_integration::config::AiloopConfig;
use anyhow::{anyhow, Context};
use reqwest::{Client, Url};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

const POLL_INTERVAL_MS: u64 = 500;

/// High-level outcome for tool interactions.
#[derive(Debug, PartialEq, Eq)]
/// Possible outcomes from an interactive tool request.
pub enum ToolInteractionOutcome {
    /// A textual answer arrived from the human operator.
    Answer(String),
    /// The requested authorization was approved.
    AuthorizationApproved,
    /// The requested authorization was denied.
    AuthorizationDenied,
    /// No response arrived within the configured timeout.
    Timeout,
    /// The prompt was cancelled before a response.
    Cancelled,
}

/// Helper API for tool scripts interacting with ailoop.
pub struct ToolClient {
    config: Arc<AiloopConfig>,
    client: Client,
}

impl ToolClient {
    /// Construct a helper client backed by the provided HTTP client and configuration.
    pub fn new(config: Arc<AiloopConfig>, client: Client) -> Self {
        ToolClient { config, client }
    }

    /// Ask a question to the human in ailoop and respect the provided timeout.
    pub async fn ask_question(
        &self,
        question: &str,
        timeout: Duration,
        choices: &[String],
    ) -> crate::Result<ToolInteractionOutcome> {
        let message_id = self
            .post_message("question", question, timeout, Some(choices))
            .await?;
        self.poll_response(&message_id, timeout).await
    }

    /// Request an authorization decision with the specified timeout.
    pub async fn request_authorization(
        &self,
        action: &str,
        timeout: Duration,
    ) -> crate::Result<ToolInteractionOutcome> {
        let message_id = self
            .post_message("authorization", action, timeout, None)
            .await?;
        self.poll_response(&message_id, timeout).await
    }

    /// Send a notification that does not block the orchestrator.
    pub async fn send_notification(&self, text: &str) -> crate::Result<()> {
        let _: String = self
            .post_message("notification", text, Duration::from_secs(0), None)
            .await?;
        Ok(())
    }

    async fn post_message(
        &self,
        kind: &str,
        text: &str,
        timeout: Duration,
        choices: Option<&[String]>,
    ) -> crate::Result<String> {
        let url = ensure_messages_endpoint(self.config.http_url.as_str())?;
        let mut content = json!({
            "type": kind,
            "text": text,
            "priority": "high",
            "schema_version": "1.0",
        });
        if timeout.as_secs() > 0 {
            content["timeout_seconds"] = json!(timeout.as_secs());
        }
        if let Some(choice_list) = choices {
            content["choices"] = json!(choice_list);
        }
        let payload = json!({
            "channel": self.config.channel,
            "sender_type": "AGENT",
            "content": content,
        });
        let response = self
            .client
            .post(url)
            .json(&payload)
            .send()
            .await
            .with_context(|| format!("posting {} to ailoop", kind))?
            .error_for_status()?;
        let message: MessageResponse = response.json().await?;
        Ok(message.id)
    }

    async fn poll_response(
        &self,
        message_id: &str,
        timeout: Duration,
    ) -> crate::Result<ToolInteractionOutcome> {
        if timeout.as_secs() == 0 {
            return Ok(ToolInteractionOutcome::Timeout);
        }
        let url = ensure_response_endpoint(self.config.http_url.as_str(), message_id)?;
        let deadline = Instant::now() + timeout;
        loop {
            if Instant::now() >= deadline {
                return Ok(ToolInteractionOutcome::Timeout);
            }
            let resp = self.client.get(url.clone()).send().await;
            match resp {
                Ok(response) => {
                    if response.status().is_success() {
                        let body: ResponsePayload = response.json().await?;
                        return Ok(map_response_type(&body));
                    }
                    if response.status().as_u16() == 404 {
                        tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
                        continue;
                    }
                    return Err(anyhow!("unexpected response {}", response.status()));
                }
                Err(err) => {
                    tracing::warn!(error = ?err, "ailoop response poll failed");
                    tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct MessageResponse {
    id: String,
}

#[derive(Deserialize)]
struct ResponsePayload {
    response_type: String,
    answer: Option<String>,
}

fn map_response_type(payload: &ResponsePayload) -> ToolInteractionOutcome {
    match payload.response_type.as_str() {
        "text" => ToolInteractionOutcome::Answer(payload.answer.clone().unwrap_or_default()),
        "authorization_approved" => ToolInteractionOutcome::AuthorizationApproved,
        "authorization_denied" => ToolInteractionOutcome::AuthorizationDenied,
        "timeout" => ToolInteractionOutcome::Timeout,
        "cancelled" => ToolInteractionOutcome::Cancelled,
        _ => ToolInteractionOutcome::Answer(payload.answer.clone().unwrap_or_default()),
    }
}

fn ensure_messages_endpoint(base: &str) -> crate::Result<reqwest::Url> {
    let mut url = base.trim_end_matches('/').to_string();
    url.push_str("/api/v1/messages");
    Url::parse(&url).map_err(|err| anyhow!("invalid ailoop message endpoint: {}", err))
}

fn ensure_response_endpoint(base: &str, message_id: &str) -> crate::Result<String> {
    let stripped = base.trim_end_matches('/');
    Ok(format!(
        "{}/api/v1/messages/{}/response",
        stripped, message_id
    ))
}
