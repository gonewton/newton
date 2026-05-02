use crate::monitor::config::MonitorEndpoints;
use crate::monitor::event::{
    ConnectionState, ConnectionStatus, MonitorCommand, MonitorEvent, ResponseType,
};
use crate::monitor::message::MonitorMessage;
use futures::{SinkExt, StreamExt};
use newton_types::{HilAction, HilEvent};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use reqwest::StatusCode;
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

    /// Fetch all active HIL workflow instance IDs from the server.
    pub async fn fetch_channels(&self) -> crate::Result<Vec<String>> {
        let url = join_path(&self.endpoints.http_url, &["api", "hil", "instances"]);
        let resp = self.http.get(url).send().await?;
        let instances = resp.json::<Vec<String>>().await?;
        tracing::debug!("Fetched {} HIL instances from server", instances.len());
        Ok(instances)
    }

    /// Fetch HIL events for a workflow instance (used for backfill/polling).
    pub async fn fetch_channel_messages(
        &self,
        instance_id: &str,
        _limit: usize,
    ) -> crate::Result<Vec<MonitorMessage>> {
        let encoded = encode_segment(instance_id);
        let url = join_path(
            &self.endpoints.http_url,
            &["api", "hil", "workflows", &encoded],
        );
        let resp = self.http.get(url).send().await?;
        let events = resp.json::<Vec<HilEvent>>().await?;
        let messages = events.into_iter().map(MonitorMessage::from).collect();
        Ok(messages)
    }

    /// Post an answer/approval to a queued HIL event.
    pub async fn post_response(
        &self,
        instance_id: &str,
        message_id: Uuid,
        answer: Option<String>,
        response_type: ResponseType,
    ) -> crate::Result<()> {
        let encoded_instance = encode_segment(instance_id);
        let url = format!(
            "{}/api/hil/workflows/{}/{}/action",
            self.endpoints.http_url.as_str().trim_end_matches('/'),
            encoded_instance,
            message_id
        );
        let action = HilAction {
            answer,
            response_type: response_type.as_str().to_string(),
        };
        let resp = self.http.post(&url).json(&action).send().await?;
        let status = resp.status();
        if status != StatusCode::OK {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("HIL action POST failed: {status} {text}"));
        }
        Ok(())
    }
}

fn encode_segment(segment: &str) -> String {
    utf8_percent_encode(segment, PATH_SEGMENT_ENCODE_SET).to_string()
}

