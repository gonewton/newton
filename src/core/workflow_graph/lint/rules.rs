use super::{LintResult, LintSeverity, WorkflowLintRule};
use crate::core::workflow_graph::expression::{EvaluationContext, ExpressionEngine};
use crate::core::workflow_graph::schema::{Condition, WorkflowDocument, WorkflowTask};
use petgraph::algo::{has_path_connecting, tarjan_scc};
use petgraph::graph::{DiGraph, NodeIndex};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet, VecDeque};

pub fn built_in_rules() -> Vec<Box<dyn WorkflowLintRule>> {
    vec![
        Box::new(DuplicateTaskIdsRule),
        Box::new(UnknownTransitionTargetsRule),
        Box::new(UnreachableTasksRule),
        Box::new(AssertCompletedUnknownRequireRule),
        Box::new(ExpressionParseFailureRule),
        Box::new(WhenExpressionBoolRule),
        Box::new(SuspiciousLoopRiskRule),
        Box::new(ShellOptInRule),
        Box::new(RequiredTriggersRule),
        Box::new(TerminalTaskMissingRule),
        Box::new(GoalGateUnreachableRule),
        Box::new(GoalGateNoRemediationRule),
        Box::new(ConflictingTerminalTasksRule),
    ]
}

struct DuplicateTaskIdsRule;

