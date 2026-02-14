# Deep Ailoop Integration

## Overview

The new `ailoop_integration` module wires the orchestrator, tool runner, and daemon helpers into the same ailoop transport that powers `newton monitor`. When configured, Newton emits structured lifecycle events, forwards tool stdout/stderr with priority hints, and injects `NEWTON_AILOOP_*` variables into every subprocess so that scripts can request human authorization, ask questions, or send notifications. The integration uses bounded queues, retry/backoff, and a transport health tracker so the orchestrator remains responsive even when ailoop is slow or unreachable.

## Configuration

Newton resolves the integration config with the following precedence (highest → lowest):

1. Explicit CLI flags (not currently exposed).
2. Environment variables: `NEWTON_AILOOP_INTEGRATION`, `NEWTON_AILOOP_HTTP_URL`, `NEWTON_AILOOP_WS_URL`, `NEWTON_AILOOP_CHANNEL`, `NEWTON_AILOOP_FAIL_FAST`.
3. Workspace configs under `.newton/configs/*.conf` (keys: `ailoop_server_http_url`, `ailoop_server_ws_url`, `ailoop_channel`, `ailoop_fail_fast`).
4. Built-in defaults (integration stays disabled when no URLs are found).

`NEWTON_AILOOP_INTEGRATION=fail-fast`, a workspace `ailoop_fail_fast=true`, or `NEWTON_AILOOP_FAIL_FAST=1` turns on fail-fast behavior: if any transport worker detects a failure after retrying up to three times, the orchestrator surfaces an error and stops.

## Tool Helper Usage

Tool scripts can read the injected variables above and build a `newton::ailoop_integration::tool_client::ToolClient`. The helper exposes:

- `ask_question(question, timeout, choices)` to surface prompts.
- `request_authorization(action, timeout)` to gate sensitive actions.
- `send_notification(text)` for fire-and-forget alerts.

Each call respects the supplied timeout and returns a `ToolInteractionOutcome` (`Answer`, `AuthorizationApproved`, `AuthorizationDenied`, `Timeout`, or `Cancelled`), so tools can make informed decisions even when humans do not respond.

## Reliability and Shutdown

- All ailoop emissions go through bounded queues with deterministic drop-oldest behavior.
- Sending attempts are retried up to three times with exponential backoff (100ms base, 1s max) before being marked as transport failures.
- The integration tracks the first failure message in `TransportState`; fail-fast runners query this tracker before every iteration and abort with a descriptive `AppError`.
- Background workers wait up to five seconds to flush pending messages during shutdown. The CLI calls `AiloopContext::shutdown()` after every `run` or `step`, logging warnings when the flush timeout is reached.

## Manual Validation

Run these commands from the repository root to exercise the new integration:

1. **Lifecycle observability**
   ```bash
   NEWTON_AILOOP_INTEGRATION=1 cargo run -- run ./workspace
   ```
   Pass: `execution_started`, per-iteration, and completion/failure events hit the monitor (or test sink) without blocking the run.

2. **Batch visibility**
   ```bash
   NEWTON_AILOOP_INTEGRATION=1 cargo run -- batch project-alpha --workspace ./workspace
   ```
   Pass: every batch item emits iteration and completion/failure events tagged with execution IDs.

3. **Tool output forwarding**
   Run a workspace tool that prints to stdout/stderr (e.g., a verbose script). Pass: lines appear locally and are forwarded with the correct priorities.

4. **Degradation behavior**
   ```bash
   NEWTON_AILOOP_INTEGRATION=1 NEWTON_AILOOP_HTTP_URL=http://127.0.0.1:1 cargo run -- run ./workspace
   ```
   Pass: the CLI logs transport errors clearly but the primary run continues/completes.

