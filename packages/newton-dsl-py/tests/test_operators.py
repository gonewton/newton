"""Tests for the operator builders added in spec 074 P8:
barrier, set_context, noop, grader_command, reconcile, change_request, grader_agent.
"""
from newton.operators import (
    barrier,
    set_context,
    noop,
    grader_command,
    reconcile,
    change_request,
    grader_agent,
)


def test_barrier_defaults_to_no_params():
    call = barrier()
    assert call.operator_type == "barrier"
    assert call.params == {}


def test_barrier_includes_expected_task_ids():
    call = barrier(expected=["t1", "t2"])
    assert call.operator_type == "barrier"
    assert call.params == {"expected": ["t1", "t2"]}


def test_set_context_produces_patch():
    call = set_context({"foo": "bar"})
    assert call.operator_type == "SetContextOperator"
    assert call.params == {"patch": {"foo": "bar"}}


def test_noop_has_no_params():
    call = noop()
    assert call.operator_type == "NoOpOperator"
    assert call.params == {}


def test_grader_command_required_params():
    call = grader_command("./grade.sh", "test-coverage-grader", "module", "mod-001")
    assert call.operator_type == "GraderCommandOperator"
    assert call.params == {
        "cmd": "./grade.sh",
        "grader": "test-coverage-grader",
        "scope": "module",
        "scope_id": "mod-001",
    }


def test_grader_command_optional_params():
    call = grader_command(
        "./grade.sh",
        "test-coverage-grader",
        "module",
        "mod-001",
        shell="zsh",
        cwd="sub/dir",
        timeout_seconds=30,
        env={"FOO": "bar"},
        state={"key": "value"},
    )
    assert call.params == {
        "cmd": "./grade.sh",
        "grader": "test-coverage-grader",
        "scope": "module",
        "scope_id": "mod-001",
        "shell": "zsh",
        "cwd": "sub/dir",
        "timeout_seconds": 30,
        "env": {"FOO": "bar"},
        "state": {"key": "value"},
    }


def test_reconcile_required_params():
    call = reconcile("module", "mod-001", {"overall_score": 90})
    assert call.operator_type == "ReconcileOperator"
    assert call.params == {
        "scope": "module",
        "scope_id": "mod-001",
        "assessment": {"overall_score": 90},
    }


def test_reconcile_optional_params():
    call = reconcile(
        "module",
        "mod-001",
        {"overall_score": 90},
        grader="test-grader",
        engine="codex",
        model="gpt-5",
        adjudication_timeout_seconds=45,
    )
    assert call.params == {
        "scope": "module",
        "scope_id": "mod-001",
        "assessment": {"overall_score": 90},
        "grader": "test-grader",
        "engine": "codex",
        "model": "gpt-5",
        "adjudication_timeout_seconds": 45,
    }


def test_change_request_required_params():
    call = change_request("module", "mod-001")
    assert call.operator_type == "ChangeRequestOperator"
    assert call.params == {"scope": "module", "scope_id": "mod-001"}


def test_change_request_optional_params():
    call = change_request(
        "module",
        "mod-001",
        max_findings=5,
        min_severity="high",
        engine="codex",
        model="gpt-5",
        synthesis_timeout_seconds=30,
    )
    assert call.params == {
        "scope": "module",
        "scope_id": "mod-001",
        "max_findings": 5,
        "min_severity": "high",
        "engine": "codex",
        "model": "gpt-5",
        "synthesis_timeout_seconds": 30,
    }


def test_grader_agent_required_params():
    call = grader_agent(
        "docs-quality-grader", "repo", "repo-001", "Grade the docs for clarity."
    )
    assert call.operator_type == "GraderAgentOperator"
    assert call.params == {
        "grader": "docs-quality-grader",
        "scope": "repo",
        "scope_id": "repo-001",
        "rubric": "Grade the docs for clarity.",
    }


def test_grader_agent_optional_params():
    call = grader_agent(
        "docs-quality-grader",
        "repo",
        "repo-001",
        "Grade the docs for clarity.",
        model="gpt-5",
        engine="codex",
        timeout_seconds=90,
    )
    assert call.params == {
        "grader": "docs-quality-grader",
        "scope": "repo",
        "scope_id": "repo-001",
        "rubric": "Grade the docs for clarity.",
        "model": "gpt-5",
        "engine": "codex",
        "timeout_seconds": 90,
    }
