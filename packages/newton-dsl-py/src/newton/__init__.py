"""
newton-dsl — Python authoring surface for Newton workflow graphs.

Public API:
    Workflow        — workflow graph builder
    command         — CommandOperator constructor
    agent           — AgentOperator constructor
    gh              — GitHub operator sub-constructors (gh.pr_create, etc.)
    human_approval  — HumanOperator constructor
    sub_workflow    — WorkflowOperator constructor
    when            — guard helper for .then(when=...)
    expr            — opaque Rhai expression passthrough
"""

from .workflow import Workflow
from .operators import command, agent, gh, human_approval, sub_workflow
from .refs import when, expr

__all__ = [
    "Workflow",
    "command",
    "agent",
    "gh",
    "human_approval",
    "sub_workflow",
    "when",
    "expr",
]
