# GhOperator

Wraps the `gh` CLI for project board lookups, status mutations, and PR
operations. Implemented in `src/workflow/operators/gh.rs`.

## Operations

- `pr_create`
- `pr_view`
- `project_resolve_board`
- `project_item_set_status`

## Optional ailoop authorization

`GhOperator` can call ailoop for an interactive authorization prompt before any
underlying `gh` invocation runs. This gate is opt-in; default behavior is
byte-for-byte identical to the un-gated path.

| Field | Type | Default | Notes |
|---|---|---|---|
| `require_authorization` | bool | `false` | When `true`, the operator calls the configured `AiloopApprover` before dispatching `gh`. |
| `authorization_prompt` | string | derived (see below) | Non-empty if present. Sent as the prompt to ailoop. |
| `authorization_channel` | string | workspace ailoop channel | Non-empty if present. Per-task channel override. |
| `authorization_timeout_seconds` | number | `300` (5 minutes) | Must be finite, `> 0`, `<= 86_400`. Applies to the entire authorization request. |
| `on_authorization_unavailable` | enum | `fail` | `fail` (default): unreachable approver returns `WFG-GH-AUTH-003`. `skip`: log a warning and proceed. |

Default prompts are derived per operation when `authorization_prompt` is
absent:

- `pr_create` → `Authorize gh pr create: title="<title>", base="<base>"`
- `pr_view` → `Authorize gh pr view: pr=<pr>`
- `project_resolve_board` → `Authorize gh project view/field-list: owner=<owner>, project=<n>`
- `project_item_set_status` → `Authorize gh project item-edit: item=<item_id>, status=<status>`

Internal subprocess retries (e.g. `pr_create`'s retry loop) reuse the single
approval granted for the `execute` invocation; ailoop is not re-prompted.

### Error codes

| Code | Category | Trigger |
|---|---|---|
| `WFG-GH-AUTH-001` | `ValidationError` | Approver returned `Denied` |
| `WFG-GH-AUTH-002` | `TimeoutError` | Approver returned `Timeout` |
| `WFG-GH-AUTH-003` | `ToolExecutionError` | Approver `Unavailable` and `on_authorization_unavailable: fail` (also fires when no approver is wired and `require_authorization: true`) |
| `WFG-GH-AUTH-004` | `ValidationError` | `on_authorization_unavailable` not one of `fail`, `skip` |
| `WFG-GH-AUTH-005` | `ValidationError` | `authorization_timeout_seconds` zero, negative, NaN, or > 86 400 |

### Example

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

## ailoop SDK requirement

The `AiloopSdkApprover` (`src/integrations/ailoop/approver.rs`) is wired
against the in-tree `ToolClient` HTTP wrapper, which speaks the same
`/authorization/<channel>` endpoint as the published `goailoop/ailoop`
server. When the standalone `ailoop-sdk` Rust crate is published, swap
`ToolClient` for the SDK client; the `AiloopApprover` trait surface does not
change.
