# Adversarial review of the generic optimization specification

Reviewer: Claude Code, explicit model `claude-opus-5-5` (confirmed by returned model usage, first-party provider).
Date: 2026-10-04.
Scope: read-only review of the specification and selected current source. No implementation changes or tests were performed by the reviewer.
Specification: [generic-optimization.md](../../specs/generic-optimization.md).
Specification SHA-256: `2131d4af3ba0010172c6d7ae71efab04e0abe843dc5c5bbda8b8ed35a37f030c`.

The review below is preserved as returned. Findings are review conclusions, not independently reproduced defects or accepted changes to the design. In particular, suggested changes to routing evidence and initial qualification require evaluation against the accepted contract.

---

# Adversarial review: Generic optimization loop spec (JSON history)

## 1. Verdict

**Not ready to hand off yet.** The direction is sound and matches ADR 0016. Five gaps would make independent implementers diverge or produce behavior the spec forbids:

- The workflow I/O contract is undefined.
- Selection, K enforcement and repeat avoidance contradict each other.
- The comparison logic the spec says to reuse can drop the incumbent.
- The stop-reason and stagnation semantics conflict with the code.
- The SQLite-backed consumers that would break are not addressed.

The real-agent E2E also has pass criteria that today either cannot pass or are not defined. All of these can be fixed with targeted amendments; none needs a redesign.

## 2. Findings

### Blockers

**B1. The grade/plan/develop output contract is unspecified, and it is the public authoring API.**
- **Evidence:**
  - `GradeOutput` is `deny_unknown_fields`, carries `change_request_id`/`open_findings` and has no observations (`crates/cli/src/cli/commands/optimize/envelopes.rs:6-15`).
  - `CandidateEvaluation` has no observation field (`crates/types/src/optimization/evaluation.rs:65-88`).
  - The grading `AssessmentObservation` has no identifier and an optional free-text severity (`crates/core/src/workflow/grading/assessment.rs:19-28`).
  - `PlanOutput` returns only `plan_id` (`envelopes.rs:19-26`). Plan content currently lives in SQLite.
  - The draft explicitly defers "precise schemas" to implementation (`docs/draft/optimization-loop-design.md:115`).
