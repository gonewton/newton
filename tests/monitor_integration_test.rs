use std::path::PathBuf;
use std::time::Duration;

use newton::monitor::client::{initial_backfill, AiloopClient};
use newton::monitor::config::MonitorEndpoints;
use newton::monitor::event::MonitorEvent;
use newton::monitor::message::MessageKind;
use reqwest::Url;
use serde_json::json;
use tokio::sync::mpsc::unbounded_channel;
use tokio::time::timeout;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn build_endpoints(server: &MockServer) -> MonitorEndpoints {
    let http_url = Url::parse(&server.uri()).expect("invalid mock server URL");
    let ws_url = Url::parse(&server.uri().replace("http", "ws")).expect("invalid mock server URL");
    MonitorEndpoints {
        http_url,
        ws_url,
        workspace_root: PathBuf::from("."),
    }
}

#[tokio::test]
async fn test_initial_backfill_retrieves_messages_from_server() {
    let mock_server = MockServer::start().await;
    let channel = "test-backfill-channel";

    Mock::given(method("GET"))
        .and(path("/api/channels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "channels": [
                {
                    "name": channel,
                    "message_count": 2,
                    "oldest_message": "2026-02-14T10:00:00Z",
                    "newest_message": "2026-02-14T11:00:00Z"
                }
            ]
        })))
        .mount(&mock_server)
        .await;

    let question_id = Uuid::new_v4().to_string();
    let auth_id = Uuid::new_v4().to_string();
    let messages = json!([
        {
            "id": question_id,
            "channel": channel,
            "content": {
                "type": "question",
                "text": "Test question?",
                "timeout_seconds": 60
            },
            "timestamp": "2026-02-14T12:00:00Z"
        },
        {
            "id": auth_id,
            "channel": channel,
            "content": {
                "type": "authorization",
                "action": "Test authorization",
                "timeout_seconds": 45
            },
            "timestamp": "2026-02-14T12:05:00Z"
        }
    ]);

    let messages_path = format!("/api/channels/{}/messages", channel);
    Mock::given(method("GET"))
        .and(path(messages_path))
        .respond_with(ResponseTemplate::new(200).set_body_json(messages))
        .mount(&mock_server)
        .await;

    let endpoints = build_endpoints(&mock_server);
    let client = AiloopClient::new(endpoints);
    let (event_tx, mut event_rx) = unbounded_channel();

    initial_backfill(&client, &event_tx)
        .await
        .expect("backfill should succeed");

    let mut received = Vec::new();
    timeout(Duration::from_secs(2), async {
        while received.len() < 2 {
            match event_rx.recv().await {
                Some(MonitorEvent::Message(message)) => {
                    received.push(message);
                }
                Some(_) => continue,
                None => break,
            }
        }
    })
    .await
    .expect("timed out waiting for backfill events");

    assert_eq!(received.len(), 2);
    assert!(received.iter().any(|msg| msg.kind == MessageKind::Question));
    assert!(received
        .iter()
        .any(|msg| msg.kind == MessageKind::Authorization));
}
