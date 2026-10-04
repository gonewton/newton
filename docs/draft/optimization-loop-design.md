# Generic optimization loop — design interview

Status: implemented design record. This document preserves the decisions and
pre-implementation inventory; present behavior is specified by
[`docs/optimization-contract.md`](../optimization-contract.md). See
[ADR 0016](../adr/0016-optimization-history-as-json.md).
Implementation requirements live in the [specification](../../specs/generic-optimization.md);
use the [developer handoff](../../specs/generic-optimization-handoff.md) for the
source map, review closure, and completion checklist. Updating all affected
documentation and skills is required for feature completion, not a follow-up.

## Settled decisions

- The loop is generic across Objectives; documentation is an example, not the
  product boundary. Observation-driven strategies select up to K improvement
  suggestions per Cycle, defaulting to five. Strategies that propose from
  measurements need no observations, severity, persistent Findings, or Plans.
- Newton is agnostic to the type of work. Repository isolation, branches, Docker,
  and delivery behavior belong to configured Workflows/operators and the E2E
  scenario; they are not universal optimizer requirements.
- The loop advances automatically as its operators permit. Operators own any
  approval or interaction requirements; Newton adds no per-cycle approval layer.
- Optimization seeks useful directional progress, not perfect satisfaction of
  all Findings. Findings guide selection and planning; unresolved or unverified
  Findings do not automatically prevent acceptance or reaching the Objective.
  Explicit acceptance constraints remain binding.
- Newton consumes existing Optimization Definitions. Definition and Workflow
  authoring happens outside Newton, using standard coding agents supported by
  skill documents. No built-in goal-to-definition authoring assistant is planned.
- Use an explicit primary Objective with constraints on other dimensions;
  preserve existing independent threshold objectives. Do not implicitly average
  away regressions.
- Proposals receive the current accepted Candidate; how new states are derived
  belongs to the Strategy. The default policy does not use unaccepted Candidates
  as speculative incumbents. Prerequisite work can be grouped in one attempt.
- If a required verification cannot run, preserve the incumbent and mark the
  Candidate unverified. Distinguish unavailable verification, failed checks, and
  lack of improvement.
- The E2E may retain the existing Pi agent client; adding Claude Code is not
  required. It uses Docker and the internal LLM gateway over VPN. The user
  identifies its backing model as Qwen on DGX Spark, with no additional inference
  charge. This is user-provided
  infrastructure context, not independently verified routing evidence.
- The coding real-agent E2E must prove actual edits, independently verified improvement,
  K enforcement, accumulation over at least two accepted Cycles, an unchanged
  original checkout, and a reviewable branch. Start with a deterministic Grader
  and real coding agent, then add a real rubric Grader scenario.
- Token and dollar spending controls are deferred.
- Stagnation counts consecutive Cycles without an accepted improvement and
  resets after acceptance. Operational/evaluator failures are distinct outcomes.
- A claim of resolution requires an explicit evaluator conclusion. Absence can support
  resolution only when the evaluator declares complete relevant coverage.
  Otherwise omitted observations have no verified resolution. Newton records this evidence;
  it does not independently prove each domain-specific conclusion. Tracking and
  matching remain best-effort rather than a requirement for exhaustive proof.
- Improvement in the Objective's declared direction counts by default, subject to
  explicit acceptance constraints. Definitions may configure a minimum delta or
  repeated grading for noisy evaluators; statistical machinery is not mandatory.
- When proposal returns no useful next action below the target, stop with the
  best accepted result and report "no actionable improvement; target not
  reached." This is distinct from operational failure and verified completion.
- Other stopping conditions remain target completion, configured stagnation,
  cycle limits, and elapsed-time limits.

## Delivery sequence

1. Define versioned run/cycle JSON contracts and simplify optimizer persistence
   around immutable assessments and attempts. Reuse the native driver's comparison
   and orchestration logic; remove its mandatory SQLite/entity-spine dependencies.
2. Supply reusable evaluation/proposal/optional-execution workflow building blocks and skill
   guidance for external coding agents authoring definitions and evaluators.
3. Complete generic selection, acceptance, resolution tracking, stopping, and
   result reporting. Keep domain-specific execution and delivery in operators
   and Workflows.
4. Reuse the existing Pi real-agent evidence harness in Docker
   through the internal Qwen gateway. Develop against a small, independently
   checkable coding fixture. Also run a non-coding scheduling-parameter scenario
   against a deterministic simulator with the same binary, no Git or observations,
   and direct candidate proposal; add rubric grading as another scenario.

## Generic core and domain adapters

See [ADR 0017](../adr/0017-domain-neutral-optimization.md). The lifecycle
is evaluate → propose → optional execute → evaluate → decide. Proposal can return
a ready Candidate, an attempt requiring execution, or no useful action. The
version-2 public contracts implement this design.

Objectives use Grades or numeric measurements with units and direction; acceptance
checks can be tests, simulations, validation rules, measurements, or human
judgments. Candidate identity can refer to parameter sets, snapshots, object
versions, or commits. The adapter defines reproducibility and execution-resource
ownership. No Git ancestry, repository, or branch is required by the core.

Every proposal receives the accepted Candidate as context. Derivation belongs to
the Strategy. The milestone's conservative default retains the best accepted
result and does not explore through unaccepted plateaus; alternative search
algorithms are not required by this milestone. Delivery is separate and returns
or applies results through domain-specific adapters.

