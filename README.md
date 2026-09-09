# Newton

**Newton** is a workflow-first CLI for deterministic automation and orchestration — **and an autonomous optimizer**. You define steps in YAML (shell commands, agents, human approvals, branching, nested workflows), and Newton runs them with explicit completion rules, checkpoints, and artifacts. On top of that, Newton drives an **optimization loop** that grades a project and improves it toward a target Grade. It fits agent-assisted coding, release checklists, and self-improving optimization loops where you want a defined graph instead of ad hoc scripts.

Version: **0.5.127** · Repository: [github.com/gonewton/newton](https://github.com/gonewton/newton)

## Installation

### macOS / Linux (Homebrew)

```bash
brew tap gonewton/cli
brew install newton
```

### Windows (Scoop)

```powershell
scoop bucket add gonewton https://github.com/gonewton/scoop-bucket
scoop install newton
```

Verify: `newton --version` and `newton --help`.

## Prerequisites

- The **Newton CLI** (installed above).
- **Optional**: Git for version control, hooks, and the optimization loop's local-merge delivery.

`newton init .` scaffolds a workspace and installs the default template via the bundled **aikit-sdk** (statically linked). You do **not** need the `aikit` binary on your `PATH` for init.

## Quick start

1. Create a project directory and `cd` into it.
2. Initialize the workspace:

   ```bash
   newton init .
   ```

3. Run a workflow:

   ```bash
   newton workflow run path/to/workflow.yaml --workspace .
   ```

For an existing repository, run `newton init .` at the repo root. Run workflow
graphs directly, or bind an Optimization Definition to drive the
[optimization loop](#optimization-loop).

## What you get

Newton runs YAML workflow graphs with:

- **Operators**: shell commands, context patches, assertions, nested workflows, human approval/decision gates, GitHub CLI actions, and agent engines (availability depends on your workflow; run `newton workflow preview <file>` to see the resolved operator list).
- **Safety**: lint, validate, and preview before run; guarded shell usage and reachability checks.
- **Durability**: checkpoint persistence, resume, artifact routing, and execution history under `.newton/`.
- **Authoring**: macros, `include_if` filtering, `{{ ... }}` interpolation, and `$expr` evaluation.

Built-in operators include `CommandOperator`, `WorkflowOperator` (nested workflows), `HumanApprovalOperator`, `HumanDecisionOperator`, `GhOperator` (GitHub CLI), and `GitOperator` (typed git operations: `clean_check`, `sync_main`, `create_branch`, `commit`, `push` with retry, `diff`, `cleanup_merge`). Recurring shell patterns are promoted to typed operators with `success`/`exit_code` outputs; `CommandOperator` remains the escape hatch for bespoke glue. Agent operators integrate with **aikit-sdk**; quota exhaustion surfaces as error code `WFG-AGENT-008` (provider-agnostic detection via aikit-sdk, not by parsing agent output).

For operator reference, see [docs/operators/](docs/operators/) and the [Newton skill](skill/newton/SKILL.md) (`skill/newton/references/`).

## Common commands

| Command | Purpose |
| --- | --- |
| `newton workflow run <file>` | Execute a workflow graph |
| `newton workflow validate\|lint\|preview\|graph` | Check or explain a workflow before run |
| `newton workflow resume --run-id <UUID>` | Continue from a checkpoint |
| `newton workflow runs list\|show` | Inspect past executions |
| `newton workflow checkpoint\|artifact` | Manage checkpoints and artifacts |
| `newton init [path]` | Scaffold `.newton/` and install template |
| `newton optimize <project_id> --definition <file>` | Run a definition-bound native optimization lifecycle |
| `newton serve` | HTTP/WebSocket API for workflow state, loop observation, and integrations |
| `newton data <verb> <entity>` | Catalog CRUD (`finding`, `change-request`, `plan`, `optimize-run`, …) |
| `newton doctor` | Environment readiness diagnostics |
| `newton schema export` | Emit the workflow IR JSON Schema (operator-discriminated) |

> `webhook` (external HTTP ingress) and `health` were removed: the optimizer is self-driving (ADR 0004), and `health` folded into `doctor`.

Run `newton <command> --help` for flags and examples. The top-level `newton run` command is deprecated; use `newton workflow run`.

### Workflow run (minimal example)

```bash
newton workflow run workflow.yaml
newton workflow run workflow.yaml --workspace ./output --trigger env=prod
newton workflow run workflow.yaml --timeout 3600 --parallel-limit 4 --verbose
```

Trigger payload merge order: `--parameters-json` (base object), then each `--trigger KEY=VAL` in order. Values prefixed with `@` load file contents.

### Optimization loop

Newton's native loop runs a reusable, versioned **Optimization Definition**. A
definition declares the strategy, objective, comparison policy, constraints,
limits, completion criteria, and workflow roles. It can use a numeric objective
with units (for example, bytes or verified vulnerabilities) or a Grade.

```
baseline grade ─→ plan ─→ candidate develop ─→ candidate grade ─→ accept ─→ promote
                                                               │
                                                     constraints + comparison
```

The software-improvement strategy may use the durable `Finding → Change Request
→ Plan → Execution` spine, but unrelated strategies do not have to. A candidate
is promoted only after its exact evaluated artifact qualifies under the active
requirements. A passing development test alone is not acceptance.

```bash
# Definition path can also be definition_file in .newton/configs/<id>.conf.
newton optimize my-project --definition .newton/definitions/my-security.yaml --once
# Inspect a finished run or continue only from a known safe durable phase.
newton optimize my-project --resume <RUN_ID>
```

The definition roles `grade`, `plan`, and `develop` are required for the native
software strategy; `promote` is optional. Each role emits a validated result
envelope. See [the optimization contract](docs/optimization-contract.md) for a
definition example, permission boundaries, requirements revisions, and recovery
semantics.

### HTTP serve API

`newton serve` exposes REST, WebSocket, and SSE endpoints for workflow state, portfolio data, human-in-the-loop, and AI tool sessions — and serves the **web UI** (embedded in the binary) at the root by default:

```bash
newton serve --host 127.0.0.1 --port 8080
# open http://127.0.0.1:8080/ in a browser for the UI (optimize runs, findings,
# change requests, plans). On startup the command prints a banner with these URLs.

newton serve --with-mcp    # mount MCP at /mcp on the same port
newton serve --no-web      # API only (no web UI); pair with the Vite dev server for UI work
```

- **Web UI**: served at `/` by default. The bundle is built from the separate `newton-ui` repo and vendored via [`scripts/vendor-web.sh`](scripts/vendor-web.sh); `--no-web` disables it.
- **OpenAPI contract**: [openapi/newton-api.yaml](openapi/newton-api.yaml)
- **Realtime contract**: [openapi/newton-realtime.asyncapi.yaml](openapi/newton-realtime.asyncapi.yaml)
- **Health**: `GET /healthz` · **API docs**: `GET /api/docs`

REST routes are versioned under `/api/v1/`. Run `newton serve --help` for the full route list.

#### Authentication (OIDC)

`newton serve` binds `127.0.0.1` by default and stays a friction-free local tool: with no OIDC configured, a loopback bind serves the API unauthenticated, exactly as before. Binding a non-loopback `--host` is the explicit act of exposing the API to other hosts, so from that point authentication is **required**, not optional — `newton serve` refuses to start if OIDC isn't configured:

```bash
# Local, unauthenticated (default) — unchanged.
newton serve

# Loopback + OIDC: auth enforced even though nothing forces it.
newton serve --oidc-issuer https://keycloak.example.com/realms/newton --oidc-audience newton-api

# Non-loopback: OIDC is mandatory. Omitting it refuses to start with an
# actionable error naming the flags/env vars below.
newton serve --host 0.0.0.0 \
  --oidc-issuer https://keycloak.example.com/realms/newton \
  --oidc-audience newton-api
```

- `--oidc-issuer <url>` / `NEWTON_OIDC_ISSUER` — the OIDC issuer URL (flag wins over env var). Must be `https://`, except `http://127.0.0.1`, `http://localhost`, or `http://[::1]` for local dev.
- `--oidc-audience <value>` / `NEWTON_OIDC_AUDIENCE` — accepted JWT `aud` value. Repeat `--oidc-audience` for multiple values (mapped to "any of these"), or set `NEWTON_OIDC_AUDIENCE` to a comma-separated list.
- Both must be set together — an issuer with no audience (or vice versa) is a startup error.
- When configured, bearer tokens are validated (signature, issuer, audience, expiry) against the issuer's JWKS on every request to the REST API (`/api/{version}` mounts), the MCP router (`/mcp`), and any additional `--with-embedded-ailoop` mount. The embedded web UI (`/`, SPA routes) and `/healthz` / `/readyz` stay public regardless of auth config, so the UI keeps loading without a token while the API itself is gated.
- A request with no/invalid token gets `401` with a `WWW-Authenticate: Bearer` challenge; if the issuer's JWKS is temporarily unreachable, requests get `503` with `Retry-After`.

### MCP mode

Expose Newton commands as MCP tools:

```bash
# Combined REST + MCP (recommended)
newton serve --with-mcp

# Dedicated MCP-only process (loopback-only)
newton mcp serve --port 8730
```

`newton serve --with-mcp` mounts MCP at `/mcp` on the serve port; when OIDC is configured, `/mcp` is gated behind the same bearer-token layer as the REST API, so it is the way to expose MCP to a non-loopback interface. The dedicated `newton mcp serve` has **no** authentication layer, so it is **loopback-only** and refuses a non-loopback `--host` (`NEWTON-MCP-003`).

See `newton mcp serve --help` for client configuration examples.

### Human-in-the-loop

`HumanApprovalOperator` and `HumanDecisionOperator` pause workflows for human input via [ailoop](https://github.com/goailoop/ailoop). Configure ailoop in `.newton/configs/*.conf` or via `NEWTON_AILOOP_*` environment variables. See [docs/operators/human_approval.md](docs/operators/human_approval.md) and [docs/operators/human_decision.md](docs/operators/human_decision.md).

## Authoring workflows in code

Workflow YAML is the IR the engine runs, but you can author it in a typed
language and compile down to that same YAML. The engine never learns where the
YAML came from — handwritten and generated definitions are
indistinguishable. These authoring surfaces live in `packages/`:

| Package | Language | Authoring entry point |
| --- | --- | --- |
| [`packages/newton-dsl-py`](packages/newton-dsl-py/) | Python (`uv` / pydantic) | `from newton import Workflow` |
| [`packages/newton-dsl-ts`](packages/newton-dsl-ts/) | TypeScript (`pnpm`) | `import { Workflow } from "@newton/dsl"` |

Both surfaces use `.then()` / `.when()` / `.otherwise()` for transitions,
`repeat_at_most()` for bounded loops, typed `.out` references between tasks, and
`wf.expects()` for inputs — then emit validated YAML for `newton workflow run`.
See each package's README for the full API.

The shared, committed JSON Schema and a conformance corpus both surfaces test
against live in [`packages/workflow-schema`](packages/workflow-schema/).

## Schema export

`newton schema export` emits the workflow IR as a single composed,
operator-discriminated JSON Schema — the contract the authoring surfaces and
external tooling validate against:

```bash
newton schema export --pretty                       # composed IR schema to stdout
newton schema export --out workflow.schema.json     # write to a file
newton schema export --outputs                      # per-operator output schemas
```

Each operator owns its own `params_schema()` and `output_schema()`
([ADR 0006](docs/adr/0006-operators-own-param-and-output-schemas.md)).

## Workspace layout

After `newton init`, Newton expects:

```
workspace/
├── .newton/
│   ├── workflows/       # Workflow YAML (from template)
│   ├── grader/          # Command-Graders: <name>/generate.sh (prints an Assessment)
│   ├── configs/         # Project bindings and integration config (*.conf)
│   ├── definitions/     # Versioned Optimization Definition YAML
│   ├── optimize/        # Durable run journals and local ownership markers
│   ├── state/           # Workflow run records
│   ├── checkpoints/     # Resume checkpoints
│   ├── artifacts/       # Generated artifacts
│   └── logs/            # newton.log
└── (your project files)
```

## Logging

Logs default to `<workspace>/.newton/logs/newton.log` (or `$HOME/.newton/logs/newton.log` when no workspace is detected). Override per invocation with `--log-dir`.

Optional tuning via `.newton/config/logging.toml` and `RUST_LOG` for tracing verbosity. Set `NEWTON_REMOTE_AGENT=1` to keep file logging active while suppressing console output in remote or batch contexts.

Inspect past runs:

```bash
newton workflow runs list
newton workflow runs show --run-id <UUID> --task <TASK_ID>
```

## Further reading

| Resource | Contents |
| --- | --- |
| [skill/newton/SKILL.md](skill/newton/SKILL.md) | Command reference and typical flows |
| [CONTEXT.md](CONTEXT.md) | Domain glossary (loop, grading, portfolio, planning) |
| [docs/DEPLOY.md](docs/DEPLOY.md) | Deployment notes |
| [CHANGELOG.md](CHANGELOG.md) | Release history |
| [architecture.md](architecture.md) | System design (contributors) |

## License

See [LICENSE](LICENSE) for details.

Contributors: see [CONTRIBUTING.md](CONTRIBUTING.md).
