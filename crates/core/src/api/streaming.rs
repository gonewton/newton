//! Streaming API endpoints (WebSocket + SSE).
//!
//! This module contains the canonical implementation. `streaming_api` remains
//! available as a backward-compatible re-export.

use crate::api::state::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response, Sse},
    routing::get,
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use newton_types::{ApiError, BroadcastEvent};
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use uuid::Uuid;

const WELCOME_FRAME: &str = r#"{"type":"welcome"}"#;

/// Default number of historical log lines replayed on a fresh logs WS
/// connection (no `since_seq` given). Spec 074 B18: a full replay of
/// potentially unbounded history is not an acceptable default; callers that
/// need everything since a known point should pass `since_seq` instead.
const DEFAULT_LOG_TAIL: i64 = 500;

/// Builds the JSON payload sent to a stream consumer when the shared broadcast
/// channel overflowed and this consumer missed `skipped` events. Same shape is
/// used for both WS text frames and SSE `data:` payloads: `{"type":"lagged","skipped":<n>}`.
/// The client should treat this as a signal to re-fetch a snapshot before
/// resuming incremental updates (it is a per-connection condition, not a wire
/// event from the engine, so it deliberately is not a `BroadcastEvent` variant).
fn lagged_frame_json(skipped: u64) -> String {
    serde_json::json!({"type": "lagged", "skipped": skipped}).to_string()
}

#[derive(Debug, Deserialize, Clone)]
/// Optional query-string filters applied to workflow event streams.
///
/// These fields are accepted on both the WebSocket and SSE streaming endpoints.
pub struct StreamFilters {
    /// Override the instance id used for filtering (primarily for legacy clients).
    pub instance_id: Option<String>,
    /// Filter down to a single node id (where applicable).
    pub node_id: Option<String>,
    /// Filter by event type (e.g. `logMessage`, `nodeStateChanged`).
    pub event_type: Option<String>,
    /// Logs WS only (spec 074 B18): resume replay from lines with `seq >
    /// since_seq` instead of the default tail-500. Ignored by every other
    /// stream endpoint.
    pub since_seq: Option<i64>,
}

/// Routes for streaming endpoints (WebSocket + SSE).
pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ws", get(heartbeat_ws))
        .route("/stream/workflow/{id}/ws", get(workflow_stream))
        .route("/stream/logs/{id}/{node_id}/ws", get(logs_stream))
        .route("/stream/workflow/{id}/sse", get(workflow_sse))
        .with_state(state)
}

async fn heartbeat_ws(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    ws.on_upgrade(move |socket| handle_heartbeat_socket(socket, state))
}

