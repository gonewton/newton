# Optimization Definitions

An Optimization Definition is a reusable YAML or JSON document. It selects a
strategy and workflows, describes the objective and acceptance requirements,
and sets finite resource limits. Loading a definition does not execute code or
grant permissions.

The schema version is `1`. Definition `id` and `revision` identify the reusable
process. Each run retains the complete bound document, context, effective
parameters, and acknowledged Requirements Revision. Editing the source file
does not change an active or resumed run.

```yaml
schema_version: 1
id: verified-security-improvement
revision: "1"
strategy: software-improvement
workflows:
  grade: grading.yaml
  plan: planning.yaml
  develop: development.yaml
  promote: promotion.yaml
requirements:
  objective:
    mode: primary
    objective:
      id: critical_vulnerabilities
      evaluator: security
      measurement:
        kind: numeric
        unit: verified_vulnerabilities
        direction: minimize
  evaluators:
    security:
      workflow: grading.yaml
      revision: protected-evaluator-content-v1
  comparison:
    kind: exact
  acceptance_constraints:
    - id: existing_tests_pass
      evaluator: security
  execution_restrictions:
    denied_actions: [merge, deploy]
    protected_paths: [audit/evaluate.sh]
  resource_limits:
    elapsed_seconds: 3600
    max_cycles: 8
    max_work: 16
    max_evaluations: 32
  completion:
    - kind: objective_target
      objective: critical_vulnerabilities
      target: 0
defaults:
  agent:
    kind: literal
    value: claude
```

The example describes a process; its workflow references and immutable evaluator
revision must be supplied by the execution host. A host that cannot enforce the
declared restrictions rejects the binding rather than claiming protection.
Built-in definitions and user-authored definitions use this same wire contract.

## Binding and permissions

Ordinary parameters resolve as definition defaults → project settings → explicit
run overrides. The binding is a new snapshot; it does not rewrite unrelated
workspace configuration or require a portfolio hierarchy.

Parameters are either `{kind: literal, value: ...}` for non-sensitive data or
`{kind: secret_reference, reference: ...}` for a host-resolved credential reference.
Do not put credentials in literals. Preview helpers redact secret references and
never resolve their values.

Execution authority is the intersection of project and environment grants, with
declared denials removed. Parameter overrides cannot add grants. Evaluation
acceptance does not authorize publication, merging, deployment, or rollback.

The host must enforce action denials across agents, commands, and nested
workflows. It must protect authoritative evaluator inputs from candidate writes.
The domain helpers validate declared capabilities; they do not implement a
sandbox. A prompt, a path checksum checked after execution, or a post-hoc score
does not establish an enforceable action prohibition.

## Evaluation and acceptance

Native measurements declare a nonempty unit and `minimize` or `maximize`.
Rubric measurements use `{kind: grade, dimension: ...}` and finite scores from
0 to 100, where larger is better. Independent objectives are not weighted.

Every evaluation identifies its run, cycle, candidate, immutable artifact,
integration base, Requirements Revision, and evaluator/input revisions. Produced
measurements and operational errors are distinct wire variants. Missing or
incompatible samples, changed identities, and evaluator errors fail validation.
Acceptance checks report `satisfied`, `violated`, or `unknown`; only `satisfied`
qualifies. Human-judged checks require an identified reviewer.

The first result passing evaluation and acceptance checks is an initial
qualification, not an improvement over a nonexistent accepted result. Initially
failing checks do not prevent starting a run. Subsequent candidates must improve
under the declared comparison policy. Exact ties preserve the accepted result.

For noisy primary measurements, use:

```yaml
comparison:
  kind: repeated
  samples: 3
  min_improvement: 2.0
```

Each evaluation must contain exactly three samples. The worst candidate sample
must improve on the best accepted sample by at least `min_improvement`, measured
in the objective's own units. Overlapping ranges are inconclusive and preserve
the accepted result. This conservative observed-range rule is not a confidence
interval or a statistical significance guarantee.

Threshold mode contains independent Grade objectives, each with `target`,
`regression_delta`, and `no_progress_cycles`. Completion requires all targets.
Incremental acceptance requires at least one improving Grade and no worsening
Grade; stop guards remain the native strategy's responsibility. No aggregate
score decides acceptance or completion.

The host preserves previous accepted artifacts while exploring. Before
promotion, it must verify that the artifact and integration base still match
the evaluation and perform an atomic integration check. A changed state needs
re-evaluation. Successful development tests alone do not establish acceptance.

## Local revisions and outcomes

Updates carry `base_revision` for compare-and-swap protection. The control boundary
supplies caller identity and update authority separately from the request file.
Evaluator, objective, and comparison changes need explicit evaluator authority.
Restriction changes need explicit restriction authority.

Updates are pending until acknowledged. Changed action restrictions require
affected work to be paused before acknowledgment; otherwise the update remains
pending. Acknowledgment records relevant prior actions without claiming
retroactive prevention. Rejected and superseded updates remain distinguishable.
The host must persist activation and history before announcing the new revision.

This initial contract conservatively requires full re-evaluation after any
Requirements Revision. Older accepted artifacts remain recoverable history;
stale evidence cannot qualify them for the current outcome.

Completion is separate from incremental acceptance and the stopping reason.
Objective targets must hold for every repeated sample. Additional completion
checks may require a human judgment. An empty work queue does not prove completion.
`cycle_complete` means a requested single cycle finished; it does not imply a
resource limit or completion. Operational failure and cancellation retain their
own stopping reasons.

The outcome reports active requirements, completion evidence, the qualifying
accepted result if present, historical result identities, blocked work, known
resource usage, and diagnostics. If none qualifies it reports `no acceptable
result found`, not impossibility. Resource counters include retries and repeated
evaluation. Token and monetary values are not advertised as hard ceilings.

## Library integration

`newton_types::optimization` provides the wire types.
`newton_core::optimization` exposes definition parsing/binding, schema export,
candidate evaluation, promotion validation, revision handling, and outcome
construction. They do not introduce an HTTP endpoint or a second execution engine.
The existing native driver owns execution, durable state, isolation, resource
enforcement, and local control transport.
