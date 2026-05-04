# `HumanDecisionOperator`

Prompts a human for one of a fixed set of choices and resolves the chosen
value into the task output.

## YAML

```yaml
- id: pick-strategy
  operator: HumanDecisionOperator
  with:
    prompt: "Which path forward?"
    choices: ["fix", "skip", "abort"]
    timeout_seconds: 600
    default_choice: "skip"
```

## Output JSON

```json
{ "choice": "<one of choices>", "timestamp": "<RFC3339>" }
```

## Configuration

Human-in-the-loop operators **require ailoop**. Newton always delegates the
prompt to ailoop; there is no console fallback. Ailoop itself decides whether
to render the prompt on a local TTY (direct mode) or relay it to a remote
operator over WebSocket (server mode). See
[`init_context_for_command_name`](../../crates/core/src/integrations/ailoop/config.rs).

Required configuration:

- Env vars (highest precedence):
  - `NEWTON_AILOOP_INTEGRATION=1` — opt in to the ailoop integration.
  - `NEWTON_AILOOP_WS_URL` — WebSocket URL of the ailoop endpoint.
  - `NEWTON_AILOOP_CHANNEL` — channel name for messages.
- File-based fallback: `.newton/configs/monitor.conf` with keys
  `ailoop_server_ws_url=…` and `ailoop_channel=…`.

### Local / developer setup

Run ailoop in **direct mode** locally (no remote server) and point
`.newton/configs/monitor.conf` at the local instance, e.g.:

```
ailoop_server_ws_url=ws://localhost:8765/
ailoop_channel=newton-dev
```

Set `NEWTON_AILOOP_INTEGRATION=1` and run the workflow normally; ailoop
renders the prompt on the local TTY.

### Error reference

When a workflow contains a `human_decision` task but no enabled
`AiloopContext` is available, the first prompt fails with error code
`HIL-AILOOP-001` (category `ValidationError`). If the configuration file is
present but malformed (bad URL, unreadable file), the helper
`require_enabled_ailoop_context` returns `HIL-AILOOP-003` (category
`IoError`).

### Upgrade note

Earlier versions of Newton silently fell back to a console interviewer (stdin
prompts) when ailoop was not configured. That behaviour is gone:
human-in-the-loop workflows now require ailoop unconditionally.

### `fail_fast` interaction

When the ailoop backend is in use, the operator follows the
`AiloopContext.fail_fast` flag on transport failures (network error,
non-2xx response, deserialization error, timeout):

- `fail_fast=true`: returns `AppError(IoError, "WFG-HUMAN-101")`.
- `fail_fast=false`: behaves as if a timeout occurred. If
  `default_choice` is configured the audit entry records
  `timeout_applied=true, default_used=true`; otherwise the operator
  returns `AppError(TimeoutError, "WFG-HUMAN-103")`.

## Audit log

Audit entries are written to
`<workspace>/.newton/state/workflows/<execution_id>/audit.jsonl`. The
`interviewer_type` field reports `"ailoop"` in production and `"mock_ailoop"`
under tests using the test double.
