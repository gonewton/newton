# Generic optimization validation

## Stability qualification (2026-10-05)

Three fresh sequential Docker/Pi trials passed with frozen runtime code,
`link-trial-v2` definition/evaluator, image and configured model. Each independently
evaluated three broken links, planned repairs, invoked the real agent in a detached
candidate, re-evaluated it, retained the commit and reached the target. No manual
candidate repairs were made during this series.

| Trial / run ID | Defects before → after | Elapsed | Successful tools | Gateway requests |
| --- | --- | --- | --- | --- |
| 1 / `318daf3a-fdd7-4d51-b32f-2ff7de2f1952` | 3 → 0 | 320 s | 9 | 10 |
| 2 / `7c053122-0340-43eb-95ed-ea72a0921aac` | 3 → 0 | 148 s | 8 | 9 |
| 3 / `c9853552-4cfa-440f-a751-742c001fee47` | 3 → 0 | 88 s | 9 | 10 |

All three outcomes are `completed`. All original HEADs and tracked files remained
unchanged, and each repository has a `trial/accepted` branch. The accepted commits
are respectively `6a9e09348b5263654d1a5161afdbe9d772e2853a`,
`928091fe7d61dc0ccf5ac17817df7a89595017a8` and
`1565f9ab1f2a0295e986145f93676a504125db55`.

The frozen image is
`sha256:6cbd48573e1ff59ce4b4efa3f30d08acbf8765c1a54818d8dccb5a851a27773e`.
All initial Git trees are `c1ab44bec921253ae5bfa97197e346f02022d2f6`.
The model selection was `internal/tools-advanced`, served through the authorized
private gateway; actual transport was observed, but its upstream hardware/model
placement was not independently attested. Each evaluator also ran the deterministic
scheduling example with the **same container's Newton binary** and an isolated
state directory. That example improves makespan 10 → 7 → 4.

These live fixtures each reached the target in one candidate cycle. They validate
real execution and retention, not live multi-cycle convergence or general writing
quality. Multi-cycle acceptance, rejection, requalification and stopping retain
deterministic regression coverage. Follow the
[reproduction recipe](testing-generic-optimization.md#repeatable-stability-trial)
to generate local reports, redacted traces, run JSON and review branches.

Additional checks passed:

- `cargo test --workspace --all-features`: **1,359 tests**, normal parallel execution.
- `cargo clippy --all-targets --all-features -- -D warnings` and formatting.
- Python evidence/transport/adapter suite: **26 tests**.
- Shell syntax, full patch whitespace and canonical/distributed skill equality.
- Workflow regressions for provider failures, recovery, nonzero exit and cancelling
  a running SDK subprocess at an outer workflow deadline.

An earlier exploratory series was stopped to correct a reused error code. The next
series exhausted a 900-second limit before candidate qualification and exposed an
agent subprocess that outlived the deadline. Neither is counted as a pass. Their
original evidence remains preserved. Cancellation was fixed, the fixture prompt
was narrowed to its actual task, the bound became 1,800 seconds, and the successful
three-trial series above restarted from fresh repositories. Tests/documentation
were refined afterward without changing the frozen runtime, fixture or model.

The records below describe earlier stages; their unexercised-inference status is
historical, superseded by this qualification.

## Implementation review follow-up (2026-10-04)

The [implementation review](generic-optimization-implementation-review.md)
records the defects found and their fixes. Follow-up validation supersedes the
environment blockers in the initial-delivery record below:

| Check | Result |
| --- | --- |
| `cargo test --workspace --all-features` | Passed: 1,356 tests across 110 suite results, no failures or ignored tests. Run outside the filesystem sandbox so existing tests can write their normal log directory. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo fmt --all -- --check` and `git diff --check` | Passed |
| `python3 -B -m unittest discover -s scripts -p 'test_optim*.py'` | Passed: 21 evidence, routing-control, loopback-transport and shipped-adapter tests. Includes relocated state and exact retained-commit evaluation. These are deterministic controls, not live inference. |
| Newton skill install and locked restore | Both passed offline in a disposable project containing the canonical local skill. The repository-wide legacy lock still lacks integrity for unrelated dependencies; it was not rewritten. |
| Canonical/distributed skill equality | Passed |
| Docker runtime availability | Host Docker works outside the sandbox; the original snap-capability error was a sandbox limitation. |
| Final Docker image | Built successfully with host networking after a bridge-network crates.io timeout. Image ID: `sha256:441bd54fb2ed07b1b38da5136a66adf517eef7a0c12e6b2b968ee693be448e67`. |
| Container prerequisites and entrypoint | Pi 0.82.1, Newton 0.5.134 and cargo-audit 0.22.2 run as uid 1000. The entrypoint starts using a read-only, credential-free test registry and a writable disposable home. |
| Docker non-coding E2E | The image's Newton binary completes scheduling from 10 to 7 to 4 as uid 1000, with a completed outcome and retained `schedule:4`. No model is called. |
| Packaged Docker transport observer | Two deterministic loopback HTTP tests pass as uid 1000 against the image's packaged observer. Stream forwarding, private-peer evidence, failure/empty/wrong-model rejection, and payload redaction are covered. |
| Pi/gateway inference | Not exercised: no active Pi `models.json` or internal gateway credential/model configuration was available. No inference request was sent. |

The gateway route gate uses a temporary forwarding proxy to observe actual
private-peer inference transport without storing credentials or payloads.
Deterministic proxy tests pass, but a configured runner must complete actual
inference before the real-agent completion checkbox can be closed.

## Initial delivery record

Validation performed on 2026-10-04 before the follow-up review:

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo test -p newton-core optimization:: --lib` | Passed: 19 tests |
| `cargo test -p newton-cli cli::commands::optimize:: --lib` | Passed: 9 tests |
| `cargo test -p newton-cli --test test_e2e_optimize` | Passed: 13 tests |
| `cargo test -p newton-cli --test test_optimization_work` | Passed: 5 tests |
| `cargo test -p newton-cli --test test_e2e_coverage_matrix` | Passed: 6 tests |
| `python3 -B scripts/test_optimize_live_evidence.py` | Passed: 16 deterministic harness/evidence controls |
| Canonical/distributed skill `diff -rq` | Passed: byte-for-byte identical |
| `fastskill project install --dry-run --offline --json` | Reached project validation; blocked by pre-existing missing `skills.lock` integrity for `cli-rust-dev`. No files changed. |
| Local Markdown-link scan | Checked 36 files. New generic-optimization links resolve; the checkout already lacks LICENSE and historical ADR files referenced by existing docs. |

`cargo test --workspace` passed the backend and CLI library suites, then stopped
in existing argument-validation integration tests because those tests try to
write `/home/sysuser/.newton/logs/newton.log`, while this managed environment
makes that location read-only. The affected feature suites were rerun with their
temporary `--log-dir` helpers and passed as listed above.

The real Pi/Docker trial was not exercised. This host has no active
`$HOME/.pi/agent/models.json`, and the installed Docker snap cannot start because
its required `cap_dac_override` capability is unavailable. No inference request
was sent, so this is not a real-agent or gateway-routing pass. The Dockerfile,
wrapper, file-history collection, negative controls, and routing-evidence checks
are present for a configured nightly/manual runner.
