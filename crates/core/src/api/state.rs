use crate::workflow::file_store::WorkflowFileStore;
use newton_types::{BroadcastEvent, OperatorDescriptor};
use std::sync::Arc;
use tokio::sync::broadcast;

pub const BROADCAST_CAPACITY: usize = 1024;

/// Application state shared across all HTTP handlers.
///
/// DashMap caches (instances, hil_events, logs) have been removed.
/// BackendStore is the single authoritative source for all runtime state.
/// events_tx remains transient by design: it is a pub/sub channel for SSE/WebSocket
/// clients and is NOT persisted.
#[derive(Clone)]
pub struct AppState {
    pub operators: Arc<Vec<OperatorDescriptor>>,
    pub events_tx: broadcast::Sender<BroadcastEvent>,
    pub backend: Arc<dyn newton_backend::BackendStore>,
    pub workflow_files: Option<Arc<dyn WorkflowFileStore>>,
}

impl AppState {
    pub fn new(
        operators: Vec<OperatorDescriptor>,
        backend: Arc<dyn newton_backend::BackendStore>,
    ) -> Self {
        let (events_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        AppState {
            operators: Arc::new(operators),
            events_tx,
            backend,
            workflow_files: None,
        }
    }

    pub fn with_workflow_files(mut self, store: Arc<dyn WorkflowFileStore>) -> Self {
        self.workflow_files = Some(store);
        self
    }
}
