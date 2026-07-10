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
    barrier         — barrier operator constructor
    set_context     — SetContextOperator constructor
    noop            — NoOpOperator constructor
    grader_command  — GraderCommandOperator constructor
    reconcile       — ReconcileOperator constructor
    change_request  — ChangeRequestOperator constructor
    grader_agent    — GraderAgentOperator constructor
    when            — guard helper for .then(when=...)
    expr            — opaque Rhai expression passthrough
"""

from .workflow import Workflow
from .operators import (
    command,
    agent,
    gh,
    git,
    human_approval,
    human_decision,
    sub_workflow,
    barrier,
    set_context,
    noop,
    grader_command,
    reconcile,
    change_request,
    grader_agent,
)
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
    "barrier",
    "set_context",
    "noop",
    "grader_command",
    "reconcile",
    "change_request",
    "grader_agent",
    "when",
    "expr",
]
