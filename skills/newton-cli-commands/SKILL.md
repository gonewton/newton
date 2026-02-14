---
name: newton-cli-commands
description: Summarizes Newton CLI workflows and links to detailed run/step/status/report/error command references. Use when documenting, explaining, or updating Newton's subcommand behavior.
---

# Newton CLI Command Playbook

Use this skill whenever you need to operate Newton from the command line and want a reminder of what each subcommand expects.

## Quick Start

1. Run `newton --help` to see the global description and available commands.
2. Inspect any subcommand with `newton <command> --help` to view supported options.
3. Bootstrap the workspace with `newton init` (defaults to the current directory) so `.newton/` is created and populated via `aikit`.
4. Refer to the reference sheets in this skill for typical workflows, mandatory arguments, and example invocations.
5. Adjust values (paths, IDs, formats) to match your workspace before executing commands.

## Primary Workflow

1. `newton init WORKSPACE`: scaffold `.newton/`, templates, and config (requires `aikit`). See [references/init.md](references/init.md).
2. `newton run WORKSPACE`: launch full optimization loop. See [references/run.md](references/run.md).
3. `newton step WORKSPACE`: manually advance one iteration. See [references/step.md](references/step.md).
4. `newton batch PROJECT_ID`: consume queued plans for a project with before/after hooks. See [references/batch.md](references/batch.md).
5. `newton status EXECUTION`: inspect state. See [references/status.md](references/status.md).
6. `newton report EXECUTION`: summarize results. See [references/report.md](references/report.md).
7. `newton error EXECUTION`: diagnose failures. See [references/error.md](references/error.md).
8. `newton monitor`: watch live channels. See [references/monitor.md](references/monitor.md).

Each reference file lists required arguments, optional flags, and example invocations so you can run Newton without diving into implementation details.

## Usage Notes

- `newton init` installs the Newton template via `aikit`, so `.newton/scripts/{evaluator,advisor,executor}.sh` and `.newton/state` exist without manual setup.
- After initialization, run `newton run` from the workspace root without supplying a path; the CLI now assumes the current directory when no workspace argument is provided.
- Interactive mode (`--interactive`) lets you confirm or override the project name, coding agent, and model before templates are rendered.
- Strict mode toggles (`--evaluator-cmd`, etc.) still require real executables and the workspace layout produced by `init`.
- Always make sure you have write permissions to the workspace before running `run` or `step`.

## Initialization

- `newton init [PATH]` renders the selected template into `.newton/`, writes `GOAL.md` when missing, and records defaults inside `newton.toml`.
- Templates live under `.newton/templates/<NAME>` and can contain scripts plus an optional `newton.toml` shim; `.sh` files are marked executable when rendered.
- Provided options include `--template`, `--name`, `--coding-agent`, `--model`, `--interactive`, and `--force`.
- Without a `scripts/run-tests.sh`, Newton defaults the evaluator test command to `cargo test`.
- The command exits with an install hint (`https://aikit.readthedocs.io`) when `aikit` is missing.

## References

- [references/init.md](references/init.md)
- [references/run.md](references/run.md)
- [references/step.md](references/step.md)
- [references/batch.md](references/batch.md)
- [references/status.md](references/status.md)
- [references/report.md](references/report.md)
- [references/error.md](references/error.md)
- [references/monitor.md](references/monitor.md)
