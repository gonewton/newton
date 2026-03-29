use chrono::{DateTime, Utc};
use newton_types::{NodeState, WorkflowInstance, WorkflowStatus};
use tokio::sync::mpsc;

enum NotifierEvent {
    WorkflowStarted(WorkflowInstance),
    NodeUpdated {
        instance_id: String,
        node: NodeState,
    },
    WorkflowCompleted {
        instance_id: String,
        status: WorkflowStatus,
        ended_at: DateTime<Utc>,
    },
}

/// HTTP notification client that pushes workflow lifecycle events to a newton serve instance.
/// Follows the WorkflowEmitter pattern: synchronous enqueue via unbounded channel,
/// async background task performs HTTP requests with fire-and-forget semantics.
#[derive(Debug)]
pub struct ServerNotifier {
    event_tx: mpsc::UnboundedSender<NotifierEvent>,
}

impl ServerNotifier {
    /// Create a new ServerNotifier that sends events to the given server URL.
    /// Spawns a background task to perform HTTP requests.
    pub fn new(server_url: String) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        tokio::spawn(Self::background_loop(server_url, event_rx));
        Self { event_tx }
    }

    /// Notify the server that a workflow has started.
    pub fn notify_workflow_started(&self, instance: WorkflowInstance) {
        let _ = self.event_tx.send(NotifierEvent::WorkflowStarted(instance));
    }

    /// Notify the server of a node state update.
    pub fn notify_node_updated(&self, instance_id: String, node: NodeState) {
        let _ = self
            .event_tx
            .send(NotifierEvent::NodeUpdated { instance_id, node });
    }

    /// Notify the server that a workflow has completed.
    pub fn notify_workflow_completed(
        &self,
        instance_id: String,
        status: WorkflowStatus,
        ended_at: DateTime<Utc>,
    ) {
        let _ = self.event_tx.send(NotifierEvent::WorkflowCompleted {
            instance_id,
            status,
            ended_at,
        });
    }

    async fn background_loop(server_url: String, mut rx: mpsc::UnboundedReceiver<NotifierEvent>) {
        let client = reqwest::Client::new();
        while let Some(event) = rx.recv().await {
            match event {
                NotifierEvent::WorkflowStarted(instance) => {
                    let url = format!("{}/api/workflows", server_url);
                    if let Err(e) = client.post(&url).json(&instance).send().await {
                        tracing::warn!(
                            code = "SERVER-NOTIFY-001",
                            error = %e,
                            "failed to notify server of workflow start"
                        );
                    }
                }
                NotifierEvent::NodeUpdated { instance_id, node } => {
                    let url = format!(
                        "{}/api/workflows/{}/nodes/{}",
                        server_url, instance_id, node.node_id
                    );
                    let update = serde_json::json!({
                        "status": node.status,
                        "started_at": node.started_at,
                        "ended_at": node.ended_at,
                        "operator_type": node.operator_type,
                    });
                    if let Err(e) = client.patch(&url).json(&update).send().await {
                        tracing::warn!(
                            code = "SERVER-NOTIFY-001",
                            error = %e,
                            "failed to notify server of node update"
                        );
                    }
                }
                NotifierEvent::WorkflowCompleted {
                    instance_id,
                    status,
                    ended_at,
                } => {
                    let url = format!("{}/api/workflows/{}", server_url, instance_id);
                    let update = serde_json::json!({
                        "status": status,
                        "ended_at": ended_at,
                    });
                    if let Err(e) = client.put(&url).json(&update).send().await {
                        tracing::warn!(
                            code = "SERVER-NOTIFY-001",
                            error = %e,
                            "failed to notify server of workflow completion"
                        );
                    }
                }
            }
        }
    }
}
