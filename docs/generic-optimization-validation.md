# Generic optimization validation

## Implementation review follow-up (2026-10-04)

The [implementation review](generic-optimization-implementation-review.md)
records the defects found and their fixes. Follow-up validation supersedes the
environment blockers in the initial-delivery record below:

| Check | Result |
| --- | --- |
| `cargo test --workspace --all-features` | Passed: 1,356 tests across 110 suite results, no failures or ignored tests. Run outside the filesystem sandbox so existing tests can write their normal log directory. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo fmt --all -- --check` and `git diff --check` | Passed |
| `python3 -B -m unittest discover -s scripts -p 'test_optim*.py'` | Passed: 18 evidence, routing-control and shipped-adapter tests. Includes relocated state and exact retained-commit evaluation. These are deterministic controls, not live inference. |
| Newton skill install and locked restore | Both passed offline in a disposable project containing the canonical local skill. The repository-wide legacy lock still lacks integrity for unrelated dependencies; it was not rewritten. |
| Canonical/distributed skill equality | Passed |
| Docker runtime availability | Host Docker works outside the sandbox; the original snap-capability error was a sandbox limitation. |
| Final Docker image | Built successfully with host networking after a bridge-network crates.io timeout. Image ID: `sha256:1b06956c3f9b46b1e7f12a7af4b30f92c0c23ff345856d63f4c9cceb1da42c4f`. |
| Container prerequisites and entrypoint | Pi 0.82.1, Newton 0.5.134 and cargo-audit 0.22.2 run as uid 1000. The entrypoint starts using a read-only, credential-free test registry and a writable disposable home. |
| Docker non-coding E2E | The image's Newton binary completes scheduling from 10 to 7 to 4 as uid 1000, with a completed outcome and retained `schedule:4`. No model is called. |
| Pi/gateway inference | Not exercised: no active Pi `models.json` or internal gateway credential/model configuration was available. No inference request was sent. |

The gateway route gate still deliberately refuses to treat private configuration
as observed transport. A configured runner must supply actual inference and
transport evidence before the real-agent completion checkbox can be closed.

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
