use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

mod models;
mod store;

pub use models::*;
pub use store::*;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowInstance {
    pub instance_id: String,
    pub workflow_id: String,
    pub status: WorkflowStatus,
    pub nodes: Vec<NodeState>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_plan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowStatus {
    Running,
    Succeeded,
    Failed,
    Paused,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodeState {
    pub node_id: String,
    pub status: NodeStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Timeout,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HilEvent {
    pub event_id: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum HilEventType {
    Question,
    Authorization,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum HilStatus {
    Pending,
    Resolved,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LogLine {
    pub instance_id: String,
    pub node_id: String,
    pub level: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OperatorDescriptor {
    pub operator_type: String,
    pub description: String,
    pub params_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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
    HilEvent {
        instance_id: String,
        event_id: String,
    },
    #[serde(rename = "plan_update")]
    PlanUpdate {
        plan_id: String,
        /// Owning workflow instance id, when this plan is currently linked to
        /// one (e.g. immediately after approval, which creates the
        /// `ExecutionRecord` that becomes the plan's running instance).
        /// `None` when no instance is linked (e.g. still awaiting approval,
        /// or rejected without ever having run) — instance-scoped streams
        /// treat `None` as "not addressable to any instance" and drop the
        /// event rather than guessing (see `should_send_event`).
        instance_id: Option<String>,
    },
    #[serde(rename = "execution_update")]
    ExecutionUpdate {
        execution_id: String,
        plan_id: Option<String>,
        status: String,
        created_at: String,
        /// Owning workflow instance id. Every Execution has one: until a real
        /// workflow instance attaches to the `ExecutionRecord`, this falls
        /// back to the execution's own id, mirroring
        /// `ExecutionItem::instance_id`'s existing NULL-fallback convention
        /// (see `list_executions_db`) so scoped streams can filter on it
        /// unconditionally.
        instance_id: String,
    },
    /// Emitted after a Finding is created, patched, or unblocked. Id-only,
    /// same shape as `PlanUpdate`/`ExecutionUpdate`: clients re-fetch the
    /// authoritative record from `GET /findings/{id}` on receipt.
    #[serde(rename = "finding_update")]
    FindingUpdate { finding_id: String },
    /// Emitted after a Change Request is created or patched. Id-only; clients
    /// re-fetch from `GET /change-requests/{id}` on receipt.
    #[serde(rename = "change_request_update")]
    ChangeRequestUpdate { change_request_id: String },
    /// Emitted after any catalog resource (Product, Component, Repo, Module,
    /// ModuleDependency, Kpi, EvalRun, Grade) is created, replaced, patched,
    /// or deleted. `resource` names the kind (e.g. `"product"`, `"module"`)
    /// so clients can route the id to the right re-fetch endpoint.
    #[serde(rename = "catalog_update")]
    CatalogUpdate { resource: String, id: String },
    /// Emitted when an `OptimizeRun`'s status changes or a `Cycle` is
    /// appended, closing the ADR-0013 event commitment. `cycle` is `Some`
    /// when the update accompanies a Cycle append, `None` for a bare Run
    /// status/field change. Published by the in-process optimize driver
    /// (spec 073) via the same `events_tx` Plans/HIL already use.
    #[serde(rename = "optimize_run_update")]
    OptimizeRunUpdate { run_id: String, cycle: Option<i64> },
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowDefinition {
    pub workflow_id: String,
    pub definition: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HilAction {
    pub answer: Option<String>,
    pub response_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiError {
    pub code: String,
    pub category: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}
