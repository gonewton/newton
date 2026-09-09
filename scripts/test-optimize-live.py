"""Opt-in real Pi/aikit optimization gate. Uses existing agent/gateway configuration.

Usage: python3 scripts/test-optimize-live.py WORKSPACE PROJECT [NEWTON_BINARY]
Supply an explicitly authorized disposable Rust repository with a remediable
Cargo.lock finding and the shipped software-security definition. No credentials
are loaded, printed, or injected by this harness. It does not fabricate evidence.
"""

import json
from pathlib import Path
import subprocess
import sys
import tempfile


def objects(text):
    decoder = json.JSONDecoder()
    cursor = 0
    while cursor < len(text):
        opening = text.find("{", cursor)
        if opening < 0:
            return
        try:
            value, consumed = decoder.raw_decode(text[opening:])
            yield value
            cursor = opening + consumed
        except json.JSONDecodeError:
            cursor = opening + 1


def main():
    if not 3 <= len(sys.argv) <= 4:
        raise SystemExit(__doc__)
    workspace = Path(sys.argv[1]).resolve()
    project = sys.argv[2]
    binary = sys.argv[3] if len(sys.argv) == 4 else "newton"
    artifacts = Path(tempfile.mkdtemp(prefix="newton-live-optimization-"))

    def invoke(*args):
        result = subprocess.run([binary, "--log-dir", str(artifacts / "logs"),
                                 "optimize", project, "--workspace", str(workspace), *args],
                                capture_output=True, text=True, timeout=3700)
        (artifacts / ("-".join(arg.lstrip("-") for arg in args) + ".stdout")).write_text(result.stdout)
        if result.returncode:
            raise RuntimeError(f"Newton failed during {args}; review its local logs at {artifacts}")
        return result.stdout

    inspection = next(objects(invoke("--inspect")))
    if inspection["definition_id"] != "software-security" or inspection["parameters"].get("agent") != "pi":
        raise RuntimeError("this gate requires the shipped software-security definition and parameter.agent=pi")
    root = Path(inspection["context"]["root"])
    head = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    invoke("--preflight")
    output = invoke("--once")
    outcome = next(value for value in objects(output) if "stop_reason" in value and "accepted_result" in value)
    (artifacts / "outcome.json").write_text(json.dumps(outcome, indent=2))
    if outcome["usage"]["work"] < 2 or outcome["usage"]["evaluations"] < 2:
        raise RuntimeError("no real develop/regrade cycle was exercised; supply a remediable initial finding")
    accepted = outcome.get("accepted_result")
    if not accepted or accepted["candidate"]["artifact_id"] == head:
        raise RuntimeError("the real agent did not produce a qualifying changed artifact")
    after = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    if head != after:
        raise RuntimeError("the original project HEAD changed; this definition must not promote")
    print(f"Real Pi/aikit candidate accepted; evidence: {artifacts / 'outcome.json'}")
    print("This demonstrates the declared dependency-audit workflow, not comprehensive security or compliance.")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"Live optimization gate failed: {error}", file=sys.stderr)
        sys.exit(1)
