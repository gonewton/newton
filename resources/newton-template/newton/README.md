# Newton workspace template

This template scaffolds a Newton workspace with **workflow YAML** definitions and small **shell helpers** under `.newton/`.

## Layout

- **`.newton/workflows/`**  
  - `develop.yaml`, `planner.yaml`, `documenter.yaml` (example workflow graphs you can run or customize).

- **`.newton/scripts/`**  
  - `newton-project-root.sh` – shared helpers (config dir, `project_root` resolution).  
  - `develop.sh`, `planner.sh`, `documenter.sh` – convenience entrypoints that invoke `newton run` with the matching workflow (see script headers for usage).

## After `newton init`

1. Edit `.newton/configs/default.conf`: set `workflow_file` to the workflow you want `newton batch` to use (path relative to `project_root` or your workspace), for example:
   - `workflow_file=.newton/workflows/develop.yaml`
2. Run a workflow directly:
   - `newton run .newton/workflows/develop.yaml --workspace .`
3. Use the helper scripts from your workspace root if you prefer (they expect a matching `<project_id>.conf` under `.newton/configs/`).

Customize workflows and scripts to match your repositories and automation.
