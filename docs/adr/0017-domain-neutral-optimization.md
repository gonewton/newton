---
status: implemented
---

# Keep the optimizer independent of the work domain

Newton optimizes objective measurements over identifiable candidate states. Its
core lifecycle is evaluate, propose, optionally execute, evaluate, and
decide. A proposal can return a candidate directly; qualitative observations,
selection by severity, a Plan, and a separate execution workflow are not universal
requirements. Version-2 workflow contracts implement this lifecycle.

## Consequences

- Grade remains a supported 0–100 rubric measurement. Numeric objectives retain
  native units and minimize/maximize direction; acceptance checks can be tests,
  simulations, validation rules, measurements, or human judgments.
- Candidate identity can denote a parameter set, snapshot, object-store version,
  or Git commit. Adapters supply stable references, reproduction/validation, and
  ownership of execution resources. The core does not assume a repository.
- Each proposal receives the retained accepted Candidate and relevant history;
  derivation belongs to the Strategy. The milestone keeps a conservative default
  that does not use unaccepted plateaus as incumbents without requiring a broader
  search-algorithm framework.
- K, default five, bounds selected improvement suggestions for observation-driven
  strategies. It does not universally limit candidate edits or require numeric
  strategies to invent Findings. Priority and severity are domain-specific.
- Acceptance produces a retained result reference and evaluation history.
  Applying settings, branches, publishing, and deployment belong to adapters or
  workflows; they are not implicit effects of acceptance.
- ADR 0016's immutable JSON history holds measurements, candidate references,
  attempts, and decisions. Observations and Plans are optional attachments.
- Demonstrate genericity with a Pi coding scenario and a non-coding bounded
  parameter search against a deterministic scheduling simulator, using the same
  Newton binary. The latter must need neither Git, observations, Plans, a separate
  execution workflow, nor a database.

## Considered alternatives

Making the software grade/plan/develop pipeline universal would force other
domains to create artificial Findings, Plans, and implementation steps. Separate
engines per domain would duplicate comparison, history, and stopping behavior.
Domain-specific workflows and adapters around one optimizer avoid both costs.

See the [specification](../../specs/generic-optimization.md) and the
[optimization contract](../optimization-contract.md).
