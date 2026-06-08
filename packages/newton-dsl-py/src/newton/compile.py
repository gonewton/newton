"""
Compiler — builds the WorkflowDocument dict and serializes to YAML.

Flow:
  1. Run checks.py (dangling refs, reachability, bounded cycles)
  2. Build WorkflowDocument dict
  3. Serialize to YAML via PyYAML
"""
from __future__ import annotations

import warnings as _warnings
from typing import Any, TYPE_CHECKING

import yaml

from .checks import check_all, CompilerWarning

if TYPE_CHECKING:
    from .workflow import Workflow


def _remove_none(obj: Any) -> Any:
    """Recursively remove None values from dicts/lists."""
    if isinstance(obj, dict):
        return {k: _remove_none(v) for k, v in obj.items() if v is not None}
    if isinstance(obj, list):
        return [_remove_none(item) for item in obj]
    return obj


class _ExprDumper(yaml.Dumper):
    """
    Custom YAML dumper that:
    - Uses block style for multiline strings
    - Preserves $expr dicts as regular mappings
    - Emits None as empty string in specific contexts
    """
    pass


def _str_representer(dumper: yaml.Dumper, data: str) -> yaml.Node:
    if "\n" in data:
        return dumper.represent_scalar("tag:yaml.org,2002:str", data, style="|")
    return dumper.represent_scalar("tag:yaml.org,2002:str", data)


_ExprDumper.add_representer(str, _str_representer)


def compile_workflow(wf: "Workflow") -> str:
    """
    Compile a Workflow object into a YAML string.
    Raises CompilerError on fatal issues; prints CompilerWarnings to stderr.
    """
    # Run checks
    compiler_warnings = check_all(wf._tasks, wf._entry_task)
    for w in compiler_warnings:
        _warnings.warn(str(w), UserWarning, stacklevel=4)

    doc = _build_document(wf)
    doc = _remove_none(doc)
    return yaml.dump(
        doc,
        Dumper=_ExprDumper,
        default_flow_style=False,
        allow_unicode=True,
        sort_keys=False,
        width=120,
    )


def _build_document(wf: "Workflow") -> dict[str, Any]:
    """Build the raw WorkflowDocument dict."""
    doc: dict[str, Any] = {
        "version": "2.0",
        "mode": "workflow_graph",
    }

    # metadata
    if wf._metadata:
        doc["metadata"] = wf._metadata

    # triggers
    if wf._triggers:
        doc["triggers"] = wf._triggers

    # workflow
    settings = _build_settings(wf)
    tasks = [t.to_dict() for t in wf._task_list]

    doc["workflow"] = {
        "settings": settings,
        "context": wf._context if wf._context else {},
        "tasks": tasks,
    }

    return doc


def _build_settings(wf: "Workflow") -> dict[str, Any]:
    """Build the workflow settings dict from the Workflow object."""
    s: dict[str, Any] = {}

    if wf._entry_task:
        s["entry_task"] = wf._entry_task
    if wf._max_time_seconds is not None:
        s["max_time_seconds"] = wf._max_time_seconds
    if wf._parallel_limit is not None:
        s["parallel_limit"] = wf._parallel_limit
    if wf._continue_on_error is not None:
        s["continue_on_error"] = wf._continue_on_error
    if wf._max_task_iterations is not None:
        s["max_task_iterations"] = wf._max_task_iterations
    if wf._max_workflow_iterations is not None:
        s["max_workflow_iterations"] = wf._max_workflow_iterations
    if wf._default_engine is not None:
        s["default_engine"] = wf._default_engine
    if wf._command_operator_settings:
        s["command_operator"] = wf._command_operator_settings
    if wf._artifact_storage:
        s["artifact_storage"] = wf._artifact_storage

    return s
