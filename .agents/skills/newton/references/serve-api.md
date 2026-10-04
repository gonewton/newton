# `newton serve` HTTP API

`newton serve` runs the Axum server: REST under `/api/v1`, WebSocket/SSE streams, health probes, the embedded web UI at `/`, and optionally MCP (`--with-mcp`) and ailoop (`--with-embedded-ailoop`) on the same listener.

## Contract

The REST contract is `openapi/newton-api.yaml` in the Newton repository (server base `/api/v1`); realtime events are in `openapi/newton-realtime.asyncapi.yaml`. A running server also serves interactive docs at `/api/docs`. Treat those files as the source of truth; the tables below are a map, not a spec.

## Endpoints (`/api/v1`)

| Area | Paths |
| --- | --- |
| Optimization loop | `/optimize-runs`, `/optimize-runs/{id}`, `/optimize-runs/{id}/cycles`, `/optimize-runs/{id}/trajectory` |
| Findings | `/findings` (GET, POST), `/findings/{id}` (GET, PATCH), `POST /findings/{id}/unblock` |
| Change Requests / Plans | `/change-requests[/{id}]`, `/plans[/{id}]`, `POST /plans/{id}/approve`, `POST /plans/{id}/reject` |
| Grading | `/kpis[/{id}]`, `/eval-runs[/{id}]`, `/eval-runs/{id}/grades`, `/grades[/{id}]` |
| Catalog | `/products[/{id}]`, `/components[/{id}]`, `/repos[/{id}]`, `/modules[/{id}]`, `/module-dependencies[/{id}]`, `/repo-dependencies` |
| Dashboard | `/pending-approvals`, `/regressions`, `/recent-actions`, `/saved-views` |
| Workflows | `/workflows[/{id}]`, `/workflows/{id}/nodes/{node_id}`, `/workflow-files[/{name}]`, `/workflow-files/validate`, `/executions`, `/executions/{id}/logs`, `/operators` |
| HIL | `/hil/instances`, `/hil/workflows/{id}`, `POST /hil/workflows/{id}/{event_id}/action` |
| Persistence | `/persistence/{key}` (GET, PUT, DELETE) |

Streams (also under `/api/v1`): `/stream/workflow/{id}/ws`, `/stream/workflow/{id}/sse`, `/stream/logs/{id}/{node_id}/ws`, and a heartbeat at `/ws`.

Health: `/healthz` and `/readyz` (outside `/api/v1`, never behind auth).

## Authentication

Off by default and loopback-only. To bind a non-loopback `--host`, configure OIDC with `--oidc-issuer` and `--oidc-audience` (or `NEWTON_OIDC_ISSUER` / `NEWTON_OIDC_AUDIENCE`); callers then send `Authorization: Bearer <JWT>`. `--with-mcp` mounts MCP behind the same OIDC layer.

## Examples

```bash
newton serve --port 8080 --no-web
curl -s localhost:8080/api/v1/optimize-runs
curl -s "localhost:8080/api/v1/findings?status=triaged&scope=repo&scope_id=my-repo"
curl -s -X PATCH localhost:8080/api/v1/findings/<id> \
  -H 'content-type: application/json' -d '{"status":"rejected"}'
```

## Storage

Serve/catalog state lives in SQLite at `<workspace>/.newton/state/backend.sqlite`
(override with `--state-dir`). Generic `newton optimize` history instead lives in
versioned JSON under `.newton/state/optimize/<run-id>/` and is not automatically
projected into these legacy optimize-run endpoints. Run a single `serve` process
per SQLite state directory.
