# newton-cli

The `newton` binary plus the library that wires it through `cli-framework`.

## Command registration

Every Newton command is declared exactly once in
`src/cli/framework_setup.rs` as a `cli_framework::command::Command`.  The
`build_app(ctx)` entry point registers them in order and is called by
`src/main.rs`.

Each command's `CommandSpec` populates four pieces of metadata:

- `summary` — one-line description (≤80 chars)
- `syntax` — usage hint (e.g. `[WORKFLOW] [INPUT_FILE] [OPTIONS]`)
- `category` — one of the constants in `src/cli/categories.rs`
  (`workflow`, `ops`, `maintenance`, `workspace`, `operational`,
  `diagnostic`)
- `args` — the `ArgSpec` array consumed by cli-framework's clap adapter

`tests/integration/test_command_metadata.rs` enforces that every
registered command carries those values.  `REGISTERED_COMMAND_IDS` in
`framework_setup.rs` is the canonical name list — adding or renaming a
command requires updating this constant.

## Operational commands

The org-baseline operational commands live in `src/cli/ops.rs`:

- `health` — print `newton OK <version>` and exit 0
- `doctor` — run local diagnostic probes (workspace, config, ailoop,
  gh, logging) and report `OK|FAIL|SKIP`
- `config show` — dump the resolved configuration as JSON with secrets
  redacted (`token|secret|password|key` keys → `***REDACTED***`)
- `completion <shell>` — emit a shell completion stub (bash, zsh, fish,
  powershell)

## `ask` command (feature-gated)

Build with `--features ask` to enable a substring-based natural-language
router (`src/cli/ask.rs`).  Run `newton ask "<query>"` to print the
top-3 commands ranked against `summary`/`syntax`/`category`.

## Logging invocation mapping

`src/main.rs` peeks the subcommand from argv and maps it to a
`LogInvocationKind` via `src/cli/log_invocation.rs:kind_for_command`.
Operational and diagnostic commands route to
`LogInvocationKind::Diagnostic`.

## See also

- The `cli-framework` skill for the upstream framework's CommandSpec
  and ArgSpec contracts.
- `tmp/231-migrate-newton-cli-to-cli-framework.md` for the full
  migration spec.
