# Newton — Context Glossary

The single canonical glossary for Newton's domain language: the optimization
loop, grading, the portfolio model, evaluation, planning, and dependency
mapping. Terms here are domain vocabulary, not implementation notes — internal
data-shape and engine terms (workflow IR, execution, checkpointing, operators,
realtime, expressions, diagnostics) live in `architecture.md`. When code and
this glossary disagree, one of them is wrong — resolve it.

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
Newton's reason to exist (and the source of its name — Newton's method, iterating
toward an objective). The autonomous cycle that drives a project toward a better
**Grade**: `change-request → plan → refine → implement → test → grade →
change-request`. It is a **Driver** that sequences **Steps**; it is not itself a
Step. Distinct from a batch: a batch drains a finite set once and stops; the
optimization loop is closed — grading feeds the next round of change requests.
The CLI command is **`optimize`** (renamed from `batch`; see ADR 0003).
The `change-request` phase is the loop's *optimizer step*: it reads the standing
**Findings** and synthesizes a **Change Request**. Full closing chain:
`grade (→ Assessment) → reconcile (→ Findings) → change-request (→ Change Request)
→ plan → implement → grade`.
(As of this writing the edge is **designed but not yet wired** in code; the
vocabulary and ADR 0005 describe the target.)

### Objective
The project-defined goal the optimization loop drives toward — *what progress
means* for this project. General and project-specific: it may be code quality,
roadmap completion, a security baseline, dependency freshness, migration
progress, and so on. Overall quality ("health") is just one possible Objective,
not a privileged one. The Objective determines what a **Grader** measures and
what the **Grade** scores.
_Avoid_: "health" as the general term (it is one specific Objective); "goal"
(collides with **Goal Gate**); "target" (collides with dependency **Target**);
"KPI"/"indicator" (a KPI is a monitored measurable, not the goal itself).

### Grade
The current scalar **measure of progress toward the Objective** (0–100) — the
value the loop maximizes; what every Step is ultimately trying to improve. The
Grade is a **single-scope** quantity computed and consumed *inside* one loop;
rolling Grades up to a coarser scope (Component/Product) gives the **governance
view** that lives *outside* the loop — a scope distinction, not a separate
metric.
_Avoid_: "grade" for a per-dimension number (that is a **Score**), for an
evaluation event (an **Assessment**), or for the goal itself (the **Objective**);
"health" for the aggregate (it is aggregated Grade).

### Grader
A pluggable evaluation unit that inspects a project's current state (codebase
plus artifacts) and emits an **Assessment**. The actor behind the loop's `grade`
phase. A Grader is defined by what it emits (an Assessment), not by how it is
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
Grader. Assessments are the durable record of evaluation and the input the loop
folds into the **Grade**.
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
One unit of the **text-gradient** within an Assessment: a single critique as the
Grader phrased it. It carries a mandatory **directional triple** — the *problem*,
*why it matters*, and a *recommended action* (without the action it is a mere
complaint, not a gradient) — plus its **dimension** and a **severity**
(`critical | high | medium | low`); optionally a flexible **location** (which may
be coarse or absent for non-local critiques) and a **confidence**. Observations
are the directional signal that drives the next change (the scalar Score only
says how far off, not which way). An Observation is **transient** — recomputed
every run, in the Grader's words, and **deliberately carries no id**: identity is
Newton's, assigned via **Reconciliation**. It becomes durable only by being
reconciled into a **Finding**.
_Avoid_: "finding" for the raw item (a Finding is its durable, reconciled form).

### Finding
The durable, triageable record of one recurring problem or improvement, into
which matching **Observations** are reconciled across grading runs. It is
`Opportunity` *renamed and extended*: beyond portfolio metadata (risk/severity,
effort, expected value, `dependsOn`/`blocks`, **Origin** `system | human`,
`source` grader, links to **Component**/**Repo**/**KPI**) it carries the
**structured text-gradient** — `dimension`, `location`, `why_it_matters`, and
`recommended_action` (the direction the **Change Request** synthesis reads) — plus
reconciliation metadata (`fingerprint`, `last_seen_at`). Lifecycle:
`awaiting_triage → triaged → approved_for_planning → structured →
deferred | rejected`, plus **`resolved`** — set *automatically* by
**Reconciliation** when the issue vanishes (the per-Finding convergence signal),
firmly distinct from the human `rejected`/`deferred` (Reconciliation never
resurrects a human-closed Finding; it reopens a `resolved` one if it recurs). A Finding's identity is
**assigned and maintained by Newton via Reconciliation — never taken from a
Grader** (Graders are non-deterministic and cannot supply stable ids). Findings
are the standing text-gradient that the loop synthesizes (Score-prioritized) into
a **Change Request** — the durable work pipeline is
`Finding → Change Request → Plan → Execution → re-grade`.
_Avoid_: "Opportunity" (the former name, being retired), "Issue", "Ticket",
"suggestion", "recommendation".

