# Generic optimization — developer handoff

The authoritative requirements are in [generic-optimization.md](generic-optimization.md).
This implementation map now also points reviewers to the completion evidence.
The current contract and review-resolution record supersede its original
pre-implementation warnings.

## Read first

1. [Specification](generic-optimization.md), including documentation deliverables,
   review closure, tests, and exclusions.
2. [ADR 0016: JSON history](../docs/adr/0016-optimization-history-as-json.md).
3. [ADR 0017: domain-neutral optimizer](../docs/adr/0017-domain-neutral-optimization.md).
4. [Glossary](../CONTEXT.md) and [design decisions](../docs/draft/optimization-loop-design.md).
5. [Adversarial review](../docs/draft/generic-optimization-adversarial-review.md).
   It reviews an earlier spec hash and is intentionally preserved unchanged.
6. [Implemented contract](../docs/optimization-contract.md),
   [review resolution](../docs/generic-optimization-review-resolution.md), and
   [architecture](../architecture.md).
7. [Implementation review](../docs/generic-optimization-implementation-review.md)
   and its regression/validation record; this follow-up corrects defects found
   after the first implementation was merged.

## Implementation entry points

| Area | Existing starting point |
| --- | --- |
| Definition and candidate/evaluation types | `crates/types/src/optimization/` |
| Comparison, validation and requirements revisions | `crates/core/src/optimization/` |
| Driver, role envelopes, recovery and work tracking | `crates/cli/src/cli/commands/optimize/` |
| Grading and existing database-backed operators | `crates/core/src/workflow/grading/`, `crates/core/src/workflow/operators/` |
| Workflow file checkpoints and outputs | `crates/core/src/workflow/checkpoint.rs`, `crates/core/src/workflow/artifacts.rs` |
| Database consumers and external projections | Optimization observation/control/projection modules, `crates/core/src/api/`, `crates/projections/` |
| Deterministic optimization tests | CLI integration fixtures/tests and core optimization tests |
| Real-agent evidence harness | `scripts/test-optimize-live.py`, `scripts/optimize_live_evidence.py` |
| Canonical authoring skill | `skill/newton/SKILL.md` and `skill/newton/references/` |
| Controlled installed skill copy | `.agents/skills/newton/`; installation mapping is in `skill-project.toml` |
| Templates, DSLs and generated contracts | `resources/newton-template/`, `packages/`, `openapi/` |
| Public and developer documentation | README, CONTRIBUTING, CLI help, `docs/`, `website/`, glossary and architecture |

These are starting points, not an exhaustive edit list. Inventory actual consumers
before changing public interfaces. Use the highest existing test seam, the public
`newton optimize` command, wherever practical.

## Suggested implementation order

1. Resolve role contracts and review findings against the generic design. Record
   public shapes, state transitions, stop statuses, compatibility policy, and
   each existing consumer's disposition before writing dependent examples/skills.
2. Implement authoritative JSON history and recovery without recreating global
   Finding/Change Request CRUD in files. Preserve prior attempt evidence.
3. Adapt orchestration and standard workflows to optional observation-driven
   planning, direct candidate proposal, and optional execution. Update readers
   and affected integrations so they do not silently require SQLite.
4. Prove coding and non-coding behavior with the same binary. Reuse Pi for the
   Docker real-agent scenario; no Claude Code migration is required.
5. Finish all affected docs, skills, templates, generated contracts, examples,
   and release guidance as part of the same feature delivery. Validate them
   against the implemented CLI and schemas, not just the intended design.

## Completion checklist

- [x] Specification behaviors implemented and review findings dispositioned with evidence.
- [x] Generic loop works without mandatory SQLite, Findings, Plans, or Git.
- [x] JSON history, interruption recovery, qualification and stopping behaviors tested.
- [ ] Coding real-agent scenario exercised in Docker; deterministic non-coding scenario passes with the same binary.
- [x] Pi harness and route-evidence status report missing configuration as not exercised rather than success.
- [x] Documentation inventory lists affected surfaces and dispositions.
- [x] Canonical skill teaches both strategies; controlled copies are synchronized.
- [x] CLI examples, generated schemas, templates, links, and skill copies are validated.
- [x] Breaking changes and old-run compatibility are explicit.
- [x] Historical review/ADRs remain intact; current documents show implemented behavior.
- [x] Validation results and unexercised real-agent requirements are included in the delivery report.

Use current workspace/repository instructions. Package tooling is uv, pnpm, and
bun; enable auto-merge if raising a PR. Before adding or changing CI, verify
repository visibility and apply the workspace runner policy: private repositories
require their platform runner pool unless a documented policy exception applies;
public repositories may use GitHub-hosted runners. This handoff does not assume
the destination's current visibility or runner availability.
