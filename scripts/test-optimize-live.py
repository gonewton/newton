"""Opt-in real Pi/aikit optimization gate using existing agent configuration.

Usage: python3 scripts/test-optimize-live.py WORKSPACE PROJECT [NEWTON_BINARY]
       [--evidence-dir PATH] [--expected-model MODEL]
       [--route {local-gateway,configured-provider,unverified}]
       [--pi-models-file PATH]

Supply an explicitly authorized disposable Git repository with a remediable
finding and a definition that returns immutable Git candidates. A local-gateway
trial requires Pi's existing active models.json and an exact provider/model on
a private endpoint. No configuration or credentials are changed. The harness
preserves every command result and never retries a failed trial.

Success requires correlated runtime Pi SDK tool and terminal events. A temporary
forwarding proxy observes local-gateway inference transport without retaining
credentials, prompts or response bodies; configuration alone cannot pass.
"""

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from optimize_live_evidence import (
    collect_agent_evidence,
    read_object,
    require_pi_execution,
)
from optimize_live_route import verify_local_route
from optimize_live_transport import GatewayTransport


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
    parser.add_argument("--full-loop", action="store_true", help="run to the definition stopping condition instead of --once")
    parser.add_argument(
        "--pi-models-file",
        type=Path,
        help="existing active Pi models.json; required to verify local-gateway configuration",
    )
    parser.add_argument(
        "--route",
        choices=("local-gateway", "configured-provider", "unverified"),
        default="unverified",
        help="local-gateway requires verified private configuration; other values make no local-route claim",
    )
    return parser.parse_args()


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
        "schema_version": 2,
        "status": "running",
        "started_at": datetime.now(timezone.utc).isoformat(),
        "route_assertion": args.route,
        "route_verification": "unverified",
        "local_gateway_gate": "not_exercised",
        "workspace": str(workspace),
        "project": args.project,
        "commands": [],
        "agent_evidence": [],
    }
    root = None
    state = workspace / ".newton" / "state"
    previous_runs = set()
    attempted = False
    transport = None
    execution_env = os.environ.copy()

    def save_report():
        report_path.write_text(json.dumps(report, indent=2) + "\n")

    def retain_trial_evidence():
        if transport is not None:
            report["transport_evidence"] = transport.evidence()
        if root is None or not attempted:
            return
        new_runs = {
            path.parent.name for path in (state / "optimize").glob("*/current.json")
        } - previous_runs
        reported_run = report.get("outcome", {}).get("run_id")
        if reported_run:
            if reported_run in previous_runs:
                raise RuntimeError(
                    "Newton returned a pre-existing Run as live-trial evidence"
                )
            new_runs.add(reported_run)
        records = []
        report["run_evidence"] = []
        for run_id in sorted(new_runs):
            journal_path = state / "optimize" / run_id / "current.json"
            try:
                journal = read_object(journal_path)
                report["run_evidence"].append(
                    {
                        "run_id": run_id,
                        "phase": journal.get("phase"),
                        "execution_id": journal.get("execution_id"),
                        "requires_reconciliation": journal.get(
                            "requires_reconciliation"
                        ),
                        "checkpoint_path": str(journal_path),
                        "report_path": str(journal_path.with_name("report.json")),
                    }
                )
            except (OSError, ValueError):
                report["run_evidence"].append(
                    {"run_id": run_id, "checkpoint_error": "unreadable current.json"}
                )
            records.extend(
                collect_agent_evidence(
                    state, None, run_id, artifacts, context_root=root
                )
            )
        report["agent_evidence"] = records
        save_report()

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
            result = subprocess.run(
                command, capture_output=True, text=True, timeout=3700, env=execution_env
            )
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
        if stage == "once":
            for value in objects(result.stdout):
                if "stop_reason" in value and "accepted_result" in value:
                    report["outcome"] = value
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
            parameters.get("agent") != "pi"
        ):
            raise RuntimeError(
                "this Git candidate gate requires parameter.agent=pi"
            )
        if args.expected_model and parameters.get("model") != args.expected_model:
            raise RuntimeError(
                f"configured model {parameters.get('model')!r} does not match "
                f"--expected-model {args.expected_model!r}"
            )
        if args.route == "local-gateway":
            report["route_evidence"] = verify_local_route(
                args.pi_models_file, parameters.get("model")
            )
            report["route_verification"] = "verified_private_configuration"
            report["local_gateway_gate"] = "configuration_verified_transport_unobserved"
        elif args.route == "configured-provider":
            report["route_verification"] = "configuration_only"
        root = Path(inspection["context"]["root"])
        state = Path(inspection.get("state_dir", state))
        head = subprocess.check_output(
            ["git", "-C", str(root), "rev-parse", "HEAD"], text=True
        ).strip()
        report["original_head"] = head
        invoke("preflight", "--preflight")
        if args.route == "local-gateway":
            transport = GatewayTransport(args.pi_models_file, parameters["model"])
            execution_env["PI_CODING_AGENT_DIR"] = str(transport.agent_dir)
        previous_runs = {
            path.parent.name
            for path in (state / "optimize").glob("*/current.json")
        }
        attempted = True
        output = invoke("once", *([] if getattr(args, "full_loop", False) else ["--once"]))
        outcome = next(
            value
            for value in objects(output)
            if "stop_reason" in value and "accepted_result" in value
        )
        (artifacts / "outcome.json").write_text(json.dumps(outcome, indent=2) + "\n")
        report["outcome"] = outcome
        retain_trial_evidence()
        report["verified_agent_execution"] = require_pi_execution(
            report["agent_evidence"], outcome, parameters.get("model")
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
        if args.route == "local-gateway":
            after_route = verify_local_route(
                args.pi_models_file, parameters.get("model")
            )
            if after_route != report["route_evidence"]:
                raise RuntimeError(
                    "Pi gateway configuration or address resolution changed during the trial"
                )
            report["transport_evidence"] = transport.evidence()
            if not report["transport_evidence"]["transport_observed"]:
                raise RuntimeError(
                    "local gateway transport was not observed; private configuration "
                    "alone cannot satisfy the route gate"
                )
            report["route_verification"] = "observed_private_gateway_transport"
            report["local_gateway_gate"] = "passed"
        report["status"] = "passed"
        report["finished_at"] = datetime.now(timezone.utc).isoformat()
        save_report()
        print(f"Real Pi/aikit candidate accepted; evidence: {report_path}")
        print(
            "This demonstrates the configured optimization workflow, "
            "not comprehensive security, compliance, or proof of the gateway's upstream model placement."
        )
    except Exception as error:
        report["status"] = "failed"
        report["error"] = str(error)
        try:
            retain_trial_evidence()
        except Exception as evidence_error:
            report["evidence_collection_error"] = str(evidence_error)
        report["finished_at"] = datetime.now(timezone.utc).isoformat()
        save_report()
        print(
            f"Live optimization gate failed: {error}; evidence: {report_path}",
            file=sys.stderr,
        )
        raise
    finally:
        if transport is not None:
            transport.close()


if __name__ == "__main__":
    try:
        main()
    except Exception:
        sys.exit(1)