### Reconciliation
The per-run step, owned by Newton (not the Grader), that matches an Assessment's
**Observations** against the currently-open **Findings** for a scope. A match
**refreshes** the existing Finding; an unmatched Observation **creates** a new
Finding; an open Finding with no matching Observation this run is marked
**resolved** (the gradient landed). Matching is hybrid: a natural-key
fingerprint (scope + dimension + normalized location + rule) first, then
**semantic similarity** for the rest — because non-deterministic AI Observations
cannot be matched by equality. Reconciliation is what gives Findings stable
identity and yields per-Finding resolution tracking, a richer convergence signal
than the scalar Grade delta.

### Plan
A unit of queued work a project's optimization loop consumes — the spec for one
Step. This is the canonical noun; "work item", "task", and "batch item" are
deprecated synonyms to be removed.

### Plan queue
The per-project backlog of Plans and their lifecycle:
`todo → completed | failed` (with `draft`, `abandoned` as holding states). The
queue is the only durable state the loop owns between Steps.

### Driver
A way to set Steps running over the one execution engine. Newton has exactly
these drivers, distinguished only by what originates the work:
- **`workflow run`** — a human/CLI runs one Step, one shot.
- **`optimize`** — the autonomous Optimization loop drives Steps toward the Grade.
- **`serve`** — exposes the engine and its state over HTTP (observe).

External HTTP *ingress* (an outside system POSTing to start a Step) is **out of
scope**: Newton's optimizer is self-driving, not event-driven. See ADR 0004.

### ailoop (not a Driver)
Outbound, WebSocket-only human-in-the-loop: mid-Step, Newton reaches *out* to a
human monitor to ask a question. The opposite direction from ingress — a client
connecting out, never a producer of Steps. Frequently confused with webhooks; it
is unrelated.

## Portfolio (governance — outside the optimization loop)

The portfolio model is the cross-scope view that *consumes* loop outputs for
human resourcing decisions. It does not drive any single loop's steps.

### Product
The top of the portfolio hierarchy: a business-level product or service that
groups **Components** under a common ownership boundary.
_Avoid_: "service", "project".

### Component
A bounded technical system owned by one team, belonging to one **Product**;
roughly a microservice or platform. Carries `owner`, **Criticality**, and
**Autonomy**.
_Avoid_: "service", "domain".

### Repo
A git repository belonging to a **Component**, and the scope most Graders target
directly. Carries quality **Scores** (e.g. qualityScore, coverage, secScore) and
execution state.

### Module
A package or library inside a **Repo** (Rust crate, npm/pip package, gem, jar).
The lowest-granularity portfolio unit; the unit of reasoning for **Dependency
mapping**.
_Avoid_: "package"/"library" except in language-specific contexts.

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

The durable work pipeline a **Finding** feeds: Finding → Change Request → Plan →
Execution. (**Finding**, **Change Request**, and **Plan** are defined above.)

### Effort
T-shirt sizing on a **Finding**: `XS | S | M | L | XL`. Set by triage, never by a
Grader.
_Avoid_: "story points", "complexity".

### Origin
Whether a **Finding** was surfaced by the system or submitted by a human:
`system | human`.
_Avoid_: "source", "provenance".

### Change Request
The synthesized, reviewable proposal of changes derived from reading the standing
**Findings** (prioritized by **Scores**) — the loop's *optimizer step* (it
applies the aggregated text-gradient). It is **WHAT/WHY**; the **Plan** it drives
is the **HOW**. Once approved it produces a Plan. Lifecycle:
`proposed → approved → planned → rejected`. A Change Request is a concrete,
pipeline-bound change even while `proposed` — not a loose suggestion. Read as
"Request for Change" (change-management sense). Unifies the loop's
`change-request` phase and the former `Request` entity — it is `Request` renamed
and extended: it links **many** Findings (`finding_ids[]`, not a single
opportunity), carries a structured `body` (the synthesized proposal) and an
**Origin** (`system` synthesized | `human` authored).
_Avoid_: "Proposal"/"suggestion" (too soft), "Request for Comments" (wrong
sense), "changelist"/"changeset" (that is a diff, not a request to change),
"ticket".

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
