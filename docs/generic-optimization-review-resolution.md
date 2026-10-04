# Generic optimization adversarial-review resolution

This record closes the concrete gaps in the historical
`docs/draft/generic-optimization-adversarial-review.md`. The original review is
retained unchanged as evidence of the earlier design state.

| Review area | Resolution | Validation evidence |
| --- | --- | --- |
| Workflow envelopes | Public `EvaluationOutput`, `ProposalOutput`, and `ExecutionOutput` types define field-level identity, evidence, optional assessment/Plan data, direct candidates, and safe failures. | Generated-schema tests and `test_e2e_optimize` |
| Genericity | Core lifecycle is evaluate/propose/optional-execute/evaluate/decide. Candidate references and native measurement units replace universal Git, Grade, Finding, and Plan assumptions. | Two-cycle scheduling fixture through the public CLI |
| Selection and K | The proposal owns selection; observation-driven strategies validate unique selections against the current assessment and configured K. Measurement-driven strategies require no selection. | K rejection integration test |
| Previous attempts | Proposal triggers include immutable prior Cycle records, retained result, and current accepted result. | Public multi-cycle test and Cycle JSON inspection |
| Retention across revisions | `retained_result` preserves the best artifact; `accepted_result` represents qualification under active requirements. Stale or unavailable evidence never requalifies implicitly. | Core outcome/revision tests |
| Stopping | Configurable consecutive stagnation resets on acceptance. Limits, completion, no action, regression, safe failure, cancellation, and operational failure remain distinct. | Core and CLI integration tests |
| Persistence | Per-run JSON is authoritative; standard optimization opens no database. Reports and file observers derive from those records. Catalog/API SQLite remains optional and separate. | `test_optimization_work` asserts no `backend.sqlite` |
| Publication and recovery | Immutable Cycle creation is the commit point. Resume rolls an older checkpoint forward, rejects conflicts, and never replays an active/uncertain dispatch. | Published-Cycle recovery test and ownership checks |
| Old runs | Pre-generic `journal.json`/SQLite runs fail with an explicit non-migration message. | CLI compatibility test |
| Real-agent evidence | Pi remains the agent harness. Correlated SDK/tool/terminal evidence is distinct from gateway configuration and transport evidence. | Python evidence tests and opt-in live report |
| Delivery | Acceptance returns a candidate and evaluation history. Review branches are coding-adapter output; applying parameters is a simulator-adapter concern. Universal promotion is rejected. | Preflight rejection test and migration guide |
