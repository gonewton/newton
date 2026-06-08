"""Tests for .out.field validation in _OutAccessor."""
import pytest
from newton.checks import CompilerError
from newton.operators import OperatorCall, command
from newton.refs import _OutAccessor
from newton.workflow import Workflow


def _wf():
    return Workflow("test", default_engine="codex")


def test_unknown_field_raises_compiler_error():
    wf = _wf()
    t = wf.task("cmd", command(cmd="echo"))
    with pytest.raises(CompilerError, match="no output field 'badfield'"):
        _ = t.out.badfield


def test_known_field_returns_out_ref():
    wf = _wf()
    t = wf.task("cmd", command(cmd="echo"))
    ref = t.out.stdout
    assert ref.rhai_expr() == "tasks.cmd.output.stdout"


def test_error_message_includes_known_fields():
    wf = _wf()
    t = wf.task("cmd", command(cmd="echo"))
    with pytest.raises(CompilerError, match="stdout"):
        _ = t.out.nonexistent


def test_unknown_schema_operator_allows_any_field():
    acc = _OutAccessor("task1", known_fields=None, operator_type="CustomOperator")
    ref = acc.anyField
    assert ref.rhai_expr() == "tasks.task1.output.anyField"


def test_operator_type_appears_in_error():
    wf = _wf()
    t = wf.task("cmd", command(cmd="echo"))
    with pytest.raises(CompilerError, match="CommandOperator"):
        _ = t.out.nosuchfield


def test_all_command_operator_fields_valid():
    wf = _wf()
    t = wf.task("cmd", command(cmd="echo"))
    for field in ["stdout", "stderr", "exit_code", "success", "duration_ms"]:
        ref = getattr(t.out, field)
        assert f"output.{field}" in ref.rhai_expr()
