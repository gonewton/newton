# `GhOperator`

Dispatches GitHub CLI (`gh`) operations from a workflow task.

## Operations

- `pr_create` — create a pull request.
- `pr_view` — read a pull request's state.
- `project_resolve_board` — resolve a GitHub Projects board (project + status field option ids).
- `project_item_set_status` — move a project item to a named status.

## Optional ailoop authorization

A workflow author MAY require human authorization (via the ailoop integration)
before `GhOperator` invokes `gh`. When `require_authorization: true`, Newton
calls the configured `AiloopApprover` once per `execute` call, before any
subprocess attempt. Internal retries inside `pr_create` and
`project_item_set_status` reuse the single approval.

### Parameters

| Field | Type | Default | Notes |
|---|---|---|---|
| `require_authorization` | bool | `false` | Master switch. When `false` or absent, behavior is identical to today. |
| `authorization_prompt` | string | derived from operation | Non-empty if present. |
| `authorization_channel` | string | workspace ailoop channel | Per-task override. Non-empty if present. |
| `authorization_timeout_seconds` | number | `300` | Must be finite, `> 0`, `<= 86_400`. |
| `on_authorization_unavailable` | enum | `fail` | One of `fail` \| `skip`. |

### Default `authorization_prompt`

- `pr_create` → `Authorize gh pr create: title="<title>", base="<base>"`
- `pr_view` → `Authorize gh pr view: pr=<pr>`
- `project_resolve_board` → `Authorize gh project view/field-list: owner=<owner>, project=<n>`
- `project_item_set_status` → `Authorize gh project item-edit: item=<item_id>, status=<status>`

### Outcome / error mapping

| Outcome | `on_authorization_unavailable` | Result |
|---|---|---|
| Approved | n/a | `gh` runs |
| Denied | n/a | `WFG-GH-AUTH-001` (`ValidationError`); `gh` not invoked |
| Timeout | n/a | `WFG-GH-AUTH-002` (`TimeoutError`); `gh` not invoked |
| Unavailable | `fail` (default) | `WFG-GH-AUTH-003` (`ToolExecutionError`); `gh` not invoked |
| Unavailable | `skip` | `tracing::warn!` and `gh` runs |

Validation also emits:

- `WFG-GH-AUTH-004` for an `on_authorization_unavailable` value that is not `fail` or `skip`.
- `WFG-GH-AUTH-005` for an `authorization_timeout_seconds` that is not finite, `> 0`, and `<= 86_400`.

### YAML example

```yaml
- id: open-pr
  operator: GhOperator
  params:
    operation: pr_create
    title: "feat: add foo"
    base: main
    require_authorization: true
    authorization_prompt: "Approve PR create for foo branch"
    authorization_channel: "release-bot"
    authorization_timeout_seconds: 300
    on_authorization_unavailable: fail
```

### ailoop SDK requirement

Authorization is delivered through an `AiloopApprover` trait. Newton ships a
`NoopApprover` default; deployments that wire a real ailoop transport (the
`ailoop-sdk` crate, version pinned in `Cargo.toml` at the time of bind) replace
it via `BuiltinOperatorDeps::gh_approver`. With the default `NoopApprover` and
`require_authorization: true`, the operator returns `WFG-GH-AUTH-003` under the
default `fail` policy — **fail-closed** when wiring is missing.
