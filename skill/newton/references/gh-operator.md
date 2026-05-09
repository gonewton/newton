# GhOperator

Wraps the `gh` CLI for project board lookups, status mutations, and PR
operations. Implemented in `crates/core/src/workflow/operators/gh.rs`.

## Operations

| Operation | Purpose |
|---|---|
| `pr_create` | Create a pull request |
| `pr_view` | View a pull request's state |
| `pr_approve` | Approve a pull request via `gh pr review --approve` |
| `project_resolve_board` | Resolve a GitHub Project board's field and option IDs |
| `project_item_set_status` | Set the status of a project board item |

## Input schema per operation

### `pr_create`

| Field | Type | Required | Default | Notes |
|---|---|---|---|---|
| `title` | string | yes | — | PR title |
| `base` | string | no | `main` | Base branch |
| `body` | string | no | `""` | PR body |
| `retry_count` | integer | no | `3` | Number of attempts |
| `retry_delay_ms` | integer | no | `5000` | Delay between retries (capped at 300 000 ms) |

### `pr_view`

| Field | Type | Required | Notes |
|---|---|---|---|
| `pr` | string or number | yes | PR number or full URL |

### `pr_approve`

| Field | Type | Required | Notes |
|---|---|---|---|
| `pr_number` | integer (>= 1) | XOR with `pr_url` | PR number to approve |
| `pr_url` | string (HTTPS) | XOR with `pr_number` | Full GitHub PR URL |
| `repository` | string | no | `owner/repo` format; only valid with `pr_number` |

Exactly one of `pr_number` or `pr_url` must be set. Setting both or neither
is a validation error (`WFG-GH-005`).

When `pr_url` is provided, the operator extracts `owner/repo` and the PR
number from the URL and passes `-R owner/repo` to `gh`.

When `pr_number` is provided with `repository`, the operator passes
`-R <repository>`. When `pr_number` is provided alone, no `-R` flag is used.

### `project_resolve_board`

| Field | Type | Required | Default | Notes |
|---|---|---|---|---|
| `owner` | string | yes | — | GitHub org or user |
| `project_number` | string or number | yes | — | Project number |
| `field_name` | string | no | `Status` | Single-select field to resolve |
| `required_option_names` | array of strings | no | `["Ready","In progress","In review","Done"]` | Options that must exist |

### `project_item_set_status`

| Field | Type | Required | Notes |
|---|---|---|---|
| `item_id` | string | yes | Project item node ID |
| `board` | object | yes | Output of `project_resolve_board` |
| `status` | string | conditional | Status name (resolved via board options) |
| `single_select_option_id` / `option_id` | string | conditional | Explicit option ID (bypasses name lookup) |
| `on_error` | string | no | `warn` (default) or `fail` |

Either `status` or `single_select_option_id`/`option_id` must be provided.

## Output schema per operation

### `pr_create`

```json
{ "pr_url": "https://github.com/owner/repo/pull/42", "pr_number": 42 }
```

### `pr_view`

```json
{ "state": "OPEN", "pr_number": 42 }
```

### `pr_approve`

```json
{
  "review_submitted": true,
  "pr_number": 36,
  "repository": "owner/repo",
  "pr_url": "https://github.com/owner/repo/pull/36"
}
```

- `pr_number` is always present.
- `repository` is present when provided as input or derived from `pr_url`.
- `pr_url` is present when input was `pr_url`, or when both `pr_number` and
  `repository` were provided (reconstructed as
  `https://github.com/<repository>/pull/<pr_number>`).

### `project_resolve_board`

```json
{
  "project_id": "PVT_abc123",
  "field_id": "FLD_status",
  "options": { "Ready": "OPT_ready", "In progress": "OPT_in_progress", ... },
  "ready_id": "OPT_ready",
  "in_progress_id": "OPT_in_progress",
  "in_review_id": "OPT_in_review",
  "done_id": "OPT_done"
}
```

### `project_item_set_status`

```json
{ "updated": true }
```

On failure with `on_error: warn`: `{ "updated": false, "warning": "..." }`.

## Optional ailoop authorization

All operations support an optional ailoop authorization gate. The gate is
opt-in; default behavior is byte-for-byte identical to the un-gated path.