JSON history contains runs, cycles/attempts, candidate references, evaluations,
and decisions. Observations, priority/severity, and Plans are optional attachments.
K bounds selected suggestions in the observation-driven strategy, not files,
incidental effects, or all possible strategies.

The genericity acceptance test runs a coding scenario and a bounded scheduling
parameter scenario using the same built Newton binary. The latter minimizes a
simulated numeric outcome subject to feasibility checks and has no observations,
Plan, separate execute workflow, repository, or database dependency.

The adversarial review is retained unchanged. Its dispositions and validation
evidence are recorded in
[`docs/generic-optimization-review-resolution.md`](../generic-optimization-review-resolution.md).

## Accepted persistence model

The generic optimizer records evaluations, proposals, attempts, decisions,
and results as per-run JSON artifacts. SQLite and a global mutable Finding backlog
are not required. Simply reproducing the current relational entities as JSON CRUD
records would not achieve the intended simplification.

Each evaluation is immutable and retains its measurements, evidence, evaluated
candidate identity, and evaluator/requirements revisions. Rubric Assessments also
retain Grades, dimension Scores, and any Observations. Observations are addressable
within their assessment; optional
best-effort links can associate them across assessments. A renamed, split, merged,
or omitted observation never rewrites prior history. A rubric change starts a new
revision and requires comparable re-evaluation before claiming improvement.

Selection and Plan content, when used by the Strategy, live in Cycle history. A separate Change Request
lifecycle, global Finding status machine, and cross-run triage are optional
platform capabilities, not prerequisites for optimization. The latest evaluation
and relevant previous attempts guide proposals without requiring exact issue
identity across cycles. Failed attempts remain visible to avoid repeating them.

Illustrative layout beneath the configured state directory:

```text
optimize/<run-id>/
  run.json             # Run identity, bound settings and revision references
  current.json         # Phase, counters, active attempt and retained-result pointer
  definition/          # Pinned definitions, workflows and evaluator assets
  cycles/0001.json     # Immutable cycle, evaluations, decision, optional plan
  cycles/0002.json
  artifacts/           # Referenced outputs, logs, traces and deliverables
```

Keep large existing workflow artifacts by reference rather than duplicating them.
Reports and objective-measurement trajectories are derived from the historical records; optional
indexes must be rebuildable and must not become a second source of truth.

Retain schema versions, atomic file replacement, exclusive run ownership, and
explicit interrupted-work state. Persist active dispatch identity before work;
publish a completed cycle before advancing the current-state pointer. Recovery
must detect incomplete work without blindly repeating external side effects.
There is no requirement for a general event-sourcing framework or transactional
file database. Precise schemas and recovery sequencing are implementation work.

The broader platform can retain its SQLite catalog and optional issue-management
features. Decouple the optimizer and standard workflows from mandatory database
use; replacing all storage throughout Newton is not part of this decision.

### Pre-version-2 artifacts and implemented representation

| Artifact | Pre-version-2 storage | Version-2 generic optimization |
| --- | --- | --- |
| Definition, workflow and helper snapshots | Files and integrity manifest | Retain pinned files and references |
| Bound settings and requirements revisions | JSON journal | Versioned run/revision records |
| Run status, phase, counters and outcomes | JSON journal plus SQLite OptimizeRun | Current checkpoint plus historical/final evidence |
| Cycle evaluations and decisions | SQLite OptimizeCycle | Immutable completed cycle records |
| Raw assessments and dimension scores | SQLite EvalRun and Grade | Immutable assessments within/referenced by cycle history |
| Findings and issue statuses | Mutable SQLite Finding | Assessment-local observations; optional continuity links |
| Change Requests | Mutable SQLite ChangeRequest | Selected work and rationale within a cycle |
| Plans, attempts and failures | SQLite Plan plus journal work ledger | Plan and attempt evidence in cycle history/current checkpoint |
| Candidates and accepted results | Journal/evaluation records and workflow artifacts | Artifact references with correlated acceptance evidence |
| Workflow checkpoints, outputs and agent traces | Files; optional backend runtime records | Reuse file artifacts and avoid mandatory backend registration |
| Ownership and pending updates | Locks and JSON files | Retain the minimal coordination/recovery mechanism |
| Portfolio, catalog and optional issue management | SQLite backend | Outside the generic optimizer's required persistence |

## Validation for the persistence change

- Track before/after measurements, constraints, optional observations/plans, failed and
  rejected attempts, retained results, revisions, counters, and stopping reasons.
- Prove the standard optimizer path runs and resumes without `backend.sqlite`.
- Test interruptions before/after cycle publication, single-writer ownership,
  invalid/incomplete records, and preservation of historical assessments.
- Test observation renaming/splitting and evaluator revision changes without
  rewriting history or requiring permanent Finding identities.
- Generate progress reports from JSON records and verify the real-agent E2E
  against those records, actual artifacts, and correlated agent evidence.

## Environment verification

Deployment details, tool versions, gateway connectivity, and implementation feasibility must be
verified from the environment during development rather than assumed.

## Implementation facts to preserve

- `docs/optimization-contract.md` documents an existing opt-in Pi real-agent
  harness, `scripts/test-optimize-live.py`. Extend its evidence model for the
  Docker/gateway route rather than creating conflicting definitions of a
  real-agent pass. Its local-gateway gate currently cannot pass without runtime
  transport evidence; configuration alone is explicitly insufficient.
- The version-2 optimizer writes immutable JSON run/Cycle history and does not
  require Findings, Change Requests, Plans, or SQLite. Existing database-backed
  operators remain available to optional catalog workflows.
