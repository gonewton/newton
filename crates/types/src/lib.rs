use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInstance {
    pub instance_id: String,
    pub workflow_id: String,
    pub status: WorkflowStatus,
    pub nodes: Vec<NodeState>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowStatus {
    Running,
    Succeeded,
    Failed,
    Paused,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeState {
    pub node_id: String,
    pub status: NodeStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Timeout,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HilEvent {
    pub event_id: Uuid,
    pub instance_id: String,
    pub node_id: Option<String>,
    pub channel: String,
    pub event_type: HilEventType,
    pub question: String,
    pub choices: Vec<String>,
    pub timeout_seconds: Option<u64>,
    pub correlation_id: Option<Uuid>,
    pub status: HilStatus,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HilEventType {
    Question,
    Authorization,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HilStatus {
    Pending,
    Resolved,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub instance_id: String,
    pub node_id: String,
    pub level: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorDescriptor {
    pub operator_type: String,
    pub description: String,
    pub params_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BroadcastEvent {
    #[serde(rename = "workflowInstanceUpdated")]
    WorkflowInstanceUpdated { instance_id: String },
    #[serde(rename = "nodeStateChanged")]
    NodeStateChanged {
        instance_id: String,
        node_id: String,
    },
    #[serde(rename = "logMessage")]
    LogMessage {
        instance_id: String,
        node_id: String,
        message: String,
    },
    #[serde(rename = "hilEvent")]
    HilEvent { instance_id: String, event_id: Uuid },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub workflow_id: String,
    pub definition: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HilAction {
    pub answer: Option<String>,
    pub response_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub category: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}
