use dashmap::DashMap;
use newton_types::{BroadcastEvent, HilEvent, LogLine, OperatorDescriptor, WorkflowInstance};
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

/// Capacity for the broadcast channel used by streaming endpoints.
pub const BROADCAST_CAPACITY: usize = 1024;

/// Shared in-memory state for the Newton HTTP API.
///
/// This state is wrapped in an `Arc` and passed to all Axum handlers via
/// `.with_state(state)` so the API remains a pure composition of resource routers.
#[derive(Clone)]
pub struct AppState {
    /// Known workflow instances, keyed by instance id.
    pub instances: Arc<DashMap<String, WorkflowInstance>>,
    /// Human-in-the-loop events, keyed by event UUID.
    pub hil_events: Arc<DashMap<Uuid, HilEvent>>,
    /// Operator descriptors exposed by `/api/operators`.
    pub operators: Arc<Vec<OperatorDescriptor>>,
    /// Broadcast channel for streaming workflow events (WebSocket + SSE).
    pub events_tx: broadcast::Sender<BroadcastEvent>,
    /// Stored log lines for workflow nodes.
    pub logs: Arc<DashMap<(String, String), Vec<LogLine>>>,
}

impl AppState {
    /// Create a new `AppState` seeded with the configured operators.
    pub fn new(operators: Vec<OperatorDescriptor>) -> Self {
        let (events_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        AppState {
            instances: Arc::new(DashMap::new()),
            hil_events: Arc::new(DashMap::new()),
            operators: Arc::new(operators),
            events_tx,
            logs: Arc::new(DashMap::new()),
        }
    }
}
