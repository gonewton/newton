use super::rows::*;
use crate::err_internal;

use chrono::{DateTime, Utc};
use newton_types::*;
use uuid::Uuid;

pub(super) fn parse_dt(s: &str) -> Result<DateTime<Utc>, ApiError> {
    s.parse::<DateTime<Utc>>()
        .map_err(|_| err_internal(&format!("invalid datetime: {s}")))
}

pub(super) fn parse_opt_dt(s: Option<&str>) -> Result<Option<DateTime<Utc>>, ApiError> {
    match s {
        None => Ok(None),
        Some(v) => Ok(Some(parse_dt(v)?)),
    }
}

pub(super) fn parse_workflow_status(s: &str) -> WorkflowStatus {
    match s {
        "running" => WorkflowStatus::Running,
        "succeeded" => WorkflowStatus::Succeeded,
        "failed" => WorkflowStatus::Failed,
        "paused" => WorkflowStatus::Paused,
        "cancelled" => WorkflowStatus::Cancelled,
        _ => WorkflowStatus::Running,
    }
}

pub(super) fn workflow_status_str(s: &WorkflowStatus) -> &'static str {
    match s {
        WorkflowStatus::Running => "running",
        WorkflowStatus::Succeeded => "succeeded",
        WorkflowStatus::Failed => "failed",
        WorkflowStatus::Paused => "paused",
        WorkflowStatus::Cancelled => "cancelled",
    }
}

pub(super) fn parse_node_status(s: &str) -> NodeStatus {
    match s {
        "pending" => NodeStatus::Pending,
        "running" => NodeStatus::Running,
        "succeeded" => NodeStatus::Succeeded,
        "failed" => NodeStatus::Failed,
        "timeout" => NodeStatus::Timeout,
        "cancelled" => NodeStatus::Cancelled,
        _ => NodeStatus::Pending,
    }
}

pub(super) fn node_status_str(s: &NodeStatus) -> &'static str {
    match s {
        NodeStatus::Pending => "pending",
        NodeStatus::Running => "running",
        NodeStatus::Succeeded => "succeeded",
        NodeStatus::Failed => "failed",
        NodeStatus::Timeout => "timeout",
        NodeStatus::Cancelled => "cancelled",
    }
}

pub(super) fn parse_hil_event_type(s: &str) -> HilEventType {
    match s {
        "authorization" => HilEventType::Authorization,
        _ => HilEventType::Question,
    }
}

pub(super) fn hil_event_type_str(t: &HilEventType) -> &'static str {
    match t {
        HilEventType::Question => "question",
        HilEventType::Authorization => "authorization",
    }
}

pub(super) fn parse_hil_status(s: &str) -> HilStatus {
    match s {
        "resolved" => HilStatus::Resolved,
        "timed_out" => HilStatus::TimedOut,
        "cancelled" => HilStatus::Cancelled,
        _ => HilStatus::Pending,
    }
}

pub(super) fn hil_status_str(s: &HilStatus) -> &'static str {
    match s {
        HilStatus::Pending => "pending",
        HilStatus::Resolved => "resolved",
        HilStatus::TimedOut => "timed_out",
        HilStatus::Cancelled => "cancelled",
    }
}

pub(super) fn wi_row_to_instance(
    row: WorkflowInstanceRow,
    nodes: Vec<NodeState>,
) -> Result<WorkflowInstance, ApiError> {
    Ok(WorkflowInstance {
        instance_id: row.instance_id,
        workflow_id: row.workflow_id,
        status: parse_workflow_status(&row.status),
        linked_plan_id: row.linked_plan_id,
        started_at: parse_dt(&row.started_at)?,
        ended_at: parse_opt_dt(row.ended_at.as_deref())?,
        definition: row
            .definition
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| err_internal(&format!("definition json: {e}")))?,
        nodes,
    })
}

pub(super) fn row_to_node_state(row: NodeStateRow) -> Result<NodeState, ApiError> {
    Ok(NodeState {
        node_id: row.node_id,
        status: parse_node_status(&row.status),
        started_at: parse_opt_dt(row.started_at.as_deref())?,
        ended_at: parse_opt_dt(row.ended_at.as_deref())?,
        operator_type: row.operator_type,
    })
}

pub(super) fn row_to_hil_event(row: HilEventRow) -> Result<HilEvent, ApiError> {
    let choices: Vec<String> = serde_json::from_str(&row.choices)
        .map_err(|e| err_internal(&format!("choices json: {e}")))?;
    let correlation_id = row
        .correlation_id_str
        .as_deref()
        .map(|s| Uuid::parse_str(s).map_err(|_| err_internal(&format!("invalid uuid: {s}"))))
        .transpose()?;
    Ok(HilEvent {
        event_id: row.event_id,
        instance_id: row.instance_id,
        node_id: row.node_id,
        channel: row.channel,
        event_type: parse_hil_event_type(&row.event_type),
        question: row.question,
        choices,
        timeout_seconds: row.timeout_seconds.map(|v| v as u64),
        correlation_id,
        status: parse_hil_status(&row.status),
        timestamp: parse_dt(&row.timestamp)?,
    })
}
