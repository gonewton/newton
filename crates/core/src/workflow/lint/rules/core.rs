use super::super::{LintResult, LintSeverity, WorkflowLintRule};
use crate::workflow::schema::{WorkflowDocument, WorkflowTask};
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

struct DuplicateTaskIdsRule;

impl WorkflowLintRule for DuplicateTaskIdsRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for task in workflow.workflow.tasks() {
            *counts.entry(task.id.clone()).or_insert(0) += 1;
        }

        let mut out = Vec::new();
        for (task_id, count) in counts {
            if count > 1 {
                out.push(LintResult::new(
                    "WFG-LINT-001",
                    LintSeverity::Error,
                    format!("duplicate task id '{task_id}' found {count} times"),
                    Some(task_id),
                    Some("rename tasks so every task id is unique".to_string()),
                ));
            }
        }
        out
    }
}

struct UnknownTransitionTargetsRule;

impl WorkflowLintRule for UnknownTransitionTargetsRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let known_ids: HashSet<&str> = workflow
            .workflow
            .tasks()
            .map(|task| task.id.as_str())
            .collect();
        let mut out = Vec::new();

        for task in workflow.workflow.tasks() {
            for transition in &task.transitions {
                if !known_ids.contains(transition.to.as_str()) {
                    out.push(LintResult::new(
                        "WFG-LINT-002",
                        LintSeverity::Error,
                        format!(
                            "transition from '{}' references unknown target '{}'",
                            task.id, transition.to
                        ),
                        Some(task.id.clone()),
                        Some("point transitions to an existing task id".to_string()),
                    ));
                }
            }
        }

        out
    }
}

struct UnreachableTasksRule;

impl WorkflowLintRule for UnreachableTasksRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
        for task in workflow.workflow.tasks() {
            adjacency.entry(task.id.as_str()).or_default();
        }
        for task in workflow.workflow.tasks() {
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
            if !reachable.insert(current.to_string()) {
                continue;
            }
            if let Some(next) = adjacency.get(current) {
                for target in next {
                    queue.push_back(target);
                }
            }
        }

        let mut out = Vec::new();
        for task in workflow.workflow.tasks() {
            if !reachable.contains(task.id.as_str()) {
                out.push(LintResult::new(
                    "WFG-LINT-003",
                    LintSeverity::Warning,
                    format!("task '{}' is unreachable from entry_task", task.id),
                    Some(task.id.clone()),
                    Some("connect the task from a reachable transition or remove it".to_string()),
                ));
            }
        }
        out
    }
}

struct AssertCompletedUnknownRequireRule;

impl WorkflowLintRule for AssertCompletedUnknownRequireRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let known_ids: HashSet<&str> = workflow
            .workflow
            .tasks()
            .map(|task| task.id.as_str())
            .collect();
        let mut out = Vec::new();

        for task in workflow.workflow.tasks() {
            if task.operator != "AssertCompletedOperator" {
                continue;
            }
            let Some(require) = task.params.get("require").and_then(Value::as_array) else {
                continue;
            };
            for id in require.iter().filter_map(Value::as_str) {
                if !known_ids.contains(id) {
                    out.push(LintResult::new(
                        "WFG-LINT-004",
                        LintSeverity::Error,
                        format!(
                            "AssertCompletedOperator in '{}' references unknown task '{}'",
                            task.id, id
                        ),
                        Some(task.id.clone()),
                        Some("update 'require' to include only valid task ids".to_string()),
                    ));
                }
            }
        }

        out
    }
}

struct SuspiciousLoopRiskRule;

impl WorkflowLintRule for SuspiciousLoopRiskRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let tasks: Vec<WorkflowTask> = workflow.workflow.tasks().cloned().collect();
        let (graph, tasks_by_idx) = build_task_graph(&tasks);
        let mut out = Vec::new();

        for component in tarjan_scc(&graph) {
            let is_cycle = if component.len() > 1 {
                true
            } else {
                let idx = component[0];
                graph.find_edge(idx, idx).is_some()
            };
            if !is_cycle {
                continue;
            }

            for idx in component {
                if let Some(task) = tasks_by_idx.get(&idx) {
                    if task.max_iterations.is_none() {
                        out.push(LintResult::new(
                            "WFG-LINT-007",
                            LintSeverity::Info,
                            format!(
                                "task '{}' is part of a cycle and has no per-task max_iterations",
                                task.id
                            ),
                            Some(task.id.clone()),
                            Some(
                                "set task.max_iterations to guard against accidental infinite loops"
                                    .to_string(),
                            ),
                        ));
                    }
                }
            }
        }

        out
    }
}

struct ShellOptInRule;

impl WorkflowLintRule for ShellOptInRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        if workflow.workflow.settings.command_operator.allow_shell {
            return Vec::new();
        }

        let mut out = Vec::new();
        for task in workflow.workflow.tasks() {
            if task.operator != "CommandOperator" {
                continue;
            }
            let shell = task
                .params
                .get("shell")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if shell {
                out.push(LintResult::new(
                    "WFG-LINT-008",
                    LintSeverity::Error,
                    "CommandOperator uses shell=true but settings.command_operator.allow_shell is not true",
                    Some(task.id.clone()),
                    Some("set settings.command_operator.allow_shell=true to opt in explicitly".to_string()),
                ));
            }
        }
        out
    }
}

fn build_task_graph(tasks: &[WorkflowTask]) -> (DiGraph<(), ()>, HashMap<NodeIndex, WorkflowTask>) {
    let mut graph = DiGraph::<(), ()>::new();
    let mut node_map = HashMap::new();
    let mut tasks_by_idx = HashMap::new();

    for task in tasks {
        let idx = graph.add_node(());
        node_map.insert(task.id.clone(), idx);
        tasks_by_idx.insert(idx, task.clone());
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

    (graph, tasks_by_idx)
}

pub(super) fn rules() -> Vec<Box<dyn WorkflowLintRule>> {
    vec![
        Box::new(DuplicateTaskIdsRule),
        Box::new(UnknownTransitionTargetsRule),
        Box::new(UnreachableTasksRule),
        Box::new(AssertCompletedUnknownRequireRule),
        Box::new(SuspiciousLoopRiskRule),
        Box::new(ShellOptInRule),
    ]
}
