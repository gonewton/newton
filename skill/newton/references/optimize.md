# newton optimize / the optimization loop

> Supersedes the old `batch.md`. `batch` was renamed to `optimize` (ADR 0003).

## Native driver

`newton optimize <project_id>` runs a durable native lifecycle. It requires a
versioned Optimization Definition supplied by `--definition <path>` or the
project config's `definition_file`. The software strategy invokes `grade`,
`plan`, and `develop`, then evaluates the exact Candidate under active
requirements. Qualified candidates are retained for review. The generic host
rejects `workflows.promote` because it cannot independently inspect and
atomically compare-and-swap an arbitrary target; use a target-specific external
promotion boundary. Legacy Plan queues and the shell driver are not the
production contract.

Use `--resume <RUN_ID>` for a known safe durable phase. A declarative local
`--requirements-update <path>` is serialized with the run; an update that cannot
be enforced while work is live remains Pending rather than becoming active.

## Shipped first-use path

```sh
newton init . --template builtin
newton optimize default --inspect
```

The embedded `software-security` definition targets committed Rust Cargo.lock
advisory matches, not comprehensive security. Configure an existing Pi/Claude/Codex
agent/model, Python 3.11 or newer, Cargo/cargo-audit, a prepared advisory database and its Git
revision in `.newton/configs/default.conf`. See `newton optimize --help` for the
current unsandboxed-host authority requirement; detached worktrees are not a sandbox.
Accepted security candidates may change Cargo lockfiles and dependency-resolution
tables. Cargo test targets, features, workspace selection, profiles, and other
manifest settings must remain identical to the candidate base. Cargo manifests,
lockfiles, and project `.cargo/config[.toml]` inputs must be committed regular
files. Authoritative evaluation uses an isolated checkout and fresh `CARGO_HOME`;
the configured executables, inherited environment, and operating system remain
trusted host prerequisites.
Optimization role workflows must be acyclic and single-attempt: task retries,
agent loop mode, and operator-internal retries that Newton cannot meter are
rejected before a run starts.

```sh
newton optimize default --preflight
newton optimize default --once --param 'model="configured-model"'
```

For an opt-in real Pi trial, use the repository harness. The local-gateway mode
validates Pi's active custom-model registry and private endpoint, then remains
failed because current Pi SDK traces do not expose transport evidence. Labels or
configuration alone cannot pass:

```sh
python3 scripts/test-optimize-live.py /path/to/workspace default ./target/debug/newton \
  --route local-gateway --expected-model local-provider/model-id \
  --pi-models-file /path/to/active/pi/agent/models.json
```

`--inspect` shows resolved non-secret values. Repeated `--param NAME=JSON` overrides
project values, which override definition defaults. Preflight checks workflows
and prerequisites without starting agents or candidates. The shipped graph retains
detached candidate commits and does not promote them into the original HEAD.

Declare local UTF-8 helpers and inputs in a definition's `assets` list and
reference their retained content through `triggers.assets["name"]`. Newton
compiles role workflows and retains assets in host memory before dispatch; it
does not disclose or reload the persistent snapshot during a run. Runs pin the
same bytes with SHA-256 hashes, and resume loads each verified copy once.
`WorkflowOperator` is rejected in optimization roles until child workflows have
the same provenance. `--resume <RUN_ID> --inspect` shows the persisted snapshot
identity. Changing installed source affects new runs only.

SIGINT and resource exhaustion have separate outcomes. Unknown in-flight effects
require reconciliation before resume. Live revision requests remain Pending until
the owner reaches its completed-evaluation boundary. It validates and persists
activation, then regrades the same candidate before acceptance. Stopped runs use
safe explicit resume. Unsupported restrictions and stale requests are rejected.
The `software-improvement` strategy requires a correlated Change Request and
stored linked ready Plan. Reconciled failures are retried up to the configured
per-Change-Request limit, then exposed as blocked work to the next grade and
planning steps. The `direct-search` strategy remains a generic loop without
Change Requests.

## Legacy shell scaffold (not the native contract)

```
optimize.sh <project_id> [--once] [--max-cycles N] [--converge-rounds K]
            [--target-grade G] [--delivery local|pr] [--auto-approve]
```

