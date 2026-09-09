# newton optimize / the optimization loop

> Supersedes the old `batch.md`. `batch` was renamed to `optimize` (ADR 0003).

## Native driver

`newton optimize <project_id>` runs a durable native lifecycle. It requires a
versioned Optimization Definition supplied by `--definition <path>` or the
project config's `definition_file`. The software strategy invokes `grade`,
`plan`, and `develop`, evaluates the exact Candidate under active requirements,
then invokes optional `promote` only for a qualifying result. Legacy Plan queues
and the shell driver are not the production contract.

Use `--resume <RUN_ID>` for a known safe durable phase. A declarative local
`--requirements-update <path>` is serialized with the run; an update that cannot
be enforced while work is live remains Pending rather than becoming active.

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
