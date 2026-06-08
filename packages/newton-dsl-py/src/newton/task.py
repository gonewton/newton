"""
Task handle returned from wf.task(...).

Supports chaining: task.then(...).then(...).retry(...)
"""
from __future__ import annotations

from typing import TYPE_CHECKING, Any

from .edges import EdgeSpec, PRIORITY_STEP
from .refs import Guard, Ref, _OutAccessor, TaskOutputRef
from .operators import OperatorCall

if TYPE_CHECKING:
    pass


class Task:
    """
    Handle for a workflow task, returned by Workflow.task().
    Stores transitions and configuration that compile.py will serialize.
    """

    def __init__(
        self,
        task_id: str,
        operator_call: OperatorCall,
        *,
        name: str | None = None,
    ) -> None:
        self.task_id = task_id
        self.operator_call = operator_call
        self.name = name
        self._edges: list[EdgeSpec] = []
        self._next_priority: int = 0
        self._retry: dict[str, Any] | None = None
        self._max_iterations: int | None = None
        self._timeout_ms: int | None = None
        self._terminal: str | None = None  # "success" | "failure"
        self._is_terminal: bool = False
        self._message: str | None = None  # for finish/fail tasks

    # ------------------------------------------------------------------
    # Transition wiring
    # ------------------------------------------------------------------

    def then(
        self,
        target: "Task",
        *,
        when: Guard | Ref | str | None = None,
        label: str | None = None,
    ) -> "Task":
        """
        Append a conditional or unconditional transition to `target`.
        Priority is determined by call order (first call = highest priority).
        Returns `self` for fluent chaining.
        """
        from .refs import Guard as _Guard, Ref as _Ref

        guard: _Guard | None = None
        if when is not None:
            if isinstance(when, _Guard):
                guard = when
            elif isinstance(when, _Ref):
                guard = _Guard(when.rhai_expr())
            else:
                guard = _Guard(str(when))

        edge = EdgeSpec(
            target.task_id,
            priority=self._next_priority,
            guard=guard,
            label=label,
        )
        self._edges.append(edge)
        self._next_priority += PRIORITY_STEP
        return self

    def otherwise(self, target: "Task", *, label: str | None = None) -> "Task":
        """
        Append an unconditional fallback transition (lowest priority).
        Returns `self` for fluent chaining.
        """
        # otherwise uses highest declared priority so far + PRIORITY_STEP, no guard
        edge = EdgeSpec(
            target.task_id,
            priority=self._next_priority,
            guard=None,
            label=label,
        )
        self._edges.append(edge)
        self._next_priority += PRIORITY_STEP
        return self

    # ------------------------------------------------------------------
    # Task configuration
    # ------------------------------------------------------------------

    def retry(
        self,
        times: int,
        *,
        wait_seconds: int = 0,
        multiplier: float | None = None,
        jitter_seconds: int | None = None,
    ) -> "Task":
        """Configure retry policy for this task."""
        r: dict[str, Any] = {
            "max_attempts": times,
            "backoff_ms": wait_seconds * 1000,
        }
        if multiplier is not None:
            r["backoff_multiplier"] = multiplier
        if jitter_seconds is not None:
            r["jitter_ms"] = jitter_seconds * 1000
        self._retry = r
        return self

    def repeat_at_most(self, n: int) -> "Task":
        """Set the max_iterations cap (per-task re-entry cap)."""
        self._max_iterations = n
        return self

    def timeout(self, seconds: int) -> "Task":
        """Set task timeout in seconds (stored as timeout_ms)."""
        self._timeout_ms = seconds * 1000
        return self

    # ------------------------------------------------------------------
    # Output reference
    # ------------------------------------------------------------------

    @property
    def out(self) -> _OutAccessor:
        """
        Returns a proxy that turns attribute access into OutRef objects.
        t.out.stdout -> OutRef("tasks.<id>.output.stdout")
        t.out itself renders as TaskOutputRef("tasks.<id>.output") (the whole output)
        """
        known = self.operator_call.output_fields()
        return _OutAccessor(self.task_id, known_fields=known)

    @property
    def output(self) -> TaskOutputRef:
        """
        Returns a reference to the entire task output object.
        t.output -> {"$expr": "tasks.<id>.output"}
        Use when passing the whole output to another operator (e.g., board= param).
        """
        return TaskOutputRef(self.task_id)

    # ------------------------------------------------------------------
    # Serialization helpers
    # ------------------------------------------------------------------

    def to_dict(self) -> dict[str, Any]:
        """Build the task dict for inclusion in the workflow YAML."""
        d: dict[str, Any] = {
            "id": self.task_id,
            "operator": self.operator_call.operator_type,
        }
        if self.name is not None:
            d["name"] = self.name
        if self._max_iterations is not None:
            d["max_iterations"] = self._max_iterations
        if self._timeout_ms is not None:
            d["timeout_ms"] = self._timeout_ms
        if self._retry is not None:
            d["retry"] = self._retry
        if self._terminal is not None:
            d["terminal"] = self._terminal

        params = self.operator_call.rendered_params()
        d["params"] = params

        d["transitions"] = [edge.to_dict() for edge in self._edges]
        return d

    def __repr__(self) -> str:
        return f"Task({self.task_id!r})"
