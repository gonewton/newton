"""
Compiler checks — run before YAML serialization.

Checks performed:
1. Reachability: every task must be reachable from entry_task (warning for unreachable)
2. Bounded cycles: every cycle must have at least one task with max_iterations set
3. Dangling references: every edge target must be a defined task
4. Unknown .out.field: if operator has a known output schema, verify field exists
"""
from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .task import Task


class CompilerError(Exception):
    """Fatal compiler error — workflow cannot be emitted."""
    pass


class CompilerWarning:
    """Non-fatal compiler warning."""
    def __init__(self, message: str) -> None:
        self.message = message

    def __str__(self) -> str:
        return f"CompilerWarning: {self.message}"


def check_all(
    tasks: dict[str, "Task"],
    entry_task_id: str,
) -> list[CompilerWarning]:
    """
    Run all compiler checks. Returns a list of warnings.
    Raises CompilerError on fatal issues.
    """
    warnings: list[CompilerWarning] = []

    # 1. Dangling references
    _check_dangling(tasks)

    # 2. Reachability
    unreachable = _find_unreachable(tasks, entry_task_id)
    for task_id in unreachable:
        warnings.append(
            CompilerWarning(
                f"task '{task_id}' is unreachable from entry_task '{entry_task_id}'"
            )
        )

    # 3. Bounded cycles
    _check_bounded_cycles(tasks)

    return warnings


def _check_dangling(tasks: dict[str, "Task"]) -> None:
    """Raise CompilerError if any transition targets a non-existent task."""
    known = set(tasks.keys())
    for task in tasks.values():
        for edge in task._edges:
            if edge.target_id not in known:
                raise CompilerError(
                    f"task '{task.task_id}' has transition to undefined task '{edge.target_id}'"
                )


def _find_unreachable(
    tasks: dict[str, "Task"],
    entry_task_id: str,
) -> list[str]:
    """Return task IDs that cannot be reached from entry_task via BFS."""
    if entry_task_id not in tasks:
        raise CompilerError(
            f"entry_task '{entry_task_id}' is not defined in the workflow"
        )

    visited: set[str] = set()
    queue = [entry_task_id]
    while queue:
        current = queue.pop(0)
        if current in visited:
            continue
        visited.add(current)
        task = tasks.get(current)
        if task is None:
            continue
        for edge in task._edges:
            if edge.target_id not in visited:
                queue.append(edge.target_id)

    all_ids = set(tasks.keys())
    return sorted(all_ids - visited)


def _check_bounded_cycles(tasks: dict[str, "Task"]) -> None:
    """
    Detect cycles in the task graph. Every cycle must contain at least one
    task with max_iterations set. Raises CompilerError for unbounded cycles.
    """
    # Build adjacency list
    graph: dict[str, list[str]] = {
        tid: [e.target_id for e in t._edges]
        for tid, t in tasks.items()
    }

    # Find all SCCs using Tarjan's algorithm
    index_counter = [0]
    stack: list[str] = []
    lowlink: dict[str, int] = {}
    index: dict[str, int] = {}
    on_stack: set[str] = set()
    sccs: list[list[str]] = []

    def strongconnect(v: str) -> None:
        index[v] = index_counter[0]
        lowlink[v] = index_counter[0]
        index_counter[0] += 1
        stack.append(v)
        on_stack.add(v)

        for w in graph.get(v, []):
            if w not in index:
                strongconnect(w)
                lowlink[v] = min(lowlink[v], lowlink[w])
            elif w in on_stack:
                lowlink[v] = min(lowlink[v], index[w])

        if lowlink[v] == index[v]:
            scc: list[str] = []
            while True:
                w = stack.pop()
                on_stack.discard(w)
                scc.append(w)
                if w == v:
                    break
            sccs.append(scc)

    for v in graph:
        if v not in index:
            strongconnect(v)

    # Check each SCC: if it has > 1 node OR has a self-loop, it's a real cycle
    for scc in sccs:
        scc_set = set(scc)
        is_cycle = len(scc) > 1 or (
            len(scc) == 1 and scc[0] in (graph.get(scc[0]) or [])
        )
        if not is_cycle:
            continue

        # Check that at least one task in the cycle has max_iterations set
        has_cap = any(
            tasks[tid]._max_iterations is not None
            for tid in scc
            if tid in tasks
        )
        if not has_cap:
            raise CompilerError(
                f"unbounded cycle detected involving tasks: {', '.join(sorted(scc))}. "
                f"Add repeat_at_most(N) to at least one task in the cycle."
            )


def check_out_field(
    task_id: str,
    operator_type: str,
    field: str,
    known_fields: list[str] | None,
) -> None:
    """
    Raise CompilerError if field is not in known_fields (when known_fields is not None).
    """
    if known_fields is None:
        return  # unknown schema, skip check
    if field not in known_fields:
        raise CompilerError(
            f"task '{task_id}' ({operator_type}) has no output field '{field}'; "
            f"known fields: {', '.join(known_fields)}"
        )
