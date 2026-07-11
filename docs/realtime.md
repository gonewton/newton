# Newton Realtime Streaming Guide

`newton serve` exposes four streaming endpoints — three WebSocket and one SSE
(Server-Sent Events). This guide covers architecture, client examples, and
event-type reference.

## Architecture Overview

All streaming endpoints share a single `tokio::sync::broadcast` channel
(`BROADCAST_CAPACITY = 1024`). The executor publishes `BroadcastEvent` values;
each connected client task holds its own `broadcast::Receiver` and forwards
events according to its filter rules.

```
executor ──publishes──► broadcast::Sender
                              │
               ┌──────────────┼──────────────┐
               ▼              ▼              ▼
          /ws client    /workflow/ws     /logs/ws
          (all events)  (per-instance)  (per-node)
```

### Snapshot-on-connect

`GET /api/stream/workflow/{id}/ws` and `GET /api/stream/workflow/{id}/sse` emit
a `workflowInstanceUpdated` event immediately after the connection is
established, before any upstream broadcast event. This ensures clients see the
current instance state without waiting for the next mutation.

### Connect confirmation (logs stream)

`GET /api/stream/logs/{id}/{nodeId}/ws` emits a synthetic `logMessage` frame
with `message: "Connected to <task_name>"` as the very first frame. The task
name is resolved from the instance definition; it falls back to `nodeId` if the
instance or definition is unavailable.

### 404 enforcement

`GET /api/stream/workflow/{id}/ws` and `GET /api/stream/workflow/{id}/sse`
return HTTP `404` with a canonical `ApiError` body if `state.backend.get_workflow_instance(id)`
(the `BackendStore`, the authoritative source) reports the instance does not
exist. The WebSocket upgrade (101 Switching Protocols) is never sent. Clients
should check the HTTP response code before treating a connection as open.

---

## Event Types

All events are JSON objects with a `"type"` discriminant field (serde tagged
enum). Wire shapes:

| Type | Fields |
|---|---|
| `workflowInstanceUpdated` | `instance_id` |
| `nodeStateChanged` | `instance_id`, `node_id` |
| `logMessage` | `instance_id`, `node_id`, `message`, `seq` |
| `hilEvent` | `instance_id`, `event_id` |
| `plan_update` | `plan_id`, `instance_id` (nullable) |
| `execution_update` | `execution_id`, `plan_id` (nullable), `status`, `created_at`, `instance_id` |
| `finding_update` | `finding_id` |
| `change_request_update` | `change_request_id` |
| `catalog_update` | `resource`, `id` |
| `optimize_run_update` | `run_id`, `cycle` (nullable) |

`logMessage.seq` is the log line's persisted sequence number (spec 074 B18), or
`0` for synthetic, non-persisted lines (e.g. the "Connected to \<task\>" frame
sent on logs WS connect). Clients reconnecting to the logs WS pass the highest
`seq` they've seen as `since_seq` to resume without a full replay.

`plan_update.instance_id` and `execution_update.instance_id` carry the owning
workflow instance id (spec 074 B13). For `plan_update` this is `None` until a
plan is approved and linked to a running instance (still awaiting approval, or
rejected without ever running) — instance-scoped streams drop the event rather
than guessing which instance it belongs to. `execution_update.instance_id` is
never null: it falls back to the execution's own id until a real workflow
instance attaches.

`finding_update`, `change_request_update`, `catalog_update`, and
`optimize_run_update` (spec 074 P3) are **not** workflow-instance-scoped —
they are domain-object mutation events, not tied to any workflow instance id,
so they pass through every stream's `instance_id` filter unconditionally.
Clients re-fetch the authoritative record from the matching REST endpoint on
receipt (e.g. `GET /findings/{id}`, `GET /change-requests/{id}`).

---

## Filter Query Parameters

All streaming endpoints accept optional query parameters:

| Parameter | Description |
|---|---|
| `instance_id` | Override the instance id used for filtering (legacy clients) |
| `node_id` | Restrict to a specific node |
| `event_type` | Restrict by event type name (e.g. `logMessage`) |
| `since_seq` | Logs WS only (spec 074 B18): resume historical replay from log lines with `seq > since_seq` instead of the default tail-500. Ignored by every other stream endpoint. |

