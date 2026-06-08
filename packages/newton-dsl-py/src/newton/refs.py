"""
Typed reference helpers that render to Rhai $expr values in the compiled YAML.

- t.out.field       -> OutRef("tasks.<id>.output.<field>")
- wf.input.x        -> InputRef("x")      renders as {"$expr": "triggers.x"}
- wf.context.x      -> ContextRef("x")   renders as {"$expr": "context.x"}
- wf.var.x          -> ContextRef("x")   (alias for context)
- wf.env("VAR")     -> EnvRef("VAR")     renders as {"$expr": 'env("VAR")'}
- ref == value      -> Guard (Rhai comparison expression)
"""
from __future__ import annotations


class Ref:
    """Base class for all typed references."""

    def rhai_expr(self) -> str:
        raise NotImplementedError

    def to_condition(self) -> dict:
        return {"$expr": self.rhai_expr()}

    def __eq__(self, other) -> "Guard":  # type: ignore[override]
        return Guard(f'{self.rhai_expr()} == {_rhai_literal(other)}')

    def __ne__(self, other) -> "Guard":  # type: ignore[override]
        return Guard(f'{self.rhai_expr()} != {_rhai_literal(other)}')

    def __repr__(self) -> str:
        return f"{self.__class__.__name__}({self.rhai_expr()!r})"


def _rhai_literal(value) -> str:
    if isinstance(value, str):
        return f'"{value}"'
    if isinstance(value, bool):
        return "true" if value else "false"
    if value is None:
        return "null"
    return str(value)


class Guard:
    """A boolean-valued Rhai expression used as a transition condition."""

    def __init__(self, expr: str) -> None:
        self._expr = expr

    def rhai_expr(self) -> str:
        return self._expr

    def to_condition(self) -> dict:
        return {"$expr": self._expr}

    def __and__(self, other: "Guard") -> "Guard":
        return Guard(f"({self._expr}) && ({other._expr})")

    def __or__(self, other: "Guard") -> "Guard":
        return Guard(f"({self._expr}) || ({other._expr})")

    def __repr__(self) -> str:
        return f"Guard({self._expr!r})"


class OutRef(Ref):
    """Reference to a task output field: tasks.<id>.output.<field>"""

    def __init__(self, task_id: str, field: str) -> None:
        self._task_id = task_id
        self._field = field

    def rhai_expr(self) -> str:
        return f"tasks.{self._task_id}.output.{self._field}"

    def __getattr__(self, name: str) -> "OutRef":
        # Allow chaining like t.out.nested.field (unusual but handled gracefully)
        if name.startswith("_"):
            raise AttributeError(name)
        return OutRef(self._task_id, f"{self._field}.{name}")


class TaskOutputRef(Ref):
    """
    Reference to a task's entire output object: tasks.<id>.output
    Use this when you need to pass the whole output to another operator
    (e.g., board: tasks.resolve_board_ids.output).
    """

    def __init__(self, task_id: str) -> None:
        self._task_id = task_id

    def rhai_expr(self) -> str:
        return f"tasks.{self._task_id}.output"

    def __getattr__(self, name: str) -> OutRef:
        if name.startswith("_"):
            raise AttributeError(name)
        return OutRef(self._task_id, name)


class _OutAccessor:
    """
    Proxy returned by Task.out — defers field lookup until attribute access.
    Calling t.out itself gives a TaskOutputRef (the whole output).
    t.out.field gives OutRef for a specific field.
    """

    def __init__(
        self,
        task_id: str,
        known_fields: list[str] | None = None,
        operator_type: str | None = None,
    ) -> None:
        self._task_id = task_id
        self._known_fields = known_fields  # None means unknown schema
        self._operator_type = operator_type

    def __call__(self) -> TaskOutputRef:
        """t.out() -> TaskOutputRef for the whole output object."""
        return TaskOutputRef(self._task_id)

    def __getattr__(self, name: str) -> OutRef:
        if name.startswith("_"):
            raise AttributeError(name)
        if self._known_fields is not None and name not in self._known_fields:
            from .checks import check_out_field
            check_out_field(
                self._task_id,
                self._operator_type or "unknown",
                name,
                self._known_fields,
            )
        return OutRef(self._task_id, name)

    def to_condition(self) -> dict:
        """Allow t.out to be used directly as a value (whole output)."""
        return TaskOutputRef(self._task_id).to_condition()


class InputRef(Ref):
    """Reference to a workflow trigger payload field: triggers.<name>"""

    def __init__(self, name: str) -> None:
        self._name = name

    def rhai_expr(self) -> str:
        return f"triggers.{self._name}"


class _InputAccessor:
    """Proxy for wf.input.<name>"""

    def __getattr__(self, name: str) -> InputRef:
        if name.startswith("_"):
            raise AttributeError(name)
        return InputRef(name)


class ContextRef(Ref):
    """Reference to a workflow context variable: context.<name>"""

    def __init__(self, name: str) -> None:
        self._name = name

    def rhai_expr(self) -> str:
        return f"context.{self._name}"


class _ContextAccessor:
    """Proxy for wf.context.<name> / wf.var.<name>"""

    def __getattr__(self, name: str) -> ContextRef:
        if name.startswith("_"):
            raise AttributeError(name)
        return ContextRef(name)


class EnvRef(Ref):
    """Reference to an environment variable: env("VAR")"""

    def __init__(self, var_name: str) -> None:
        self._var_name = var_name

    def rhai_expr(self) -> str:
        return f'env("{self._var_name}")'


class AmbientRef(Ref):
    """Reference to an injected ambient variable (declared via wf.expects)."""

    def __init__(self, name: str) -> None:
        self._name = name

    def rhai_expr(self) -> str:
        return self._name


def when(guard: Guard | Ref | str) -> Guard:
    """
    Guard helper — wraps a Guard or Ref into a Guard.
    Usage: task.then(target, when=when(ref == "value"))
    """
    if isinstance(guard, Guard):
        return guard
    if isinstance(guard, Ref):
        return Guard(guard.rhai_expr())
    # Raw string passthrough
    return Guard(str(guard))


def expr(raw: str) -> Guard:
    """Opaque passthrough — wraps a raw Rhai expression string as a Guard."""
    return Guard(raw)
