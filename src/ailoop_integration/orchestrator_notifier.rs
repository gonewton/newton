use crate::ailoop_integration::bounded_queue::BoundedQueue;
use crate::ailoop_integration::config::AiloopConfig;
use crate::ailoop_integration::workflow_emitter::WorkflowEvent;
use crate::ailoop_integration::TransportState;
use anyhow::{anyhow, Context};
use reqwest::Client;
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
use url::Url;

const QUEUE_CAPACITY: usize = 128;
const MAX_RETRIES: usize = 3;
const BASE_DELAY: Duration = Duration::from_millis(100);
const MAX_DELAY: Duration = Duration::from_secs(1);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Notifier that emits orchestrator lifecycle events to ailoop.
pub struct OrchestratorNotifier {
    queue: Arc<BoundedQueue<WorkflowEvent>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl OrchestratorNotifier {
    /// Spawn the background worker that streams workflow events to ailoop.
    pub fn new(config: Arc<AiloopConfig>, client: Client, state: Arc<TransportState>) -> Self {
        let queue = BoundedQueue::new(QUEUE_CAPACITY);
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_queue = queue.clone();
        let worker_shutdown = shutdown.clone();
        let worker_config = config.clone();
        let worker_client = client.clone();
        let worker_state = state.clone();
        let handle = tokio::spawn(async move {
            loop {
                if worker_shutdown.load(Ordering::SeqCst) && worker_queue.is_empty() {
                    break;
                }
                let event = worker_queue.next().await;
                if let Err(err) =
                    send_event(&worker_client, &worker_config, &worker_state, &event).await
                {
                    tracing::warn!(error = ?err, "failed to emit ailoop orchestrator event");
                }
            }
        });

        OrchestratorNotifier {
            queue,
            shutdown,
            handle: Some(handle),
        }
    }

    /// Enqueue a workflow event for delivery.
    pub fn enqueue(&self, event: WorkflowEvent) {
        self.queue.push(event);
    }

    /// Signal the worker to stop and wait for it to flush.
    pub async fn shutdown(mut self) -> crate::Result<()> {
        self.shutdown.store(true, Ordering::SeqCst);
        self.queue.notify_one();
        if let Some(handle) = self.handle.take() {
            timeout(SHUTDOWN_TIMEOUT, handle)
                .await
                .map_err(|_| anyhow!("timed out waiting for orchestrator notifier to stop"))?
                .map_err(|err| anyhow!("ailoop notifier task panicked: {}", err))?;
        }
        Ok(())
    }
}

async fn send_event(
    client: &Client,
    config: &AiloopConfig,
    state: &TransportState,
    event: &WorkflowEvent,
) -> std::result::Result<(), anyhow::Error> {
    let url = ensure_messages_endpoint(config.http_url.as_str())?;
    let summary = event
        .message
        .clone()
        .unwrap_or_else(|| event.event_type.as_str().to_string());
    let payload = json!({
        "channel": config.channel,
        "sender_type": "AGENT",
        "content": {
            "type": "workflow_progress",
            "priority": "normal",
            "text": summary,
            "details": event.to_payload(),
        },
        "timestamp": event.timestamp.to_rfc3339(),
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
                .with_context(|| "sending ailoop workflow event")?;
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
    Fut: Future<Output = Result<(), anyhow::Error>>,
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
    Url::parse(&url).map_err(|err| anyhow!("invalid ailoop message endpoint: {}", err))
}

impl Drop for OrchestratorNotifier {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.queue.notify_one();
    }
}
