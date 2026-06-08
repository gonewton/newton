"""
Operator constructors — Layer 2 thin wrappers over Layer-1 param models.

Each constructor returns an OperatorCall (dict-like) with an `operator_type` key
that compile.py uses to emit the correct YAML operator name and params.
"""
from __future__ import annotations

from typing import Any

from .refs import Ref, Guard
from ._generated.output_schemas import OUTPUT_SCHEMAS as _OUTPUT_SCHEMAS


def _render_value(v: Any) -> Any:
    """Recursively render Ref/Guard values into $expr dicts."""
    if isinstance(v, (Ref, Guard)):
        return v.to_condition()
    if isinstance(v, dict):
        return {k: _render_value(val) for k, val in v.items()}
    if isinstance(v, list):
        return [_render_value(item) for item in v]
    return v


class OperatorCall:
    """
    Encapsulates an operator type and its params.
    Passed to wf.task(id, operator_call).
    """

    def __init__(self, operator_type: str, params: dict[str, Any]) -> None:
        self.operator_type = operator_type
        self.params = params

    def rendered_params(self) -> dict[str, Any]:
        return _render_value(self.params)

    # Generated from `newton schema export --outputs` — do not edit by hand.
    OUTPUT_SCHEMAS: dict[str, list[str]] = _OUTPUT_SCHEMAS

    def output_fields(self) -> list[str] | None:
        """Return known output field names or None if unknown."""
        return self.OUTPUT_SCHEMAS.get(self.operator_type)

    def __repr__(self) -> str:
        return f"OperatorCall({self.operator_type!r}, {self.params!r})"


def command(
    cmd: str,
    *,
    shell: bool = False,
    capture_stdout: bool = True,
    capture_stderr: bool = False,
    env: dict[str, Any] | None = None,
    cwd: str | None = None,
    write_stdout: str | None = None,
    write_stderr: str | None = None,
) -> OperatorCall:
    """CommandOperator constructor."""
    params: dict[str, Any] = {"cmd": cmd}
    if shell:
        params["shell"] = True
    if capture_stdout:
        params["capture_stdout"] = True
    if capture_stderr:
        params["capture_stderr"] = True
    if env:
        params["env"] = env
    if cwd is not None:
        params["cwd"] = cwd
    if write_stdout is not None:
        params["write_stdout"] = write_stdout
    if write_stderr is not None:
        params["write_stderr"] = write_stderr
    return OperatorCall("CommandOperator", params)


def agent(
    *,
    engine: Any = None,
    model: Any = None,
    prompt: Any = None,
    prompt_file: str | None = None,
    signals: dict[str, str] | None = None,
    require_signal: bool = False,
    stream_stdout: bool | None = None,
    context_fidelity: str | None = None,
) -> OperatorCall:
    """AgentOperator constructor."""
    params: dict[str, Any] = {}
    if engine is not None:
        params["engine"] = engine
    if model is not None:
        params["model"] = model
    if prompt is not None:
        params["prompt"] = prompt
    if prompt_file is not None:
        params["prompt_file"] = prompt_file
    if signals is not None:
        params["signals"] = signals
    if require_signal:
        params["require_signal"] = True
    if stream_stdout is not None:
        params["stream_stdout"] = stream_stdout
    if context_fidelity is not None:
        params["context_fidelity"] = context_fidelity
    return OperatorCall("AgentOperator", params)


def human_approval(
    prompt: str,
    *,
    timeout_seconds: int | None = None,
    default_on_timeout: str | None = None,
) -> OperatorCall:
    """HumanApprovalOperator — blocks until a human approves or rejects."""
    params: dict[str, Any] = {"prompt": prompt}
    if timeout_seconds is not None:
        params["timeout_seconds"] = timeout_seconds
    if default_on_timeout is not None:
        params["default_on_timeout"] = default_on_timeout
    return OperatorCall("HumanApprovalOperator", params)


def human_decision(
    *,
    options: list[dict[str, Any]] | None = None,
    prompt: str | None = None,
    choices: list[str] | None = None,
    timeout_seconds: int | None = None,
    default_choice: str | None = None,
) -> OperatorCall:
    """HumanDecisionOperator — prompts a human for a multi-option decision.

    Structured form: pass `options` (list of dicts with "label"/"description").
    Legacy form: pass `prompt` + `choices`.
    """
    params: dict[str, Any] = {}
    if options is not None:
        params["options"] = options
    if prompt is not None:
        params["prompt"] = prompt
    if choices is not None:
        params["choices"] = choices
    if timeout_seconds is not None:
        params["timeout_seconds"] = timeout_seconds
    if default_choice is not None:
        params["default_choice"] = default_choice
    return OperatorCall("HumanDecisionOperator", params)


