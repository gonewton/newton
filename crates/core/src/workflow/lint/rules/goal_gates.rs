use super::super::{LintResult, LintSeverity, WorkflowLintRule};
use crate::workflow::schema::{WorkflowDocument, WorkflowTask};
use petgraph::algo::has_path_connecting;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::{HashMap, HashSet, VecDeque};

struct TerminalTaskMissingRule;

impl WorkflowLintRule for TerminalTaskMissingRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        if !workflow.workflow.settings.completion.stop_on_terminal {
            return Vec::new();
        }
        let has_terminal = workflow.workflow.tasks().any(|t| t.terminal.is_some());
        if has_terminal {
            return Vec::new();
        }
        vec![LintResult::new(
            "WFG-LINT-101",
            LintSeverity::Warning,
            "completion.stop_on_terminal is true but no task has a terminal field set; \
             the workflow can only stop via timeout, iteration limit, or empty ready queue",
            None,
            Some(
                "define at least one task with `terminal: success` or `terminal: failure`"
                    .to_string(),
            ),
        )]
    }
}

struct GoalGateUnreachableRule;

impl WorkflowLintRule for GoalGateUnreachableRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        if !workflow.workflow.settings.completion.require_goal_gates {
            return Vec::new();
        }

        let goal_gates: Vec<&str> = workflow
            .workflow
            .tasks()
            .filter(|t| t.goal_gate)
            .map(|t| t.id.as_str())
            .collect();

        if goal_gates.is_empty() {
            return Vec::new();
        }

        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
        for task in workflow.workflow.tasks() {
            adjacency.entry(task.id.as_str()).or_default();
            for transition in &task.transitions {
                adjacency
                    .entry(task.id.as_str())
                    .or_default()
                    .push(transition.to.as_str());
            }
        }

        let mut reachable = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(workflow.workflow.settings.entry_task.as_str());
        while let Some(current) = queue.pop_front() {
            if !reachable.insert(current) {
                continue;
            }
            if let Some(next) = adjacency.get(current) {
                for target in next {
                    queue.push_back(target);
                }
            }
        }

        let mut out = Vec::new();
        for gate_id in goal_gates {
            if !reachable.contains(gate_id) {
                out.push(LintResult::new(
                    "WFG-LINT-102",
                    LintSeverity::Error,
                    format!(
                        "goal gate task '{}' is not reachable from entry_task '{}'",
                        gate_id, workflow.workflow.settings.entry_task
                    ),
                    Some(gate_id.to_string()),
                    Some("add a transition path from the entry task to this goal gate".to_string()),
                ));
            }
        }
        out
    }
}

struct GoalGateNoRemediationRule;

impl WorkflowLintRule for GoalGateNoRemediationRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        use crate::workflow::schema::GoalGateFailureBehavior;
        if workflow
            .workflow
            .settings
            .completion
            .goal_gate_failure_behavior
            != GoalGateFailureBehavior::Fail
        {
            return Vec::new();
        }

        let goal_gate_ids: HashSet<&str> = workflow
            .workflow
            .tasks()
            .filter(|t| t.goal_gate)
            .map(|t| t.id.as_str())
            .collect();

        if goal_gate_ids.is_empty() {
            return Vec::new();
        }

        let tasks: Vec<WorkflowTask> = workflow.workflow.tasks().cloned().collect();
        let (graph, node_map) = build_task_graph_with_node_map(&tasks);

        let mut out = Vec::new();
        for gate_id in &goal_gate_ids {
            let Some(&gate_idx) = node_map.get(*gate_id) else {
                continue;
            };

            let successors: Vec<NodeIndex> = graph.neighbors(gate_idx).collect();

            let has_remediation = successors
                .iter()
                .any(|&succ_idx| has_path_connecting(&graph, succ_idx, gate_idx, None));

            if !has_remediation {
                out.push(LintResult::new(
                    "WFG-LINT-103",
                    LintSeverity::Warning,
                    format!(
                        "goal gate task '{gate_id}' has no retry or remediation path back to it; \
                         if it fails the workflow cannot recover"
                    ),
                    Some((*gate_id).to_string()),
                    Some(
                        "add a transition from a successor task back to this goal gate, \
                         or set goal_gate_failure_behavior=allow"
                            .to_string(),
                    ),
                ));
            }
        }
        out
    }
}

struct ConflictingTerminalTasksRule;

impl WorkflowLintRule for ConflictingTerminalTasksRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        if !workflow.workflow.settings.completion.stop_on_terminal {
            return Vec::new();
        }

        let terminal_tasks: Vec<&WorkflowTask> = workflow
            .workflow
            .tasks()
            .filter(|t| t.terminal.is_some())
            .collect();

        if terminal_tasks.len() < 2 {
            return Vec::new();
        }

        let tasks: Vec<WorkflowTask> = workflow.workflow.tasks().cloned().collect();
        let (graph, node_map) = build_task_graph_with_node_map(&tasks);

        let mut out = Vec::new();
        for i in 0..terminal_tasks.len() {
            for j in (i + 1)..terminal_tasks.len() {
                let a = terminal_tasks[i];
                let b = terminal_tasks[j];
                let Some(&a_idx) = node_map.get(a.id.as_str()) else {
                    continue;
                };
                let Some(&b_idx) = node_map.get(b.id.as_str()) else {
                    continue;
                };
                if !has_path_connecting(&graph, a_idx, b_idx, None)
                    && !has_path_connecting(&graph, b_idx, a_idx, None)
                {
                    out.push(LintResult::new(
                        "WFG-LINT-104",
                        LintSeverity::Info,
                        format!(
                            "terminal tasks '{}' and '{}' can execute in the same scheduler tick; \
                             tie-breaking rule WFG-TERM-001 applies (task-id alphabetical order)",
                            a.id, b.id
                        ),
                        Some(a.id.clone()),
                        Some(
                            "ensure this tie-breaking behaviour is acceptable or serialize \
                             these terminal tasks with a dependency"
                                .to_string(),
                        ),
                    ));
                }
            }
        }
        out
    }
}

fn build_task_graph_with_node_map(
    tasks: &[WorkflowTask],
) -> (DiGraph<(), ()>, HashMap<String, NodeIndex>) {
    let mut graph = DiGraph::<(), ()>::new();
    let mut node_map: HashMap<String, NodeIndex> = HashMap::new();

    for task in tasks {
        let idx = graph.add_node(());
        node_map.insert(task.id.clone(), idx);
    }

    for task in tasks {
        if let Some(&from) = node_map.get(&task.id) {
            for transition in &task.transitions {
                if let Some(&to) = node_map.get(&transition.to) {
                    graph.add_edge(from, to, ());
                }
            }
        }
    }

    (graph, node_map)
}

pub(super) fn rules() -> Vec<Box<dyn WorkflowLintRule>> {
    vec![
        Box::new(TerminalTaskMissingRule),
        Box::new(GoalGateUnreachableRule),
        Box::new(GoalGateNoRemediationRule),
        Box::new(ConflictingTerminalTasksRule),
    ]
}
