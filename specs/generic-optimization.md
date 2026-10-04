## Problem Statement

Users want Newton to improve a candidate state toward a measurable Objective
with minimal setup: evaluate, propose a candidate or attempt, execute when needed,
re-evaluate, retain progress, and repeat. The optimizer must be agnostic to the
type of work. Documentation, software quality, configuration tuning, and
scheduling parameters are examples, not product boundaries.

Newton already has a native definition-bound optimization driver, candidate
comparison, requirements revisions, file checkpoints, grading operators, and an
opt-in Pi real-agent harness. However, connecting evaluators and workflows still
requires substantial work. Optimization state overlaps between a JSON journal and
SQLite, while the software strategy requires mutable Findings, Change Requests,
and Plans. Maintaining permanent issue identities is difficult when an evaluator
rewords, splits, combines, omits, or revises its findings.

Users need a trustworthy history of what was assessed, attempted, accepted,
and stopped. They do not need a mandatory issue-management database or exhaustive
resolution of every finding to obtain useful improvement.

## Solution

Make an existing Optimization Definition sufficient to configure a reusable,
generic improvement loop. External coding agents author definitions and workflows
with Newton skill guidance. Newton runs the configured steps automatically as
operators permit and retains objective improvements subject to explicit
acceptance constraints. A Grade is one measurement type; numeric objectives
retain their units and minimize/maximize direction. The observation-driven
strategy selects up to K improvement suggestions per Cycle (default five);
measurement-only strategies need no observations or Plan.

Make per-run JSON history authoritative: immutable Assessments and completed Cycle
records plus a small recovery checkpoint. Preserve original evidence and optional
links between observations without requiring a global Finding backlog. Derive
progress and before/after reports from this history.

Validate real-agent execution through the existing Pi integration in Docker,
and demonstrate genericity with a separate non-coding parameter-optimization
scenario against a deterministic simulator. Both scenarios use the same Newton
binary. Git branches and repository isolation belong only to the coding scenario;
the generic result is an accepted candidate reference and its evaluation history.

## User Stories

