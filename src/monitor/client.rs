use crate::monitor::config::MonitorEndpoints;
use crate::monitor::event::{
    ConnectionState, ConnectionStatus, MonitorCommand, MonitorEvent, ResponseType,
};
use crate::monitor::message::MonitorMessage;
use futures::{SinkExt, StreamExt};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::time::Duration as StdDuration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::error;
use uuid::Uuid;

/// ASCII set for encoding path segments (slashes included).
const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &CONTROLS.add(b' ').add(b'/').add(b'?').add(b'#');

/// HTTP + WebSocket client targeting the configured ailoop instance.
#[derive(Clone)]
pub struct AiloopClient {
    http: reqwest::Client,
    endpoints: MonitorEndpoints,
}

impl AiloopClient {
    /// Build a new client using the resolved endpoints.
    pub fn new(endpoints: MonitorEndpoints) -> Self {
        AiloopClient {
            http: reqwest::Client::new(),
            endpoints,
        }
    }

    /// Return the configured WebSocket URL.
    pub fn ws_url(&self) -> &reqwest::Url {
        &self.endpoints.ws_url
    }

    /// Fetch all known channels from the server.
    pub async fn fetch_channels(&self) -> crate::Result<Vec<String>> {
        let url = join_path(&self.endpoints.http_url, &["api", "channels"])?;
        let resp = self.http.get(url).send().await?;
        let value: Value = resp.json().await?;

        // Parse structured ChannelInfo objects from "channels" array
        if let Some(entries) = value.get("channels").and_then(|section| section.as_array()) {
            let channels: Vec<String> = entries
                .iter()
                .filter_map(|entry| {
                    // Extract the "name" field from each ChannelInfo object
                    entry
                        .get("name")
                        .and_then(|name| name.as_str())
                        .map(|s| s.to_string())
                })
                .collect();

            if !channels.is_empty() {
                tracing::debug!("Fetched {} channels from server", channels.len());
                return Ok(channels);
            }
        }

        // Fallback: try parsing as direct array for backward compatibility
        if let Some(arr) = value.as_array() {
            let channels: Vec<String> = arr
                .iter()
                .filter_map(|entry| entry.as_str().map(|s| s.to_string()))
                .collect();

            if !channels.is_empty() {
                tracing::debug!(
                    "Fetched {} channels from server (legacy format)",
                    channels.len()
                );
                return Ok(channels);
            }
        }

        tracing::warn!(
            "Channel list parsing returned empty result. Response: {:?}",
            value
        );
        Ok(Vec::new())
    }

    /// Fetch the most recent messages for a channel (used for backfill/polling).
    pub async fn fetch_channel_messages(
        &self,
        channel: &str,
        limit: usize,
    ) -> crate::Result<Vec<MonitorMessage>> {
        let encoded = encode_segment(channel);
        let mut url = join_path(
            &self.endpoints.http_url,
            &["api", "channels", &encoded, "messages"],
        )?;
        let limit = limit.min(200);
        url.push_str(&format!("?limit={limit}"));
        let resp = self.http.get(url).send().await?;
        let messages = resp.json::<Vec<MonitorMessage>>().await?;
        Ok(messages)
    }

