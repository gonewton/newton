# Newton — Context Glossary

The single canonical glossary for Newton's domain language: the optimization
loop, grading, the portfolio model, evaluation, planning, and dependency
mapping. Terms here are domain vocabulary, not implementation notes — internal
data-shape and engine terms (workflow IR, execution, checkpointing, operators,
realtime, expressions, diagnostics) live in `architecture.md`. This glossary
reflects the implemented generic optimization model. Historical design context
is retained in `docs/draft/optimization-loop-design.md`; the current wire
contract is `docs/optimization-contract.md`.

## Loop, grading & operational surface

### Liveness
"Is the process alive and able to respond?" A cheap, dependency-free signal for
automation (load balancers, uptime polling). In Newton, liveness is an **HTTP
concern only** — exposed at the `/health` route by `serve`. It is *not* a CLI
command: a human at a shell wants a diagnostic, not a one-line pulse, and a
machine polling liveness already speaks HTTP. (The `health` CLI command is being
removed; its sole check folds into Doctor.)

### Doctor
The readiness/diagnostic surface. Human-facing, multi-probe (version, workspace
writability, config presence, ailoop reachability, `gh` on PATH, logging). May
grow probes over time. The *only* CLI entry point for "is my environment
healthy?" — any check liveness would have done belongs here instead.

### Step (a workflow run)
A single mutation pass over a project: one workflow graph, executed once. The
unit of work `workflow run` performs. A Step has no opinion about what runs
before or after it.

### Optimization loop
The process that evaluates candidate states, proposes improvements, executes
attempts when needed, and retains accepted progress toward an Objective. The
core is agnostic to the work and delivery; Strategies, Workflows, and adapters
provide those behaviors. Observations and Plans are optional, and stopping short
of the target is distinct from operational failure and verified completion.

### Objective
The declared meaning of progress for an optimization problem. It may concern
quality, waiting time, throughput, resource use, or another measurable outcome;
an Evaluator supplies measurements with explicit comparison and acceptance rules.
_Avoid_: "health" as the general term; "Grade" for the Objective itself.

### Optimization Definition
A reusable declaration of an Objective, its evaluators, acceptance and comparison
criteria, completion criteria, resource limits, and referenced Workflows. It
describes the optimization problem; a Workflow describes executable steps.
_Avoid_: "workflow definition" when referring to the Optimization Definition.

### Candidate
An identifiable proposed state or artifact evaluated against an Objective and
its acceptance criteria. Its stable identity can denote parameters, a file
snapshot, a versioned external artifact, or a Git commit; it implies no repository
or source-code ancestry.

### Objective Measurement
A value used to judge progress toward an Objective: either a Grade or a numeric
quantity with explicit units and a minimize/maximize direction. Measurements
are compared only under compatible evaluation criteria.

### Evaluator
A unit that evaluates an identified Candidate and returns Objective Measurements,
acceptance-check results, and evidence. Qualitative Observations are optional;
a simulator can return measurements without manufacturing Findings.

### Evaluation
The immutable result of applying an Evaluator to an identified Candidate under
identified criteria. It records measurements, checks, and evidence; a rubric
Assessment is a supported form of evaluation feedback.

### Strategy
The policy for proposing and deriving Candidates from the accepted state,
measurements, and relevant history. An observation-driven Strategy can select
suggestions and create a Plan; a parameter Strategy can propose a Candidate
directly. A search policy is distinct from the generic evaluation/history contract.

### Accepted Result
A retained Candidate and the evidence that qualified it under identified
criteria. Acceptance does not imply applying, publishing, merging, or deploying
that result; delivery belongs to the configured adapter or workflow.

### Execution Resource
A domain-specific resource on which execution can have effects and for which an
adapter can define ownership or coordination. A repository is one example, not
the universal execution boundary.
_Avoid_: bare "Target", which also names a dependency-planning concept.

### Review Branch
A local branch identifying the retained optimization result for developer
review. Delivery of a Review Branch does not imply merging or publication.
_Avoid_: "merged result" or "published result" for a locally delivered result.

### Grade
A 0–100 rubric-based Objective Measurement for which larger is better. It is
one supported measurement type, not the required scale for all optimization.
_Avoid_: "Grade" for an arbitrary numeric measurement with native units, a
per-dimension Score, an Assessment, or the Objective itself.

### Grader
An Evaluator that inspects a candidate state and emits an **Assessment** with
a Grade, dimension Scores, and optional Observations. A Grader is defined by what it emits (an Assessment), not by how it is
implemented; it takes one of two forms:
- a **command Grader** — an external program, in any language, that prints an
  Assessment; or
- a **rubric Grader** — a rubric spec (instructions + output schema + model) a
  built-in agent runs.

