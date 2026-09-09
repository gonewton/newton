# Newton workspace template

This template scaffolds a Newton workspace with **workflow YAML** definitions and small **shell helpers** under `.newton/`.

## Layout

- **`.newton/workflows/`**  
  - `develop.yaml`, `planner.yaml`, `documenter.yaml`, `vulnerability.yaml` (example workflow graphs you can run or customize).

- **`.newton/scripts/`**  
  - `newton-project-root.sh` – shared helpers (config dir, `project_root` resolution).  
  - `develop.sh`, `planner.sh`, `documenter.sh`, `vulnerability.sh` – convenience entrypoints that invoke `newton run` with the matching workflow (see script headers for usage).

## After `newton init`

1. Run a workflow directly:
   - `newton run .newton/workflows/develop.yaml --workspace .`
2. Use the helper scripts from your workspace root if you prefer (they expect a matching `<project_id>.conf` under `.newton/configs/`).
3. To use `newton optimize`, add a versioned Optimization Definition and point a
   project config at it:

   ```text
   project_root=.
   definition_file=.newton/definitions/my-definition.yaml
   optimize_allowed_actions=agent,command,network,commit,draft_pull_request,merge
   ```

   The native software strategy requires `grade`, `plan`, and `develop` workflow
   roles, and validates candidate evidence before an optional `promote` role.
   Read Newton's optimization contract before granting workflow actions.
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
