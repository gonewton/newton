use crate::ailoop_integration::bounded_queue::BoundedQueue;
use crate::ailoop_integration::config::AiloopConfig;
use crate::ailoop_integration::TransportState;
use anyhow::{anyhow, Context};
use chrono::Utc;
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use reqwest::{Client, Url};
use serde_json::json;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::{
    task::JoinHandle,
    time::{sleep, timeout, Duration},
};
use uuid::Uuid;

const QUEUE_CAPACITY: usize = 512;
const SCHEMA_VERSION: &str = "1.0";
const MAX_RETRIES: usize = 3;
const BASE_DELAY: Duration = Duration::from_millis(100);
const MAX_DELAY: Duration = Duration::from_secs(1);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
static SECRET_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(password|token|secret)=\S+").unwrap());

#[derive(Debug, Clone, Copy)]
/// Classification for forwarded tool output streams.
pub enum StreamKind {
    /// Stdout lines carry normal priority.
    Stdout,
    /// Stderr lines carry high priority.
    Stderr,
}

impl StreamKind {
    fn priority(&self) -> &'static str {
        match self {
            StreamKind::Stdout => "normal",
            StreamKind::Stderr => "high",
        }
    }

    fn message_type(&self) -> &'static str {
        match self {
            StreamKind::Stdout => "stdout",
            StreamKind::Stderr => "stderr",
        }
    }
}

#[derive(Debug)]
struct OutputMessage {
    execution_id: Uuid,
    workspace: String,
    text: String,
    stream: StreamKind,
    timestamp: chrono::DateTime<Utc>,
}

/// Handles forwarding stdout/stderr lines to ailoop while preserving local capture.
pub struct OutputForwarder {
    queue: Arc<BoundedQueue<OutputMessage>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    workspace: String,
}

impl OutputForwarder {
    pub fn new(config: Arc<AiloopConfig>, client: Client, state: Arc<TransportState>) -> Self {
        let queue = BoundedQueue::new(QUEUE_CAPACITY);
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_queue = queue.clone();
        let worker_shutdown = shutdown.clone();
        let worker_client = client.clone();
        let worker_channel = config.channel.clone();
        let worker_workspace = config.workspace_identifier.clone();
        let worker_http = config.http_url.clone();
        let worker_state = state.clone();

        let handle = tokio::spawn(async move {
            loop {
                if worker_shutdown.load(Ordering::SeqCst) && worker_queue.is_empty() {
                    break;
                }
                let message = worker_queue.next().await;
                if let Err(err) = send_message(
                    &worker_client,
                    &worker_http,
                    &worker_channel,
                    &worker_state,
                    &message,
                )
                .await
                {
                    tracing::warn!(error = ?err, "failed to forward output to ailoop");
                }
            }
        });

        OutputForwarder {
            queue,
            shutdown,
            handle: Some(handle),
            workspace: worker_workspace,
        }
    }

    /// Forward a single line of output to the configured channel.
    pub fn forward_line(&self, execution_id: Uuid, stream: StreamKind, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let redacted = redact_text(text);
        let message = OutputMessage {
            execution_id,
            workspace: self.workspace.clone(),
            text: redacted,
            stream,
            timestamp: Utc::now(),
        };
        self.queue.push(message);
    }

    /// Stop forwarding and flush pending output messages.
    pub async fn shutdown(mut self) -> crate::Result<()> {
        self.shutdown.store(true, Ordering::SeqCst);
        self.queue.notify_one();
        if let Some(handle) = self.handle.take() {
            timeout(SHUTDOWN_TIMEOUT, handle)
                .await
                .map_err(|_| anyhow!("timed out waiting for output forwarder to stop"))?
                .map_err(|err| anyhow!("ailoop forwarder task panicked: {}", err))?;
        }
        Ok(())
    }
}

async fn send_message(
    client: &Client,
    base_url: &Url,
    channel: &str,
    state: &TransportState,
    message: &OutputMessage,
) -> std::result::Result<(), anyhow::Error> {
    let url = ensure_messages_endpoint(base_url.as_str())?;
    let payload = json!({
        "channel": channel,
        "sender_type": "AGENT",
        "content": {
            "type": message.stream.message_type(),
            "priority": message.stream.priority(),
            "text": message.text,
            "schema_version": SCHEMA_VERSION,
            "details": {
                "execution_id": message.execution_id.to_string(),
                "workspace_identifier": message.workspace,
            },
        },
        "timestamp": message.timestamp.to_rfc3339(),
    });

    send_with_retries(|| {
        let client = client.clone();
        let payload = payload.clone();
        let url = url.clone();
        async move {
            let resp = client
                .post(url)
                .json(&payload)
                .send()
                .await
                .with_context(|| "sending ailoop stdout/stderr message")?;
            resp.error_for_status()
                .map(|_| ())
                .map_err(|err| anyhow!(err))
        }
    })
    .await
    .inspect_err(|err| {
        state.mark_failure(err.to_string());
    })
}

async fn send_with_retries<O, Fut>(mut operation: O) -> std::result::Result<(), anyhow::Error>
where
    O: FnMut() -> Fut,
    Fut: Future<Output = std::result::Result<(), anyhow::Error>>,
{
    let mut attempt = 0;
    let mut delay = BASE_DELAY;
    loop {
        match operation().await {
            Ok(_) => return Ok(()),
            Err(err) => {
                attempt += 1;
                if attempt >= MAX_RETRIES {
                    return Err(err);
                }
                sleep(delay).await;
                delay = (delay * 2).min(MAX_DELAY);
            }
        }
    }
}

fn ensure_messages_endpoint(base: &str) -> Result<Url, anyhow::Error> {
    let mut url = base.trim_end_matches('/').to_string();
    url.push_str("/api/v1/messages");
    reqwest::Url::parse(&url)
        .map_err(|err| anyhow::anyhow!("invalid ailoop message endpoint: {}", err))
}

fn redact_text(text: &str) -> String {
    SECRET_PATTERN
        .replace_all(text, |caps: &Captures| {
            let label = &caps[1];
            format!("{}=[REDACTED]", label)
        })
        .to_string()
}

impl Drop for OutputForwarder {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.queue.notify_one();
    }
}
