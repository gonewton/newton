use crate::monitor::message::MonitorMessage;
use uuid::Uuid;

/// Commands emitted by the UI that require HTTP requests.
#[derive(Debug)]
pub enum MonitorCommand {
    /// Respond to a question or authorization via POST /api/v1/messages/:id/response.
    Respond {
        /// Original message being answered/approved.
        message_id: Uuid,
        /// Optional textual answer (used for `text` responses).
        answer: Option<String>,
        /// How the response should be treated by ailoop.
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