    /// Post an answer/approval to a queued message.
    pub async fn post_response(
        &self,
        message_id: Uuid,
        answer: Option<String>,
        response_type: ResponseType,
    ) -> crate::Result<()> {
        let url = format!(
            "{}/api/v1/messages/{}/response",
            self.endpoints.http_url.as_str().trim_end_matches('/'),
            message_id
        );
        let payload = ResponsePayload {
            answer,
            response_type: response_type.as_str(),
        };
        let resp = self.http.post(&url).json(&payload).send().await?;
        let status = resp.status();
        if status != StatusCode::OK {
            let text = resp.text().await.unwrap_or_else(|_| "".to_string());
            return Err(anyhow::anyhow!(
                "ailoop response POST failed: {} {}",
                status,
                text
            ));
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ResponsePayload<'a> {
    answer: Option<String>,
    response_type: &'a str,
}

fn encode_segment(segment: &str) -> String {
    utf8_percent_encode(segment, PATH_SEGMENT_ENCODE_SET).to_string()
}

fn join_path(base: &reqwest::Url, segments: &[&str]) -> crate::Result<String> {
    let mut url = base.as_str().trim_end_matches('/').to_string();
    for segment in segments {
        if !segment.is_empty() {
            url.push('/');
            url.push_str(segment);
        }
    }
    Ok(url)
}

/// Run a synchronous backfill that harvests existing messages before the UI starts.
pub async fn initial_backfill(
    client: &AiloopClient,
    event_tx: &UnboundedSender<MonitorEvent>,
) -> crate::Result<()> {
    let channels = client.fetch_channels().await?;
    for channel in channels {
        if let Ok(messages) = client.fetch_channel_messages(&channel, 50).await {
            for message in messages {
                let _ = event_tx.send(MonitorEvent::Message(message));
            }
        }
    }
    Ok(())
}

/// Spawn a WebSocket loop that reconnects with exponential backoff.
pub async fn websocket_loop(client: AiloopClient, event_tx: UnboundedSender<MonitorEvent>) {
    let mut backoff = StdDuration::from_secs(1);
    loop {
        let _ = event_tx.send(MonitorEvent::ConnectionStatus(ConnectionStatus {
            state: ConnectionState::Connecting,
            detail: Some("connecting".to_string()),
        }));
        match connect_async(client.ws_url()).await {
            Ok((stream, _)) => {
                let _ = event_tx.send(MonitorEvent::ConnectionStatus(ConnectionStatus {
                    state: ConnectionState::Connected,
                    detail: Some("connected".to_string()),
                }));
                backoff = StdDuration::from_secs(1);
                let (mut writer, mut reader) = stream.split();

                // Subscribe to all channels by sending a notification message
                // Note: ailoop automatically subscribes connections when they send messages
                let subscribe_msg = serde_json::json!({
                    "id": Uuid::new_v4().to_string(),
                    "channel": "*",
                    "sender_type": "AGENT",
                    "content": {
                        "type": "notification",
                        "text": "newton monitor connected",
                        "priority": "low"
                    },
                    "timestamp": chrono::Utc::now().to_rfc3339()
                });
                if let Ok(msg_text) = serde_json::to_string(&subscribe_msg) {
                    tracing::info!("Sending subscription message: {}", msg_text);
                    if let Err(e) = writer.send(Message::Text(msg_text)).await {
                        tracing::error!("Failed to send subscription: {}", e);
                    } else {
                        tracing::info!("Subscription message sent successfully");
                    }
                }

                while let Some(message) = reader.next().await {
                    match message {
                        Ok(Message::Text(txt)) => {
                            tracing::debug!("Received WebSocket text: {}", txt);
                            match serde_json::from_str::<MonitorMessage>(&txt) {
                                Ok(parsed) => {
                                    tracing::info!("Parsed message: {:?}", parsed.summary);
                                    let _ = event_tx.send(MonitorEvent::Message(parsed));
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to parse message: {} - Raw: {}", e, txt);
                                }
                            }
                        }
                        Ok(Message::Binary(bytes)) => {
                            if let Ok(txt) = String::from_utf8(bytes) {
                                if let Ok(parsed) = serde_json::from_str::<MonitorMessage>(&txt) {
                                    let _ = event_tx.send(MonitorEvent::Message(parsed));
                                }
                            }
                        }
                        Ok(Message::Ping(payload)) => {
                            let _ = writer.send(Message::Pong(payload)).await;
                        }
                        Ok(Message::Close(_)) => break,
                        Err(err) => {
                            error!("WebSocket error: {}", err);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Err(err) => {
                let _ = event_tx.send(MonitorEvent::ConnectionStatus(ConnectionStatus {
                    state: ConnectionState::Disconnected,
                    detail: Some(format!("ws connect failed: {}", err)),
                }));
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = std::cmp::min(backoff * 2, StdDuration::from_secs(60));
    }
}

/// Fallback polling loop that keeps message buffers fresh in case viewer subscribe is ignored.
pub async fn polling_loop(client: AiloopClient, event_tx: UnboundedSender<MonitorEvent>) {
    let mut interval = tokio::time::interval(StdDuration::from_secs(5));
    let mut seen = HashSet::new();
    loop {
        interval.tick().await;
        match client.fetch_channels().await {
            Ok(channels) => {
                for channel in channels {
                    match client.fetch_channel_messages(&channel, 20).await {
                        Ok(messages) => {
                            for message in messages {
                                if seen.insert(message.id) {
                                    let _ = event_tx.send(MonitorEvent::Message(message));
                                }
                            }
                        }
                        Err(err) => {
                            error!("polling messages failed for {}: {}", channel, err);
                        }
                    }
                }
            }
            Err(err) => {
                error!("polling channels failed: {}", err);
            }
        }
    }
}

/// Handle commands issued by the UI (answer/approve/deny).
pub async fn command_loop(
    client: AiloopClient,
    mut command_rx: UnboundedReceiver<MonitorCommand>,
    event_tx: UnboundedSender<MonitorEvent>,
) {
    while let Some(command) = command_rx.recv().await {
        match command {
            MonitorCommand::Respond {
                message_id,
                answer,
                response_type,
            } => {
                if let Err(err) = client
                    .post_response(message_id, answer, response_type)
                    .await
                {
                    error!("failed to post response: {}", err);
                    let _ = event_tx.send(MonitorEvent::ConnectionStatus(ConnectionStatus {
                        state: ConnectionState::Disconnected,
                        detail: Some(format!("response failed: {}", err)),
                    }));
                } else {
                    let _ = event_tx.send(MonitorEvent::ConnectionStatus(ConnectionStatus {
                        state: ConnectionState::Connected,
                        detail: Some("response posted".to_string()),
                    }));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn build_endpoints(mock_server: &MockServer) -> MonitorEndpoints {
        let http_url = reqwest::Url::parse(&mock_server.uri()).unwrap();
        let ws_url = reqwest::Url::parse(&mock_server.uri().replace("http", "ws")).unwrap();
        MonitorEndpoints {
            http_url,
            ws_url,
            workspace_root: PathBuf::from("."),
        }
    }

    #[tokio::test]
    async fn test_fetch_channels_parses_structured_response() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/channels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "channels": [
                    {
                        "name": "project1/branch1",
                        "message_count": 5,
                        "oldest_message": "2026-02-14T10:00:00Z",
                        "newest_message": "2026-02-14T11:30:00Z"
                    },
                    {
                        "name": "project2/branch2",
                        "message_count": 3,
                        "oldest_message": "2026-02-14T09:00:00Z",
                        "newest_message": "2026-02-14T10:15:00Z"
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let endpoints = build_endpoints(&mock_server);

        let client = AiloopClient::new(endpoints);
        let channels = client.fetch_channels().await.unwrap();

        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0], "project1/branch1");
        assert_eq!(channels[1], "project2/branch2");
    }

    #[tokio::test]
    async fn test_fetch_channels_handles_empty_list() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/channels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "channels": []
            })))
            .mount(&mock_server)
            .await;

        let endpoints = build_endpoints(&mock_server);

        let client = AiloopClient::new(endpoints);
        let channels = client.fetch_channels().await.unwrap();

        assert!(channels.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_channels_backwards_compatible() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/channels"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!(["simple/channel", "another/channel"])),
            )
            .mount(&mock_server)
            .await;

        let endpoints = build_endpoints(&mock_server);

        let client = AiloopClient::new(endpoints);
        let channels = client.fetch_channels().await.unwrap();

        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0], "simple/channel");
        assert_eq!(channels[1], "another/channel");
    }
}
