# `HumanDecisionOperator`

Prompts a human for one of a fixed set of structured choices and resolves the
selected option `id` into the task output. Renders a structured decision card
in the ailoop UI with per-option detail and an optional recommendation.

## YAML (structured — preferred)

```yaml
- id: pick-strategy
  operator: HumanDecisionOperator
  params:
    summary: "Which rollout strategy should we use?"
    decision_id: "rollout-strategy"          # optional; defaults to task id
    context_markdown: |
      ## Background
      Current p99 latency is 120 ms.
    options:
      - id: "canary"
        label: "Canary (5% → 100%)"
        detail_markdown: "Safest; takes 2 days."
      - id: "blue_green"
        label: "Blue/green cutover"
        detail_markdown: "Fast; requires dual fleet."
      - id: "skip"
        label: "Skip this release"
    recommendation:
      option_id: "canary"
      rationale_markdown: "Aligns with SLA commitments."
    timeout_seconds: 3600
    default_choice: "skip"
```

## YAML (legacy — deprecated, one-release migration window)

```yaml
- id: ask
  operator: HumanDecisionOperator
  params:
    prompt: "Release to production?"
    choices: ["yes", "no"]
    timeout_seconds: 600
    default_choice: "yes"
```

Legacy `prompt`/`choices` params are still accepted for one release to allow
existing workflows to migrate without a flag day. A deprecation warning is
recorded in `execution.json["warnings"]` (via the audit entry `prompt` field)
when the legacy shape is used. **Remove `prompt`/`choices` before the next
release.**

Detection: if `params` contains `options` → structured path. If `params`
contains `prompt` and no `options` → legacy path. Both present or neither →
`ValidationError`.

## Output JSON

```json
{
  "choice": "canary",
  "label": "Canary (5% → 100%)",
  "timestamp": "2026-05-11T12:00:00Z",
  "timeout_applied": false,
  "default_used": false
}
```

`choice` is the selected option **`id`** (stable programmer-chosen token).
Use `choice` in transition expressions:

```yaml
when: { $expr: 'tasks.pick-strategy.output.choice == "canary"' }
```

`label` is the display string for log readability; do not use it in `$expr`
transitions since labels may change independently of ids.

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
renders the structured decision card on the local TTY.

### Error reference

| Code | Category | Trigger |
|---|---|---|
| `HIL-AILOOP-001` | `ValidationError` | No enabled `AiloopContext` available |
| `HIL-AILOOP-003` | `IoError` | Configuration file present but malformed (bad URL, unreadable) |
| `WFG-HUMAN-002` | `ValidationError` | `timeout_seconds` set but `default_choice` absent |
| `WFG-HUMAN-101` | `IoError` | ailoop transport failure with `fail_fast=true` |
| `WFG-HUMAN-103` | `TimeoutError` | Timeout with no `default_choice` configured |
| `WFG-HUMAN-104` | `ValidationError` | ailoop answer does not match any declared option `id` |
| `WFG-HUMAN-201` | `ValidationError` | `options` array has fewer than 2 non-empty entries |
| `WFG-HUMAN-202` | `ValidationError` | Two or more options share the same `id` (case-sensitive) |
| `WFG-HUMAN-203` | `ValidationError` | `recommendation.option_id` does not match any option `id` |
| `WFG-HUMAN-204` | `ValidationError` | `default_choice` does not match any option `id` |

### Upgrade note

Earlier versions of Newton silently fell back to a console interviewer (stdin
prompts) when ailoop was not configured. That behaviour is gone:
human-in-the-loop workflows now require ailoop unconditionally.

### `fail_fast` interaction

When the ailoop backend is in use, the operator follows the
`AiloopContext.fail_fast` flag on transport failures (network error,
non-2xx response, deserialization error, timeout):

- `fail_fast=true`: returns `AppError(IoError, "WFG-HUMAN-101")`.
- `fail_fast=false`: behaves as if a timeout occurred. If `default_choice`
  is configured the audit entry records `timeout_applied=true, default_used=true`;
  otherwise the operator returns `AppError(TimeoutError, "WFG-HUMAN-103")`.

## Audit log

Audit entries are written to
`<workspace>/.newton/state/workflows/<execution_id>/audit.jsonl`. The
`interviewer_type` field reports `"ailoop"` in production and `"mock_ailoop"`
under tests using the test double. The `decision_id` field is set to the
resolved decision ID for structured-path tasks and `null` for legacy-path tasks.