1. As a user, I want to run an existing Optimization Definition, so that I can improve an artifact without assembling the orchestration each time.
2. As a definition author, I want coding-agent skill guidance and examples, so that I can create definitions outside Newton using my usual tools.
3. As a definition author, I want reusable evaluation, proposal, and optional execution workflow building blocks, so that each new Objective does not require a new engine.
4. As a user, I want Newton to remain agnostic to the work, so that the same optimizer can serve different quality and improvement objectives.
5. As a definition author, I want an explicit Objective and evaluator, so that improvement has a declared meaning.
6. As a definition author, I want constraints on other dimensions, so that improving one Grade does not silently excuse an unacceptable regression elsewhere.
7. As a definition author, I want to retain independent threshold objectives and numeric measurements, so that existing supported problems remain expressible.
8. As a user, I want the baseline evaluated before attempting improvements, so that I can understand the starting state.
9. As a user, I want evaluators to produce measurements and evidence with optional improvement suggestions, so that each strategy receives relevant feedback.
10. As a user, I want up to K suggestions selected per Cycle in observation-driven optimization with a default of five, so that work happens in manageable batches.
11. As a user, I want a proposal and rationale for each attempt, with a Plan when needed, so that I can see what Newton intended to improve.
12. As a user, I want the loop to advance automatically as operators permit, so that Newton does not introduce unnecessary approval interruptions.
13. As an operator author, I want approval and interaction requirements to remain with operators, so that domain-specific execution policy stays at the appropriate boundary.
14. As a user, I want candidates checked and re-evaluated under comparable criteria, so that acceptance reflects measured progress.
15. As a user, I want improvement in the objective’s declared direction accepted by default when constraints pass, so that useful incremental progress does not require statistical proof.
16. As a definition author, I want optional minimum deltas and repeated grading, so that I can accommodate noisy evaluators when necessary.
17. As a user, I want the current accepted candidate supplied to the proposal strategy, so that it can preserve progress without requiring Git ancestry.
18. As a user, I want rejected and inconclusive candidates to preserve the incumbent, so that unsuccessful work does not displace a better result.
19. As a user, I want unavailable required verification distinguished from failed checks, so that uncertainty is not reported as acceptance.
20. As a user, I want unresolved observations to guide work without automatically blocking completion, so that the loop pursues the Objective rather than a perfect checklist.
21. As a user, I want evidence for claims that an issue was fixed, so that an omitted observation does not masquerade as a verified resolution.
22. As a user, I want each Assessment preserved as originally produced, so that changing evaluator wording does not rewrite history.
23. As a user, I want evaluator and requirements revisions recorded, so that I can interpret score changes in their original context.
24. As a user, I want optional links between related observations, so that useful continuity is available without mandatory permanent issue identity.
25. As a user, I want failed and rejected attempts retained, so that the planner and I can avoid repeating ineffective work.
26. As a user, I want optimization tracking without a SQLite database, so that runs are inspectable and portable as files.
27. As a user, I want safe recovery from interruption, so that an attempted external action is not blindly repeated.
28. As a user, I want stagnation counted as consecutive Cycles without accepted improvement, so that the loop stops when it stops helping.
29. As a user, I want cycle and elapsed-time limits, so that optimization remains bounded.
30. As a user, I want the best accepted result returned when no useful next action exists, so that stopping below target still delivers value.
31. As a user, I want target completion, stagnation, limits, no actionable work, and operational failure reported distinctly, so that I know why the run ended.
32. As a user, I want before/after objective measurements, checks, attempted work, and evidence in a report, so that I can judge the result without querying a database.
33. As a platform integrator, I want optional catalog and issue-management features to remain separate, so that they do not become prerequisites for the generic loop.
34. As a maintainer, I want real Pi-agent tests to demonstrate actual multi-cycle improvement, so that a passing test means more than a successful model response.
35. As a maintainer, I want coding and non-coding Objectives exercised with the same binary, so that the generic abstraction is demonstrated rather than merely claimed.
36. As a maintainer, I want diagnostic artifacts retained for unsuccessful real-agent trials, so that infrastructure and optimization failures can be investigated separately.

37. As a definition author, I want a proposal to return a candidate directly, so that parameter or artifact generation does not require an empty execution stage.
38. As a strategy author, I want to propose from measurements without producing observations, so that numeric search does not manufacture issue records or Plans.
39. As an adapter author, I want to identify versioned states and execution resources without Git, so that configuration and simulation problems use the same core.
40. As a user, I want an accepted result reference and evaluation history independently of delivery, so that publishing or applying the result can follow my domain's workflow.
41. As a user, I want every affected public guide and command example updated with the release, so that following the documentation exercises the implemented behavior.
42. As an agent-assisted author, I want the canonical Newton skill and its references to teach the new contracts, so that my coding agent can create valid definitions without relying on outdated chat context.
43. As a maintainer, I want generated references and distributed skill copies synchronized from their authoritative sources, so that different entry points do not teach conflicting contracts.
44. As an existing user, I want explicit breaking-change and old-run compatibility guidance, so that I know which definitions, workflows, and integrations need adaptation.
45. As an implementing developer, I want documented review resolutions and validation evidence at handoff, so that completion can be assessed against the agreed requirements.

## Implementation Decisions

- Preserve the Optimization Definition as the reusable contract for Objective,
  evaluators, comparison, acceptance and completion criteria, limits, parameters,
  and referenced Workflows. It is distinct from a Workflow's executable steps.
- Reuse the native driver's orchestration, revision handling, and candidate
  comparison. Improve standard workflow composition and external authoring skills
  instead of adding a built-in goal-to-definition assistant.
- Keep domain-specific execution, permissions, approval, isolation, and delivery
  in Workflows/operators and hosts. Do not require Git, a review branch, Docker,
  or a particular repository layout as the universal optimizer abstraction.