| Field | Type | Default | Notes |
|---|---|---|---|
| `require_authorization` | bool | `false` | When `true`, the operator calls the configured `AiloopApprover` before dispatching `gh`. |
| `authorization_prompt` | string | derived (see below) | Sent as the prompt to ailoop. |
| `authorization_channel` | string | workspace ailoop channel | Per-task channel override. |
| `authorization_timeout_seconds` | number | `300` (5 minutes) | Must be `> 0` and `<= 86400`. |
| `on_authorization_unavailable` | enum | `fail` | `fail`: returns `WFG-GH-AUTH-003`. `skip`: log warning and proceed. |

### Default prompts

When `authorization_prompt` is absent, a default is derived per operation:

- `pr_create` → `Authorize gh pr create: title="<title>", base="<base>"`
- `pr_view` → `Authorize gh pr view: pr=<pr>`
- `pr_approve` → `Authorize gh pr review --approve: pr=<selector>` (with `, repository=<repo>` when present)
- `project_resolve_board` → `Authorize gh project view/field-list: owner=<owner>, project=<n>`
- `project_item_set_status` → `Authorize gh project item-edit: item=<item_id>, status=<status>`

Internal subprocess retries (e.g. `pr_create`'s retry loop) reuse the single
approval granted for the `execute` invocation; ailoop is not re-prompted.

## Error codes

| Code | Category | Trigger |
|---|---|---|
| `WFG-GH-001` | `ToolExecutionError` | Project board JSON parse/lookup failures |
| `WFG-GH-002` | `ToolExecutionError` | PR view JSON parse failures |
| `WFG-GH-003` | `ToolExecutionError` | Failed to execute `gh` binary |
| `WFG-GH-004` | `ToolExecutionError` | `gh` command returned non-zero exit code |
| `WFG-GH-005` | `ValidationError` | `pr_approve` has both `pr_number` and `pr_url`, or has neither |
| `WFG-GH-006` | `ValidationError` | `pr_url` is not HTTPS, host lacks `github`, or path does not end with `/pull/<positive_integer>` |
| `WFG-GH-007` | `ValidationError` | `repository` does not match `owner/repo` format |
| `WFG-GH-008` | `ValidationError` | `pr_number` is missing, non-integer, or `< 1` |
| `WFG-GH-AUTH-001` | `ValidationError` | Approver returned `Denied` |
| `WFG-GH-AUTH-002` | `TimeoutError` | Approver returned `Timeout` |
| `WFG-GH-AUTH-003` | `ToolExecutionError` | Approver `Unavailable` and `on_authorization_unavailable: fail` |
| `WFG-GH-AUTH-004` | `ValidationError` | `on_authorization_unavailable` not one of `fail`, `skip` |
| `WFG-GH-AUTH-005` | `ValidationError` | `authorization_timeout_seconds` zero, negative, NaN, or > 86 400 |

## Example YAML

### `pr_create`

```yaml
- id: open-pr
  operator: GhOperator
  params:
    operation: pr_create
    title: "feat: add foo"
    base: main
    body: "Adds the foo feature"
    retry_count: 3
```

### `pr_view`

```yaml
- id: check-pr
  operator: GhOperator
  params:
    operation: pr_view
    pr: 42
```

### `pr_approve` (by number and repository)

```yaml
- id: approve-pr
  operator: GhOperator
  params:
    operation: pr_approve
    pr_number: 36
    repository: goailoop/ailoop
    require_authorization: true
    authorization_channel: release-bot
    on_authorization_unavailable: fail
```

### `pr_approve` (by URL)

```yaml
- id: approve-pr-by-url
  operator: GhOperator
  params:
    operation: pr_approve
    pr_url: https://github.com/goailoop/ailoop/pull/36
```

### `project_resolve_board`

```yaml
- id: resolve-board
  operator: GhOperator
  params:
    operation: project_resolve_board
    owner: myorg
    project_number: 1
```

### `project_item_set_status`

```yaml
- id: set-status
  operator: GhOperator
  params:
    operation: project_item_set_status
    item_id: "PVTI_abc123"
    board: { $expr: 'tasks.resolve_board.output' }
    status: "In progress"
    on_error: fail
```

### Authorization example

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
