use dashmap::DashMap;
use newton_types::{BroadcastEvent, HilEvent, LogLine, OperatorDescriptor, WorkflowInstance};
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

pub const BROADCAST_CAPACITY: usize = 1024;

#[derive(Clone)]
pub struct AppState {
    pub instances: Arc<DashMap<String, WorkflowInstance>>,
    pub hil_events: Arc<DashMap<Uuid, HilEvent>>,
    pub operators: Arc<Vec<OperatorDescriptor>>,
    pub events_tx: broadcast::Sender<BroadcastEvent>,
    pub logs: Arc<DashMap<(String, String), Vec<LogLine>>>,
}

impl AppState {
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