- The domain-neutral lifecycle is evaluate → propose → optional execute →
  evaluate → decide. Proposal can return a ready Candidate, an attempt requiring
  execution, or no useful next action. A separate execution phase and a Plan are
  unnecessary when proposal already produces the complete Candidate.
  Follow accepted ADR 0017, "Keep the optimizer independent of the work domain."
- A Strategy determines how candidates are proposed. The observation-driven
  strategy selects up to K assessment-local improvement suggestions, default five;
  measurement-driven strategies may directly propose configurations or parameters.
  K limits selected suggestions, not changed files or incidental improvements,
  and is not a mandatory control for strategies without suggestions.
- Severity is optional, not a universal priority model. Proposal workflows can
  prioritize by relevance to the Objective and prior attempts; software-oriented
  templates can retain severity guidance. No universal issue taxonomy is required.
- Candidate identity denotes an identifiable, stable state or artifact, such as
  a parameter set, content-addressed file snapshot, object-store version, or Git
  commit. The adapter supplies reproducible references and domain-specific
  validation; Git ancestry and a local repository are not generic requirements.
- Every proposal receives the current accepted candidate and relevant history.
  How the next candidate is derived belongs to the Strategy/workflow. The
  milestone's default policy retains the best accepted result and does not use
  unaccepted plateaus as speculative incumbents; this is a strategy policy, not
  a universal assertion about optimization algorithms.
- The generic output is a retained accepted-result reference and evaluation
  history. Applying settings, creating branches, publishing, and deployment are
  optional adapter/delivery behavior outside candidate acceptance.
- Ownership protects an execution target or resource identified by its adapter.
  Repository ownership is one example; parameter-only runs need no synthetic
  repository. Preserve appropriate single-writer and target coordination.
- Improvement in the Objective measurement’s declared direction is the default
  acceptance signal, subject to explicit
  constraints. Preserve the existing comparison framework for repeated grading,
  independent thresholds, and directional numeric objectives. Do not implicitly
  combine dimensions to compensate for a violated constraint.
- In the default policy, unchanged or inconclusive candidates do not displace
  the retained result. Prerequisite work may be grouped and evaluated in one
  attempt; strategies need not express that relationship as a source-code edit.
- A required unknown check prevents acceptance. Report unavailable verification,
  violated checks, unsuccessful improvement, and operational errors distinctly.
- Stagnation counts consecutive Cycles without accepted improvement and resets
  on acceptance. Preserve configured targets and cycle/time limits. No useful next
  action below target returns the best accepted result with an honest stop reason,
  not invented completion or automatic operational failure.
- Follow accepted ADR 0016, "Store optimization history as per-run JSON artifacts."
  Remove mandatory SQLite use from the generic driver and standard optimizer
  workflows. Retain the broader platform's database-backed features separately.
- Persist versioned run metadata and pinned definition/evaluator assets, a small
  mutable recovery checkpoint, immutable completed Cycle records, and referenced
  workflow artifacts. This replaces overlapping authoritative journal/database
  state rather than introducing an additional persistence layer.
- Historical records retain assessed candidate identities and relevant derivation
  references when supplied by the strategy, definition and
  evaluator revisions, objective measurements, attempts, checks, decisions, result
  references, and stopping
  evidence. Assessments, Observations, selected suggestions, and Plan content are
  optional strategy-specific attachments. Record unsuccessful attempts as well
  as successful ones.
- Observations have identity within an Assessment. Optional links can express
  continuity or resolution across Assessments; rewording, splitting, merging, or
  omission never rewrites earlier records or requires a permanent Finding ID.
- Claims of resolution require an evaluator conclusion with evidence, or absence
  under declared complete coverage of the relevant scope. Otherwise resolution
  is unverified. No exhaustive global status lifecycle or perfect matching is
  required; explicit acceptance/completion criteria determine what matters.
- Fold selected-work rationale and Plan content, when present, into Cycle history. Persistent
  Findings, separate Change Request lifecycles, cross-run triage, and approval
  queues remain optional platform/workflow concerns.