impl WorkflowLintRule for DuplicateTaskIdsRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for task in &workflow.workflow.tasks {
            *counts.entry(task.id.clone()).or_insert(0) += 1;
        }

        let mut out = Vec::new();
        for (task_id, count) in counts {
            if count > 1 {
                out.push(LintResult::new(
                    "WFG-LINT-001",
                    LintSeverity::Error,
                    format!("duplicate task id '{}' found {} times", task_id, count),
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
            .tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect();
        let mut out = Vec::new();

        for task in &workflow.workflow.tasks {
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
        for task in &workflow.workflow.tasks {
            adjacency.entry(task.id.as_str()).or_default();
        }
        for task in &workflow.workflow.tasks {
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
        for task in &workflow.workflow.tasks {
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
            .tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect();
        let mut out = Vec::new();

        for task in &workflow.workflow.tasks {
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

struct ExpressionParseFailureRule;

impl WorkflowLintRule for ExpressionParseFailureRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let engine = ExpressionEngine::default();
        let mut exprs = Vec::new();
        collect_expr_values(&workflow.workflow.context, &mut exprs, None);
        for task in &workflow.workflow.tasks {
            collect_expr_values(&task.params, &mut exprs, Some(task.id.as_str()));
            for transition in &task.transitions {
                if let Some(Condition::Expr { expr }) = &transition.when {
                    exprs.push((expr.clone(), Some(task.id.clone())));
                }
            }
        }

        let mut out = Vec::new();
        for (expr, location) in exprs {
            if let Err(err) = engine.compile(&expr) {
                out.push(LintResult::new(
                    "WFG-LINT-005",
                    LintSeverity::Error,
                    format!("$expr parse failure for '{}': {}", expr, err.message),
                    location,
                    Some("fix syntax so the expression compiles".to_string()),
                ));
            }
        }
        out
    }
}

struct WhenExpressionBoolRule;

impl WorkflowLintRule for WhenExpressionBoolRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let engine = ExpressionEngine::default();
        let mut out = Vec::new();

        for task in &workflow.workflow.tasks {
            for transition in &task.transitions {
                let Some(Condition::Expr { expr }) = &transition.when else {
                    continue;
                };
                if expr_depends_on_tasks(expr) {
                    continue;
                }

                let eval_ctx = EvaluationContext::new(
                    workflow.workflow.context.clone(),
                    Value::Object(Map::new()),
                    Value::Object(Map::new()),
                );

                match engine.evaluate(expr, &eval_ctx) {
                    Ok(Value::Bool(_)) => {}
                    Ok(_) => out.push(LintResult::new(
                        "WFG-LINT-006",
                        LintSeverity::Error,
                        format!(
                            "$expr in transition 'when' for task '{}' does not evaluate to bool",
                            task.id
                        ),
                        Some(task.id.clone()),
                        Some("ensure transition 'when' expressions return true/false".to_string()),
                    )),
                    Err(_) => {}
                }
            }
        }

        out
    }
}

struct SuspiciousLoopRiskRule;

impl WorkflowLintRule for SuspiciousLoopRiskRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let (graph, tasks_by_idx) = build_task_graph(&workflow.workflow.tasks);
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
        for task in &workflow.workflow.tasks {
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

struct RequiredTriggersRule;

impl WorkflowLintRule for RequiredTriggersRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        if workflow.workflow.settings.required_triggers.is_empty() {
            return Vec::new();
        }
        if workflow.triggers.is_some() {
            return Vec::new();
        }
        vec![LintResult::new(
            "WFG-LINT-009",
            LintSeverity::Warning,
            "required_triggers is set but workflow has no triggers block",
            None,
            Some("add a triggers block or provide trigger payloads at run time".to_string()),
        )]
    }
}

/// WFG-LINT-101: No terminal task defined when stop_on_terminal is true (default).
struct TerminalTaskMissingRule;

impl WorkflowLintRule for TerminalTaskMissingRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        if !workflow.workflow.settings.completion.stop_on_terminal {
            return Vec::new();
        }
        let has_terminal = workflow.workflow.tasks.iter().any(|t| t.terminal.is_some());
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

/// WFG-LINT-102: Goal gate unreachable from entry_task.
struct GoalGateUnreachableRule;

impl WorkflowLintRule for GoalGateUnreachableRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        if !workflow.workflow.settings.completion.require_goal_gates {
            return Vec::new();
        }

        let goal_gates: Vec<&str> = workflow
            .workflow
            .tasks
            .iter()
            .filter(|t| t.goal_gate)
            .map(|t| t.id.as_str())
            .collect();

        if goal_gates.is_empty() {
            return Vec::new();
        }

        // BFS from entry_task to find reachable nodes.
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
        for task in &workflow.workflow.tasks {
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

/// WFG-LINT-103: Goal gate has no retry/remediation path (best-effort, false negatives OK).
struct GoalGateNoRemediationRule;

impl WorkflowLintRule for GoalGateNoRemediationRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        use crate::core::workflow_graph::schema::GoalGateFailureBehavior;
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
            .tasks
            .iter()
            .filter(|t| t.goal_gate)
            .map(|t| t.id.as_str())
            .collect();

        if goal_gate_ids.is_empty() {
            return Vec::new();
        }

        let (graph, node_map) = build_task_graph_with_node_map(&workflow.workflow.tasks);

        let mut out = Vec::new();
        for gate_id in &goal_gate_ids {
            let Some(&gate_idx) = node_map.get(*gate_id) else {
                continue;
            };

            // Collect direct successors of this gate.
            let successors: Vec<NodeIndex> = graph.neighbors(gate_idx).collect();

            // Check if any successor has a path back to the gate.
            let has_remediation = successors
                .iter()
                .any(|&succ_idx| has_path_connecting(&graph, succ_idx, gate_idx, None));

            if !has_remediation {
                out.push(LintResult::new(
                    "WFG-LINT-103",
                    LintSeverity::Warning,
                    format!(
                        "goal gate task '{}' has no retry or remediation path back to it; \
                         if it fails the workflow cannot recover",
                        gate_id
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

/// WFG-LINT-104: Conflicting terminal tasks that can run in the same tick.
struct ConflictingTerminalTasksRule;

impl WorkflowLintRule for ConflictingTerminalTasksRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        if !workflow.workflow.settings.completion.stop_on_terminal {
            return Vec::new();
        }

        let terminal_tasks: Vec<&WorkflowTask> = workflow
            .workflow
            .tasks
            .iter()
            .filter(|t| t.terminal.is_some())
            .collect();

        if terminal_tasks.len() < 2 {
            return Vec::new();
        }

        let (graph, node_map) = build_task_graph_with_node_map(&workflow.workflow.tasks);

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
                // If neither can reach the other, they can run concurrently.
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

fn expr_depends_on_tasks(expr: &str) -> bool {
    expr.contains("tasks.") || expr.contains("tasks[")
}

fn collect_expr_values(
    value: &Value,
    out: &mut Vec<(String, Option<String>)>,
    location: Option<&str>,
) {
    match value {
        Value::Object(map) => {
            if map.len() == 1 && map.contains_key("$expr") {
                if let Some(Value::String(expr)) = map.get("$expr") {
                    out.push((expr.clone(), location.map(ToOwned::to_owned)));
                    return;
                }
            }
            for child in map.values() {
                collect_expr_values(child, out, location);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_expr_values(child, out, location);
            }
        }
        _ => {}
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
