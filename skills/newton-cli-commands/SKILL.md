---
name: newton-cli-commands
description: Summarizes Newton workflow-graph CLI usage and links to reference sheets for init, run, batch, and monitor. Use when documenting or explaining Newton subcommands, flags, or workspace layout.
---

# Newton CLI Command Playbook

Use this skill when operating Newton from the command line. Newton runs **workflow graphs** defined in YAML (tasks, operators, checkpoints, goal gates), not a legacy evaluator or advisor loop.

## Quick Start

1. Run `newton --help` for the command list; use `newton <command> --help` for flags.
2. Run `newton init [PATH]` to create `.newton/` and install the template via `aikit` (PATH defaults to the current directory).
3. Run a workflow with `newton run <workflow.yaml> --workspace <root>` (see [references/run.md](references/run.md)).
4. Use the reference sheets below for init, batch queue mode, and monitor.

## CLI commands (source order)

These thirteen subcommands match the current CLI:

| Command | Role |
| --- | --- |
| `run` | Execute a workflow graph from YAML |
| `init` | Create `.newton/` and install the default template |
| `batch` | Process queued plans under `.newton/plan/<project_id>/` |
| `serve` | HTTP/WebSocket API for workflow state and streaming |
| `monitor` | Terminal UI for ailoop HIL channels |
| `validate` | Validate workflow YAML before run |
| `dot` | Emit Graphviz DOT for the workflow graph |
| `lint` | Best-practice checks on a workflow file |
| `explain` | Human-readable description of workflow behavior |
| `resume` | Continue from a checkpoint (`--execution-id`) |
| `checkpoints` | `list` / `clean` checkpoint data |
| `artifacts` | `clean` old execution artifacts |
| `webhook` | `serve` or `status` for webhook-triggered runs |

For commands without a reference file here, treat `newton <cmd> --help` as the source of truth for flags and examples.

## Typical flows

1. **New workspace**: `newton init .` then set `workflow_file` in `.newton/configs/default.conf` when using batch; run workflows with `newton run path/to/workflow.yaml --workspace .`.
2. **Queue of plans**: Configure `.newton/configs/<project_id>.conf` with `project_root` and `workflow_file`; place plans in `.newton/plan/<project_id>/todo/`; run `newton batch <project_id>`.
3. **Live HIL**: Start [ailoop](https://github.com/goailoop/ailoop), point `.newton/configs/monitor.conf` at HTTP and WebSocket URLs (or pass `--http-url` / `--ws-url`), then `newton monitor`.
4. **API / dashboards**: `newton serve` exposes REST, WebSocket, and SSE endpoints for workflow instances and streams (see `newton serve --help` and repository `README.md` when updated).

## Usage notes

- `newton init` requires `aikit` on `PATH` and refuses to run if `.newton` already exists (remove it or pick another directory).
- `newton run` resolves the workflow path from `--file` if set, otherwise the first positional argument.
- `--server <URL>` on `newton run` registers the run with a Newton API instance started via `newton serve` for lifecycle notifications.
- Checkpoint and artifact layouts live under `.newton/` inside the workspace you pass with `--workspace` (or the discovered project root for batch).

## References

- [references/init.md](references/init.md)
- [references/run.md](references/run.md)
- [references/batch.md](references/batch.md)
- [references/monitor.md](references/monitor.md)
