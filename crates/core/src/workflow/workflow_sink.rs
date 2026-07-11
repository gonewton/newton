use chrono::{DateTime, Utc};
use newton_types::{NodeState, WorkflowInstance, WorkflowStatus};
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::mpsc;

use newton_types::BackendStore;

/// Trait for receiving workflow lifecycle events.
pub trait WorkflowSink: Send + Sync + Debug {
    fn notify_workflow_started(&self, instance: WorkflowInstance);
    fn notify_node_updated(&self, instance_id: String, node: NodeState);
    fn notify_workflow_completed(
        &self,
        instance_id: String,
        status: WorkflowStatus,
        ended_at: DateTime<Utc>,
    );
}

enum SinkEvent {
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

#[derive(Debug)]
pub struct DbSink {
    event_tx: mpsc::UnboundedSender<SinkEvent>,
}

impl DbSink {
    pub fn new(backend: Arc<dyn BackendStore>) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        tokio::spawn(Self::background_loop(backend, event_rx));
        Self { event_tx }
    }

    async fn background_loop(
        backend: Arc<dyn BackendStore>,
        mut rx: mpsc::UnboundedReceiver<SinkEvent>,
    ) {
        while let Some(event) = rx.recv().await {
            match event {
                SinkEvent::WorkflowStarted(instance) => {
                    if let Err(e) = backend.upsert_workflow_instance(&instance).await {
                        tracing::warn!(
                            code = "DB-SINK-001",
                            error = %e.message,
                            "DbSink: failed to upsert workflow instance"
                        );
                    }
                }
                SinkEvent::NodeUpdated { instance_id, node } => {
                    if let Err(e) = backend.upsert_node_state(&instance_id, &node).await {
                        tracing::warn!(
                            code = "DB-SINK-001",
                            error = %e.message,
                            "DbSink: failed to upsert node state"
                        );
                    }
                }
                SinkEvent::WorkflowCompleted {
                    instance_id,
                    status,
                    ended_at,
                } => {
                    if let Err(e) = backend
                        .update_workflow_status(&instance_id, status, ended_at)
                        .await
                    {
                        tracing::warn!(
                            code = "DB-SINK-001",
                            error = %e.message,
                            "DbSink: failed to update completion status"
                        );
                    }
                }
            }
        }
    }
}

impl WorkflowSink for DbSink {
    fn notify_workflow_started(&self, instance: WorkflowInstance) {
        if let Err(e) = self.event_tx.send(SinkEvent::WorkflowStarted(instance)) {
            tracing::debug!(error = %e, "DbSink: failed to enqueue workflow-started event");
        }
    }

    fn notify_node_updated(&self, instance_id: String, node: NodeState) {
        if let Err(e) = self
            .event_tx
            .send(SinkEvent::NodeUpdated { instance_id, node })
        {
            tracing::debug!(error = %e, "DbSink: failed to enqueue node-updated event");
        }
    }

    fn notify_workflow_completed(
        &self,
        instance_id: String,
        status: WorkflowStatus,
        ended_at: DateTime<Utc>,
    ) {
        if let Err(e) = self.event_tx.send(SinkEvent::WorkflowCompleted {
            instance_id,
            status,
            ended_at,
        }) {
            tracing::debug!(error = %e, "DbSink: failed to enqueue workflow-completed event");
        }
    }
}

#[derive(Debug)]
pub struct FanoutSink(pub Vec<Arc<dyn WorkflowSink>>);

impl WorkflowSink for FanoutSink {
    fn notify_workflow_started(&self, instance: WorkflowInstance) {
        for s in &self.0 {
            s.notify_workflow_started(instance.clone());
        }
    }

    fn notify_node_updated(&self, instance_id: String, node: NodeState) {
        for s in &self.0 {
            s.notify_node_updated(instance_id.clone(), node.clone());
        }
    }

