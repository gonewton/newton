---
name: newton-cli-commands
description: Summarizes Newton CLI workflows and links to detailed run/step/status/report/error command references. Use when documenting, explaining, or updating Newton's subcommand behavior.
---

# Newton CLI Command Playbook

Use this skill whenever you need to operate Newton from the command line and want a reminder of what each subcommand expects.

## Quick Start

1. Run `newton --help` to see the global description and available commands.
2. Inspect any subcommand with `newton <command> --help` to view supported options.
3. Refer to the reference sheets in this skill for typical workflows, mandatory arguments, and example invocations.
4. Adjust values (paths, IDs, formats) to match your workspace before executing commands.

## Primary Workflow

1. `newton run WORKSPACE`: launch full optimization loop. See [references/run.md](references/run.md).
2. `newton step WORKSPACE`: manually advance one iteration. See [references/step.md](references/step.md).
3. `newton status EXECUTION`: inspect state. See [references/status.md](references/status.md).
4. `newton report EXECUTION`: summarize results. See [references/report.md](references/report.md).
5. `newton error EXECUTION`: diagnose failures. See [references/error.md](references/error.md).

Each reference file lists required arguments, optional flags, and example invocations so you can run Newton without diving into implementation details.

## Usage Notes

- For additional context, check `newton --help` or the specific command's `--help` output at any time.
- Strict mode toggles (`--evaluator-cmd`, etc.) require valid executable commands available in your environment.
- When specifying workspaces or artifact paths, provide absolute or workspace-relative paths that already exist.
- Always make sure you have write permissions to the workspace before running `run` or `step`.

## Helpful Commands

```bash
cargo run -- --help
cargo run -- run ./workspace --max-iterations 2 --max-time 60
cargo run -- status exec_123 --workspace ./workspace
```

## References

- [references/run.md](references/run.md)
- [references/step.md](references/step.md)
- [references/status.md](references/status.md)
- [references/report.md](references/report.md)
- [references/error.md](references/error.md)
