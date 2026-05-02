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

## Transport

The operator delegates the prompt to an `Interviewer` backend. Two backends
are available; selection is automatic per workflow run:

| Backend  | When selected                                         | Behavior                                |
| -------- | ----------------------------------------------------- | --------------------------------------- |
| console  | Default; no `AiloopContext` initialized for the run.  | Reads the answer from stdin.            |
| ailoop   | An `AiloopContext` is initialized and enabled.        | Posts an `ask` request to ailoop.       |

### Environment override

`NEWTON_HITL_TRANSPORT` forces the backend regardless of the `AiloopContext`:

- `NEWTON_HITL_TRANSPORT=console` — always use the console backend.
- `NEWTON_HITL_TRANSPORT=ailoop` — require ailoop; if no `AiloopContext` is
  available the run logs a warning and falls back to console.

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
`<workspace>/.newton/state/workflows/<execution_id>/audit.jsonl`. The schema
is unchanged across backends; the `interviewer_type` field reports
`"console"` or `"ailoop"` depending on which transport was selected.

## Breaking change note

When ailoop is enabled in a workspace the operator no longer prompts on
stdin. To preserve the prior console behavior set
`NEWTON_HITL_TRANSPORT=console`.
