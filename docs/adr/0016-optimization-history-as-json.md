---
status: implemented
---

# Store optimization history as per-run JSON artifacts

Newton's generic optimizer retains immutable assessments and completed cycle
records, with a small mutable recovery checkpoint, instead of requiring SQLite
and a persistent Finding → Change Request → Plan backlog. Evaluator observations
can change wording, split, merge, or change under a new rubric; preserving each
assessment is more useful than requiring one permanent issue identity. This
decision is implemented by the version-2 native optimization driver.

## Consequences

- Keep assessed artifact identities, evaluator/requirements revisions, selected
  observations, plan content, attempts, checks, decisions, and accepted results.
- Address observations within an assessment and allow optional continuity links;
  do not require global Finding reconciliation or a Change Request lifecycle.
- Preserve evidence for claimed fixes without requiring every observation to be
  resolved for useful progress or completion.
- Derive reports from history. Any optional database index is rebuildable, not
  authoritative. The broader platform may retain its existing SQLite features.
- Retain atomic writes, schema versions, exclusive ownership, and interrupted-work
  recovery. Do not blindly repeat work whose external effects are uncertain.
- Standard optimizer workflows must work without a database. Cross-run issue
  management and complex concurrent querying are outside this core contract.
- [ADR 0017](0017-domain-neutral-optimization.md) clarifies the generic scope:
  evaluation measurements and candidate references are fundamental; rubric
  Assessments, Observations, selected suggestions, and Plans are optional
  strategy-specific content, not mandatory records for every optimization.

## Considered alternatives

Keeping the current relational model would preserve convenient catalog queries
but retain identity reconciliation and duplicate run-state maintenance. Moving
the same mutable entities into JSON would change the medium without simplifying
the model. Full event sourcing would add machinery unnecessary for bounded,
single-writer optimization runs.

See the [implementation design](../draft/optimization-loop-design.md). Numbering
continues after ADR 0015 referenced by `architecture.md`; older ADR files are
absent from this checkout, so their referenced numbers are not reused.