- `--once` — run a single cycle and exit.
- `--max-cycles N` — hard cap (default 8).
- `--converge-rounds K` — consecutive `decision: none` rounds to declare converged (default 2; forced to 1 for a deterministic grader).
- `--delivery local|pr` — `local` merges to main with `git merge --ff-only` (zero GitHub); `pr` opens a PR.
- `--auto-approve` — bypass HIL approval gates (loops/tests).

## Historical Findings-driven loop shape

1. **Grade** — for each configured grader, run `.newton/grader/<name>/generate.sh <repo_id> <repo_path>`, which **prints an Assessment to stdout**. `GraderCommandOperator` validates + persists it. (The script must NOT self-persist.)
2. **Reconcile** — `ReconcileOperator` matches Observations → durable **Findings** (refresh / create / resolve).
3. **Change-request** — `ChangeRequestOperator` synthesizes one **Change Request** over the standing Findings (`decision: propose | none`).
4. **Break check** — evaluate the conditions below against the Trajectory.
5. **Approve** — auto (`optimize_auto_approve=true`) or an ailoop HIL gate.
6. **Plan** — `planner.yaml` enriches the approved CR into a durable **Plan** (`status: ready`).
7. **Develop** — `develop.yaml` renders `Plan.body` → implements → runs `optimize_test_cmd` (gate) → commits → merges (or PR). Success → `Plan: complete`; failure after retries → `Plan: failed`.
8. **Re-grade** — record the cycle in the **Trajectory** and loop.

## Historical strategy guards

| Condition | Fires when |
| --- | --- |
| `converged` *(success)* | `decision: none` for K rounds **and** zero `blocked` Findings |
| `stalled_on_blocked` *(needs human)* | no actionable work left but ≥1 `blocked` Finding remains |
| `max_cycles` | cycle count hits `optimize_max_cycles` |
| `target` | **every** grader clears its own `optimize_target_grade[_<grader>]` (conjunction) |
| `regressed` | **any** grader drops > its `optimize_regression_tolerance[_<grader>]` vs last cycle (disjunction) |
| `no_progress` | grade + open-Finding count unchanged for K cycles (failed-develop cycles count) |

## Historical failed-Plan quarantine

When a Plan fails develop after `optimize_max_failed_attempts` (default 2), its linked Finding(s) become **`blocked`**: fenced from change-request synthesis (never re-planned), still open, **human-cleared only**. The loop keeps optimizing the rest. A human un-blocks via:

```bash
curl -X POST localhost:8080/api/v1/findings/<id>/unblock   # 409 if not blocked
```

## Observe over `serve` (read-only)

```bash
GET  /api/v1/optimize-runs                 # list runs
GET  /api/v1/optimize-runs/{id}            # run + outcome reason
GET  /api/v1/optimize-runs/{id}/trajectory # per-cycle rows
GET  /api/v1/optimize-runs/{id}/cycles     # cycles
GET  /api/v1/findings?status=blocked       # blocked findings (inline block context)
POST /api/v1/findings/{id}/unblock         # un-block a Finding
```

The HTTP surface is **read-only + unblock** (the loop is self-driving; no HTTP route starts/stops/configures a run — ADR 0004). Run/cycle state is mirrored to the store by the driver via the local CLI (`newton data post optimize-run|optimize-cycle`).

## Historical shell configuration

```sh
optimize_repo_id="…"                 # Newton Repo UUID = grading scope_id
optimize_repo_path="/abs/path/repo"  # filesystem path to grade + develop
optimize_test_cmd="pytest -q"        # develop's run_tests gate
optimize_graders="maintainability"   # space list; each → .newton/grader/<name>/generate.sh
optimize_max_cycles=8
optimize_converge_rounds=2
optimize_target_grade=85             # + optimize_target_grade_<grader> overrides
optimize_regression_tolerance=3      # + optimize_regression_tolerance_<grader> overrides
optimize_max_failed_attempts=2       # same-CR develop failures → Findings blocked
optimize_auto_approve=true           # false → ailoop approval gate
delivery="local"                     # local | pr
```

See `CONTEXT.md` for the full glossary (Optimize Run, Cycle, Trajectory, Grader, Assessment, Finding, Change Request, Plan, Reconciliation).