- **Failure scenario:** Skill docs and examples (the spec's main deliverable for external authors) get written against one guessed shape while the driver is built against another. One implementer puts observations in `CandidateEvaluation` and another in the grade envelope. A third has to decide how `overall_score`/`scores` map to `ObjectiveMeasurement`.
- **Minimum clarification:** a field-level contract for each of the three roles:
  - **Grade:** observations with an Assessment-local id, severity and evidence; an optional coverage declaration; optional links; optional resolution claims. Also how Assessment Grades map to objective measurements.
  - **Plan:** the selected observation references (≤K), rationale and plan content, or `none`.
  - **Develop:** a candidate, or a failure that doesn't reference a Change Request (see I1).

**B2. Who selects K, and how repeats are avoided, is contradictory.**
- **Evidence:**
  - "Existing severity-based prioritization" lives in `ChangeRequestOperator`. It reads SQLite Findings, filters by Finding status, and defaults to 10, not 5 (`crates/core/src/workflow/operators/change_request_op.rs:82-101, 252-276`).
  - The only current repeat-avoidance is the per-Change-Request quarantine after `max_failed_attempts` (default 2) (`native.rs:258-272`, `software_work.rs:149-172`), and the spec removes Change Requests.
- **Failure scenario:** With no identity across Assessments, a deterministic severity top-K re-selects the same five unfixable observations every Cycle until the stagnation limit fires. Story 25 ("avoid repeating ineffective work") is not met. The E2E's "K enforcement" is untestable if nobody owns the limit.
- **Minimum clarification:**
  - State whether Newton ranks and truncates, or the planner picks ≤K given prior attempts.
  - Have the driver reject a plan that selects more than K as a contract error.
  - Define the severity order, including missing or unknown severity, and tie-breaking.
  - State where K is configured.
  - State that repeat avoidance is best-effort by the planner, with stagnation as the backstop.
  - Decide whether `max_failed_attempts` is removed.

**B3. Reusing the current comparison path drops the incumbent, contradicting stories 18, 19 and 30.**
- **Evidence:**
  - Every Cycle re-grades the incumbent (`native.rs:445-471`).
  - `refresh_incumbent` then sets `accepted = None` when that same-artifact re-grade does not qualify (`native.rs:1018-1032`).
  - `regrade_incumbent` does the same after a requirements revision (`native.rs:946-949`).
  - With no incumbent, the next candidate becomes `InitialQualification` and is accepted without any comparison (`crates/core/src/optimization/decision.rs:74-75, 85-93`).
- **Failure scenario:** In Cycle 3, the incumbent's re-grade gets an `Unknown` required check because a tool was briefly unavailable, so the incumbent is cleared. The Cycle 3 candidate is worse but passes its checks, so it is accepted as "initial qualification". The final report then returns a worse artifact as the best result.
- **Minimum clarification:**
  - Once an incumbent exists, an `Unknown` re-grade must not discard it (that cycle is inconclusive).
  - Initial qualification is only allowed when no incumbent has ever existed.
  - Specify what happens when the incumbent is `Violated` under a new revision: a stop reason, or a fallback to some prior result.

**B4. Stagnation and stop reasons conflict with the code, and one path discards an accepted improvement.**
- **Evidence:**
  - Primary mode has no stagnation guard at all.
  - Threshold mode counts per objective and resets on `score > best` or on a falling open-findings count, not on acceptance (`thresholds.rs:70-80`).
  - Threshold mode also has a `Regression` stop (`thresholds.rs:63-69, 103-104`).
  - In `accept_evaluated`, a threshold stop returns *before* `journal.accepted` is updated (`native.rs:894-910`).
- **Failure scenario:** Objective A improves while objective B ties at its `no_progress_cycles` limit. The decision is `Improvement`, but the run stops with `threshold_stop` and the improved candidate is never retained.
- **Further evidence:** `OptimizationStopReason` also has `Regression`, `NoProgress`, `NeedsIntervention`, `CycleComplete` and `Cancelled` (`evaluation.rs:165-184`). The externally visible status strings `converged` and `stalled_on_blocked` (`native.rs:166-176`) are not mapped to the spec's five outcomes.
- **Minimum clarification:**
  - Add a stop-reason table (old → new, with status strings).
  - Name where the stagnation limit is configured and its default.
  - Decide whether per-objective `no_progress_cycles` and `regression_delta` survive.
  - State that acceptance is persisted before any stop guard is evaluated.

**B5. Removing SQLite breaks existing surfaces that the spec doesn't list.**
- **Evidence:**
  - The public embedding API `ObservedOptimizationRun`/`OptimizeRunObservationSource` reads `OptimizeRunTrajectory` from the backend store (`crates/cli/src/cli/commands/optimize/observation.rs:27-66`, `crates/core/src/optimization/observation.rs:52-182`).
  - `newton serve`'s `/optimize` routes and realtime stream depend on it too (`crates/core/src/api/mod.rs:9,121`, `crates/core/src/api/optimize_run.rs`).
  - The GitHub projection reads `get_optimize_run` (`projection.rs:170-182`).
  - Requirements control patches the run in SQLite (`control.rs:128-142`).
  - The workflow executor builds its operator registry with the store (`workflow.rs:72-91`), and `GraderAgentOperator` always persists Assessments (`grader_agent.rs:278-288`).
- **Failure scenario:** After the change, new runs disappear from `newton serve` and embedding observers fail. A standard workflow that uses `GraderAgentOperator` cannot pass the spec's "no `backend.sqlite`" test.
- **Minimum clarification:**
  - For each surface, decide: read from JSON, feed from a rebuildable index, or deprecate.
  - State which operators are allowed in standard workflows, and require store-free grading.

### Important (blocks the E2E or a key failure path)

**E1. The E2E pass criteria can't be met, or aren't defined.**
- **Route evidence:** The harness's local-gateway route always fails, because Pi doesn't expose the transport it used (`scripts/test-optimize-live.py:265-276`). Define a separate result: the optimization can pass while route verification is reported as `transport_unobserved`.
- **"Review branch produced by the scenario's workflow":**
  - The driver only has the `grade`, `plan` and `develop` roles (`native.rs:39-43`).
  - `develop` can't know whether its candidate will be accepted.
  - The shipped flow makes detached-worktree commits (`security.py:329-338`), and the harness requires that nothing is promoted.

  Decide between an optional post-run delivery role and a harness step that creates the branch from the accepted `artifact_id`. The first adds a role to the core contract; the second means the branch isn't "the workflow's".
- **K=1 with ≥2 accepted Cycles:** A real agent may fix several opportunities at once and hit the target in Cycle 1. Decide whether K limits *selection* or *edits*. Then design the fixture so one Cycle can't reach the target, or treat over-fixing as a K violation.
- **Harness adaptation:** It is hardcoded to `software-security`, Pi, `journal.json` and `--once` (lines 104, 201-206, 232). Also define "without changing the optimizer core" checkably, for example as named paths that stay unchanged between scenarios.

**I1. A failed attempt that isn't a Change Request has no defined outcome.** In `direct-search`, which the shipped definition and the harness use, a `develop` failure is a hard error: "requires the software-improvement strategy" (`native.rs:597-600`). It ends as an operational failure that needs reconciliation. Real agents fail often. Specify:
- the failure envelope for the generic loop;
- what counts as a known-safe failure (today: `reconciled` plus evidence, `software_work.rs:121-127`);
- that a safe failure is a recorded unsuccessful Cycle that counts toward stagnation, not an operational failure.

**I2. The recovery commit point and compatibility with existing runs.** Idempotency today compares against the SQLite cycle row (`lifecycle.rs:324-336`). State three things:
- The Cycle file is the commit point. On resume, an existing Cycle file with an older checkpoint rolls the checkpoint forward; conflicting content requires reconciliation.
- Whether runs that exist as `journal.json` from before the change can be resumed or must be finished first.
- Whether the cross-run, never-released plan claims (`native.rs:552-562`, `optimize/plan-claims/<plan_id>`) go away, with plan identity becoming per-run and driver-assigned.

### Improvements (not blocking)

- **Before/after under noisy graders:** Each accepted candidate is graded, then graded again as the next baseline, and that re-grade replaces the incumbent's evidence (`native.rs:1022-1028`). Define "before" as the first baseline Assessment and "after" as the accepted result's qualifying Assessment. Show revision boundaries, and decide whether a candidate's Assessment may be reused as the next baseline.
- **What a Cycle record holds:** A Cycle can span a revision activation, with the candidate graded under revision 1, then incumbent and candidate re-graded under revision 2 (`native.rs:861-879`). Repeated samples are merged into one output (`native.rs:778-852`). "Preserved as originally produced" needs a decision: one Assessment per sample and per purpose, or the merged output.
- **Minimum delta:** `Exact` has no minimum delta (`definition.rs:132-143`). Say whether a field is added or authors should use `Repeated { samples: 1, min_improvement }`.
- **Resolution claims:** Give the coverage declaration a shape, and state that claims are report-only unless an explicit completion check uses them.
- **Other gaps:**
  - `max_work`/`max_evaluations` exist and are required (`definition.rs:215-226`); the spec only mentions cycle and time limits.
  - A deadline that expires mid-step becomes an "uncertain" stop that needs reconciliation (`workflow.rs:94-108`); say whether that is intended.
  - The fate of the `strategy` values isn't stated (`native.rs:44-52`).
- **Terminology:**
  - "Observation" collides with the existing `OptimizeRunObservation` API.
  - The glossary defines a single 0–100 Grade (`CONTEXT.md:67-70`), but Numeric and multi-threshold objectives remain. Say "objective improvement" in the acceptance rules.

## 3. What the spec gets right

- Delivery, isolation, Git and approval stay in workflows and operators; nothing is added to the core.
- Not knowing is not passing: a required check that can't run blocks acceptance. This matches `combine_statuses` (`decision.rs:262-270`).
- Dimensions are never combined to excuse a violated constraint. This matches `decision.rs:76-84`.
- JSON history replaces both stores rather than adding one, and it explicitly refuses to re-encode the old entities.
- It is honest about evidence: a model alias isn't routing proof, missing configuration is reported as "not exercised", and the doc says the refactor isn't implemented yet.
- The out-of-scope list is tight, and tests go through the public `newton optimize` command.

## 4. Minimal amendments before implementation

1. Field-level grade/plan/develop output schemas, including a non-Change-Request failure and the Assessment → measurement mapping (B1, I1).
2. Selection ownership, where K is configured and validated, severity ordering, and the repeat-avoidance stance; decide the fate of `max_failed_attempts` (B2).
3. Incumbent-retention rules for re-grades that come back `Unknown` or `Violated`; no initial qualification after an incumbent has existed (B3).
4. A stop-reason and status mapping table, the stagnation setting and default, the fate of threshold guards, and "persist acceptance before stop guards" (B4).
5. A per-surface decision for embedding observation, the HTTP/realtime/UI, projection and control, plus the list of store-free operators for standard workflows (B5).
6. E2E pass criteria: route status reported separately, who creates the review branch, the K meaning and fixture rule, and a checkable "core unchanged" (E1).
7. The Cycle-file commit point, pre-change run compatibility, and plan-claim scope (I2).

## Limitations

- I only read code; I ran no commands or tests.
- I did not read these in detail: `api/optimize_run.rs`, the projections crate, `revisions.rs`, `snapshot.rs`, `optimize_live_evidence.py`, the uncommitted diffs to `docs/optimization-contract.md`/`architecture.md`, or the integration tests.
- The claim that Pi's transport can't be observed comes from the harness docstring and code, not from testing.
- The B3 and B4 failure scenarios come from tracing the code, not from reproducing them.
