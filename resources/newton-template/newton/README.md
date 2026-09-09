# Newton workspace template

This template installs workflow graphs, shell helpers, and a reusable
dependency-security definition under `.newton/`.

## Reusable dependency-security optimization

`newton init . --template builtin` installs the embedded optimization bundle
offline. Normal template installation also receives it.

```sh
newton optimize default --inspect
```

The definition at `.newton/definitions/software-security/definition.yaml` audits
committed Rust `Cargo.lock` dependencies with cargo-audit. It checks tests and
limits accepted changes to dependency manifests/lockfiles. This is not a
comprehensive security or compliance assessment.

Set these ordinary bindings in `.newton/configs/default.conf`:

```ini
parameter.agent=pi
parameter.model=your-configured-model
parameter.advisory_db=/absolute/path/to/rustsec-advisory-db
parameter.advisory_db_revision=full-git-commit-id
parameter.test_command=["cargo","test","--locked"]
```

Install Python 3, Cargo/cargo-audit and Pi, Claude, or Codex. Prepare the RustSec
database yourself; the definition does not fetch it. Existing agent authentication
and gateway configuration are reused.

The host is unsandboxed. Execution requires explicit trusted-host authority:

```ini
optimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy
```

Do not grant this to untrusted agents or assume detached worktrees restrict their
permissions. The supplied graph has no promotion step: it retains candidate
commits and evidence for review, leaving the original HEAD unchanged.

```sh
newton optimize default --preflight
newton optimize default --once
```

Use `--param 'agent="codex"'` for an explicit non-secret run override. Another
project config can set its own `project_root` and reference this same definition.
Inspection/preflight do not start a run or create candidate worktrees.
The definition declares `security.py` as a local UTF-8 asset. Newton retains its
content in host memory and supplies it as `triggers.assets["security.py"]`; roles
do not receive the persistent snapshot path. A run pins workflow and helper
copies with SHA-256 hashes, and resume loads verified bytes once instead of
rereading them between dispatches. Preflight checks the installed helper content,
not an embedded substitute.

## Layout

- **`.newton/workflows/`**  
  - `develop.yaml`, `planner.yaml`, `documenter.yaml`, `vulnerability.yaml` (example workflow graphs you can run or customize).

- **`.newton/scripts/`**  
  - `newton-project-root.sh` – shared helpers (config dir, `project_root` resolution).  
  - `develop.sh`, `planner.sh`, `documenter.sh`, `vulnerability.sh` – convenience entrypoints that invoke `newton run` with the matching workflow (see script headers for usage).

- `.newton/definitions/software-security/`: definition, workflow roles and a
  standard-library Python adapter. Artifacts stay under each context's
  `.newton/optimize-artifacts/<RUN_ID>/security/`.

## After `newton init`

1. Run a workflow directly:
   - `newton run .newton/workflows/develop.yaml --workspace .`
2. Use the helper scripts from your workspace root if you prefer (they expect a matching `<project_id>.conf` under `.newton/configs/`).
3. For a custom optimization problem, point a project config at its definition:

   ```text
   project_root=.
   definition_file=.newton/definitions/my-definition.yaml
   # Grant only explicitly reviewed authority supported by the host.
   ```

   The native software strategy requires `grade`, `plan`, and `develop` workflow
   roles. It validates candidate evidence and retains qualified candidates for
   review. The generic host rejects a `promote` role because it cannot verify an
   arbitrary target or atomically compare-and-swap its integration base. Read
   Newton's optimization contract before granting workflow actions.
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
