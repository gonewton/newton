# newton init

## Purpose

Create a **Newton workspace**: `.newton/` layout, plan queue directories, default config stub, and template content installed via **aikit**.

## Requirements

- Target path must be an existing directory.
- `.newton` must **not** already exist under that path (command errors if it does).
- **`aikit`** must be on `PATH`; templates are fetched/rendered through `aikit-sdk`.

## Arguments and options

- **`PATH`** (optional positional): Directory to initialize (default: current working directory, canonicalized).
- `--template-source <SOURCE>`: Template locator (GitHub slug, URL, or local path). Default: `gonewton/newton-templates`.

## What gets created

- `.newton/configs/`, `.newton/tasks/`, `.newton/plan/default/{todo,completed,failed,draft}/`, `.newton/state/`.
- `.newton/configs/default.conf` with `project_root`, `coding_model`, and a commented `workflow_file=` line. Set `workflow_file` when using `newton batch` with the default project layout.

## Example

```bash
newton init .

newton init /path/to/repo --template-source gonewton/newton-templates
```

## Next steps

After init, point `workflow_file` at your YAML (for batch), then run:

```bash
newton run path/to/workflow.yaml --workspace .
```

See the repository `README.md` for batch plan format and monitor setup.
