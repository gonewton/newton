"""
newton-dsl — Python authoring surface for Newton workflow graphs.

Public API:
    Workflow        — workflow graph builder
    command         — CommandOperator constructor
    agent           — AgentOperator constructor
    gh              — GitHub operator sub-constructors (gh.pr_create, etc.)
    git             — Git operator sub-constructors (git.commit, etc.)
    human_approval  — HumanApprovalOperator constructor
    human_decision  — HumanDecisionOperator constructor
    sub_workflow    — WorkflowOperator constructor
    when            — guard helper for .then(when=...)
    expr            — opaque Rhai expression passthrough
"""

from .workflow import Workflow
from .operators import command, agent, gh, git, human_approval, human_decision, sub_workflow
from .refs import when, expr

__all__ = [
    "Workflow",
    "command",
    "agent",
    "gh",
    "git",
    "human_approval",
    "human_decision",
    "sub_workflow",
    "when",
    "expr",
]
