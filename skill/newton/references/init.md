# newton init

## Purpose

Create a **Newton workspace**: `.newton/` layout, plan queue directories, default config stub, and template content installed via **aikit** (templates fetched or rendered through **aikit-sdk**; **`aikit` must be on `PATH`**).

## Requirements

- Target path must be an **existing** directory.
- `.newton` must **not** already exist there (remove it or pick another path).
- **`aikit`** available for template install (see Newton repository `README.md`).

## Arguments

- **`PATH`** (optional positional): Directory to initialize (defaults to current directory; canonicalized to absolute).
- `--template-source <SOURCE>`: Template locator (GitHub slug, URL, or local path). Default: `gonewton/newton-templates`.

## What gets created

- `.newton/configs/`, `.newton/tasks/`, `.newton/plan/default/{todo,completed,failed,draft}/`, `.newton/state/`.
- `.newton/configs/default.conf` with `project_root`, `coding_model`, and a commented `workflow_file=` line. Set `workflow_file` (or `workflow_path`) when using `newton batch`.

## Example

```bash
newton init .

newton init /path/to/repo --template-source gonewton/newton-templates
```

## Next steps

```bash
newton run path/to/workflow.yaml --workspace .
```

See [configuration.md](configuration.md) for `.conf` keys and [batch.md](batch.md) for the plan queue.
