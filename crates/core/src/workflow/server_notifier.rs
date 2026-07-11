use chrono::{DateTime, Utc};
use newton_types::{NodeState, WorkflowInstance, WorkflowStatus};
use tokio::sync::mpsc;

use crate::workflow::workflow_sink::WorkflowSink;

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
        if let Err(e) = self.event_tx.send(NotifierEvent::WorkflowStarted(instance)) {
            tracing::debug!(error = %e, "ServerNotifier: failed to enqueue workflow-started event");
        }
    }

    /// Notify the server of a node state update.
    pub fn notify_node_updated(&self, instance_id: String, node: NodeState) {
        if let Err(e) = self
            .event_tx
            .send(NotifierEvent::NodeUpdated { instance_id, node })
        {
            tracing::debug!(error = %e, "ServerNotifier: failed to enqueue node-updated event");
        }
    }

    /// Notify the server that a workflow has completed.
    pub fn notify_workflow_completed(
        &self,
        instance_id: String,
        status: WorkflowStatus,
        ended_at: DateTime<Utc>,
    ) {
        if let Err(e) = self.event_tx.send(NotifierEvent::WorkflowCompleted {
            instance_id,
            status,
            ended_at,
        }) {
            tracing::debug!(error = %e, "ServerNotifier: failed to enqueue workflow-completed event");
        }
    }

    async fn background_loop(server_url: String, mut rx: mpsc::UnboundedReceiver<NotifierEvent>) {
        let client = reqwest::Client::new();
        while let Some(event) = rx.recv().await {
            match event {
                NotifierEvent::WorkflowStarted(instance) => {
                    let url = format!("{server_url}/api/v1/workflows");
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
                        "{}/api/v1/workflows/{}/nodes/{}",
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
                    let url = format!("{server_url}/api/v1/workflows/{instance_id}");
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

impl WorkflowSink for ServerNotifier {
    fn notify_workflow_started(&self, instance: WorkflowInstance) {
        self.notify_workflow_started(instance);
    }

    fn notify_node_updated(&self, instance_id: String, node: NodeState) {
        self.notify_node_updated(instance_id, node);
    }

    fn notify_workflow_completed(
        &self,
        instance_id: String,
        status: WorkflowStatus,
        ended_at: DateTime<Utc>,
    ) {
        self.notify_workflow_completed(instance_id, status, ended_at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use newton_types::{NodeStatus, WorkflowStatus};

    /// A dropped receiver (background loop task gone, e.g. during shutdown) means
    /// `event_tx.send` fails. S7: this must be logged at `debug!` and MUST NOT panic
    /// or escalate to `warn!`/`error!`. We construct `ServerNotifier` directly
    /// (bypassing `ServerNotifier::new`'s spawned background loop) so we control the
    /// receiver's lifetime and can drop it before exercising the send-failure path.
    /// There's no log-assertion crate (e.g. `tracing-test`) already in use in this
    /// codebase, so this test proves the non-panicking behavior rather than
    /// asserting log content.
    #[tokio::test]
    async fn test_server_notifier_dropped_receiver_does_not_panic() {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        drop(event_rx);
        let notifier = ServerNotifier { event_tx };

        let instance = WorkflowInstance {
            instance_id: "dropped-001".to_string(),
            workflow_id: "wf-dropped".to_string(),
            status: WorkflowStatus::Running,
            nodes: vec![],
            started_at: chrono::Utc::now(),
            ended_at: None,
            definition: None,
            linked_plan_id: None,
        };
        notifier.notify_workflow_started(instance);

        let node = NodeState {
            node_id: "node-dropped".to_string(),
            status: NodeStatus::Running,
            started_at: Some(chrono::Utc::now()),
            ended_at: None,
            operator_type: None,
        };
        notifier.notify_node_updated("dropped-001".to_string(), node);

        notifier.notify_workflow_completed(
            "dropped-001".to_string(),
            WorkflowStatus::Succeeded,
            chrono::Utc::now(),
        );
        // Reaching this point without panicking proves the send-failure branch is
        // handled gracefully.
    }
}
