# Newton configuration (`.newton/configs`)

Workspace Newton metadata lives under `.newton/`. This sheet describes the **`key=value` `.conf` files** in `.newton/configs/` and **which keys the Newton CLI reads** for batch and monitor.

The same file may also be sourced or extended by **organization-specific** shell, CI, or wrappers; only the keys in the sections below are read by Newton's `batch` and `monitor` subcommands. Anything else is outside the tool unless your distribution documents it.

Lines support `#` comments. Unknown keys are ignored by each consumer unless noted.

## `newton batch <project_id>` — `.newton/configs/<project_id>.conf`

The batch runner loads **only** the following keys from Rust (`BatchProjectConfig`):

### Required

| Key | Meaning |
| --- | --- |
| `project_root` | Directory that contains a `.newton/` tree (absolute path, or relative to the **Newton workspace root** passed to `--workspace` / discovered by walking upward for `.newton`). |
| `workflow_file` **or** `workflow_path` | Path to the workflow YAML Newton runs for each queued plan. Use **`workflow_file`** in new configs. **`workflow_path`** is an alias when `workflow_file` is unset (same resolution). If both are set, **`workflow_file` wins**. Resolution: absolute values are used as-is; otherwise if `project_root/<value>` exists that path wins, else `workspace_root/<value>`. |

### What batch does

1. Picks the next markdown plan from `.newton/plan/<project_id>/todo/`.
2. Copies it to `project_root/.newton/tasks/<task_id>/input/spec.md`.
3. Runs the configured workflow like `newton run` with a **manual** trigger payload containing `input_file` and `workspace` (see repository `README.md`).
4. Moves the plan to `completed/` or `failed/`.

No `coding_agent`, `evaluator_cmd`, `pre_run_script`, or classic-loop keys are read by `newton batch` anymore; drive agents and shell from **workflow YAML** and operators instead.

### Example (minimal)

```bash
# .newton/configs/myapp.conf
project_root=/home/me/repos/myapp
workflow_file=.newton/workflows/planner.yaml
```

Relative workflow under project root:

```bash
project_root=./repos/myapp
workflow_file=newton/workflows/ci.yaml
```

### Plan metadata

Plans are Markdown files in `.newton/plan/<project_id>/todo/`. Optional YAML frontmatter can include `branch:` for workflows that consume it (see main Newton `README.md` for the plan format).

---

## `newton init` — `.newton/configs/default.conf`

After `newton init .`, Newton writes `default.conf` with:

- `project_root=` set to the initialized directory (absolute).
- `coding_model=` default (from built-in executor defaults, often a Z.ai-style id string).
- A commented `# workflow_file=...` hint for batch.

`coding_model` in that stub is **not** consumed by `newton batch`; it is legacy/extra context for templates and humans. Set `workflow_file` (or `workflow_path`) when you use batch.

---

## `newton monitor` — ailoop endpoints

Monitor needs **both** HTTP and WebSocket base URLs. It reads `.newton/configs/monitor.conf` first, then scans other `.conf` files in alphabetical order until both values are set.

| Key | Purpose |
| --- | --- |
| `ailoop_server_http_url` | ailoop HTTP API base (e.g. `http://127.0.0.1:8081`). |
| `ailoop_server_ws_url` | ailoop WebSocket URL (e.g. `ws://127.0.0.1:8080`). |
| `workflow_service_url` | Optional Newton HTTP API (`newton serve`) for workflow UI integration. |

CLI overrides: `newton monitor --http-url ... --ws-url ...`.

Keys such as `ailoop_ws_url` alone are **not** read by Newton monitor; use the pair above or pass URLs on the command line.

---

## Model and engine strings

Valid model and engine identifiers depend on the **workflow YAML**, the **agent operator**, and your provider. Confirm with `newton run --help` and your workflow definitions; do not treat examples in the wild as stable API.

---

## Validation errors (batch)

Typical failures:

- Missing `project_root` or both `workflow_file` and `workflow_path`.
- `project_root` does not contain `.newton/`.
- Unreadable `.conf` path (wrong `--workspace` or missing `.newton/configs/<project_id>.conf`).
