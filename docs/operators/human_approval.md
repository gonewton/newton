# `HumanApprovalOperator`

Asks a human to approve or reject a proposed action and records the
outcome.

## YAML

```yaml
- id: gate
  operator: HumanApprovalOperator
  with:
    prompt: "Approve release?"
    timeout_seconds: 600
    default_on_timeout: "approve"
```

## Output JSON

```json
{ "approved": true, "reason": "<reason or empty>", "timestamp": "<RFC3339>" }
```

## Transport

The operator delegates the prompt to an `Interviewer` backend. Two backends
are available; selection is automatic per workflow run:

| Backend  | When selected                                         | Behavior                                  |
| -------- | ----------------------------------------------------- | ----------------------------------------- |
| console  | Default; no `AiloopContext` initialized for the run.  | Reads the answer from stdin.              |
| ailoop   | An `AiloopContext` is initialized and enabled.        | Posts an `authorize` request to ailoop.   |

### Environment override

`NEWTON_HITL_TRANSPORT` forces the backend regardless of the `AiloopContext`:

- `NEWTON_HITL_TRANSPORT=console` — always use the console backend.
- `NEWTON_HITL_TRANSPORT=ailoop` — require ailoop; if no `AiloopContext` is
  available the run logs a warning and falls back to console.

### `fail_fast` interaction

When the ailoop backend is in use, the operator follows the
`AiloopContext.fail_fast` flag on transport failures (network error,
non-2xx response, deserialization error, timeout):

- `fail_fast=true`: returns `AppError(IoError, "WFG-HUMAN-102")`.
- `fail_fast=false`: behaves as if a timeout occurred. If
  `default_on_timeout` is configured the audit entry records
  `timeout_applied=true, default_used=true`; otherwise the operator
  returns `AppError(TimeoutError, "WFG-HUMAN-105")`.

## Audit log

Audit entries are written to
`<workspace>/.newton/state/workflows/<execution_id>/audit.jsonl`. The schema
is unchanged across backends; the `interviewer_type` field reports
`"console"` or `"ailoop"` depending on which transport was selected.

## Breaking change note

When ailoop is enabled in a workspace the operator no longer prompts on
stdin. To preserve the prior console behavior set
`NEWTON_HITL_TRANSPORT=console`.