- Changing evaluator requirements preserves old history and establishes a new
  revision. Re-evaluate the incumbent and candidate under comparable active
  criteria before claiming improvement; never compare incompatible scores as
  if the rubric were unchanged.
- Derive progress, objective-measurement trajectory, and final reporting from JSON history.
  Optional indexes must be rebuildable and cannot become authoritative stores.
  Reuse large logs, traces, checkpoints, and outputs by reference.
- Retain atomic writes, schema versions, exclusive ownership, and explicit
  interrupted-work handling. Record dispatch identity before work, publish
  completed Cycle evidence before advancing the checkpoint, and detect interrupted
  transitions without blindly replaying uncertain side effects. Do not build a
  general event-sourcing framework or relational database in files.
- Keep the existing Pi client for real-agent validation. Docker plus an operator-configured
  inference endpoint is the agreed E2E environment; adding Claude Code is not
  required. Verify configuration and routing evidence rather than treating a model
  alias as proof of the selected endpoint.
- Update the glossary, implementation contract, architecture documentation,
  authoring skills, and examples with the implemented behavior. Mark any remaining
  database-backed optional operators accurately; do not advertise incomplete
  functionality as available.

### Documentation and skills are required deliverables

Documentation changes are part of implementation completion, not deferred cleanup.
Inventory all documentation artifacts affected by the changed contracts, including
shipped and generated content. For each artifact record its authoritative source,
whether it is updated, regenerated, retired, or unaffected, and how it was checked.
The inventory must cover these surfaces wherever present:

| Surface | Required result |
| --- | --- |
| User overview, quick start, tutorials and website documentation | Explain generic objectives, starting from an existing definition, results and limitations; demonstrate runnable coding and non-coding examples |
| CLI help, command references and configuration documentation | Match actual flags, defaults, strategy controls, validation errors, statuses, stopping and recovery behavior |
| Optimization and workflow contracts | Publish field-level inputs/outputs, optional execution, candidate references, measurements, checks, observation selection where applicable, and failure semantics |
| Persistence and reporting documentation | Describe authoritative JSON records, schema versions, immutable history, checkpoints, recovery and the boundary with optional database features |
| Glossary, architecture and ADRs | Use domain-neutral vocabulary and accurately mark accepted, implemented, superseded and remaining behavior |
| Operator, SDK, embedding, API and realtime references | Reflect changed interfaces and database independence; explicitly document retained optional database-backed integrations |
| Generated schemas, typed outputs and reference artifacts | Regenerate affected contracts from their authoritative generators; align published schemas with runtime validation |
| Canonical Newton skill and reference documents | Teach external agents to author and validate both observation-driven and measurement-only definitions and workflows |
| Distributed or installed skill copies and templates | Synchronize controlled copies using the project's packaging/install mechanism; do not maintain divergent handwritten versions |
| Contribution, testing, CI and troubleshooting guides | Explain deterministic tests, Pi/Docker trials, configuration requirements, diagnostic artifacts and honest not-exercised results |
| Release and breaking-change guidance | State old-run compatibility, changed workflow envelopes, removed requirements, affected consumers and adaptation steps |

Skill guidance must cover evaluator/measurement mapping, candidate identity,
evaluate/propose/optional-execute behavior, K only where applicable, default search
policy, acceptance constraints, evidence and coverage, JSON history, interruption
handling, and domain-specific execution/delivery. Include a non-coding example
without Git, observations, a Plan, or a separate execution workflow. Skills must
not imply Newton generates definitions internally or that every objective is a
0–100 Grade.

Update the canonical skill first, then regenerate or synchronize repository-owned
distributed copies. Record how external workspace installations can be refreshed;
do not require modifying unrelated users' installations. Retain historical ADRs
and review reports as history and mark supersession through current documents,
rather than rewriting the original review to appear resolved.

### Review closure before implementation completion

The implementation must resolve the adversarial review's concrete contract gaps
against the accepted domain-neutral design. Record each finding's disposition,
the resulting contract decision, and its validation evidence. Resolve public
interfaces before depending on them in workflows, skills, or downstream code:

- Field-level evaluate/propose/optional-execute outputs, measurement mapping,
  candidate correlation, optional observation/coverage data, and generic failures.
- Selection ownership and K validation for observation-driven strategies,
  previous-attempt context, and the fate of legacy retry/quarantine controls.
- Retained historical results versus qualification under active requirements,
  especially unknown checks and incompatible evaluator revisions.
- Stagnation configuration, existing threshold/regression guards, public stop
  statuses, remaining work/evaluation limits, and acceptance persistence order.
- JSON consumption by observers, reporting, HTTP/realtime, requirements updates,
  projections, and standard operators that currently require database writes.
- Cycle publication/recovery rules, attempt identity and resource ownership,
  and an explicit old-run compatibility policy without implied migration support.
- Real-agent versus routing-evidence verdicts, scenario-specific delivery,
  multi-cycle fixture behavior, and same-binary non-coding validation.

These are required engineering decisions within the accepted product scope, not
permission to reintroduce mandatory issue management, coding-specific core
concepts, or the other excluded features.

## Testing Decisions

- Use the existing public `newton optimize` command as the primary integration
  seam. Run small fixture definitions through the production driver and workflow
  execution path, then inspect persisted results and observable artifact changes.
  This is the high-level seam already agreed during the design interview.
- A good test checks externally meaningful behavior: proposals, actual candidate changes,
  objective movement under declared criteria, accepted-result continuity, stop reasons,
  history preservation, and recovery. Avoid asserting internal call sequences,
  exact model prose, private helper structure, or implementation-mirroring snapshots.
- Reuse existing deterministic optimization fixtures for acceptance, constraints,
  resource limits, requirements revisions, and workflow validation. Extend their
  assertions for the file-based contract rather than inventing multiple new test
  harnesses. Use lower-level tests only where interruption or malformed-record
  behavior cannot be exercised reliably at the public seam.
- Verify the standard optimizer can start, report progress, stop, and resume
  without a SQLite database or mandatory database registration by its workflows.
- Cover default K and configured K, accepted and rejected candidates, preservation
  of an incumbent on ties/inconclusive results, unavailable required checks,
  accumulation across Cycles, stagnation resets, limits, target completion, and
  no actionable work below target.
- Cover immutable historical Assessments when observations are renamed, split,
  merged, or omitted; optional continuity links must not become required for
  progress. Test supported resolution claims and absent/incomplete coverage.
- Cover requirements/evaluator changes with preserved prior evidence and
  comparable re-evaluation. Verify reports do not present rubric changes as
  unqualified improvement.
- Exercise interruptions before and after Cycle publication, conflicting writers,
  incomplete/invalid persisted records, and uncertain in-flight effects. Verify
  recovery does not silently discard completed evidence or duplicate execution.
- Adapt the existing Pi real-agent evidence harness, which already checks
  correlated actual agent/tool traces, changed accepted commits, and unchanged
  original HEAD. Preserve its distinction between configuration evidence and
  runtime evidence when validating the configured inference route.
- Start with a real coding agent and deterministic Grader on a disposable Git
  fixture inside Docker. Use K=1 and enough independently checkable opportunities
  to require at least two accepted Cycles. Assert actual useful changes, K
  enforcement, retained earlier improvements, unchanged original checkout, and
  a usable review branch produced by the scenario's workflow.
- Add a non-coding scenario that tunes bounded scheduling parameters against a
  deterministic simulator: candidates are parameter sets, the Objective minimizes
  a numeric measure such as mean waiting time, and feasibility is a separate
  acceptance check. Proposal returns candidates directly without observations,
  severity, Plans, an execute workflow, Git, or a persistent issue backlog.
  Assert parameter identities, independently recomputed measurements, constraints,
  retained-result continuity, stopping, JSON history, and reporting.
- Run the coding and non-coding scenarios with the same built Newton binary;
  change only definitions, workflows, adapters, and fixtures. Both must work
  without mandatory SQLite persistence. This is the checkable genericity test.