A Grader is never the operator. A **grading operator** is the Step-level adapter
that runs a Grader and records its Assessment; the two operator kinds
(command-running, rubric-running) are interchangeable because both emit the same
Assessment and differ only in which form of Grader they know how to run. A
grading operator **succeeds whenever it produced a valid Assessment** — a poor
grade is a success, not a task failure; only operational failures (grader crash,
invalid output) fail the task. **Grade quality lives only in the Assessment**,
never in an exit code, and the **gate is workflow policy** (transition `when`
conditions over `tasks.<id>.output` + goal-gate placement), not an operator flag.

### Rubric
The explicit criteria a **Grader** evaluates against: the **Dimensions** to
score, what each means, and how to score them. A Rubric *operationalizes* an
abstract **Objective** ("be secure") into checkable measures — one Objective may
be served by different Rubrics. Applying a Rubric is what yields the Assessment:
one **Score** per Rubric Dimension, plus **Observations** where criteria are
violated. The Rubric is **explicit, authored data** for a *rubric Grader* (the
built-in agent form — it is the operator's `rubric` input) and **embedded and
opaque** inside the program for a *command Grader* (e.g. dk's methodology pack;
the osv grader's implicit "no vulnerabilities"). Either way Newton sees only the
resulting Assessment.
_Avoid_: conflating with **Objective** (the goal) — a Rubric is *how* progress
toward that goal is measured, not the goal itself.

### Assessment
What one Grader run emits: one evaluation event carrying an overall score, a
verdict, per-dimension **Scores**, and a set of **Observations** (its actionable
feedback). An Assessment is an **absolute**
statement about the project's current state, never a self-reported delta — it
carries no baseline. Movement (did the Grade improve?) is derived by the loop
comparing successive Assessments from the same Grader, not reported by the
Grader. Assessments are immutable historical records of evaluation, retaining the
evaluated state and criteria revision. Later evaluations never replace their
original Scores or Observations.
A Grader reports facts and advice only: scores, a **verdict** (advisory; a
required, ordered enum `approve | approve_with_comments | request_changes |
reject`), and **Observations**. It does not decide pass/fail — that is
**policy**, owned by the loop's goal gate, which derives the binding decision
from a declared rule over the Assessment (e.g. a score threshold or accepted
verdicts). An Assessment therefore carries no self-veto / `acceptable` flag.
_Avoid_: "GraderResult" (that is the wire/contract encoding of an Assessment),
"EvalRun" (that is its storage encoding).

### Score
One per-dimension entry inside an Assessment — `{dimension, score (0–100),
rationale}` — a single criterion's measured value. An Assessment has many Scores
plus one **overall score** (grader-reported and holistic, *not* a mandated
aggregation of the Scores). Integrity is one-directional: every **Observation**'s
dimension must be a scored dimension, but a scored dimension may carry no
Observations (meaning "this axis is clean").
_Avoid_: "grade" (reserved for the objective), "metric", "grade row".

### Observation
An actionable critique within an Assessment: the problem, why it matters, and a
recommended action, with supporting evidence and optional domain-specific priority or severity. It is preserved
as originally reported and is addressable within that Assessment; optional links
to earlier observations express continuity without requiring permanent identity.
_Avoid_: treating a reworded, split, merged, or omitted observation as proof that
an earlier problem was fixed.

### Finding
An optional durable, triageable issue that groups related Observations across
Assessments. The generic optimizer does not require a persistent Finding backlog;
its evaluator-reported findings are assessment-local Observations. Persistent
issue tracking belongs to workflows or platform features that need it.
_Avoid_: requiring every Observation to acquire a global Finding identity.

### Reconciliation
Best-effort association of related Observations across Assessments, optionally
supporting a persistent Finding backlog. It is not a mandatory stage of generic
optimization and does not rewrite historical Assessments.

A claim of resolution requires an evaluator conclusion with evidence, or absence
under declared complete relevant coverage. Otherwise resolution is unverified;
that does not automatically block progress or completion. Explicit acceptance
and completion criteria determine what is required.

### Plan
The intended work for an optimization attempt: selected Observations, rationale,
implementation instructions, and intended verification. Finishing a Plan does
not itself establish improvement, acceptance, merging, or publication.

### Plan queue
An optional backlog of Plans awaiting execution. A queue and its approval
lifecycle are workflow or platform concerns, not prerequisites for the generic
Optimization loop.

### Trajectory
The ordered history of an Optimize Run's Evaluations, proposals, attempts,
Candidates, decisions, and results. It explains progress and stopping without
requiring perfect continuity of Finding identities.
_Avoid_: confusing a Trajectory with one workflow Execution.

### Optimize Run
One invocation of the Optimization loop, binding an Objective and its evaluation
criteria to a sequence of Cycles. It retains historical evidence and the best
accepted result, and distinguishes target completion, stagnation, limits, no
actionable improvement, and operational failure.
_Avoid_: bare "run" when it could mean a single workflow Step.

### Cycle
One iteration of an Optimize Run: evaluate, propose, optionally execute, and
evaluate and decide on the resulting Candidate. Proposal may directly produce
a Candidate or report no useful next action; a separate Plan and execution phase
are not mandatory.
_Avoid_: "Step" (one workflow run within a Cycle).

### Driver
A way to set Steps running over the one execution engine. Newton has exactly
these drivers, distinguished only by what originates the work:
- **`workflow run`** — a human/CLI runs one Step, one shot.
- **`optimize`** — the autonomous Optimization loop drives Steps toward the Objective.
- **`serve`** — exposes the engine and its state over HTTP (observe).

External HTTP *ingress* (an outside system POSTing to start a Step) is **out of
scope**: Newton's optimizer is self-driving, not event-driven. See ADR 0004.

### Projection
A representation of Newton-owned state in an external work surface, such as a
project board or issue tracker. A Projection may help humans discover or trigger
work, but it is never the source of truth for the loop. The durable entity remains
in Newton — for example, a **Change Request** may be projected as an external
issue, but the issue is not the Change Request, the **Finding**, or the **Plan**.
External status is a mirror of Newton lifecycle state; in the ideal loop Newton
drives Projection state changes rather than inferring domain truth from the
external surface.
_Avoid_: treating external issue status as the durable Plan queue or using
"issue" as a synonym for any Newton entity.

### ailoop (not a Driver)
Outbound, WebSocket-only human-in-the-loop: mid-Step, Newton reaches *out* to a
human monitor to ask a question. The opposite direction from ingress — a client
connecting out, never a producer of Steps. Frequently confused with webhooks; it
is unrelated.

## Portfolio (governance — outside the optimization loop)

The portfolio model is the cross-scope view that *consumes* loop outputs for
human resourcing decisions. It does not drive any single loop's steps.

### Scope hierarchy (read this first)
The four scopes nest **Product → Component → Repo → Module**, largest to smallest:

```
Product        e.g. "Newton"              (business product)
└─ Component   e.g. "newton-engine"       (team-owned system; a microservice/platform)
   └─ Repo     e.g. github.com/org/newton (one git repository)
      └─ Module e.g. crate `newton-core`  (one crate/package inside the repo)
```

**The ordering is counterintuitive on purpose — guard against it.** A *Component*
is **larger** than a *Repo* (it can own several), even though "component" colloquially
sounds like a small part; and a *Module* is **smaller** than a Repo (a Repo holds
many), even though a Repo may have exactly one. A **Component can contain multiple
Repos; a Repo can contain multiple Modules.** Graders target a **Repo** by default;
a **Plan** may narrow to a single **Module** within that Repo (e.g. "refactor crate
`newton-core`") when work is crate-scoped. When in doubt: Module = the publishable
unit (a crate/npm package), Repo = the git boundary, Component = the ownership/
deployment boundary, Product = the business boundary.

### Product
The top of the portfolio hierarchy: a business-level product or service that
groups **Components** under a common ownership boundary.
_Avoid_: "service", "project".

### Component
A bounded technical system owned by one team, belonging to one **Product**;
roughly a microservice or platform. **It is larger than a Repo and may own several
Repos** — not a part *of* a Repo. Carries `owner`, **Criticality**, and
**Autonomy**.
_Avoid_: "service", "domain"; **and do not read it as a *code* component** (a UI
widget, a Rust crate, a software sub-module) — those are **Modules**, the smallest
scope, the opposite end of the hierarchy.

### Repo
A git repository belonging to a **Component**, and the scope most Graders target
directly. Carries quality **Scores** (e.g. qualityScore, coverage, secScore) and
execution state.

### Module
A package or library inside a **Repo** (Rust crate, npm/pip package, gem, jar) —
a **whole publishable unit**, the lowest-granularity portfolio scope and the unit
of reasoning for **Dependency mapping**. A Repo holds **one or many** Modules.
_Avoid_: "package"/"library" except in language-specific contexts; **and do not
read it as a *language-level* module** (a Rust `mod`, a Python `.py` file) — those
are intra-crate, far below this scope. In Rust terms a Module here is a **crate/
package**, never a `mod`.

### Portfolio
An aggregated view linking a **Product** to its active **Plans** and
**Executions**, including **Grade(s)** rolled up across scopes for the governance
view (where to invest resources). This aggregation is *outside* the optimization
loop. (Quality — once called "Health" — is just one **Objective** that can be
aggregated this way; it is not a privileged or fixed metric.)

### Criticality
Risk classification of a **Component** (`critical | high | medium | low`) that
drives how the portfolio prioritizes work on it.
_Avoid_: "priority", "severity".

### Autonomy
Governance level controlling how much Newton acts without human approval
(`manual | supervised | assisted | autonomous`). Called **Policy Level** in
execution/governance contexts.

### Trend
Directional indicator (`positive | negative`) of whether a **Grade** or **Score**
is improving or declining between **Assessments**.

## Evaluation model

### KPI
A catalog entry describing *what to monitor*, independent of any run: has
`threshold`, `weight`, `aggFn`, `scopeLevel`. KPIs are few and change rarely; a
**Score** may bind to one. They are a governance/reporting catalog, not a loop
input.
_Avoid_: "metric", "indicator".

### Dimension
The qualitative axis a **Score** measures — e.g. `tests`, `security`,
`coverage`. Names *which aspect* of quality is being graded.

### Evaluation Mode
How a human-supplied **Score** relates to a system one: `complement` (adds a
dimension), `override` (replaces the system value), or `train` (a labeled
example).

### Regression
A detected deterioration in a scope's **Grade** or **Scores** between
**Assessments**: references a `kpiId`, carries `delta`, `severity`, and **Trend**.
A Regression is a common **Origin** for a system **Finding**.
_Avoid_: "degradation", "decline".

## Planning & improvement

The generic loop selects Observations and executes Plans. Optional platform
issue management can connect Findings and Change Requests to those Plans.

### Effort
T-shirt sizing on a **Finding**: `XS | S | M | L | XL`. Set by triage, never by a
Grader.
_Avoid_: "story points", "complexity".

### Origin
Whether a **Finding** was surfaced by the system or submitted by a human:
`system | human`.
_Avoid_: "source", "provenance".

### Change Request
An optional reviewable proposal explaining what should change and why, often
grouping Findings before a Plan describes how. The generic loop can retain this
rationale with its selected work and Plan without a separate Change Request
lifecycle.

### PlanSection
An authored content subdivision within a **Plan** (e.g. "Background", "Proposed
changes").

### PlanPolicyCheck
A governance validation rule attached to a **Plan** (`required | optional |
blocking`) that must pass before approval.

### PlanApprover
A named role that must sign off on a **Plan** before it proceeds.

## Dependency mapping

> Status: terms resolved in spec `056-dependencies-crate`; see ADR 0001.

### Dependency
A directed "relies on" link `from → to`, reasoned at **Module** granularity
(links may also be recorded at **Repo** level). Carries a **Discovery** and,
where the version scheme allows, a constraint.
_Avoid_: "edge", "reference".

### Discovery
How a **Dependency** became known: `Detected` (read from a real
manifest/lockfile), `Declared` (stated by a person), or `Suggested` (proposed by
analysis/AI — must be reviewed before trusted). One value per dependency.
_Avoid_: "provenance", "source".

### Discovery Process
The automated pass that reads manifests/lockfiles to produce `Detected`
dependencies. It captures package-level edges but cannot see **non-package**
dependencies (cross-service calls, shared schemas, runtime contracts) — those
must be `Declared`.

### Baseline
A trusted **Dependency** map: `Detected` edges plus human-`Declared` ones,
blessed once before planner agents rely on it. Re-running **Discovery** refreshes
`Detected` edges but never drops `Declared` ones.

### Confirmed
A derived yes/no flag: true when **Discovery** is `Detected` or `Declared`. Only
Confirmed dependencies drive release sequencing automatically; `Suggested` ones
are surfaced for human promotion first.

### Impact Sequence
The computed, ordered list of **Modules** that must be re-released to carry a
change from a modified Module up to a named **Target** — propagation-driven, not
breakage-driven (a Module is included if it lies on a path to the Target *even if
it could absorb the change*). An output, not a stored entity. See ADR 0001.
_Avoid_: "release plan", "blast radius".

### Target
The boundary scoping an **Impact Sequence** — typically the **Product** being
worked on. Propagation follows only paths that reach the Target.

### Effort Class
A per-hop label on each Module in an **Impact Sequence**: `bump-only`
(compatible — mechanical re-release), `adapt` (breaking — needs code changes), or
`unknown` (no signal — treated as `adapt`). Driven by the **Compatibility
Signal**.

### Compatibility Signal
Whether a change to a Module breaks consumers: `breaking | non-breaking |
unknown`. Derived from the version delta only when the project's scheme encodes
compatibility (semver); otherwise must be stated, defaulting to `unknown`.
Newton never assigns versions.

### Co-release Group
A strongly-connected set of Modules (a dependency cycle). Plain release order is
undefined across it, so the analysis surfaces it explicitly for the planner to
resolve — by breaking the cycle temporally or coordinating the group as one
release unit.
_Avoid_: "cycle", "deadlock".
