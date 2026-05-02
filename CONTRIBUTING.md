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

## Build Commands

```bash
cargo build --workspace          # build all crates
cargo build -p newton-cli        # build the newton binary
cargo test --workspace           # run all tests
cargo fmt --all -- --check       # check formatting
cargo clippy --all-targets       # lint
cargo tree -p newton-core        # verify no clap/ratatui/crossterm in core
```
