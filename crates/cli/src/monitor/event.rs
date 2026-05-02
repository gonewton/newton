use crate::monitor::message::MonitorMessage;
use newton_types::BroadcastEvent;
use uuid::Uuid;

/// Commands emitted by the UI that require HTTP requests.
#[derive(Debug)]
pub enum MonitorCommand {
    /// Respond to a question or authorization via POST /api/hil/workflows/:instance_id/:event_id/action.
    Respond {
        /// Original message being answered/approved.
        message_id: Uuid,
        /// Workflow instance that owns the HIL event.
        instance_id: String,
        /// Optional textual answer (used for `text` responses).
        answer: Option<String>,
        /// How the response should be treated.
        response_type: ResponseType,
    },
}

/// Response types recognized by the ailoop POST API.
#[derive(Debug, Copy, Clone)]
pub enum ResponseType {
    Text,
    AuthorizationApproved,
    AuthorizationDenied,
    Timeout,
    Cancelled,
}

impl ResponseType {
    /// Convert to the canonical string used by the API.
    pub fn as_str(self) -> &'static str {
        match self {
            ResponseType::Text => "text",
            ResponseType::AuthorizationApproved => "authorization_approved",
            ResponseType::AuthorizationDenied => "authorization_denied",
            ResponseType::Timeout => "timeout",
            ResponseType::Cancelled => "cancelled",
        }
    }
}

/// Events flowing from the networking layer into the UI.
#[derive(Debug)]
pub enum MonitorEvent {
    /// WebSocket/HTTP connection status change.
    ConnectionStatus(ConnectionStatus),
    /// Message received from a channel or from backfill/polling.
    Message(MonitorMessage),
    /// Workflow event received from the backend API server.
    Workflow(WorkflowEvent),
}

/// Workflow-specific events from the backend API.
#[derive(Debug, Clone)]
pub enum WorkflowEvent {
    /// Workflow instance was updated.
    InstanceUpdated { instance_id: String },
    /// Node state changed.
    NodeStateChanged {
        instance_id: String,
        node_id: String,
    },
    /// Log message received.
    LogMessage {
        instance_id: String,
        node_id: String,
        message: String,
    },
    /// HIL event occurred.
    HilEvent {
        instance_id: String,
        event_id: String,
    },
    /// Product-facing plan record changed.
    PlanUpdate { plan_id: String },
    /// Product-facing execution record changed.
    ExecutionUpdate {
        execution_id: String,
        plan_id: Option<String>,
        status: String,
        created_at: String,
    },
}

impl From<BroadcastEvent> for WorkflowEvent {
    fn from(event: BroadcastEvent) -> Self {
        match event {
            BroadcastEvent::WorkflowInstanceUpdated { instance_id } => {
                WorkflowEvent::InstanceUpdated { instance_id }
            }
            BroadcastEvent::NodeStateChanged {
                instance_id,
                node_id,
            } => WorkflowEvent::NodeStateChanged {
                instance_id,
                node_id,
            },
            BroadcastEvent::LogMessage {
                instance_id,
                node_id,
                message,
            } => WorkflowEvent::LogMessage {
                instance_id,
                node_id,
                message,
            },
            BroadcastEvent::HilEvent {
                instance_id,
                event_id,
            } => WorkflowEvent::HilEvent {
                instance_id,
                event_id,
            },
            BroadcastEvent::PlanUpdate { plan_id } => WorkflowEvent::PlanUpdate { plan_id },
            BroadcastEvent::ExecutionUpdate {
                execution_id,
                plan_id,
                status,
                created_at,
            } => WorkflowEvent::ExecutionUpdate {
                execution_id,
                plan_id,
                status,
                created_at,
            },
        }
    }
}

/// Current connection health for display in the UI.
#[derive(Debug, Clone)]
pub struct ConnectionStatus {
    /// Enumerated state of the WebSocket.
    pub state: ConnectionState,
    /// Descriptive detail (error message, health check result, etc.).
    pub detail: Option<String>,
}

/// Low-level state for the WebSocket connection.
#[derive(Debug, Clone, Copy)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnected,
}
