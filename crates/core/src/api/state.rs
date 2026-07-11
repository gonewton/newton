use crate::workflow::file_store::WorkflowFileStore;
use newton_types::{BroadcastEvent, OperatorDescriptor};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

pub const BROADCAST_CAPACITY: usize = 1024;

/// Default interval between server-initiated WebSocket `Ping` frames on the
/// streaming endpoints (`/ws`, `/stream/workflow/{id}/ws`,
/// `/stream/logs/{id}/{node_id}/ws`). Serves two purposes: keeping idle
/// connections alive through proxies/load balancers that reap quiet sockets,
/// and giving each handler's `tokio::select!` loop a bounded upper bound on
/// how long it can take to notice a dead peer (a failed ping send breaks the
/// loop, same as any other failed send).
///
/// Stored per-`AppState` (see `ws_ping_interval` / `with_ws_ping_interval`)
/// rather than used as a bare constant everywhere so integration tests can
/// override it to something much shorter than 30 real seconds.
pub const HEARTBEAT_PING_INTERVAL: Duration = Duration::from_secs(30);

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
    pub backend: Arc<dyn newton_types::BackendStore>,
    pub workflow_files: Option<Arc<dyn WorkflowFileStore>>,
    /// WS ping cadence for the streaming endpoints; defaults to
    /// `HEARTBEAT_PING_INTERVAL`. Overridable via `with_ws_ping_interval`
    /// (test-only in practice — there is no HTTP surface to change it).
    pub ws_ping_interval: Duration,
}

impl AppState {
    pub fn new(
        operators: Vec<OperatorDescriptor>,
        backend: Arc<dyn newton_types::BackendStore>,
    ) -> Self {
        let (events_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        AppState {
            operators: Arc::new(operators),
            events_tx,
            backend,
            workflow_files: None,
            ws_ping_interval: HEARTBEAT_PING_INTERVAL,
        }
    }

    pub fn with_workflow_files(mut self, store: Arc<dyn WorkflowFileStore>) -> Self {
        self.workflow_files = Some(store);
        self
    }

    /// Override the WS ping interval (default: `HEARTBEAT_PING_INTERVAL`,
    /// 30s). Intended for integration tests that need to observe ping
    /// cadence without waiting out the real interval; production code never
    /// calls this.
    pub fn with_ws_ping_interval(mut self, interval: Duration) -> Self {
        self.ws_ping_interval = interval;
        self
    }
}
