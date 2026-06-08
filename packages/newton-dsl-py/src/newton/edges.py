"""
Edge / transition helpers.

Transitions are added via Task.then() and Task.otherwise().
Priority is determined by declaration order:
  - First .then() call gets priority 0
  - Next gets priority 5, 10, 15, ...
  - .otherwise() always gets lowest priority (no `when` condition)
"""
from __future__ import annotations

from typing import TYPE_CHECKING, Any

from .refs import Guard, Ref, when as _when

if TYPE_CHECKING:
    pass


PRIORITY_STEP = 5


class EdgeSpec:
    """
    Internal representation of a single transition edge before compilation.
    """

    def __init__(
        self,
        target_id: str,
        *,
        priority: int,
        guard: Guard | None = None,
        label: str | None = None,
    ) -> None:
        self.target_id = target_id
        self.priority = priority
        self.guard = guard
        self.label = label

    def to_dict(self) -> dict[str, Any]:
        d: dict[str, Any] = {"to": self.target_id}
        if self.priority != 100:
            d["priority"] = self.priority
        if self.guard is not None:
            d["when"] = self.guard.to_condition()
        if self.label is not None:
            d["label"] = self.label
        return d