async fn handle_heartbeat_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.events_tx.subscribe();
    if socket
        .send(Message::Text(WELCOME_FRAME.into()))
        .await
        .is_err()
    {
        return;
    }
    loop {
        tokio::select! {
            result = rx.recv() => match result {
                Ok(event) => {
                    if let Ok(json) = serde_json::to_string(&event) {
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            _ = tokio::time::sleep(state.ws_ping_interval) => {
                if socket.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn workflow_stream(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Query(filters): Query<StreamFilters>,
) -> Response {
    if Uuid::parse_str(&id).is_err() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError {
                code: "ERR_VALIDATION".to_string(),
                category: "validation".to_string(),
                message: "Invalid workflow instance ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    // Check instance exists via BackendStore (authoritative source)
    match state.backend.get_workflow_instance(&id).await {
        Ok(_) => {}
        Err(e) if e.code == "ERR_NOT_FOUND" => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    code: "ERR_NOT_FOUND".to_string(),
                    category: "not_found".to_string(),
                    message: format!("Workflow instance '{}' not found", id),
                    details: None,
                }),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    code: "ERR_INTERNAL".to_string(),
                    category: "internal".to_string(),
                    message: "Internal storage error".to_string(),
                    details: None,
                }),
            )
                .into_response();
        }
    }

    ws.on_upgrade(move |socket| handle_workflow_socket(socket, id, state, filters))
}

async fn handle_workflow_socket(
    mut socket: WebSocket,
    instance_id: String,
    state: Arc<AppState>,
    filters: StreamFilters,
) {
    let mut rx = state.events_tx.subscribe();
    let ping_interval = state.ws_ping_interval;
    // Not used again below: drop the AppState/Sender reference this task
    // holds so the receive loop's `RecvError::Closed` arm is actually
    // reachable once every *other* reference (router, other connections)
    // also drops, instead of this task itself being the last one keeping
    // the shared broadcast channel open forever.
    drop(state);

    if let Ok(json) = serde_json::to_string(&BroadcastEvent::WorkflowInstanceUpdated {
        instance_id: instance_id.clone(),
    }) {
        if socket.send(Message::Text(json.into())).await.is_err() {
            return;
        }
    }

    // Split so the loop below can `select!` over reading the client's half
    // of the socket (to notice a client-initiated Close promptly, and to
    // drain the socket so OS receive-buffer backpressure never stalls it)
    // at the same time as the broadcast receiver and the ping tick (spec
    // 074 B14).
    let (mut ws_sender, mut ws_receiver) = socket.split();

    loop {
        tokio::select! {
            ws_msg = ws_receiver.next() => match ws_msg {
                // Client closed the connection: stop pushing to it.
                Some(Ok(Message::Close(_))) => break,
                // Clients don't send meaningful data on this read-only
                // stream, but the socket must still be drained (Pong,
                // stray Text/Binary, etc.) or backpressure could stall it.
                Some(Ok(_)) => continue,
                // Socket error reading from the client.
                Some(Err(_)) => break,
                // Stream ended: connection dropped.
                None => break,
            },
            recv_result = rx.recv() => match recv_result {
                Ok(event) => {
                    if should_send_event(&event, &instance_id, &filters) {
                        if let Ok(json) = serde_json::to_string(&event) {
                            if ws_sender.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    let frame = lagged_frame_json(n);
                    if ws_sender.send(Message::Text(frame.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = tokio::time::sleep(ping_interval) => {
                if ws_sender.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn logs_stream(
    ws: WebSocketUpgrade,
    Path((instance_id, node_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Query(filters): Query<StreamFilters>,
) -> Response {
    if Uuid::parse_str(&instance_id).is_err() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError {
                code: "ERR_VALIDATION".to_string(),
                category: "validation".to_string(),
                message: "Invalid workflow instance ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    ws.on_upgrade(move |socket| handle_logs_socket(socket, instance_id, node_id, state, filters))
}

async fn resolve_task_name(state: &AppState, instance_id: &str, node_id: &str) -> String {
    state
        .backend
        .get_workflow_instance(instance_id)
        .await
        .ok()
        .and_then(|inst| inst.definition)
        .and_then(|def| def["tasks"][node_id]["name"].as_str().map(str::to_owned))
        .unwrap_or_else(|| node_id.to_owned())
}

async fn handle_logs_socket(
    mut socket: WebSocket,
    instance_id: String,
    node_id: String,
    state: Arc<AppState>,
    filters: StreamFilters,
) {
    // Subscribe first to avoid missing events during historical replay
    let mut rx = state.events_tx.subscribe();
    let ping_interval = state.ws_ping_interval;

    let task_name = resolve_task_name(&state, &instance_id, &node_id).await;
    let connect_line = BroadcastEvent::LogMessage {
        instance_id: instance_id.clone(),
        node_id: node_id.clone(),
        message: format!("Connected to {task_name}"),
        // Synthetic, never persisted: 0 is a documented sentinel, not a real
        // seq (real seqs start at 1).
        seq: 0,
    };
    if let Ok(json) = serde_json::to_string(&connect_line) {
        if socket.send(Message::Text(json.into())).await.is_err() {
            return;
        }
    }

    // Replay historical log lines (spec 074 B18): `since_seq` resumes from
    // that point (everything after it); with no `since_seq`, default to the
    // last DEFAULT_LOG_TAIL lines rather than a full, potentially unbounded
    // replay.
    let historical = match filters.since_seq {
        Some(since_seq) => {
            state
                .backend
                .list_log_lines(&instance_id, &node_id, since_seq)
                .await
        }
        None => {
            state
                .backend
                .list_log_lines_tail(&instance_id, &node_id, DEFAULT_LOG_TAIL)
                .await
        }
    };
    if let Ok(historical) = historical {
        for line in historical {
            let event = BroadcastEvent::LogMessage {
                instance_id: line.instance_id.clone(),
                node_id: line.node_id.clone(),
                message: line.message.clone(),
                seq: line.seq,
            };
            if let Ok(json) = serde_json::to_string(&event) {
                if socket.send(Message::Text(json.into())).await.is_err() {
                    return;
                }
            }
        }
    }

    // Not used again below: drop the AppState/Sender reference this task
    // holds so the receive loop's `RecvError::Closed` arm is actually
    // reachable once every *other* reference (router, other connections)
    // also drops, instead of this task itself being the last one keeping
    // the shared broadcast channel open forever.
    drop(state);

    // Split so the loop below can `select!` over reading the client's half
    // of the socket (to notice a client-initiated Close promptly, and to
    // drain the socket so OS receive-buffer backpressure never stalls it)
    // at the same time as the broadcast receiver and the ping tick (spec
    // 074 B14).
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Forward live broadcast events
    loop {
        tokio::select! {
            ws_msg = ws_receiver.next() => match ws_msg {
                // Client closed the connection: stop pushing to it.
                Some(Ok(Message::Close(_))) => break,
                // Clients don't send meaningful data on this read-only
                // stream, but the socket must still be drained (Pong,
                // stray Text/Binary, etc.) or backpressure could stall it.
                Some(Ok(_)) => continue,
                // Socket error reading from the client.
                Some(Err(_)) => break,
                // Stream ended: connection dropped.
                None => break,
            },
            recv_result = rx.recv() => match recv_result {
                Ok(event) => {
                    if should_send_event(&event, &instance_id, &filters) {
                        if let BroadcastEvent::LogMessage {
                            instance_id: ref evt_inst,
                            node_id: ref evt_node,
                            ..
                        } = event
                        {
                            if evt_inst == &instance_id && evt_node == &node_id {
                                if let Ok(json) = serde_json::to_string(&event) {
                                    if ws_sender.send(Message::Text(json.into())).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    let frame = lagged_frame_json(n);
                    if ws_sender.send(Message::Text(frame.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = tokio::time::sleep(ping_interval) => {
                if ws_sender.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn workflow_sse(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Query(filters): Query<StreamFilters>,
) -> Response {
    if Uuid::parse_str(&id).is_err() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError {
                code: "ERR_VALIDATION".to_string(),
                category: "validation".to_string(),
                message: "Invalid workflow instance ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    // Check instance exists via BackendStore (authoritative source)
    match state.backend.get_workflow_instance(&id).await {
        Ok(_) => {}
        Err(e) if e.code == "ERR_NOT_FOUND" => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    code: "ERR_NOT_FOUND".to_string(),
                    category: "not_found".to_string(),
                    message: format!("Workflow instance '{}' not found", id),
                    details: None,
                }),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    code: "ERR_INTERNAL".to_string(),
                    category: "internal".to_string(),
                    message: "Internal storage error".to_string(),
                    details: None,
                }),
            )
                .into_response();
        }
    }

    let rx = state.events_tx.subscribe();
    let id_clone = id.clone();
    let filters_clone = filters.clone();

    let stream = async_stream::stream! {
        let snapshot = BroadcastEvent::WorkflowInstanceUpdated { instance_id: id_clone.clone() };
        if let Ok(json) = serde_json::to_string(&snapshot) {
            let sse_event = axum::response::sse::Event::default().data(json);
            yield Ok::<_, Infallible>(sse_event);
        }
        let mut rx = rx;
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if should_send_event(&event, &id_clone, &filters_clone) {
                        if let Ok(json) = serde_json::to_string(&event) {
                            let sse_event = axum::response::sse::Event::default().data(json);
                            yield Ok::<_, Infallible>(sse_event);
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    let json = lagged_frame_json(n);
                    let sse_event = axum::response::sse::Event::default().data(json);
                    yield Ok::<_, Infallible>(sse_event);
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(Duration::from_secs(10))
                .text("keepalive"),
        )
        .into_response()
}

fn should_send_event(event: &BroadcastEvent, instance_id: &str, filters: &StreamFilters) -> bool {
    if let Some(ref filter_inst) = filters.instance_id {
        if filter_inst != instance_id {
            return false;
        }
    }

    if let Some(ref filter_type) = filters.event_type {
        let event_type = match event {
            BroadcastEvent::WorkflowInstanceUpdated { .. } => "workflowInstanceUpdated",
            BroadcastEvent::NodeStateChanged { .. } => "nodeStateChanged",
            BroadcastEvent::LogMessage { .. } => "logMessage",
            BroadcastEvent::HilEvent { .. } => "hilEvent",
            BroadcastEvent::PlanUpdate { .. } => "plan_update",
            BroadcastEvent::ExecutionUpdate { .. } => "execution_update",
            BroadcastEvent::FindingUpdate { .. } => "finding_update",
            BroadcastEvent::ChangeRequestUpdate { .. } => "change_request_update",
            BroadcastEvent::CatalogUpdate { .. } => "catalog_update",
            BroadcastEvent::OptimizeRunUpdate { .. } => "optimize_run_update",
        };

        if filter_type != event_type {
            return false;
        }
    }

    match event {
        BroadcastEvent::WorkflowInstanceUpdated {
            instance_id: ref evt_id,
        } => evt_id == instance_id,
        BroadcastEvent::NodeStateChanged {
            instance_id: ref evt_id,
            node_id: ref evt_node,
        } => {
            if evt_id != instance_id {
                return false;
            }
            if let Some(ref filter_node) = filters.node_id {
                filter_node == evt_node
            } else {
                true
            }
        }
        BroadcastEvent::LogMessage {
            instance_id: ref evt_id,
            node_id: ref evt_node,
            ..
        } => {
            if evt_id != instance_id {
                return false;
            }
            if let Some(ref filter_node) = filters.node_id {
                filter_node == evt_node
            } else {
                true
            }
        }
        BroadcastEvent::HilEvent {
            instance_id: ref evt_id,
            ..
        } => evt_id == instance_id,
        // Plan/Execution events genuinely have an owning workflow instance
        // once a plan is approved and executed (spec 074 B13): scope them
        // like WorkflowInstanceUpdated/NodeStateChanged/HilEvent above so a
        // workflow-A-scoped stream never sees workflow-B's plan/execution
        // events.
        BroadcastEvent::PlanUpdate {
            instance_id: ref evt_id,
            ..
        } => {
            // `None` means the plan has no linked execution/instance (still
            // awaiting approval, or rejected without ever running). There is
            // no instance to match against, so drop it from every
            // instance-scoped stream rather than guessing which one wants it.
            matches!(evt_id, Some(id) if id == instance_id)
        }
        BroadcastEvent::ExecutionUpdate {
            instance_id: ref evt_id,
            ..
        } => evt_id == instance_id,
        // Not workflow-instance-scoped: these are domain-object mutation
        // events (Finding/ChangeRequest/Catalog/OptimizeRun), not tied to a
        // workflow instance id, so `instance_id` filtering (handled above via
        // `filters.instance_id`) doesn't apply to them and they pass through
        // unconditionally here.
        BroadcastEvent::FindingUpdate { .. }
        | BroadcastEvent::ChangeRequestUpdate { .. }
        | BroadcastEvent::CatalogUpdate { .. }
        | BroadcastEvent::OptimizeRunUpdate { .. } => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_filters() -> StreamFilters {
        StreamFilters {
            instance_id: None,
            node_id: None,
            event_type: None,
            since_seq: None,
        }
    }

    /// Literal T3 acceptance gate (spec 074 B13): a workflow-A-scoped stream
    /// receives no workflow-B plan events.
    #[test]
    fn plan_update_is_scoped_to_its_owning_instance() {
        let filters = no_filters();
        let event = BroadcastEvent::PlanUpdate {
            plan_id: "plan-b".to_string(),
            instance_id: Some("instance-b".to_string()),
        };

        assert!(
            !should_send_event(&event, "instance-a", &filters),
            "a workflow-A-scoped stream must not receive a PlanUpdate owned by instance B"
        );
        assert!(
            should_send_event(&event, "instance-b", &filters),
            "a workflow-B-scoped stream must receive a PlanUpdate owned by instance B"
        );
    }

    /// Same acceptance gate, for ExecutionUpdate.
    #[test]
    fn execution_update_is_scoped_to_its_owning_instance() {
        let filters = no_filters();
        let event = BroadcastEvent::ExecutionUpdate {
            execution_id: "exec-b".to_string(),
            plan_id: Some("plan-b".to_string()),
            status: "running".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            instance_id: "instance-b".to_string(),
        };

        assert!(
            !should_send_event(&event, "instance-a", &filters),
            "a workflow-A-scoped stream must not receive an ExecutionUpdate owned by instance B"
        );
        assert!(
            should_send_event(&event, "instance-b", &filters),
            "a workflow-B-scoped stream must receive an ExecutionUpdate owned by instance B"
        );
    }

    /// A PlanUpdate with no linked instance (still awaiting approval, or
    /// rejected without ever running) has nothing to match against, so every
    /// instance-scoped stream must drop it rather than guess.
    #[test]
    fn plan_update_with_no_instance_is_dropped_from_every_scoped_stream() {
        let filters = no_filters();
        let event = BroadcastEvent::PlanUpdate {
            plan_id: "plan-a".to_string(),
            instance_id: None,
        };

        assert!(!should_send_event(&event, "instance-a", &filters));
        assert!(!should_send_event(&event, "instance-b", &filters));
    }

    /// Domain-object mutation events with no instance concept at all
    /// (Finding/ChangeRequest/Catalog/OptimizeRun) must remain unconditional
    /// pass-through, unaffected by B13's Plan/Execution scoping change.
    #[test]
    fn non_instance_scoped_events_remain_unconditional_pass_through() {
        let filters = no_filters();
        let events = [
            BroadcastEvent::FindingUpdate {
                finding_id: "finding-1".to_string(),
            },
            BroadcastEvent::ChangeRequestUpdate {
                change_request_id: "cr-1".to_string(),
            },
            BroadcastEvent::CatalogUpdate {
                resource: "product".to_string(),
                id: "product-1".to_string(),
            },
            BroadcastEvent::OptimizeRunUpdate {
                run_id: "run-1".to_string(),
                cycle: None,
            },
        ];

        for event in &events {
            assert!(should_send_event(event, "instance-a", &filters));
            assert!(should_send_event(event, "instance-b", &filters));
        }
    }
}
