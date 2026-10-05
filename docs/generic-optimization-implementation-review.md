# Generic optimization implementation review

Reviewed the implementation merged in PR #539 against the specification, public
CLI, native driver, policy/comparison code, JSON persistence/readers, requirements
updates, shipped software-security adapter, Pi evidence harness, Docker runtime,
and documentation. This is a code and execution review, not another spec review.

## Stability follow-up (2026-10-05)

Real inference exposed failures that the initial deterministic controls missed:

| Defect | Correction and evidence |
| --- | --- |
| Pi reported provider errors with process exit zero, so the workflow could snapshot an unsuccessful agent run. | Consume the SDK's final terminal verdict and process status; return `WFG-AGENT-012`, retain events, and block dependent tasks. Workflow regressions cover HTTP 400/401, aborted turns, nonzero exits and recovery before the final turn. |
| Outer workflow deadlines dropped an async future while its blocking SDK subprocess kept running. | A cancellation guard owns the SDK process-group handle. A real subprocess regression reproduces and prevents the leak. The expired live attempt remains a failed qualification record. |
| The initial artifact-root correction still assumed `state/artifacts`. | Read each workflow's configured artifact root, defaulting to `.newton/artifacts` relative to the execution context; test independent state/artifact relocation and invalid links. |
| A retained candidate re-evaluated in a later cycle lost its qualifying execution provenance in the harness. | Match immutable candidate identity/artifact/revision and allow evaluation at or after the originating execution cycle; continue rejecting future or unrelated executions. |
| A nested example evaluator inherited its parent's state directory. | Give the nested CLI a fresh state directory; validate the executable example using the same image/binary. |
| Rejected-attempt diagnostics were omitted from the trial's next proposal. | Include recent rejection diagnostics and candidate evaluations in the adapter's bounded prompt context; retain full JSON history. |
| Parallel catalog tests inherited another test's temporary environment override. | Give catalog fixtures explicit state paths. The complete suite passes with normal parallel execution. |

The reproducible fixture and three-run recipe are in
[Testing generic optimization](testing-generic-optimization.md). Current results
and limitations are in the [validation record](generic-optimization-validation.md).

## Initial findings corrected (2026-10-04)

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
| P2 | The live harness confused state and artifact storage locations. | Initially exported resolved `state_dir`; the remaining incorrect `state/artifacts` assumption was discovered and corrected in the 2026-10-05 follow-up above. |
| P1 | The local-gateway route gate always failed, with no way to observe transport even on a configured runner. | A per-trial loopback proxy forwards inference to the verified private gateway through a disposable Pi registry. It records actual peer/model/status/byte evidence, forwards streaming responses, and stores no credentials or request/response bodies. Loopback HTTP tests cover successful, failed, empty and wrong-model exchanges. |

## Validation and remaining boundaries

The [validation record](generic-optimization-validation.md) contains executed
checks. Docker and the deterministic non-coding scenario are independently
testable without an inference credential. A live Pi/gateway pass still requires
the operator's gateway configuration and actual transport evidence. The proxy
provides the latter during a configured run; simulated SDK events, loopback test
servers and private configuration labels are not a real inference pass.

The domain-neutral driver still uses the existing software-security-specific
prerequisite check in CLI preflight. That adapter coupling is an architectural
cleanup opportunity; the optimization state machine and non-coding path do not
depend on it. Generic prerequisite hooks and database-backed UI projection are
not introduced by this review.

No mathematical convergence claim or exhaustive observation-resolution policy
was added. Acceptance remains the definition's directional comparison plus its
declared checks, with target/stagnation/resource stopping.