    fn notify_workflow_completed(
        &self,
        instance_id: String,
        status: WorkflowStatus,
        ended_at: DateTime<Utc>,
    ) {
        for s in &self.0 {
            s.notify_workflow_completed(instance_id.clone(), status.clone(), ended_at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use newton_types::{NodeStatus, WorkflowStatus};

    #[tokio::test]
    async fn test_db_sink_workflow_started() {
        let backend = Arc::new(
            newton_backend::SqliteBackendStore::new_in_memory()
                .await
                .unwrap(),
        );
        let backend_dyn: Arc<dyn BackendStore> = backend.clone();
        let sink = DbSink::new(backend_dyn);

        let instance = WorkflowInstance {
            instance_id: "test-001".to_string(),
            workflow_id: "wf-1".to_string(),
            status: WorkflowStatus::Running,
            nodes: vec![],
            started_at: chrono::Utc::now(),
            ended_at: None,
            definition: None,
            linked_plan_id: None,
        };
        sink.notify_workflow_started(instance);

        let node = NodeState {
            node_id: "node-a".to_string(),
            status: NodeStatus::Running,
            started_at: Some(chrono::Utc::now()),
            ended_at: None,
            operator_type: None,
        };
        sink.notify_node_updated("test-001".to_string(), node);

        let ended_at = chrono::Utc::now();
        sink.notify_workflow_completed("test-001".to_string(), WorkflowStatus::Succeeded, ended_at);

        // Give background loop time to process all three events
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let fetched = backend.get_workflow_instance("test-001").await.unwrap();
        assert_eq!(fetched.instance_id, "test-001");
        assert_eq!(fetched.status, WorkflowStatus::Succeeded);
        assert!(fetched.ended_at.is_some());

        let node_states = backend
            .list_node_states_for_instance("test-001")
            .await
            .unwrap();
        assert!(node_states.iter().any(|n| n.node_id == "node-a"));
    }

    /// A dropped receiver (background loop task gone, e.g. during shutdown) means
    /// `event_tx.send` fails. S7: this must be logged at `debug!` and MUST NOT panic
    /// or escalate to `warn!`/`error!`. We construct `DbSink` directly (bypassing
    /// `DbSink::new`'s spawned background loop) so we control the receiver's lifetime
    /// and can drop it before exercising the send-failure path. There's no
    /// log-assertion crate (e.g. `tracing-test`) already in use in this codebase, so
    /// this test proves the non-panicking behavior rather than asserting log content.
    #[tokio::test]
    async fn test_db_sink_dropped_receiver_does_not_panic() {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        drop(event_rx);
        let sink = DbSink { event_tx };

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
        sink.notify_workflow_started(instance);

        let node = NodeState {
            node_id: "node-dropped".to_string(),
            status: NodeStatus::Running,
            started_at: Some(chrono::Utc::now()),
            ended_at: None,
            operator_type: None,
        };
        sink.notify_node_updated("dropped-001".to_string(), node);

        sink.notify_workflow_completed(
            "dropped-001".to_string(),
            WorkflowStatus::Succeeded,
            chrono::Utc::now(),
        );
        // Reaching this point without panicking proves the send-failure branch is
        // handled gracefully.
    }

    #[tokio::test]
    async fn test_fanout_sink_routes_to_both() {
        let backend1 = Arc::new(
            newton_backend::SqliteBackendStore::new_in_memory()
                .await
                .unwrap(),
        );
        let backend2 = Arc::new(
            newton_backend::SqliteBackendStore::new_in_memory()
                .await
                .unwrap(),
        );
        let b1_dyn: Arc<dyn BackendStore> = backend1.clone();
        let b2_dyn: Arc<dyn BackendStore> = backend2.clone();
        let sink1 = Arc::new(DbSink::new(b1_dyn)) as Arc<dyn WorkflowSink>;
        let sink2 = Arc::new(DbSink::new(b2_dyn)) as Arc<dyn WorkflowSink>;
        let fanout = FanoutSink(vec![sink1, sink2]);

        let instance = WorkflowInstance {
            instance_id: "test-002".to_string(),
            workflow_id: "wf-2".to_string(),
            status: WorkflowStatus::Running,
            nodes: vec![],
            started_at: chrono::Utc::now(),
            ended_at: None,
            definition: None,
            linked_plan_id: None,
        };
        fanout.notify_workflow_started(instance);

        let node = NodeState {
            node_id: "node-b".to_string(),
            status: NodeStatus::Succeeded,
            started_at: Some(chrono::Utc::now()),
            ended_at: Some(chrono::Utc::now()),
            operator_type: None,
        };
        fanout.notify_node_updated("test-002".to_string(), node);

        let ended_at = chrono::Utc::now();
        fanout.notify_workflow_completed(
            "test-002".to_string(),
            WorkflowStatus::Succeeded,
            ended_at,
        );

        // Give background loop time to process all three events in both sinks
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let r1 = backend1.get_workflow_instance("test-002").await.unwrap();
        let r2 = backend2.get_workflow_instance("test-002").await.unwrap();
        assert_eq!(r1.instance_id, "test-002");
        assert_eq!(r1.status, WorkflowStatus::Succeeded);
        assert!(r1.ended_at.is_some());
        assert_eq!(r2.instance_id, "test-002");
        assert_eq!(r2.status, WorkflowStatus::Succeeded);
        assert!(r2.ended_at.is_some());

        let nodes1 = backend1
            .list_node_states_for_instance("test-002")
            .await
            .unwrap();
        let nodes2 = backend2
            .list_node_states_for_instance("test-002")
            .await
            .unwrap();
        assert!(nodes1.iter().any(|n| n.node_id == "node-b"));
        assert!(nodes2.iter().any(|n| n.node_id == "node-b"));
    }
}
