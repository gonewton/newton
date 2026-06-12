# Contributing to Newton

Thank you for contributing to Newton. This document covers development setup, standards, and the pull request process. End-user documentation lives in [README.md](README.md); system design is in [architecture.md](architecture.md).

## Crate layout

The repository is a Cargo workspace:

| Directory | Package | Role |
| --- | --- | --- |
| `crates/core` | `newton-core` | Workflow engine, batch logic, HTTP API, integrations, logging. No CLI/TUI deps. |
| `crates/cli` | `newton-cli` | Binary `newton`: clap/cli-framework wiring. Depends on `newton-core`. |
| `crates/types` | `newton-types` | Shared API and domain types (leaf crate). |
| `crates/backend` | `newton-backend` | SQLite persistence store. Depends on `newton-types`. |
| `crates/test-utils` | `ws001-test-utils` | Shared test helpers (HTTP fixtures, temp workspaces). |

Dependency direction:

```
newton-cli → newton-core → { newton-types, newton-backend }
```

**Invariant**: `newton-core` MUST NOT depend on `clap`, `ratatui`, or `crossterm`. CI verifies this with `cargo tree -p newton-core`.

- Build the binary: `cargo build -p newton-cli`
- Embed the engine: add `newton-core = { path = "crates/core" }` to your `Cargo.toml`

### Non-Rust packages

Code-based authoring surfaces and the shared schema live under `packages/` (outside the Cargo workspace):

| Directory | Stack | Role |
| --- | --- | --- |
| `packages/workflow-schema` | committed JSON | Canonical workflow IR `workflow.schema.json` + per-operator `output_schemas.json`, plus a `conformance/` corpus both surfaces test against. |
| `packages/newton-dsl-py` | Python (`uv` / pydantic) | Python authoring surface; compiles to the YAML IR. |
| `packages/newton-dsl-ts` | TypeScript (`pnpm`) | TypeScript authoring surface; compiles to the YAML IR. |

The committed schemas are the source of truth for both surfaces. Each surface generates typed models from them (`packages/*/codegen/generate.sh`) and verifies they are current (`packages/*/codegen/check_drift.sh`). Regenerate after changing any operator's `params_schema()` / `output_schema()`:

```bash
newton schema export --out packages/workflow-schema/workflow.schema.json --pretty
newton schema export --outputs --out packages/workflow-schema/output_schemas.json --pretty
bash packages/newton-dsl-py/codegen/generate.sh
bash packages/newton-dsl-ts/codegen/generate.sh
```

Generated files (`src/newton/_generated/`, `src/generated/`) are committed; `generate.sh` runs with `--disable-timestamp` so output is deterministic and `check_drift.sh` does not trip on clock differences.

## Development environment

### Rust toolchain

Pin is in [rust-toolchain.toml](rust-toolchain.toml) (currently **1.93.1** with `rustfmt` and `clippy`). CI uses Rust **1.94**; keep local toolchains reasonably current.

Install components:

```bash
rustup toolchain install 1.93.1 --component rustfmt clippy
```

### Build and test

```bash
cargo build --workspace          # build all crates
cargo build -p newton-cli        # build the newton binary
cargo test --workspace           # run all tests
cargo fmt --all -- --check       # check formatting
cargo clippy --all-targets --all-features -- -D warnings
cargo tree -p newton-core        # verify no clap/ratatui/crossterm in core
```

Run a workflow locally:

```bash
cargo run -p newton-cli -- workflow run examples/hello.yaml
cargo run -p newton-cli -- workflow run examples/hello.yaml --workspace ./ws --verbose
```

### Embedded web UI

`newton serve` serves the web UI from a single gzip-compressed `index.html`
compiled into the binary at `crates/core/assets/web/index.html.gz`. The UI source
lives in the **separate** `newton-ui` repo. After changing the UI, regenerate the
vendored bundle and commit it:

```bash
scripts/vendor-web.sh [path-to-newton-ui]   # default: ../newton-ui
scripts/vendor-web.sh --check                # CI/pre-PR: fail if the bundle is stale
```

CI runs `--check` only when a `NEWTON_UI_RO_TOKEN` secret is configured (newton-ui
is private), so refreshing the bundle is currently a manual step.

### OpenAPI parity

When adding or modifying backend CRUD endpoints (NEWTON-0028):

```bash
./scripts/generate-openapi.sh
git diff openapi/newton-backend-parity.yaml   # commit updated contract in the same PR
```

CI runs the generator and fails if `openapi/newton-backend-parity.yaml` is out of date.

### Operators and the IR schema

Every `Operator` implements `params_schema()` and `output_schema()` ([ADR 0006](docs/adr/0006-operators-own-param-and-output-schemas.md)). When adding or changing an operator:

