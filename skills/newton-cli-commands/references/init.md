# newton init

## Purpose
Bootstrap a workspace from a Newton template. `init` recreates the expected `.newton/state` layout, renders scripts/configuration from the selected template, and generates a placeholder `GOAL.md`.

## Requirements
- `aikit` must be available on `PATH` (`aikit --version` is used) because templates are distributed via aikit packages.
- At least one template directory must exist under `.newton/templates/<template-name>`.

## Important Flags
- `--template <NAME>`: Template subdirectory name (default `basic`).
- `--name <NAME>`: Project name injected into `newton.toml` and the GOAL stub.
- `--coding-agent <AGENT>` / `--model <MODEL>`: Override executor configuration values in the generated config.
- `--interactive`: Prompt for missing values instead of assuming defaults automatically.
- `--force`: Proceed even if `.newton/` already exists.

## Example Invocation
```bash
newton init . --template basic --interactive
```

## Notes
- The command writes `newton.toml` only when it does not already exist, defaults the `project.template` setting to the selected template, and records a test command (`scripts/run-tests.sh` or `cargo test` fallback).
- Templates should include script files (`evaluator.sh`, `advisor.sh`, `executor.sh`, etc.) and may provide their own `newton.toml`. Rendered `.sh` files are automatically made executable.
