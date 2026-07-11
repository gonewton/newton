"""
Tests for the NEWTON_REQUIRE_BINARY hard-fail gate and NEWTON_BIN path
resolution in conftest.py (spec 074 P15).

These monkeypatch the module-level NEWTON_BIN / env vars rather than
relying on the real newton binary being absent (or present) in the
developer's own environment, so they're deterministic either way.
"""
from __future__ import annotations

import pytest

from . import conftest as rt_conftest
from .conftest import validate_with_newton


@pytest.fixture(autouse=True)
def _missing_binary_path(monkeypatch):
    """
    Point NEWTON_BIN at a path that is guaranteed not to exist, and isolate
    the module-level skip counter so these intentional skip/fail exercises
    don't pollute the real pytest_terminal_summary banner for the rest of
    the suite.
    """
    monkeypatch.setattr(
        rt_conftest, "NEWTON_BIN", rt_conftest.Path("/nonexistent/newton-binary-for-test")
    )
    monkeypatch.setattr(rt_conftest, "_skipped_missing_binary_count", 0)


def test_require_binary_hard_fails_when_binary_missing(monkeypatch):
    """NEWTON_REQUIRE_BINARY=1 + missing binary must FAIL, not skip."""
    monkeypatch.setenv("NEWTON_REQUIRE_BINARY", "1")

    with pytest.raises(pytest.fail.Exception, match="NEWTON_REQUIRE_BINARY is set"):
        validate_with_newton("name: test\n")


def test_missing_binary_without_require_flag_still_skips(monkeypatch):
    """Without NEWTON_REQUIRE_BINARY set, missing binary keeps skipping."""
    monkeypatch.delenv("NEWTON_REQUIRE_BINARY", raising=False)

    with pytest.raises(pytest.skip.Exception, match="newton binary not found"):
        validate_with_newton("name: test\n")


@pytest.mark.parametrize(
    "value,expect_fail",
    [
        ("1", True),
        ("true", True),
        ("yes", True),
        ("0", False),
        ("", False),
    ],
)
def test_require_binary_truthy_values(monkeypatch, value, expect_fail):
    monkeypatch.setenv("NEWTON_REQUIRE_BINARY", value)

    if expect_fail:
        with pytest.raises(pytest.fail.Exception):
            validate_with_newton("name: test\n")
    else:
        with pytest.raises(pytest.skip.Exception):
            validate_with_newton("name: test\n")


def test_require_binary_unset_skips(monkeypatch):
    monkeypatch.delenv("NEWTON_REQUIRE_BINARY", raising=False)
    with pytest.raises(pytest.skip.Exception):
        validate_with_newton("name: test\n")


class TestResolveNewtonBin:
    """CARGO_TARGET_DIR must be honored so run-tests.sh's override (and any
    developer's custom target dir) resolves to the actual build output."""

    def test_respects_cargo_target_dir(self, monkeypatch):
        monkeypatch.setenv("CARGO_TARGET_DIR", "/tmp/some-target-dir")
        resolved = rt_conftest._resolve_newton_bin()
        assert resolved == rt_conftest.Path("/tmp/some-target-dir") / "debug" / "newton"

    def test_defaults_to_repo_target_debug_without_cargo_target_dir(self, monkeypatch):
        monkeypatch.delenv("CARGO_TARGET_DIR", raising=False)
        resolved = rt_conftest._resolve_newton_bin()
        assert resolved.parts[-3:] == ("target", "debug", "newton")