1. Register it in `crates/core/src/workflow/operators/mod.rs`.
2. Keep the schema methods accurate — `newton schema export` composes them into the published IR schema.
3. Regenerate the committed schema and the authoring-surface models (see [Non-Rust packages](#non-rust-packages)).

Recurring, failure-prone shell embedded in workflows should be promoted to a typed operator (e.g. `GitOperator`) rather than left in `CommandOperator`, which is the escape hatch for bespoke glue ([ADR 0008](docs/adr/0008-shell-patterns-promoted-to-typed-operators.md)).

## Development rules

See [AGENTS.md](AGENTS.md) for the full rule set. Key requirements:

| Rule | Requirement |
| --- | --- |
| NEWTON-0001/0008–0010 | Conventional commits: `type(scope): description`, imperative mood, first line ≤ 72 chars |
| NEWTON-0002 | Never use `--no-verify` when committing |
| NEWTON-0003/0004 | Run `cargo test --workspace` and `cargo fmt --all` before pushing |
| NEWTON-0005 | Run `cargo clippy --all-targets` and fix warnings |
| NEWTON-0007/0015/0016 | Unit tests for public functions; integration tests for complex workflows |
| NEWTON-0011 | Public APIs must have documentation |
| NEWTON-0019/0020 | All CI and security-audit checks must pass before merging |
| NEWTON-0026/0027 | README is end-user only; contributor material belongs here or in architecture.md |

Git hooks in `.githooks/` enforce commit message format and pre-push checks. Enable with:

```bash
git config core.hooksPath .githooks
```

## CLI framework wiring

Newton's CLI is built on [cli-framework](https://github.com/aroff/cli-framework). Commands are declared in `crates/cli/src/cli/framework_setup/` and registered by `build_app()`.

Each command carries `CommandSpec` metadata (`summary`, `syntax`, `category`, `args`). When adding or renaming a command:

1. Add the command module under `framework_setup/commands/`
2. Update `REGISTERED_COMMAND_IDS` in `framework_setup/mod.rs`
3. Update integration tests in `crates/cli/tests/integration/test_command_metadata.rs`

See [crates/cli/README.md](crates/cli/README.md) for the metadata contract and operational commands (`health`, `doctor`, `config show`, `completion`).

## Pull request process

1. **Branch** from `main` with a descriptive name (`feat/…`, `fix/…`, `refactor/…`).
2. **Implement** with focused commits following conventional commit format.
3. **Test** locally: `cargo test --workspace --all-features`, `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`.
4. **Open a PR** against `main` with a clear summary and test plan.
5. **CI must pass** before merge (see below).
6. **Review** feedback: address comments or explain deferrals.

Do not force-push to `main`. Avoid amending published commits unless you own the branch and no one else has pulled it.

## Continuous integration

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on push and pull requests to `main`:

| Step | What it checks |
| --- | --- |
| `cargo fmt --all -- --check` | Formatting |
| `./scripts/generate-openapi.sh` + git diff | OpenAPI contract parity |
| AsyncAPI validation | `openapi/newton-realtime.asyncapi.yaml` |
| `cargo clippy --all-targets --all-features` | Lint (warnings denied via `RUSTFLAGS=-D warnings`) |
| `cargo build --workspace --release` | Release build |
| `cargo test --workspace --all-features` | Tests |
| `cargo tree -p newton-core` | No CLI/TUI deps in core |
| Security audit job | `cargo audit` with documented ignores |
| Coverage job | `cargo llvm-cov` with 50% line threshold |

Nightly and release workflows live under `.github/workflows/`.

## Exploring the codebase

[repomix-output.xml](repomix-output.xml) is a packed snapshot of the repository for AI-assisted exploration and code review. Regenerate when making large structural changes (if your workflow uses repomix).

Useful entry points:

| Path | Contents |
| --- | --- |
| `crates/core/src/workflow/` | Engine, operators, executor, checkpoints |
| `crates/core/src/workflow/operators/` | Built-in operators (`git/`, `gh.rs`, `command.rs`, `agent/`, …) |
| `crates/core/src/workflow/schema_export.rs` | Composed IR + per-operator output schema generation |
| `packages/` | Code-based authoring surfaces and the shared workflow schema |
| `crates/core/src/api/` | HTTP handlers and OpenAPI generation |
| `crates/core/src/integrations/` | ailoop, external service wiring |
| `crates/backend/src/store/` | SQLite store modules |
| `crates/cli/src/cli/framework_setup/` | Command registration |
| `openapi/` | HTTP and realtime API contracts |

Domain terminology: [CONTEXT.md](CONTEXT.md) (and implementation/internal terms in [architecture.md](architecture.md)).

## Project skills

Agent-oriented command and workflow documentation:

| Location | Purpose |
| --- | --- |
| [skill/newton/](skill/newton/) | Primary Newton skill (commands, batch, operators, configuration) |
| [.agents/skills/newton/](.agents/skills/newton/) | Workspace copy of the Newton skill bundle |
| [.agents/skills/tools-cli-framework/](.agents/skills/tools-cli-framework/) | cli-framework reference for CLI changes |

When changing CLI behavior, update the skill references and `skill/newton/SKILL.md` if user-facing flows change.

## Ailoop integration (developer notes)

Newton integrates with ailoop for HITL and orchestration notifications. Implementation lives in `crates/core/src/integrations/ailoop/` and `crates/core/src/workflow/human/`.

- **Transport**: WebSocket only via `ailoop-core` (not HTTP).
- **Configuration** (highest priority first): `NEWTON_AILOOP_WS_URL` + `NEWTON_AILOOP_CHANNEL` env vars, then `.newton/configs/*.conf` keys. Integration requires explicit enablement (`NEWTON_AILOOP_INTEGRATION=1` or a complete config).
- **Interviewer selection**: `resolve_interviewer()` returns `AiloopInterviewer` when enabled, otherwise `HIL-AILOOP-001`. No console fallback.

Full component breakdown and data flow: [architecture.md](architecture.md#ailoop-human-in-the-loop).

## Questions

Open a GitHub issue or discussion on [github.com/gonewton/newton](https://github.com/gonewton/newton) for bugs, features, or design questions.
