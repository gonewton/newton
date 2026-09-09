"""Opt-in real Pi/aikit optimization gate using existing agent configuration.

Usage: python3 scripts/test-optimize-live.py WORKSPACE PROJECT [NEWTON_BINARY]
       [--evidence-dir PATH] [--expected-model MODEL]
       [--route {local-gateway,configured-provider,unverified}]

Supply an explicitly authorized disposable Rust repository with a remediable
Cargo.lock finding and the shipped software-security definition. The route value
is an operator assertion about the existing Pi configuration; it is recorded but
does not reconfigure or inspect credentials. The harness preserves every command
result and never retries a failed trial.
"""

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
import sys
import tempfile


def objects(value):
    decoder = json.JSONDecoder()
    cursor = 0
    while cursor < len(value):
        opening = value.find("{", cursor)
        if opening < 0:
            return
        try:
            item, consumed = decoder.raw_decode(value[opening:])
            yield item
            cursor = opening + consumed
        except json.JSONDecodeError:
            cursor = opening + 1


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("workspace", type=Path)
    parser.add_argument("project")
    parser.add_argument("newton_binary", nargs="?", default="newton")
    parser.add_argument("--evidence-dir", type=Path)
    parser.add_argument("--expected-model")
    parser.add_argument(
        "--route",
        choices=("local-gateway", "configured-provider", "unverified"),
        default="unverified",
        help="record the operator-verified route used by the existing Pi configuration",
    )
    return parser.parse_args()


def collect_agent_evidence(state_dir, artifact_dir, run_id):
    """Collect redacted engine traces and reported token usage for this run."""
    records = []
    workflows = state_dir / "workflows"
    if not workflows.is_dir():
        return records

    def token_usage(value):
        found = []
        if isinstance(value, dict):
            if isinstance(value.get("token_usage"), dict):
                found.append(value["token_usage"])
            for child in value.values():
                found.extend(token_usage(child))
        elif isinstance(value, list):
            for child in value:
                found.extend(token_usage(child))
        return found

    for workflow in sorted(workflows.iterdir()):
        definition_path = workflow / "workflow_definition.json"
        checkpoint_path = workflow / "checkpoint.json"
        if not definition_path.is_file() or not checkpoint_path.is_file():
            continue
        definition = json.loads(definition_path.read_text())
        payload = definition.get("triggers", {}).get("payload", {})
        if payload.get("run_id") != run_id or payload.get("parameters", {}).get(
            "agent", {}
        ).get("value") != "pi":
            continue
        checkpoint = json.loads(checkpoint_path.read_text())
        traces = sorted(
            str(path)
            for path in (artifact_dir / "workflows" / workflow.name).glob(
                "task/*/*/events.ndjson"
            )
        )
        records.append(
            {
                "workflow_id": workflow.name,
                "token_usage": token_usage(checkpoint),
                "redacted_event_traces": traces,
            }
        )
    return records