def sub_workflow(
    workflow_path: Any,
    *,
    triggers: dict[str, Any] | None = None,
    context: dict[str, Any] | None = None,
) -> OperatorCall:
    """WorkflowOperator constructor."""
    params: dict[str, Any] = {"workflow_path": workflow_path}
    if triggers:
        params["triggers"] = triggers
    if context:
        params["context"] = context
    return OperatorCall("WorkflowOperator", params)


class gh:
    """GitHub operator sub-constructors — one per operation (ADR 0006)."""

    @staticmethod
    def pr_create(
        base: str,
        title: Any,
        body: str,
        *,
        retry_count: int | None = None,
        retry_delay_ms: int | None = None,
        draft: bool | None = None,
    ) -> OperatorCall:
        params: dict[str, Any] = {
            "operation": "pr_create",
            "base": base,
            "title": title,
            "body": body,
        }
        if retry_count is not None:
            params["retry_count"] = retry_count
        if retry_delay_ms is not None:
            params["retry_delay_ms"] = retry_delay_ms
        if draft is not None:
            params["draft"] = draft
        return OperatorCall("GhOperator", params)

    @staticmethod
    def pr_view(pr: Any) -> OperatorCall:
        return OperatorCall("GhOperator", {"operation": "pr_view", "pr": pr})

    @staticmethod
    def pr_approve(pr_number: Any) -> OperatorCall:
        return OperatorCall("GhOperator", {"operation": "pr_approve", "pr_number": pr_number})

    @staticmethod
    def project_resolve_board(
        owner: Any,
        project_number: Any,
        *,
        required_option_names: list[str] | None = None,
    ) -> OperatorCall:
        params: dict[str, Any] = {
            "operation": "project_resolve_board",
            "owner": owner,
            "project_number": project_number,
        }
        if required_option_names is not None:
            params["required_option_names"] = required_option_names
        return OperatorCall("GhOperator", params)

    @staticmethod
    def project_item_set_status(
        item_id: Any,
        board: Any,
        status: str,
        *,
        on_error: str | None = None,
    ) -> OperatorCall:
        params: dict[str, Any] = {
            "operation": "project_item_set_status",
            "item_id": item_id,
            "board": board,
            "status": status,
        }
        if on_error is not None:
            params["on_error"] = on_error
        return OperatorCall("GhOperator", params)


class git:
    """Git operator sub-constructors — one per operation."""

    @staticmethod
    def clean_check() -> OperatorCall:
        """Assert the working tree is clean (no untracked/modified files)."""
        return OperatorCall("GitOperator", {"operation": "clean_check"})

    @staticmethod
    def sync_main() -> OperatorCall:
        """Fetch and fast-forward the current branch from origin/main."""
        return OperatorCall("GitOperator", {"operation": "sync_main"})

    @staticmethod
    def create_branch(name: Any) -> OperatorCall:
        """Create and switch to a new branch."""
        return OperatorCall("GitOperator", {"operation": "create_branch", "name": name})

    @staticmethod
    def stage(*, exclude: list[str] | None = None) -> OperatorCall:
        """Stage all changes (git add -A), optionally excluding paths."""
        params: dict[str, Any] = {"operation": "stage"}
        if exclude:
            params["exclude"] = exclude
        return OperatorCall("GitOperator", params)

    @staticmethod
    def commit(message: Any, *, allow_empty: bool = False) -> OperatorCall:
        """Commit staged changes."""
        params: dict[str, Any] = {"operation": "commit", "message": message}
        if allow_empty:
            params["allow_empty"] = True
        return OperatorCall("GitOperator", params)

    @staticmethod
    def push(
        *,
        remote: str = "origin",
        force: bool = False,
        retry_count: int = 3,
        retry_delay_ms: int = 5000,
    ) -> OperatorCall:
        """Push the current branch to a remote."""
        return OperatorCall("GitOperator", {
            "operation": "push",
            "remote": remote,
            "force": force,
            "retry_count": retry_count,
            "retry_delay_ms": retry_delay_ms,
        })

    @staticmethod
    def diff(*, base: str = "main", max_bytes: int = 65536) -> OperatorCall:
        """Produce a unified diff between base and HEAD."""
        return OperatorCall("GitOperator", {
            "operation": "diff",
            "base": base,
            "max_bytes": max_bytes,
        })

    @staticmethod
    def cleanup_merge() -> OperatorCall:
        """Abort any in-progress merge/rebase/cherry-pick."""
        return OperatorCall("GitOperator", {"operation": "cleanup_merge"})
