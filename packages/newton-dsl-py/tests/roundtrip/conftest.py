"""
Shared test utilities for round-trip tests.
"""
from __future__ import annotations

import os
import subprocess
import tempfile
from pathlib import Path
from typing import Any

import pytest
import yaml


# Path to the newton binary
# tests/roundtrip/conftest.py -> tests/roundtrip -> tests -> newton-dsl-py -> packages -> repo_root
NEWTON_BIN = Path(__file__).parents[4] / "target" / "debug" / "newton"
# Path to conformance fixtures
# tests/roundtrip -> tests -> newton-dsl-py -> packages -> workflow-schema
CONFORMANCE_DIR = Path(__file__).parents[3] / "workflow-schema" / "conformance"


def normalize_yaml(doc: Any) -> Any:
    """
    Normalize a parsed YAML document for semantic comparison:
    - Sort keys recursively
    - Normalize None vs empty dict/list
    - Remove fields that are non-semantic (e.g., null values)
    """
    if isinstance(doc, dict):
        result = {}
        for k, v in sorted(doc.items()):
            nv = normalize_yaml(v)
            # Skip None values
            if nv is None:
                continue
            result[k] = nv
        return result
    if isinstance(doc, list):
        return [normalize_yaml(item) for item in doc]
    return doc


def semantic_equal(a: Any, b: Any) -> bool:
    """Check semantic equality between two parsed YAML documents."""
    return normalize_yaml(a) == normalize_yaml(b)


def validate_with_newton(yaml_content: str) -> tuple[bool, str]:
    """
    Write yaml_content to a temp file and run `newton workflow validate`.
    Returns (success, output).
    """
    if not NEWTON_BIN.exists():
        pytest.skip(f"newton binary not found at {NEWTON_BIN}")

    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".yaml", delete=False
    ) as f:
        f.write(yaml_content)
        tmp_path = f.name

    try:
        result = subprocess.run(
            [str(NEWTON_BIN), "workflow", "validate", tmp_path],
            capture_output=True,
            text=True,
            timeout=30,
        )
        output = result.stdout + result.stderr
        return result.returncode == 0, output
    except subprocess.TimeoutExpired:
        return False, "timeout"
    finally:
        os.unlink(tmp_path)


def load_fixture(case_name: str) -> Any:
    """Load the expected.yaml fixture for a conformance case."""
    fixture_path = CONFORMANCE_DIR / "cases" / case_name / "expected.yaml"
    with open(fixture_path) as f:
        return yaml.safe_load(f)


def normalize_task(t: dict) -> dict:
    """
    Normalize a task dict for semantic comparison:
    - Drop priority from transitions (declaration order determines semantics)
    - Drop transitions: [] if empty (same as absent)
    - Treat capture_stdout=False and absent capture_stdout as equivalent
    - Sort params keys
    """
    t = dict(t)

    # Normalize transitions: strip priority, keep order + conditions
    transitions = t.get("transitions") or []
    normalized_transitions = []
    for tr in transitions:
        nt = {"to": tr["to"]}
        if "when" in tr:
            nt["when"] = tr["when"]
        if "label" in tr:
            nt["label"] = tr["label"]
        normalized_transitions.append(nt)
    if normalized_transitions:
        t["transitions"] = normalized_transitions
    else:
        t.pop("transitions", None)

    # Normalize params: drop None, sort
    if "params" in t:
        params = t["params"]
        if isinstance(params, dict):
            # Remove capture_stdout: False (treat as absent)
            if params.get("capture_stdout") is False:
                params = {k: v for k, v in params.items() if k != "capture_stdout"}
            # Remove capture_stderr: False (treat as absent)
            if params.get("capture_stderr") is False:
                params = {k: v for k, v in params.items() if k != "capture_stderr"}
        t["params"] = normalize_yaml(params)

    # Remove None values
    return {k: v for k, v in t.items() if v is not None}


def tasks_semantic_equal(
    compiled: list[dict],
    expected: list[dict],
    *,
    skip_task_ids: set[str] | None = None,
) -> tuple[bool, str]:
    """
    Compare task lists semantically, optionally skipping certain task IDs.
    Returns (equal, diff_description).
    """
    if skip_task_ids:
        compiled = [t for t in compiled if t.get("id") not in skip_task_ids]
        expected = [t for t in expected if t.get("id") not in skip_task_ids]

    comp_map = {t["id"]: normalize_task(t) for t in compiled}
    exp_map = {t["id"]: normalize_task(t) for t in expected}

    missing = set(exp_map) - set(comp_map)
    extra = set(comp_map) - set(exp_map)
    diffs = []

    if missing:
        diffs.append(f"Missing tasks: {missing}")
    if extra:
        diffs.append(f"Extra tasks: {extra}")

    for tid in set(comp_map) & set(exp_map):
        if comp_map[tid] != exp_map[tid]:
            diffs.append(f"Task '{tid}' differs:\n  compiled={comp_map[tid]}\n  expected={exp_map[tid]}")

    if diffs:
        return False, "\n".join(diffs)
    return True, ""
