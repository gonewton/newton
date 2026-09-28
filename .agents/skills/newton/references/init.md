# newton init

## Purpose

Create a **Newton workspace**: `.newton/` layout, default config, the shipped optimization definition assets, and template content installed via the statically linked **aikit-sdk** (the `aikit` binary is **not** required on `PATH`).

## Requirements

- `.newton` must **not** already exist at the target (remove it or pick another path). Existing settings are never overwritten.
- Network access to GitHub for the default template source. Not needed with `--template builtin` or when `--template` points at a local path.

## Arguments

- **`PATH`** (optional positional): Directory to initialize. Defaults to the current directory; created if missing and canonicalized to absolute.
- `--template <SOURCE>`: Template source: `builtin` (offline; installs only the embedded optimization definition), a GitHub slug, a URL, or a local path. Default: `gonewton/newton-templates`.

## What gets created

- `.newton/configs/`, `.newton/tasks/`, `.newton/plan/default/{todo,completed,failed,draft}/`, `.newton/state/`.
- The shipped `software-security` optimization definition under `.newton/definitions/`.
- `.newton/configs/default.conf` with `project_root`, `coding_model`, and a commented `definition_file=` binding plus `parameter.*` hints. See [configuration.md](configuration.md).

## Example

```bash
newton init .

newton init /path/to/repo --template gonewton/newton-templates

# Offline: install only the shipped definition, then inspect it
newton init . --template builtin
newton optimize default --inspect
```

## Next steps

```bash
newton workflow run path/to/workflow.yaml --workspace .
```

See [configuration.md](configuration.md) for `.conf` keys and [optimize.md](optimize.md) for the optimization loop.
