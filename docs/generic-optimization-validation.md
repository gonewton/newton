# Generic optimization validation

Validation performed on 2026-10-04 from the feature branch:

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