- Add a real rubric-Grader scenario with independent artifact checks so a
  self-reported Grade increase alone cannot pass. The coding scenario does not
  substitute for the non-coding genericity test.
- Reuse existing Docker and real-agent CI prior art for ephemeral non-root
  execution, runtime-injected credentials, timeouts, and retained diagnostic
  artifacts. Keep fast deterministic checks in ordinary CI; run real-agent trials
  manually and nightly when configured. Missing configuration is reported as not
  exercised, never as proof of success. Report infrastructure failure separately
  from an agent execution failure or lack of useful improvement.
- Validate documented commands and packaged example definitions against the
  built CLI: check help/options, load and validate workflows, run deterministic
  examples, and exercise the documented real-agent path when configured. Never
  report an example as executed when only its syntax was checked.
- Check affected documentation links and skill references; use available schema,
  skill/package and documentation validators. Verify controlled skill copies and
  generated contracts match their authoritative sources after regeneration.
- Check skill authoring guidance against both coding and measurement-only
  fixtures. Expected evidence is valid, runnable definitions through the public
  CLI, not a brittle assertion about an agent's exact prose.
- Search affected documentation, help, templates and skills for obsolete
  mandatory SQLite, global Finding lifecycle, universal Git/Plan/execute and
  Grade-only assumptions. Remove obsolete advice or explicitly scope it to
  historical or optional features.

## Out of Scope

- Hard token or monetary budgets and provider cost accounting.
- A built-in conversational definition/workflow authoring assistant.
- Automatic rubric generation or refinement inside the optimization run.
- Mandatory statistical optimization, exhaustive finding resolution, permanent
  observation identities, or exact semantic matching across evaluations.
- Implementing speculative search through unaccepted plateaus, a general search
  algorithm framework, or advanced benefit/cost ranking in this milestone. Keep
  the default strategy policy distinct from core candidate/evaluation contracts.
- A mandatory global issue backlog, separate Change Request workflow, or new
  portfolio/catalog functionality.
- Replacing every database-backed feature in Newton or creating a general
  event-sourcing, concurrent shared-file database, or indexing platform.
- A new generic permission sandbox or an approval layer above operators.
- Implementing cross-repository orchestration, PR/CI synchronization, package
  publishing, automatic merges, or deployment as part of this milestone. These
  remain possible work-specific integrations rather than restrictions on the
  optimizer's generic nature.
- Replacing Pi with Claude Code solely for the E2E test.

## Further Notes

Developer handoff starts with the current specification and accepted persistence
and genericity decisions. The saved adversarial review concerns an earlier
revision; its concrete concerns remain implementation inputs until resolved.
The final delivery must include a documentation inventory/disposition, review
resolution record, breaking-change notes, and validation evidence. It is not
complete if code ships while affected skills or public instructions remain stale.

The evaluate/propose/optional-execute roles are target contracts. Current
software-oriented grade/plan/develop envelopes and strategy assumptions require
an explicit transition; do not document the renamed lifecycle as implemented.
The original adversarial review remains historical evidence. This genericity
amendment does not by itself resolve all reviewed workflow-schema, recovery,
incumbent-retention, stopping, and integration questions.


Real-agent execution must use runtime-supplied endpoint configuration and
credentials, without embedding deployment-specific details or secrets in the
specification, test image, or published diagnostics. Record the selected agent
and model configuration needed to interpret the test results.

The existing Pi harness distinguishes endpoint configuration from actual runtime
transport evidence. Preserve that distinction: do not report routing verification
from configuration labels alone, and report missing evidence explicitly.

The current code still uses SQLite and a mutable software-work entity model.
Accepted design documentation does not mean the refactor has been implemented.
The implementation should reduce the number of required concepts and sources of
truth while preserving useful evidence, rather than merely re-encoding the same
database entities as JSON.

Definition and workflow examples for different Objectives should demonstrate the
same reusable loop. The goal is useful directional improvement and an intelligible
record of the process, not a proof that every possible problem has been solved.