fn join_path(base: &reqwest::Url, segments: &[&str]) -> String {
    let mut url = base.as_str().trim_end_matches('/').to_string();
    for segment in segments {
        if !segment.is_empty() {
            url.push('/');
            url.push_str(segment);
        }
    }
    url
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
                    if !handle_ws_message(message, &mut writer, &event_tx).await {
                        break;
                    }
                }
            }
            Err(err) => {
                let _ = event_tx.send(MonitorEvent::ConnectionStatus(ConnectionStatus {
                    state: ConnectionState::Disconnected,
                    detail: Some(format!("ws connect failed: {err}")),
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
        if let Ok(channels) = client.fetch_channels().await {
            for channel in channels {
                let _ = poll_channel_messages(&client, &channel, &mut seen, &event_tx).await;
            }
        }
    }
}

async fn handle_ws_message(
    message: Result<Message, tokio_tungstenite::tungstenite::Error>,
    writer: &mut futures::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    event_tx: &UnboundedSender<MonitorEvent>,
) -> bool {
    match message {
        Ok(Message::Text(txt)) => {
            tracing::debug!("Received WebSocket text: {}", txt);
            handle_text_message(&txt, event_tx);
            true
        }
        Ok(Message::Binary(bytes)) => {
            handle_binary_message(bytes, event_tx);
            true
        }
        Ok(Message::Ping(payload)) => {
            let _ = writer.send(Message::Pong(payload)).await;
            true
        }
        Ok(Message::Close(_)) => false,
        Err(err) => {
            error!("WebSocket error: {}", err);
            false
        }
        _ => true,
    }
}

fn handle_text_message(txt: &str, event_tx: &UnboundedSender<MonitorEvent>) {
    match serde_json::from_str::<MonitorMessage>(txt) {
        Ok(parsed) => {
            tracing::info!("Parsed message: {:?}", parsed.summary);
            let _ = event_tx.send(MonitorEvent::Message(parsed));
        }
        Err(e) => {
            tracing::warn!("Failed to parse message: {} - Raw: {}", e, txt);
        }
    }
}

fn handle_binary_message(bytes: Vec<u8>, event_tx: &UnboundedSender<MonitorEvent>) {
    if let Ok(txt) = String::from_utf8(bytes) {
        if let Ok(parsed) = serde_json::from_str::<MonitorMessage>(&txt) {
            let _ = event_tx.send(MonitorEvent::Message(parsed));
        }
    }
}

async fn poll_channel_messages(
    client: &AiloopClient,
    channel: &str,
    seen: &mut HashSet<uuid::Uuid>,
    event_tx: &UnboundedSender<MonitorEvent>,
) -> crate::Result<()> {
    match client.fetch_channel_messages(channel, 20).await {
        Ok(messages) => {
            for message in messages {
                if seen.insert(message.id) {
                    let _ = event_tx.send(MonitorEvent::Message(message));
                }
            }
            Ok(())
        }
        Err(err) => {
            error!("polling channel {} failed: {}", channel, err);
            Err(err)
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
                instance_id,
                answer,
                response_type,
            } => {
                if let Err(err) = client
                    .post_response(&instance_id, message_id, answer, response_type)
                    .await
                {
                    error!("failed to post response: {}", err);
                    let _ = event_tx.send(MonitorEvent::ConnectionStatus(ConnectionStatus {
                        state: ConnectionState::Disconnected,
                        detail: Some(format!("response failed: {err}")),
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
            workflow_service_url: None,
        }
    }

    fn hil_event_json(event_id: &str, instance_id: &str, channel: &str) -> serde_json::Value {
        json!({
            "event_id": event_id,
            "instance_id": instance_id,
            "channel": channel,
            "event_type": "question",
            "question": "Proceed?",
            "choices": ["yes", "no"],
            "timestamp": "2026-04-11T10:00:00Z",
            "correlation_id": null,
            "timeout_seconds": 60,
            "status": "pending"
        })
    }

    #[tokio::test]
    async fn test_fetch_hil_instances_returns_list() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/hil/instances"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!(["instance-aaa", "instance-bbb"])),
            )
            .mount(&mock_server)
            .await;

        let client = AiloopClient::new(build_endpoints(&mock_server));
        let instances = client.fetch_channels().await.unwrap();

        assert_eq!(instances, vec!["instance-aaa", "instance-bbb"]);
    }

    #[tokio::test]
    async fn test_fetch_hil_instances_empty() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/hil/instances"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&mock_server)
            .await;

        let client = AiloopClient::new(build_endpoints(&mock_server));
        let instances = client.fetch_channels().await.unwrap();

        assert!(instances.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_channel_messages_converts_hil_events() {
        let mock_server = MockServer::start().await;
        let event_id = "a1b2c3d4-0000-0000-0000-000000000001";
        let instance_id = "instance-xyz";

        Mock::given(method("GET"))
            .and(path(format!("/api/hil/workflows/{instance_id}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!([hil_event_json(
                    event_id,
                    instance_id,
                    "project/main"
                )])),
            )
            .mount(&mock_server)
            .await;

        let client = AiloopClient::new(build_endpoints(&mock_server));
        let messages = client
            .fetch_channel_messages(instance_id, 20)
            .await
            .unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].instance_id, instance_id);
        assert_eq!(messages[0].channel, "project/main");
    }
}