---

## Heartbeat WebSocket (`GET /ws`)

Receives all `BroadcastEvent` variants without filtering. Sends a WS `Ping`
frame every 30 seconds. The very first frame is always `{"type":"welcome"}`.

### JavaScript WebSocket Example

```javascript
const ws = new WebSocket('ws://localhost:8080/ws');

ws.addEventListener('open', () => {
  console.log('connected to heartbeat socket');
});

ws.addEventListener('message', (event) => {
  const msg = JSON.parse(event.data);
  if (msg.type === 'welcome') {
    console.log('server ready');
    return;
  }
  console.log('broadcast event:', msg);
});

ws.addEventListener('close', (event) => {
  console.log('disconnected', event.code, event.reason);
});
```

---

## Per-Instance Workflow WebSocket (`GET /api/stream/workflow/{id}/ws`)

Receives events filtered to the given instance. Emits a snapshot
`workflowInstanceUpdated` immediately on connect.

### JavaScript WebSocket Example

```javascript
const instanceId = '550e8400-e29b-41d4-a716-446655440000';
const ws = new WebSocket(`ws://localhost:8080/api/stream/workflow/${instanceId}/ws`);

ws.addEventListener('open', () => {
  console.log('workflow stream connected');
});

ws.addEventListener('message', (event) => {
  const msg = JSON.parse(event.data);
  // First frame is always a workflowInstanceUpdated snapshot.
  console.log(`[${msg.type}]`, msg);
});

ws.addEventListener('close', (event) => {
  if (event.code === 1001) {
    console.log('server going away — reconnect');
  }
});
```

**Tip:** if the server returns HTTP `404` instead of `101`, the instance does
not exist yet. Check `ws.readyState` on `open` vs `close` to distinguish.

---

## SSE Fallback (`GET /api/stream/workflow/{id}/sse`)

Use `EventSource` for environments without native WebSocket support. The first
event is always `workflowInstanceUpdated`. The server sends a text
`"keepalive"` comment every 10 seconds.

### JavaScript EventSource Example

```javascript
const instanceId = '550e8400-e29b-41d4-a716-446655440000';
const es = new EventSource(
  `http://localhost:8080/api/stream/workflow/${instanceId}/sse`
);

es.addEventListener('message', (event) => {
  const msg = JSON.parse(event.data);
  // First event is always a workflowInstanceUpdated snapshot.
  console.log(`[${msg.type}]`, msg);
});

es.addEventListener('error', (err) => {
  console.error('SSE error:', err);
  if (es.readyState === EventSource.CLOSED) {
    console.log('stream closed — reconnect if needed');
  }
});
```

**Note:** `EventSource` always uses GET and follows redirects. If the server
returns `404`, the browser fires an `error` event and closes the connection.

---

## Log Stream WebSocket (`GET /api/stream/logs/{id}/{nodeId}/ws`)

Receives only `logMessage` events for the specified instance and node. The
first frame is always a synthetic `logMessage` with `"Connected to <name>"`
(`seq: 0`, a documented sentinel — real seqs start at 1). After the connect
frame, historical log lines are replayed before live forwarding begins: with
`since_seq` set, everything with `seq > since_seq`; otherwise the last 500
lines (`DEFAULT_LOG_TAIL`) rather than a full, potentially unbounded replay
(spec 074 B18). Clients should dedup by `seq` regardless.

### JavaScript WebSocket Example

```javascript
const instanceId = '550e8400-e29b-41d4-a716-446655440000';
const nodeId = 'build-task';
const ws = new WebSocket(
  `ws://localhost:8080/api/stream/logs/${instanceId}/${nodeId}/ws`
);

ws.addEventListener('message', (event) => {
  const msg = JSON.parse(event.data);
  // msg.type === 'logMessage', msg.message is the log line text.
  console.log(`[${msg.node_id}] ${msg.message}`);
});
```

---

## Machine-Readable Contract

The full AsyncAPI 3.0 contract covering all four endpoints and every event
schema lives at [`openapi/newton-realtime.asyncapi.yaml`](../openapi/newton-realtime.asyncapi.yaml).
