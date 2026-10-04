# `newton optimize`

Use `newton optimize <project-id>` to run an existing, versioned Optimization
Definition over a local candidate context. Newton orchestrates evaluate → propose
→ optional execute → evaluate → decide. It does not author the definition,
invent a rubric, or deliver the retained result to a target.

## Authoring checklist

1. Choose a stable Objective measurement. Use `grade` only for a rubric score
   from 0 to 100. Otherwise use `numeric` with an explicit unit and
   `minimize`/`maximize` direction.
2. Define immutable evaluator revisions. Map the Objective and every constraint
   to evaluator keys.
3. Choose `measurement_driven` or `observation_driven`. For the latter, set
   `max_suggestions` or accept the default of five.
4. Add a `propose` workflow. Add `execute` only if proposals return executable
   attempts. Never add `promote` to a generic definition.
5. Declare finite elapsed, cycle, work, and evaluation limits; completion
   criteria; stagnation; comparison; constraints; restrictions; and protected
   inputs.
6. Give every workflow a nonempty `workflow.settings.io.result_map`. Keep role
   workflows acyclic and single-attempt; the driver must meter all work.
7. Run `--inspect`, then `--preflight`, then `--once` on a disposable context.

## Definition shape

```yaml
schema_version: 2
id: queue-tuning
revision: '1'
strategy: {kind: measurement_driven}
workflows:
  propose: propose.yaml
requirements:
  objective:
    mode: primary
    objective:
      id: mean_wait
      evaluator: simulator
      measurement: {kind: numeric, unit: seconds, direction: minimize}
  evaluators:
    simulator: {workflow: evaluate.yaml, revision: simulator-v1}
  comparison: {kind: exact}
  acceptance_constraints:
    - {id: feasible, evaluator: simulator}
  execution_restrictions: {denied_actions: [], protected_paths: []}
  resource_limits:
    {elapsed_seconds: 300, max_cycles: 8, max_work: 8, max_evaluations: 16}
  completion:
    - {kind: objective_target, objective: mean_wait, target: 2.5}
  stagnation_cycles: 3
```

The non-coding form above can have `propose` return a candidate directly. Its
evaluator identifies the parameter artifact, independently computes `mean_wait`,
and supplies the `feasible` check. It requires no Git, observations, Plan,
execute workflow, or SQLite.

For a rubric definition, use an Objective such as
`measurement: {kind: grade, dimension: documentation_quality}` and an
observation-driven strategy. The optional `assessment` may contain assessment-
local observations. Each observation has an id, title, rationale, suggested
action, optional priority/evidence/links, and optional resolution. Resolution
and complete-coverage claims require evidence. Omission alone is not proof that
earlier work was fixed.

## Typed workflow results

Evaluator result:

```json
{"candidate": {"id":"c1","artifact_id":"state:7","base_artifact_id":"state:base","created_under_revision":1},
 "evaluation": {"id":"e1","run_id":"<run>","cycle":1,
   "candidate_id":"c1","artifact_id":"state:7","base_artifact_id":"state:base",
   "requirements_revision":1,"evaluator_revisions":{"simulator":"v1"},
   "measurements":{"mean_wait":{"status":"produced","measurement":{"kind":"numeric","unit":"seconds","direction":"minimize"},"samples":[3.1]}},
   "constraints":{"feasible":{"evaluator":"simulator","status":"satisfied","evidence":["simulation.json"]}},
   "completion_checks":{}},
 "assessment":null}
```

Proposal result is one of:

- `{"decision":"candidate", "proposal_id":"...", "candidate":{...}, "rationale":"...", "selected_observations":[], "plan":null}`
- `{"decision":"execute", "proposal_id":"...", "attempt":{...}, "rationale":"...", "selected_observations":[], "plan":{...}}`
- `{"decision":"none", "reason":"..."}`
- `{"decision":"failed", "proposal_id":"...", "failure":{"reason":"...", "evidence":["..."]}}`

An execute workflow returns `{"status":"candidate","candidate":{...}}` or a
known-safe `failed` result. Uncertain effects are operational failures, so do not
label them safe merely to allow a retry.

The driver supplies run/cycle/revision identities, parameters, retained and
currently accepted results, earlier immutable Cycle attempts, and stage/candidate
data in workflow triggers. Preserve these identities exactly in outputs.

## Acceptance and history

Exact comparison accepts any directional improvement and preserves ties.
Repeated comparison requires the declared number of compatible samples and a
non-overlapping improvement by `min_improvement`. All required checks must be
`satisfied`; `unknown` and evaluator errors never qualify.

Inspect `.newton/state/optimize/<RUN_ID>/report.json` for before/after evidence
and every attempt. `cycles/*.json` files are immutable. `current.json` is only a
recovery checkpoint. A published Cycle is the commit point and resume rolls an
older checkpoint forward without replaying the attempt. An active dispatch or
uncertain effect requires reconciliation.

`retained_result` preserves the best artifact. `accepted_result` is present only
when its evidence qualifies under the active requirements revision. Do not
interpret `no_actionable_work` or a resource stop as target completion.

## Commands

```bash
newton optimize project --definition definition.yaml --inspect
newton optimize project --definition definition.yaml --preflight
newton optimize project --definition definition.yaml --once
newton optimize project --resume <RUN_ID> --once
newton optimize project --resume <RUN_ID> --requirements-update revision.yaml
```

Version-1 definitions and old `journal.json`/SQLite runs are not resumed by the
version-2 driver. Adapt the roles and output types, then start a new run. Optional
catalog Findings/Plans and SQLite-backed serve endpoints remain available as
separate platform features; generic runs are not automatically projected there.

For a real coding-agent trial, use `scripts/test-optimize-live.py`. A valid pass
requires correlated Pi SDK/tool/terminal events, a changed accepted artifact, and
an unchanged original checkout. Gateway configuration and observed runtime
transport are separate evidence; report missing configuration as not exercised.

The full contract is `docs/optimization-contract.md`; migration guidance is
`docs/generic-optimization-migration.md`.
