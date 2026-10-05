# Testing generic optimization

Fast tests use the public `newton optimize` command and the production driver.
The scheduling fixture under `crates/cli/tests/fixtures/scheduling/` minimizes a
deterministic simulated makespan from 10 to 7 to 4. It returns candidates
directly and uses no Git, observations, Plan, execute workflow, or SQLite.

```bash
cargo test -p newton-core optimization:: --lib
cargo test -p newton-cli --test test_optimization_work
cargo test -p newton-cli --test test_e2e_optimize
python3 -B -m unittest discover -s scripts -p 'test_optim*.py'
```

These checks cover typed contracts, acceptance/rejection, K validation,
known-safe failures, optional execution, immutable history, before/after reports,
resume, and the published-Cycle crash window.

The Pi harness is opt-in because it dispatches a real configured agent:

```bash
cargo build -p newton-cli
python3 scripts/test-optimize-live.py /path/to/disposable/workspace default \
  ./target/debug/newton --route configured-provider
```

The Docker wrapper builds Newton, `cargo-audit`, and Pi (running with Bun),
runs as the caller's uid, mounts the disposable workspace and Pi configuration,
and uses host networking
so an already configured local/VPN gateway remains reachable:

```bash
PI_AGENT_DIR="$HOME/.pi/agent" \
  NEWTON_LIVE_ENV_FILE=/path/to/runtime-provider.env \
  scripts/test-optimize-live-docker.sh /path/to/disposable/workspace default \
  /path/to/evidence --route configured-provider
```

Exit code 3 means the real-agent path was not exercised because the active Pi
registry was unavailable. `NEWTON_LIVE_ENV_FILE` is passed to Docker at runtime;
do not add that file to the image, workspace, or evidence directory.
The image build excludes local state, build output and secret files. Runtime
configuration is copied into a disposable writable Pi home; the original mount
stays read-only. Evidence is written under `<EVIDENCE_DIR>/trial/`, which must not
already exist. Pi model paths passed as arguments refer to the container, for
example `--pi-models-file /tmp/newton-home/.pi/agent/models.json`.

If dependency downloads time out on the Docker bridge, build the image explicitly
with `docker build --network host -f scripts/Dockerfile.optimize-live -t
newton-optimize-live:local .`. Cargo registry, Git and build caches persist across
builds. The harness uses the resolved state directory reported by `--inspect`.

For a private local gateway, add the exact expected model and Pi's active model
registry. The harness reports configuration verification separately from runtime
transport observation. A successful optimization does not by itself prove which
network route served inference.

With `--route local-gateway`, the harness verifies the original active registry,
then gives only the child process a disposable copy pointing to a per-trial
loopback forwarding proxy. The proxy forwards streaming inference to the
configured gateway and records the connected private peer, model, HTTP status,
and response byte count. No credentials, prompts, or response bodies enter its
evidence. The original registry is unchanged and the copy is removed on exit.
Passing requires both successful observed transport and correlated Pi/tool/result
evidence. Upstream placement behind the gateway remains unverified.

Run the harness only against a disposable coding fixture. Passing requires
correlated Pi SDK, tool, and terminal traces, a changed accepted commit, retained
earlier improvements, and an unchanged original HEAD. Keep the harness report
and run directory when a trial fails. Report missing credentials/model/endpoint
as `not_exercised`, infrastructure failures separately from agent failures, and
never convert a skipped real-agent trial into success.

The same built Newton binary must run the scheduling fixture and coding trial.
Only definitions, workflows, adapters, and fixtures may differ. A Docker wrapper
may inject the agent configuration and gateway credentials at runtime; it must
not bake secrets or deployment-specific endpoints into the image or diagnostics.

## Repeatable stability trial

Build once, then run three fresh repositories sequentially against the same image:

```bash
docker build --network host -f scripts/Dockerfile.optimize-live \
  -t newton-optimize-live:stability .
export NEWTON_OPTIMIZE_LIVE_IMAGE=newton-optimize-live:stability
export PI_AGENT_DIR=/path/to/pi-config
export NEWTON_LIVE_MODEL=provider/model
# Supply credentials through OPENAI_API_KEY or NEWTON_LIVE_ENV_FILE.
# For a tailnet hostname unavailable in Docker DNS, also set:
# NEWTON_LIVE_GATEWAY_HOST=gateway.example NEWTON_LIVE_GATEWAY_IP=100.x.y.z
scripts/test-optimize-stability.sh /path/to/new-evidence-directory
```

The fixture has three broken documentation links, selects one finding per cycle,
and runs an embedded scheduling example using the same Newton binary. The
criterion is a fixed defect count, not a semantic writing-quality score. Newton's
core remains domain-neutral; Git, Markdown and the fixture's checks belong to its
adapter. The fixture grants the action declarations required for an unsandboxed
agent inside the disposable container. These declarations are not a network or
filesystem security boundary; use a test key and mount no production credentials.

The runner pins the image ID, records the fixture hash/model, refuses reused
workspaces, stops on the first failed gate and creates a local `trial/accepted`
branch in each successful repository. Each trial runs the full bounded loop
(`--full-loop`); the general harness retains its `--once` default. Any code,
fixture, evaluator or model repair requires a new three-trial series. A valid
resource-limit or stagnation stop can pass if a changed candidate was accepted
and the evidence checks pass; achieving zero is not the stability criterion.

Inspect `run-N/trial/report.json`, the repository's `.newton/state/optimize/`
records and `git show trial/accepted`. The report checks actual Pi tool/result
activity and gateway transport. It does not attest the model hardware behind the
gateway. Full workflow artifacts can include prompts and agent output; redacted
harness traces omit these. Keep raw evidence local and review before sharing.

The evaluator explicitly gives nested Newton invocations their own
`NEWTON_STATE_DIR`, preventing inherited parent state from creating false failures.
Recent rejected attempt diagnostics and candidate evaluations are included in the
next proposal; the prompt receives a bounded excerpt and the full history remains
in run JSON. Candidate execution provenance is independent of the cycle in which
that same immutable candidate was most recently evaluated.

Agent failures now use `WFG-AGENT-012` for nonzero process exits or a final SDK
terminal error/aborted turn, even if the process exits zero. The events artifact
is retained and downstream workflow tasks do not run. A failed earlier turn that
recovers before the final turn is judged by the final terminal outcome. Existing
quota and timeout codes retain their meanings.
