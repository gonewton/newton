# Generic optimization implementation review

Reviewed the implementation merged in PR #539 against the specification, public
CLI, native driver, policy/comparison code, JSON persistence/readers, requirements
updates, shipped software-security adapter, Pi evidence harness, Docker runtime,
and documentation. This is a code and execution review, not another spec review.

## Findings corrected

| Priority | Defect and effect | Correction and evidence |
| --- | --- | --- |
| P1 | The shipped grade workflow invoked `grade`, but its helper only accepted `evaluate`; real coding runs stopped at baseline. | Correct the authored role. A Git-backed adapter test invokes that exact role and checks retained-artifact evaluation and unchanged original HEAD. |
| P1 | Docker had no Pi executable; its wrapper also pre-created the evidence directory the harness insists on creating exclusively. | Install pinned Pi with Bun; use a fresh `trial/` evidence subdirectory and a disposable writable configuration copy. Build/runtime smoke checks cover prerequisites. |
| P1 | Resume reopened a finished checkpoint before acquiring ownership; a second process could overwrite active state. | Acquire the context claim, then reload and reopen the checkpoint. A held-lock CLI test asserts zero checkpoint mutation. |
| P1 | A crash after acceptance was saved but before Cycle publication recomputed the decision against the newly accepted candidate. | Reuse the persisted decision and counters. The public CLI crash-window test preserves acceptance and performs no additional dispatch. |
| P1 | Recovery from a committed terminal Cycle could start new work instead of honoring completion or no-action stopping. | Evaluate recovered stopping conditions before dispatch; both stop paths have public CLI regression tests. |
| P1 | Safe mid-Cycle resource exhaustion left a missing Cycle; extending the budget could not resume coherently. | Publish a `resource_limit` Cycle and test an explicit budget extension followed by resumed optimization. |
| P2 | `create_new` exposed partial immutable JSON while it was being written. | Write and sync a temporary file, then atomically publish without overwriting. Test preservation of existing evidence and temporary-file cleanup. |
| P2 | History readers disagreed on schema/gap validation, ignored some directory errors and sorted filenames lexically, breaking at Cycle 10,000. | Use one typed reader with schema, filename, identity, continuity and numeric-order validation. Test unknown schemas, foreign runs, gaps and 10,000 Cycles. |
| P2 | Requirements revisions retained old stagnation and regression baselines. | Clear revision-specific stopping state and verify requalification can proceed. |
| P2 | The coding adapter fell back to original HEAD when the retained result was temporarily unqualified. | Honor the supplied retained candidate during re-evaluation and preparation. The adapter test checks its exact commit and file contents. |
| P2 | The live harness searched the target repository and `.newton/artifacts`, while the driver writes to workspace state and `state/artifacts`. | Export resolved `state_dir` in inspection, consume it in the harness, and resolve relative traces against the execution context. Relocated-state evidence has a regression test. |

## Validation and remaining boundaries

The [validation record](generic-optimization-validation.md) contains executed
checks. Docker and the deterministic non-coding scenario are independently
testable without an inference credential. A live Pi/gateway pass still requires
the operator's gateway configuration and actual transport evidence; simulated
SDK events and private configuration labels are not a substitute.

The domain-neutral driver still uses the existing software-security-specific
prerequisite check in CLI preflight. That adapter coupling is an architectural
cleanup opportunity; the optimization state machine and non-coding path do not
depend on it. Generic prerequisite hooks and database-backed UI projection are
not introduced by this review.

No mathematical convergence claim or exhaustive observation-resolution policy
was added. Acceptance remains the definition's directional comparison plus its
declared checks, with target/stagnation/resource stopping.
