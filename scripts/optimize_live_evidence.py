"""Read Newton's persisted task/SDK contracts without collecting prompt content.

These records establish the observed local execution path, not cryptographic
attestation of an agent binary or proof of which provider endpoint it contacted.
"""

import hashlib
import json
from pathlib import Path


def read_object(path):
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        raise ValueError(f"expected JSON object: {path}")
    return value


def summarize_trace(path, retained_path=None):
    """Validate canonical SDK events and retain only allowlisted metadata."""
    summary = {
        "source": str(path),
        "event_count": 0,
        "agent_keys": [],
        "completed_tool_calls": 0,
        "assistant_final": False,
        "terminal_result": False,
        "token_usage": [],
        "issues": [],
    }
    digest = hashlib.sha256()
    keys, calls, completed = set(), set(), set()
    previous = -1
    last_kind = None
    retained = []
    complete_read = True
    try:
        with path.open("rb") as source:
            for line in source:
                digest.update(line)
                if not line.strip():
                    continue
                event = json.loads(line)
                seq = event["seq"]
                payload = event["payload"]
                if type(seq) is not int or seq <= previous or len(payload) != 1:
                    raise ValueError("invalid event sequence or payload")
                previous = seq
                key = event["agent_key"]
                if not isinstance(key, str):
                    raise ValueError("invalid agent identity")
                keys.add(key)
                kind, value = next(iter(payload.items()))
                last_kind = kind
                summary["event_count"] += 1
                item = {
                    "agent_key": key,
                    "seq": seq,
                    "stream": event["stream"],
                    "event_type": kind,
                }
                if kind == "tool_use" and value.get("call_id"):
                    calls.add(value["call_id"])
                    item["call_id"] = value["call_id"]
                elif kind == "tool_result" and value.get("call_id"):
                    item.update(
                        call_id=value["call_id"], is_error=value.get("is_error")
                    )
                    if value["call_id"] in calls and value.get("is_error") is False:
                        completed.add(value["call_id"])
                elif kind == "stream_message":
                    item.update(
                        {field: value.get(field) for field in ("phase", "role", "kind")}
                    )
                    if (
                        value.get("phase") == "final"
                        and value.get("role") == "assistant"
                        and value.get("kind") == "message"
                        and value.get("text", "").strip()
                    ):
                        summary["assistant_final"] = True
                elif kind == "result":
                    summary["terminal_result"] = bool(value.get("text", "").strip())
                elif kind == "token_usage_line":
                    usage = {
                        name: count
                        for name, count in value.get("usage", {}).items()
                        if name.endswith("_tokens")
                        and type(count) is int
                        and count >= 0
                    }
                    summary["token_usage"].append(usage)
                    item["token_usage"] = usage
                elif kind == "quota_exceeded":
                    summary["issues"].append("SDK reported quota failure")
                retained.append(item)
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
        complete_read = False
        summary["issues"].append(
            f"unreadable or malformed SDK trace ({type(error).__name__})"
        )
    summary.update(
        agent_keys=sorted(keys),
        completed_tool_calls=len(completed),
        source_sha256=digest.hexdigest() if complete_read else None,
    )
    summary["verified_pi_activity"] = bool(
        not summary["issues"]
        and keys == {"pi"}
        and completed
        and summary["assistant_final"]
        and summary["terminal_result"]
        and last_kind == "result"
    )
    if retained_path is not None:
        retained_path.parent.mkdir(parents=True, exist_ok=True)
        retained_path.write_text("".join(json.dumps(item) + "\n" for item in retained))
        summary["retained_redacted_trace"] = str(retained_path)
    return summary


