# Contributing to Newton

## Crate Layout

The repository is a Cargo workspace with four member crates:

| Crate | Package | Role |
|---|---|---|
| `crates/core` | `newton-core` | Library: workflow engine, batch runner, HTTP API, integrations, logging, utils. No CLI/TUI deps. |
| `crates/cli` | `newton-cli` | Binary `newton`: argument parsing, logging bootstrap, TUI monitor. Depends on `newton-core`. |
| `crates/types` | `newton-types` | Shared types (leaf crate). |
| `crates/backend` | `newton-backend` | Persistence/store models. Depends on `newton-types`. |

Dependency direction: `newton-cli` → `newton-core` → `{ newton-types, newton-backend }`. `newton-core` MUST NOT depend on `clap`, `ratatui`, or `crossterm`; this invariant is verified in CI via `cargo tree -p newton-core`.

To build the binary: `cargo build -p newton-cli`. To embed the engine as a library: add `newton-core = { path = "crates/core" }` to your `Cargo.toml`.

## Development Rules

See [AGENTS.md](AGENTS.md) for the full rule set. Key requirements:

- NEWTON-0001: Use conventional commit messages (`type(scope): description`, imperative mood, first line ≤ 72 chars).
- NEWTON-0002: Never use `--no-verify` when committing.
- NEWTON-0003/0004: Run `cargo test --workspace` and `cargo fmt --all` before pushing.
- NEWTON-0005: Run `cargo clippy --all-targets` and fix warnings.
- NEWTON-0007/0015/0016: Write unit tests for all public functions and integration tests for complex workflows.
- NEWTON-0019/0020: All CI and security-audit checks must pass before merging.

## Ailoop Integration Architecture

Newton optionally integrates with an ailoop server for real-time notifications and human-in-the-loop (HITL) prompts. The integration is in `crates/core/src/integrations/ailoop/`.

### Transport

All ailoop communication uses **WebSocket only** via the `ailoop-core` git dependency. Newton does not use ailoop's HTTP API for the integration transport. The `NEWTON_AILOOP_HTTP_URL` environment variable is accepted but silently ignored (kept for backwards compatibility with old config files).

Configuration is resolved from (highest priority first):

1. `NEWTON_AILOOP_WS_URL` + `NEWTON_AILOOP_CHANNEL` environment variables
2. `ailoop_server_ws_url` / `ailoop_channel` keys in `.newton/configs/monitor.conf` (or other `.conf` files in that directory)

Integration is disabled unless explicitly enabled via `NEWTON_AILOOP_INTEGRATION=1` or via env vars that provide a complete config.

### Components

| Module | Struct | Direction | Content type |
|---|---|---|---|
| `integrations/ailoop/output_forwarder.rs` | `OutputForwarder` | Newton → ailoop | `MessageContent::Stdout` / `Stderr` |
| `integrations/ailoop/orchestrator_notifier.rs` | `OrchestratorNotifier` | Newton → ailoop | `MessageContent::Notification` |
| `integrations/ailoop/workflow_emitter.rs` | `WorkflowEmitter` | Newton → ailoop | `MessageContent::WorkflowProgress` |
| `workflow/human/ailoop.rs` | `AiloopInterviewer` | bidirectional | `MessageContent::Authorization` / `Question` → `Response` |

`OutputForwarder` and `WorkflowEmitter` are fire-and-forget (no response expected). `OrchestratorNotifier` uses retry with exponential backoff. `AiloopInterviewer` blocks until the human responds or a timeout fires.

### HITL Interviewer Selection

`workflow::human::build_interviewer()` selects the backend:

1. `NEWTON_HITL_TRANSPORT=console` → always use `ConsoleInterviewer`
2. `NEWTON_HITL_TRANSPORT=ailoop` + context available → `AiloopInterviewer`
3. Enabled `AiloopContext` present → `AiloopInterviewer`
4. Fallback → `ConsoleInterviewer`

`AiloopInterviewer` calls `ailoop_core::client::authorize()` (for `HumanApprovalOperator`) and `ailoop_core::client::ask()` (for `HumanDecisionOperator`). Error codes `WFG-HUMAN-101…105` are mapped from transport and response outcomes.

### TUI Monitor

The CLI's `newton monitor` command uses a **separate** HTTP endpoint for the ailoop REST API (`/api/...` routes). This is configured via `MonitorEndpoints` in `crates/cli/src/monitor/config.rs` and is unrelated to the integration transport above.

## Build Commands

```bash
cargo build --workspace          # build all crates
cargo build -p newton-cli        # build the newton binary
cargo test --workspace           # run all tests
cargo fmt --all -- --check       # check formatting
cargo clippy --all-targets       # lint
cargo tree -p newton-core        # verify no clap/ratatui/crossterm in core
```
