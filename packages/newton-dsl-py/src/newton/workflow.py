"""
Workflow class — top-level builder for a workflow graph.

Usage:
    wf = Workflow("my-workflow", default_engine="codex")
    t1 = wf.task("step1", command("echo hello", shell=True))
    t2 = wf.finish("done")
    t1.then(t2)
    print(wf.to_yaml())
"""
from __future__ import annotations

from typing import Any

from .task import Task
from .operators import OperatorCall, command as _command
from .refs import (
    _InputAccessor,
    _ContextAccessor,
    EnvRef,
    AmbientRef,
)
from .compile import compile_workflow


class Workflow:
    """
    Workflow graph builder.

    Parameters
    ----------
    name : str
        Workflow name (used in metadata.name).
    default_engine : str | None
        Default coding engine for AgentOperator tasks.
    parallel_limit : int
        Max concurrent tasks (default 1).
    max_time_seconds : int
        Global workflow timeout in seconds.
    entry_task : str | None
        ID of the entry task (if not provided, the first task added is used).
    max_task_iterations : int | None
        Default max iterations per task.
    max_workflow_iterations : int | None
        Global max iterations for the whole workflow.
    continue_on_error : bool
        Whether to continue on error (default False).
    description : str | None
        Workflow description for metadata.
    allow_shell : bool
        Whether to allow shell commands (default False, set to True if using shell=True in command()).
    """

    def __init__(
        self,
        name: str,
        *,
        default_engine: str | None = None,
        parallel_limit: int = 1,
        max_time_seconds: int = 3600,
        entry_task: str | None = None,
        max_task_iterations: int | None = None,
        max_workflow_iterations: int | None = None,
        continue_on_error: bool = False,
        description: str | None = None,
        allow_shell: bool = False,
        artifact_storage: dict[str, Any] | None = None,
    ) -> None:
        self._name = name
        self._default_engine = default_engine
        self._parallel_limit = parallel_limit
        self._max_time_seconds = max_time_seconds
        self._entry_task: str | None = entry_task
        self._max_task_iterations = max_task_iterations
        self._max_workflow_iterations = max_workflow_iterations
        self._continue_on_error = continue_on_error
        self._description = description
        self._allow_shell = allow_shell
        self._artifact_storage = artifact_storage

        self._tasks: dict[str, Task] = {}
        self._task_list: list[Task] = []  # preserve insertion order
        self._context: dict[str, Any] = {}
        self._trigger_payload: dict[str, Any] = {}
        self._expected_vars: list[str] = []

        # Computed settings fields
        self._command_operator_settings: dict[str, Any] | None = (
            {"allow_shell": True} if allow_shell else None
        )

        # Metadata
        self._metadata: dict[str, Any] = {"name": name}
        if description:
            self._metadata["description"] = description

        # Triggers
        self._triggers: dict[str, Any] = {
            "type": "manual",
            "schema_version": "1.0",
            "payload": {},
        }

    # ------------------------------------------------------------------
    # Configuration
    # ------------------------------------------------------------------

    def inputs(self, **defaults: Any) -> "Workflow":
        """
        Declare workflow trigger payload fields with defaults.
        Example: wf.inputs(prompt="", output_path="")
        """
        self._trigger_payload.update(defaults)
        self._triggers["payload"] = self._trigger_payload
        return self

    def expects(self, *names: str) -> "Workflow":
        """
        Declare injected ambient variables (from WorkflowOperator context or --set).
        These become typed references accessible via wf.var.<name>.
        """
        self._expected_vars.extend(names)
        return self

    def set_context(self, **kwargs: Any) -> "Workflow":
        """Set workflow-level context variables."""
        self._context.update(kwargs)
        return self

    def allow_shell(self, value: bool = True) -> "Workflow":
        """Enable/disable shell commands."""
        self._allow_shell = value
        self._command_operator_settings = {"allow_shell": True} if value else None
        return self

    # ------------------------------------------------------------------
    # Typed reference factories
    # ------------------------------------------------------------------

    @property
    def input(self) -> _InputAccessor:
        """wf.input.x -> InputRef("x") -> {"$expr": "triggers.x"}"""
        return _InputAccessor()

    @property
    def var(self) -> _ContextAccessor:
        """wf.var.x -> ContextRef("x") -> {"$expr": "context.x"}"""
        return _ContextAccessor()

    @property
    def context(self) -> _ContextAccessor:
        """wf.context.x -> ContextRef("x") -> {"$expr": "context.x"}"""
        return _ContextAccessor()

    def env(self, name: str) -> EnvRef:
        """wf.env("VAR") -> EnvRef -> {"$expr": 'env("VAR")'}"""
        return EnvRef(name)

    def ambient(self, name: str) -> AmbientRef:
        """wf.ambient("var_name") -> AmbientRef -> {"$expr": "var_name"}"""
        return AmbientRef(name)

    # ------------------------------------------------------------------
    # Task registration
    # ------------------------------------------------------------------

    def task(
        self,
        task_id: str,
        operator_call: OperatorCall,
        *,
        name: str | None = None,
    ) -> Task:
        """
        Register a task in this workflow.
        Returns a Task handle for wiring transitions.
        """
        if task_id in self._tasks:
            raise ValueError(f"Task '{task_id}' already registered in workflow '{self._name}'")

        t = Task(task_id, operator_call, name=name)
        self._tasks[task_id] = t
        self._task_list.append(t)

        # First task becomes entry if not set
        if self._entry_task is None:
            self._entry_task = task_id

        return t

    def finish(self, task_id: str, *, message: str | None = None) -> Task:
        """
        Register a terminal success task.
        Emits a CommandOperator that prints `message` (or a default) and marks terminal: success.
        """
        msg = message or f"Workflow {self._name} completed successfully."
        op = _command(f"echo {repr(msg)}", shell=True, capture_stdout=False)
        t = self.task(task_id, op)
        t._terminal = "success"
        t._message = message
        return t

    def fail(self, task_id: str, *, message: str | None = None) -> Task:
        """
        Register a terminal failure task.
        Emits a CommandOperator that prints `message` and exits 1, terminal: failure.
        """
        msg = message or f"Workflow {self._name} failed."
        op = _command(
            f"echo {repr(msg)} >&2; exit 1",
            shell=True,
            capture_stdout=False,
        )
        t = self.task(task_id, op)
        t._terminal = "failure"
        t._message = message
        return t

    def _register_raw_task(self, t: Task) -> None:
        """Internal: register a pre-built Task object directly (used by tests)."""
        if t.task_id in self._tasks:
            raise ValueError(f"Task '{t.task_id}' already registered")
        self._tasks[t.task_id] = t
        self._task_list.append(t)
        if self._entry_task is None:
            self._entry_task = t.task_id

    # ------------------------------------------------------------------
    # Serialization
    # ------------------------------------------------------------------

    def to_yaml(self) -> str:
        """Compile the workflow and return a YAML string."""
        return compile_workflow(self)

    def compile(self) -> str:
        """Alias for to_yaml()."""
        return self.to_yaml()