def collect_agent_evidence(state_dir, artifact_dir, run_id, evidence_dir=None):
    """Correlate one Run's workflow, checkpoint, resolved task and SDK trace."""
    records = []
    workspace = state_dir.parent.parent
    for workflow in sorted((state_dir / "workflows").glob("*")):
        try:
            definition = read_object(workflow / "workflow_definition.json")
            payload = (definition.get("triggers") or {}).get("payload", {})
            if payload.get("run_id") != run_id:
                continue
        except (OSError, ValueError, AttributeError):
            continue
        record = {
            "run_id": run_id,
            "workflow_id": workflow.name,
            "role": payload.get("role"),
            "cycle": payload.get("cycle"),
            "candidate_id": payload.get("candidate_id"),
            "tasks": [],
            "issues": [],
        }
        records.append(record)
        documents = {}
        for name in ("execution", "checkpoint", "completion"):
            try:
                document = read_object(workflow / f"{name}.json")
                if document.get("execution_id") != workflow.name:
                    raise ValueError("workflow identity mismatch")
                documents[name] = document
            except (OSError, ValueError):
                record["issues"].append(
                    f"missing, malformed or uncorrelated {name}.json"
                )
        execution = documents.get("execution", {})
        record["status"] = execution.get("status")
        trigger = execution.get("trigger_payload", {})
        if any(
            trigger.get(key) != payload.get(key)
            for key in ("run_id", "role", "cycle", "candidate_id")
        ):
            record["issues"].append("execution trigger identity mismatch")
        completion = documents.get("completion", {})
        record["candidate"] = (completion.get("result") or {}).get("candidate")
        if completion.get("status") != "success":
            record["issues"].append("workflow did not complete successfully")
        operators = {
            task["id"]: task.get("operator")
            for task in definition.get("workflow", {}).get("tasks", [])
            if "id" in task
        }
        traces = {
            path.resolve(): path
            for path in (artifact_dir / "workflows" / workflow.name).glob(
                "task/*/*/events.ndjson"
            )
        }
        for task in documents.get("checkpoint", {}).get("completed", {}).values():
            task_id, run_seq = task.get("task_id"), task.get("run_seq")
            if (
                not isinstance(task_id, str)
                or Path(task_id).name != task_id
                or type(run_seq) is not int
            ):
                record["issues"].append("invalid checkpoint task identity")
                continue
            params = task.get("resolved_params_snapshot") or {}
            output_ref = task.get("output_ref", {})
            output = (
                output_ref.get("value", {})
                if output_ref.get("type") == "inline"
                else {}
            )
            trace_path = (
                artifact_dir
                / "workflows"
                / workflow.name
                / "task"
                / task_id
                / str(run_seq)
                / "events.ndjson"
            )
            if params.get("engine") != "pi" and not trace_path.is_file():
                continue
            retained = (
                evidence_dir
                / "agent-traces"
                / workflow.name
                / task_id
                / str(run_seq)
                / "events.redacted.ndjson"
                if evidence_dir
                else None
            )
            trace = summarize_trace(trace_path, retained)
            traces.pop(trace_path.resolve(), None)
            reported = output.get("events_artifact")
            linked = bool(
                reported and (workspace / reported).resolve() == trace_path.resolve()
            )
            record["tasks"].append(
                {
                    "task_id": task_id,
                    "run_seq": run_seq,
                    "operator": operators.get(task_id),
                    "engine": params.get("engine"),
                    "model": params.get("model"),
                    "status": task.get("status"),
                    "error_code": (task.get("error") or {}).get("code"),
                    "exit_code": output.get("exit_code"),
                    "trace_linked_to_output": linked,
                    "trace": trace,
                    "verified_pi_execution": bool(
                        operators.get(task_id) == "AgentOperator"
                        and params.get("engine") == "pi"
                        and task.get("status") == "success"
                        and output.get("exit_code") == 0
                        and linked
                        and trace["verified_pi_activity"]
                    ),
                }
            )
        # A timeout can leave an event artifact without a completed task record.
        record["uncommitted_task_traces"] = [
            summarize_trace(
                path,
                evidence_dir
                / "agent-traces"
                / workflow.name
                / "incomplete"
                / f"{index}.ndjson"
                if evidence_dir
                else None,
            )
            for index, path in enumerate(traces.values())
        ]
    return records


def require_pi_execution(records, outcome, expected_model):
    """Reject label-only, stale, failed or unrelated evidence of development."""
    accepted = outcome.get("accepted_result") or {}
    candidate = accepted.get("candidate")
    evaluation = accepted.get("evaluation") or {}
    for workflow in records:
        if (
            workflow["run_id"] != outcome["run_id"]
            or workflow["role"] != "develop"
            or workflow["status"] != "Completed"
            or workflow["issues"]
            or not candidate
            or workflow["candidate"] != candidate
            or workflow["candidate_id"] != candidate["id"]
            or workflow["cycle"] != evaluation.get("cycle")
        ):
            continue
        for task in workflow["tasks"]:
            if task["verified_pi_execution"] and task["model"] == expected_model:
                return {
                    "workflow_id": workflow["workflow_id"],
                    "task_id": task["task_id"],
                    "run_seq": task["run_seq"],
                }
    raise RuntimeError(
        "no correlated successful Pi/aikit development task with tool and terminal SDK evidence; "
        "configured agent/model labels and a changed artifact are insufficient"
    )
