"""Deterministic live-gate controls; never call an agent, provider or Git."""

import argparse
from contextlib import redirect_stderr, redirect_stdout
import copy
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from optimize_live_evidence import (
    collect_agent_evidence,
    require_pi_execution,
    summarize_trace,
)
import optimize_live_route as ROUTE


SPEC = importlib.util.spec_from_file_location(
    "live_gate", Path(__file__).with_name("test-optimize-live.py")
)
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value))


def events():
    payloads = [
        {
            "tool_use": {
                "call_id": "call-1",
                "tool_name": "bash",
                "input": {"command": "PRIVATE_PAYLOAD"},
            }
        },
        {
            "tool_result": {
                "call_id": "call-1",
                "output": "PRIVATE_PAYLOAD",
                "is_error": False,
            }
        },
        {
            "token_usage_line": {
                "usage": {"input_tokens": 20, "output_tokens": 10},
                "source": "pi",
            }
        },
        {
            "stream_message": {
                "phase": "final",
                "role": "assistant",
                "kind": "message",
                "text": "PRIVATE_PAYLOAD",
            }
        },
        {"result": {"text": "PRIVATE_PAYLOAD", "session_id": None}},
    ]
    return [
        {"agent_key": "pi", "seq": index + 1, "stream": "stdout", "payload": value}
        for index, value in enumerate(payloads)
    ]


class LiveGateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="newton-live-gate-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.state = self.root / ".newton" / "state"
        self.artifacts = self.root / ".newton" / "artifacts"
        self.evidence = self.root / "evidence"
        self.run_id = "trial-run"
        self.model = "fixture/fixture-model"
        self.workflow_id = "develop-workflow"
        self.workflow = self.state / "workflows" / self.workflow_id
        self.trace = (
            self.artifacts
            / "workflows"
            / self.workflow_id
            / "task"
            / "remediate"
            / "1"
            / "events.ndjson"
        )
        self.candidate = {
            "id": "trial-run-1",
            "artifact_id": "changed",
            "base_artifact_id": "original",
            "created_under_revision": 1,
        }
        self.outcome = {
            "run_id": self.run_id,
            "stop_reason": "cycle_complete",
            "accepted_result": {
                "candidate": self.candidate,
                "evaluation": {"cycle": 1},
            },
            "usage": {"work": 2, "evaluations": 2},
        }

    def fixture(self, engine="pi", status="success", trace_events=None):
        payload = {
            "run_id": self.run_id,
            "role": "develop",
            "cycle": 1,
            "candidate_id": self.candidate["id"],
            # Labels deliberately claim Pi even for the command negative control.
            "parameters": {
                "agent": {"value": "pi"},
                "model": {"value": self.model},
            },
        }
        self.definition = {
            "triggers": {"payload": payload},
            "workflow": {"tasks": [{"id": "remediate", "operator": "AgentOperator"}]},
        }
        self.execution = {
            "execution_id": self.workflow_id,
            "trigger_payload": payload,
            "status": "Completed" if status == "success" else "Failed",
        }
        output = {"exit_code": 0 if status == "success" else 1}
        if engine == "pi":
            output["events_artifact"] = str(self.trace.relative_to(self.root))
        self.task = {
            "task_id": "remediate",
            "run_seq": 1,
            "status": status,
            "error": None if status == "success" else {"code": "WFG-SDK-001"},
            "resolved_params_snapshot": {
                "engine": engine,
                "model": self.model,
                "prompt": "PRIVATE_PAYLOAD",
            },
            "output_ref": {"type": "inline", "value": output},
        }
        self.checkpoint = {
            "execution_id": self.workflow_id,
            "completed": {"remediate": self.task},
        }
        self.completion = {
            "execution_id": self.workflow_id,
            "status": status,
            "result": {"candidate": self.candidate},
        }
        self.save_workflow()
        write_json(
            self.state / "optimize" / self.run_id / "journal.json",
            {
                "run_id": self.run_id,
                "phase": "finished" if status == "success" else "failed",
                "execution_id": self.workflow_id,
            },
        )
        if engine == "pi" or trace_events is not None:
            self.trace.parent.mkdir(parents=True, exist_ok=True)
            self.trace.write_text(
                "".join(
                    json.dumps(event) + "\n"
                    for event in (events() if trace_events is None else trace_events)
                )
            )

    def save_workflow(self):
        for name, value in (
            ("workflow_definition", self.definition),
            ("execution", self.execution),
            ("checkpoint", self.checkpoint),
            ("completion", self.completion),
        ):
            write_json(self.workflow / f"{name}.json", value)

    def collect(self):
        return collect_agent_evidence(
            self.state, self.artifacts, self.run_id, self.evidence
        )

    def verify(self):
        return require_pi_execution(self.collect(), self.outcome, self.model)

    def test_correlated_sdk_execution_passes_and_redacts_content(self):
        self.fixture()
        self.assertEqual(self.verify()["task_id"], "remediate")
        records = self.collect()
        trace = records[0]["tasks"][0]["trace"]
        self.assertEqual(trace["completed_tool_calls"], 1)
        self.assertEqual(
            trace["token_usage"], [{"input_tokens": 20, "output_tokens": 10}]
        )
        self.assertNotIn("PRIVATE_PAYLOAD", json.dumps(records))
        self.assertNotIn(
            "PRIVATE_PAYLOAD", Path(trace["retained_redacted_trace"]).read_text()
        )

    def test_command_engine_with_pi_labels_and_even_planted_trace_fails(self):
        for trace_events in (None, events()):
            with self.subTest(planted_trace=trace_events is not None):
                self.fixture(engine="command", trace_events=trace_events)
                with self.assertRaisesRegex(
                    RuntimeError, "no correlated successful Pi"
                ):
                    self.verify()

    def test_wrong_workflow_task_model_candidate_or_artifact_link_fails(self):
        for mutation in (
            "run",
            "cycle",
            "candidate",
            "model",
            "task",
            "link",
            "failed",
            "checkpoint",
        ):
            with self.subTest(mutation=mutation):
                self.fixture()
                if mutation == "run":
                    self.execution["trigger_payload"] = {"run_id": "unrelated"}
                elif mutation == "cycle":
                    self.outcome["accepted_result"]["evaluation"]["cycle"] = 2
                elif mutation == "candidate":
                    self.completion["result"]["candidate"] = {
                        **self.candidate,
                        "artifact_id": "other",
                    }
                elif mutation == "model":
                    self.task["resolved_params_snapshot"]["model"] = "other"
                elif mutation == "task":
                    self.definition["workflow"]["tasks"][0]["operator"] = (
                        "SetContextOperator"
                    )
                elif mutation == "link":
                    self.task["output_ref"]["value"]["events_artifact"] = (
                        "another-task/events.ndjson"
                    )
                elif mutation == "failed":
                    self.task["status"] = "failed"
                else:
                    self.checkpoint["execution_id"] = "unrelated"
                self.save_workflow()
                with self.assertRaises(RuntimeError):
                    self.verify()
                self.outcome["accepted_result"]["evaluation"]["cycle"] = 1

    def test_incomplete_malformed_wrong_agent_and_unpaired_traces_fail(self):
        for mutation in ("terminal", "tool", "agent", "sequence", "quota", "malformed"):
            with self.subTest(mutation=mutation):
                stream = copy.deepcopy(events())
                if mutation == "terminal":
                    stream.pop()
                elif mutation == "tool":
                    stream[1]["payload"]["tool_result"]["call_id"] = "unrelated"
                elif mutation == "agent":
                    stream[0]["agent_key"] = "claude"
                elif mutation == "sequence":
                    stream[1]["seq"] = 1
                elif mutation == "quota":
                    stream[2]["payload"] = {
                        "quota_exceeded": {"info": {"raw_message": "PRIVATE_PAYLOAD"}}
                    }
                self.fixture(trace_events=stream)
                if mutation == "malformed":
                    with self.trace.open("a") as target:
                        target.write("not-json\n")
                self.assertFalse(summarize_trace(self.trace)["verified_pi_activity"])
                with self.assertRaises(RuntimeError):
                    self.verify()

    def run_harness(self, mode, route="configured-provider"):
        if mode == "stale-run":
            self.fixture()
        args = argparse.Namespace(
            workspace=self.root,
            project="default",
            newton_binary="fake-newton",
            evidence_dir=self.evidence,
            expected_model=self.model,
            route=route,
            pi_models_file=None,
        )
        if route == "local-gateway" and mode != "missing-registry":
            args.pi_models_file = self.root / "models.json"
            write_json(
                args.pi_models_file,
                {
                    "providers": {
                        "fixture": {
                            "baseUrl": "http://127.0.0.1:11434/v1",
                            "models": [{"id": "fixture-model"}],
                        }
                    }
                },
            )
        inspection = {
            "parameters": {"agent": "pi", "model": self.model},
            "definition_id": "software-security",
            "definition_revision": "fixture",
            "context": {"root": str(self.root)},
        }

        def run(command, **_kwargs):
            stage = command[-1]
            if stage == "--inspect":
                return subprocess.CompletedProcess(
                    command, 0, json.dumps(inspection), ""
                )
            if stage == "--preflight":
                return subprocess.CompletedProcess(command, 0, "ready", "")
            if mode in ("nonzero", "timeout"):
                self.fixture(status="failed", trace_events=events()[:-1])
                if mode == "timeout":
                    raise subprocess.TimeoutExpired(
                        command, 3700, b"partial stdout", b"partial stderr"
                    )
                return subprocess.CompletedProcess(
                    command, 1, "no outcome", "backend failed"
                )
            self.fixture(engine="command" if mode == "fake-command" else "pi")
            if mode == "changed-route":
                with args.pi_models_file.open("a") as config:
                    config.write("\n")
            return subprocess.CompletedProcess(command, 0, json.dumps(self.outcome), "")

        with (
            patch.object(GATE, "parse_args", return_value=args),
            patch.object(GATE.subprocess, "run", side_effect=run) as process,
            patch.object(GATE.subprocess, "check_output", return_value="original\n"),
            patch.object(
                ROUTE, "active_pi_registry", return_value=self.root / "models.json"
            ),
            redirect_stdout(io.StringIO()),
            redirect_stderr(io.StringIO()),
        ):
            if mode == "success" and route != "local-gateway":
                GATE.main()
            else:
                with self.assertRaises(RuntimeError):
                    GATE.main()
            self.assertEqual(
                process.call_count,
                1 if mode == "missing-registry" else 3,
                "a failed trial must not be retried",
            )
        return json.loads((self.evidence / "report.json").read_text())

    def test_harness_fake_command_negative_control_is_failed_not_passed(self):
        report = self.run_harness("fake-command", route="local-gateway")
        self.assertEqual(report["status"], "failed")
        self.assertIn("configured agent/model labels", report["error"])
        self.assertEqual(report["outcome"], self.outcome)
        self.assertEqual(
            report["local_gateway_gate"],
            "configuration_verified_transport_unobserved",
        )

    def test_harness_success_does_not_claim_verified_gateway(self):
        report = self.run_harness("success")
        self.assertEqual(report["status"], "passed")
        self.assertEqual(
            report["verified_agent_execution"]["workflow_id"], self.workflow_id
        )
        self.assertEqual(report["route_verification"], "configuration_only")

    def test_local_gateway_label_without_active_registry_fails_before_execution(self):
        report = self.run_harness("missing-registry", route="local-gateway")
        self.assertEqual(report["status"], "failed")
        self.assertIn("requires --pi-models-file", report["error"])
        self.assertEqual(report["local_gateway_gate"], "not_exercised")

    def test_local_gateway_configuration_cannot_substitute_for_transport_proof(self):
        report = self.run_harness("success", route="local-gateway")
        self.assertEqual(report["status"], "failed")
        self.assertEqual(
            report["local_gateway_gate"],
            "configuration_verified_transport_unobserved",
        )
        self.assertFalse(report["route_evidence"]["transport_observed"])
        self.assertIn("transport was not observed", report["error"])

    def test_gateway_registry_change_during_trial_fails_with_agent_evidence_retained(
        self,
    ):
        report = self.run_harness("changed-route", route="local-gateway")
        self.assertEqual(report["status"], "failed")
        self.assertIn("changed during the trial", report["error"])
        self.assertIn("verified_agent_execution", report)
        self.assertEqual(
            report["local_gateway_gate"],
            "configuration_verified_transport_unobserved",
        )

    def test_preexisting_run_cannot_be_replayed_as_new_live_trial(self):
        report = self.run_harness("stale-run")
        self.assertEqual(report["status"], "failed")
        self.assertIn("pre-existing Run", report["error"])

    def test_failed_once_without_outcome_retains_correlated_agent_evidence(self):
        report = self.run_harness("nonzero")
        self.assertEqual(report["status"], "failed")
        task = report["agent_evidence"][0]["tasks"][0]
        self.assertEqual(task["error_code"], "WFG-SDK-001")
        self.assertFalse(task["verified_pi_execution"])
        self.assertTrue(Path(task["trace"]["retained_redacted_trace"]).is_file())

    def test_timeout_retains_partial_output_and_agent_evidence(self):
        report = self.run_harness("timeout")
        self.assertTrue(report["commands"][-1]["timed_out"])
        self.assertEqual((self.evidence / "once.stderr").read_text(), "partial stderr")
        self.assertEqual(len(report["agent_evidence"][0]["tasks"]), 1)

    def test_uncommitted_trace_is_retained_but_cannot_pass(self):
        self.fixture()
        self.checkpoint["completed"] = {}
        self.save_workflow()
        record = self.collect()[0]
        self.assertEqual(len(record["uncommitted_task_traces"]), 1)
        with self.assertRaises(RuntimeError):
            self.verify()


class LocalRouteTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="newton-route-test-")
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "models.json"
        self.patch = patch.object(ROUTE, "active_pi_registry", return_value=self.path)
        self.patch.start()
        self.addCleanup(self.patch.stop)

    def config(self, endpoint="http://127.0.0.1:11434/v1"):
        value = {
            "providers": {
                "local": {
                    "baseUrl": endpoint,
                    "apiKey": "!DO_NOT_EXECUTE_PRIVATE_KEY",
                    "headers": {"secret": "PRIVATE_HEADER"},
                    "models": [{"id": "coder"}],
                }
            }
        }
        write_json(self.path, value)
        return value

    def test_private_loopback_tailnet_and_ipv6_configurations_are_verified(self):
        for host in (
            "127.0.0.1",
            "10.0.0.2",
            "172.16.0.1",
            "192.168.1.2",
            "100.100.2.3",
            "[::1]",
            "[fd00::1]",
        ):
            with self.subTest(host=host):
                self.config(f"http://{host}:1234/v1")
                result = ROUTE.verify_local_route(self.path, "local/coder")
                self.assertEqual(result["status"], "verified_private_configuration")
                self.assertFalse(result["transport_observed"])
                self.assertNotIn("PRIVATE", json.dumps(result))

    def test_public_reserved_metadata_and_mixed_dns_endpoints_fail(self):
        for host in (
            "8.8.8.8",
            "169.254.169.254",
            "192.0.2.1",
            "224.0.0.1",
            "0.0.0.0",
            "[::]",
            "[2001:4860:4860::8888]",
        ):
            with self.subTest(host=host):
                self.config(f"http://{host}/v1")
                with self.assertRaises(RuntimeError):
                    ROUTE.verify_local_route(self.path, "local/coder")
        self.config("https://gateway.example/v1")
        mixed = [(2, 1, 6, "", (ip, 443)) for ip in ("10.0.0.1", "8.8.8.8")]
        with (
            patch.object(ROUTE.socket, "getaddrinfo", return_value=mixed),
            self.assertRaises(RuntimeError),
        ):
            ROUTE.verify_local_route(self.path, "local/coder")

    def test_exact_model_active_registry_and_supported_endpoint_shape_are_required(
        self,
    ):
        self.config()
        for model in ("coder", "local/other", "other/coder", "local/cod*"):
            with self.subTest(model=model), self.assertRaises(RuntimeError):
                ROUTE.verify_local_route(self.path, model)
        with self.assertRaisesRegex(RuntimeError, "not Pi's active"):
            ROUTE.verify_local_route(
                self.path.with_name("unrelated.json"), "local/coder"
            )
        for endpoint in (
            "http://secret@127.0.0.1/v1",
            "http://127.0.0.1/v1?token=secret",
            "file:///tmp/provider",
        ):
            self.config(endpoint)
            with self.assertRaises(RuntimeError):
                ROUTE.verify_local_route(self.path, "local/coder")
        value = self.config()
        value["providers"]["local"]["models"][0]["baseUrl"] = "https://8.8.8.8"
        write_json(self.path, value)
        with self.assertRaises(RuntimeError):
            ROUTE.verify_local_route(self.path, "local/coder")


if __name__ == "__main__":
    unittest.main()
