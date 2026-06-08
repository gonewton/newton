# GraderCommandOperator

> Status: **design** (not yet implemented). Settled via the grading design
> sessions; see [CONTEXT.md](../../CONTEXT.md) (Grader, Assessment, …) and
> [ADR 0005](../adr/0005-grading-is-text-gradient-driven.md).

Runs a **command Grader** — an external program, in any language, that evaluates
the project's current state and prints an **Assessment** — then validates it and
exposes it for gating and downstream persistence/reconciliation. It is the
adapter form of the grading operator; the sibling `GraderAgentOperator` (a
*rubric Grader* run via aikit-sdk) emits the same Assessment and is otherwise
interchangeable.

## What it does

1. Runs `cmd` (reusing `CommandOperator` execution semantics: `shell`, `cwd`,
   `env`, byte-limited capture), with the `NEWTON_*` state environment injected.
2. Parses the grader's stdout as the **content** of an Assessment.
3. Stamps the **identity envelope** (`grader`, `scope`, `scope_id` from params;
   `evaluated_at` from the clock) onto the parsed content.
4. Validates the merged object against
   [`assessment-v1.schema.json`](../../crates/core/schemas/assessment-v1.schema.json).
5. Returns the gating scalars (inline) plus the full Assessment.

The grader owns the *evaluation content*; the operator owns *identity*. External
graders never need to know Newton's portfolio ids.

## Success vs. failure (important)

- **Success** = a valid Assessment was produced. **A poor grade is a success.**
  Grade quality lives *only* in the Assessment JSON — never in the exit code or
  task status.
- **Failure** (`Err`, retryable `ToolExecutionError`) = *operational* only: the
  grader exits non-zero, prints no/invalid JSON, or fails schema validation after
  retries.

Rationale: `completion.success_requires_no_task_failures` defaults true, so
failing the task on a bad grade would fail the whole workflow and trigger
pointless re-grading. The gate is workflow policy, not task failure.

## Params

| Param | Req | Notes |
| --- | --- | --- |
| `cmd` | ✔ | The grader command. |
| `grader` | ✔ | Authoritative grader name → `Assessment.grader` / `EvalRun.source`. |
| `scope` | ✔ | `product \| component \| repo \| module`. |
| `scope_id` | ✔ | Authoritative scoped-entity id. |
| `shell` | | `CommandOperator` semantics (default false). |
| `cwd` | | Working dir, relative to workspace. |
| `timeout_seconds` | | Agentic graders are slow. |
| `env` | | Extra environment. |
| `state` | | Map handed to the grader as `NEWTON_STATE_*` env (e.g. `base_ref`, `head_ref`). |

`cmd`/`env` support `{{context.*}}` template interpolation.

## State-passing convention (env)

```
NEWTON_WORKSPACE        = <workspace path>
NEWTON_SCOPE            = <scope>
NEWTON_SCOPE_ID         = <scope_id>
NEWTON_GRADER           = <grader>
NEWTON_STATE_<KEY>      = <value>     # one per `state` entry, KEY upper-cased
```

Contract: the grader reads env + the repo, prints Assessment **content** JSON to
stdout, and exits 0.

## Output (`tasks.<id>.output`)

```jsonc
{
  "overall_score": 73,                       // hoisted scalar
  "verdict": "request_changes",              // hoisted scalar
  "score_by_dimension": { "tests": 60, "security": 90 },  // scores[] as a map
  "counts": { "critical": 0, "high": 2, "medium": 5, "low": 1, "total": 8 },
  "assessment": { /* full canonical Assessment, for persist + reconcile */ }
}
```

The gating scalars are kept **inline** (so transition conditions can read them
even if the full `assessment` is routed to an artifact).

## Gating (workflow policy, not an operator flag)

There is no `gate_on` param. Gate with ordinary transitions + a goal-gate node:

```yaml
- id: grade
  operator: GraderCommandOperator
  params:
    cmd: "dk review --output-format json"
    grader: dk-review
    scope: repo
    scope_id: "{{context.repo_id}}"
  transitions:
    - to: gate_ok        # gate_ok has goal_gate: true
      when: { $expr: 'tasks.grade.output.counts.critical == 0 && tasks.grade.output.overall_score >= 70' }
    - to: remediate      # else → fix and re-grade
      when: { $expr: 'tasks.grade.output.overall_score < 70 || tasks.grade.output.counts.critical > 0' }
```

## Open (later branches)

- **Persistence + Reconciliation**: operators have no backend handle
  (`ExecutionContext` exposes none), so writing `EvalRun`/`Grade` and reconciling
  Observations into Findings happens *outside* this operator — path TBD
  (branch #3).
- **`GraderAgentOperator`**: the rubric-Grader sibling.