def main():
    args = parse_args()
    workspace = args.workspace.resolve()
    if args.evidence_dir:
        artifacts = args.evidence_dir.resolve()
        artifacts.mkdir(parents=True, exist_ok=False)
    else:
        artifacts = Path(tempfile.mkdtemp(prefix="newton-live-optimization-"))
    report_path = artifacts / "report.json"
    report = {
        "schema_version": 1,
        "status": "running",
        "started_at": datetime.now(timezone.utc).isoformat(),
        "route_assertion": args.route,
        "workspace": str(workspace),
        "project": args.project,
        "commands": [],
    }

    def save_report():
        report_path.write_text(json.dumps(report, indent=2) + "\n")

    def invoke(stage, *extra):
        command = [
            args.newton_binary,
            "--log-dir",
            str(artifacts / "logs"),
            "optimize",
            args.project,
            "--workspace",
            str(workspace),
            *extra,
        ]
        try:
            result = subprocess.run(command, capture_output=True, text=True, timeout=3700)
        except subprocess.TimeoutExpired as error:
            stdout = (
                error.stdout.decode(errors="replace")
                if isinstance(error.stdout, bytes)
                else error.stdout
            )
            stderr = (
                error.stderr.decode(errors="replace")
                if isinstance(error.stderr, bytes)
                else error.stderr
            )
            (artifacts / f"{stage}.stdout").write_text(stdout or "")
            (artifacts / f"{stage}.stderr").write_text(stderr or "")
            report["commands"].append(
                {"stage": stage, "exit_code": None, "timed_out": True}
            )
            save_report()
            raise RuntimeError(f"Newton timed out during {stage}") from error
        (artifacts / f"{stage}.stdout").write_text(result.stdout)
        (artifacts / f"{stage}.stderr").write_text(result.stderr)
        report["commands"].append(
            {"stage": stage, "exit_code": result.returncode, "timed_out": False}
        )
        save_report()
        if result.returncode:
            raise RuntimeError(f"Newton failed during {stage}")
        return result.stdout

    save_report()
    try:
        inspection = next(objects(invoke("inspect", "--inspect")))
        parameters = inspection["parameters"]
        report.update(
            {
                "agent": parameters.get("agent"),
                "model": parameters.get("model"),
                "definition_id": inspection["definition_id"],
                "definition_revision": inspection["definition_revision"],
            }
        )
        if (
            inspection["definition_id"] != "software-security"
            or parameters.get("agent") != "pi"
        ):
            raise RuntimeError(
                "this gate requires the shipped software-security definition and parameter.agent=pi"
            )
        if args.expected_model and parameters.get("model") != args.expected_model:
            raise RuntimeError(
                f"configured model {parameters.get('model')!r} does not match "
                f"--expected-model {args.expected_model!r}"
            )
        root = Path(inspection["context"]["root"])
        head = subprocess.check_output(
            ["git", "-C", str(root), "rev-parse", "HEAD"], text=True
        ).strip()
        report["original_head"] = head
        invoke("preflight", "--preflight")
        output = invoke("once", "--once")
        outcome = next(
            value
            for value in objects(output)
            if "stop_reason" in value and "accepted_result" in value
        )
        (artifacts / "outcome.json").write_text(json.dumps(outcome, indent=2) + "\n")
        report["outcome"] = outcome
        report["agent_evidence"] = collect_agent_evidence(
            root / ".newton" / "state",
            root / ".newton" / "artifacts",
            outcome["run_id"],
        )
        report["cost_accounting"] = {
            "status": "unavailable",
            "reason": "Newton/aikit did not report monetary cost for this run",
        }
        if outcome["usage"]["work"] < 2 or outcome["usage"]["evaluations"] < 2:
            raise RuntimeError(
                "no real develop/regrade cycle was exercised; supply a remediable initial finding"
            )
        accepted = outcome.get("accepted_result")
        if not accepted or accepted["candidate"]["artifact_id"] == head:
            raise RuntimeError(
                "the real agent did not produce a qualifying changed artifact"
            )
        after = subprocess.check_output(
            ["git", "-C", str(root), "rev-parse", "HEAD"], text=True
        ).strip()
        report["final_head"] = after
        if head != after:
            raise RuntimeError(
                "the original project HEAD changed; this definition must not promote"
            )
        report["status"] = "passed"
        report["finished_at"] = datetime.now(timezone.utc).isoformat()
        save_report()
        print(f"Real Pi/aikit candidate accepted; evidence: {report_path}")
        print(
            "This demonstrates the declared dependency-audit workflow, "
            "not comprehensive security or compliance."
        )
    except Exception as error:
        report["status"] = "failed"
        report["error"] = str(error)
        report["finished_at"] = datetime.now(timezone.utc).isoformat()
        save_report()
        print(
            f"Live optimization gate failed: {error}; evidence: {report_path}",
            file=sys.stderr,
        )
        raise


if __name__ == "__main__":
    try:
        main()
    except Exception:
        sys.exit(1)
