# Newton workspace template

This template scaffolds a Newton workspace with **workflow YAML** definitions and small **shell helpers** under `.newton/`.

## Layout

- **`.newton/workflows/`**  
  - `develop.yaml`, `planner.yaml`, `documenter.yaml`, `vulnerability.yaml` (example workflow graphs you can run or customize).

- **`.newton/scripts/`**  
  - `newton-project-root.sh` – shared helpers (config dir, `project_root` resolution).  
  - `develop.sh`, `planner.sh`, `documenter.sh`, `vulnerability.sh` – convenience entrypoints that invoke `newton run` with the matching workflow (see script headers for usage).

## After `newton init`

1. Edit `.newton/configs/default.conf`: set `workflow_file` to the workflow you want `newton batch` to use (path relative to `project_root` or your workspace), for example:
   - `workflow_file=.newton/workflows/develop.yaml`
2. Run a workflow directly:
   - `newton run .newton/workflows/develop.yaml --workspace .`
3. Use the helper scripts from your workspace root if you prefer (they expect a matching `<project_id>.conf` under `.newton/configs/`).
4. For vulnerability grading via workflow/wrapper, add keys like:
   - `vuln_grader_agent=claude`
   - `vuln_grader_model=sonnet`
   - `vuln_lockfile_path=/abs/path/to/Cargo.lock`
   - `vuln_grader_prompt_file=/abs/path/to/vuln-prompt.txt` (or `vuln_grader_prompt=...`)
   - optional `vuln_grader_workflow_path=.newton/workflows/vulnerability.yaml`

## Vulnerability scanner dependency

The vulnerability workflow checks that `osv-scanner` is installed and fails fast if it is missing.

Install `osv-scanner` using the official guide:
- <https://google.github.io/osv-scanner/installation/>

Customize workflows and scripts to match your repositories and automation.
