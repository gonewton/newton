---
name: newton-cli-commands
description: Summarizes Newton CLI workflows and links to detailed run/step/status/report/error command references. Use when documenting, explaining, or updating Newton's subcommand behavior.
---

# Newton CLI Command Playbook

Use this skill whenever you need to operate Newton from the command line and want a reminder of what each subcommand expects.

## Quick Start

1. Run `newton --help` to see the global description and available commands.
2. Inspect any subcommand with `newton <command> --help` to view supported options.
3. Bootstrap the workspace with `newton init` (defaults to the current directory) so `.newton/` is created and populated by **aikit-sdk**.
4. Refer to the reference sheets in this skill for typical workflows, mandatory arguments, and example invocations.
5. Adjust values (paths, IDs, formats) to match your workspace before executing commands.

## Primary Workflow

1. `newton init [PATH]`: initialize a workspace using the Newton template. Defaults to the current directory. See [references/init.md](references/init.md).
2. `newton run WORKSPACE`: launch full optimization loop. See [references/run.md](references/run.md).
3. `newton step WORKSPACE`: manually advance one iteration. See [references/step.md](references/step.md).
4. `newton batch PROJECT_ID`: consume queued plans for a project with before/after hooks. See [references/batch.md](references/batch.md).
5. `newton status EXECUTION`: inspect state. See [references/status.md](references/status.md).
6. `newton report EXECUTION`: summarize results. See [references/report.md](references/report.md).
7. `newton error EXECUTION`: diagnose failures. See [references/error.md](references/error.md).

Each reference file lists required arguments, optional flags, and example invocations so you can run Newton without diving into implementation details.

## Usage Notes

- For additional context, check `newton --help` or the specific command's `--help` output at any time.
- `newton init` installs the Newton template via **aikit-sdk** so `.newton/scripts/*.sh` and `.newton/configs/default.conf` exist without manual setup.
- After initialization, simply running `newton run` inside the workspace uses the current directory (no PATH argument required) and picks up `.newton/scripts/{evaluator,advisor,executor}.sh`.
- Strict mode toggles (`--evaluator-cmd`, etc.) require valid executable commands available in your environment.
- When specifying workspaces or artifact paths, provide absolute or workspace-relative paths that already exist.
- Always make sure you have write permissions to the workspace before running `run` or `step`.

## Initialization

- Run `newton init` (or `newton init ./workspace`) to scaffold `.newton/` via the Newton template. The command uses **aikit-sdk** to copy README, script, and plan artifacts into your workspace.
- `--template-source <SOURCE>` lets you point to a GitHub repo, URL, or local path instead of the default `gonewton/newton-templates`.
- If the template omits `executor.sh`, Newton writes a lightweight executable stub so `newton run` has a fallback.
- After init completes, invoke `newton run` from the workspace root without supplying a path—the CLI now assumes the current directory when no path argument is given.

## Helpful Commands

```bash
cargo run -- --help
cargo run -- run ./workspace --max-iterations 2 --max-time 60
cargo run -- status exec_123 --workspace ./workspace
```

## References

- [references/init.md](references/init.md)
- [references/run.md](references/run.md)
- [references/step.md](references/step.md)
- [references/batch.md](references/batch.md)
- [references/status.md](references/status.md)
- [references/report.md](references/report.md)
- [references/error.md](references/error.md)
