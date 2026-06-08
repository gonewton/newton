# Diagnostic: orphan_task

A task that is defined in the workflow but is not reachable from the
`entry_task` via any transition path should be flagged by the compiler.

This is reported as a warning (not a hard error) because the existing
`planning_enriching.yaml` has a dead `task-1` at the end. The compiler should
emit a reachability warning but still allow compilation to proceed.

Example: In `planning_enriching.yaml`, `task-1` is never targeted by any
transition from the reachable task set starting at `enrich_spec`.

Expected compiler output:
`CompilerWarning: task 'task-1' is unreachable from entry_task 'enrich_spec'`

Note: This is surfaced as a warning so the round-trip test for
planning_enriching still passes while flagging the dead task.
