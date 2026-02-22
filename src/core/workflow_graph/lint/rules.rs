use crate::core::workflow_graph::expression::{EvaluationContext, ExpressionEngine};
use crate::core::workflow_graph::lint::{LintResult, LintSeverity, WorkflowLintRule};
use crate::core::workflow_graph::schema::{Condition, WorkflowDocument};
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::Bfs;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct DuplicateTaskIdsRule;

impl WorkflowLintRule for DuplicateTaskIdsRule {
    fn validate(&self, workflow: &WorkflowDocument, _engine: &ExpressionEngine) -> Vec<LintResult> {
        let mut seen = HashSet::new();
        let mut results = Vec::new();
        for task in &workflow.workflow.tasks {
            if !seen.insert(task.id.clone()) {
                results.push(LintResult::new(
                    "WFG-LINT-001",
                    LintSeverity::Error,
                    format!("duplicate task id '{}'", task.id),
                    Some(task.id.clone()),
                    Some("Use a unique task id".to_string()),
                ));
            }
        }
        results
    }
}

#[derive(Default)]
pub struct UnknownTransitionTargetsRule;

impl WorkflowLintRule for UnknownTransitionTargetsRule {
    fn validate(&self, workflow: &WorkflowDocument, _engine: &ExpressionEngine) -> Vec<LintResult> {
        let known_ids = task_id_set(workflow);
        let mut results = Vec::new();
        for task in &workflow.workflow.tasks {
            for transition in &task.transitions {
                if !known_ids.contains(&transition.to) {
                    results.push(LintResult::new(
                        "WFG-LINT-002",
                        LintSeverity::Error,
                        format!(
                            "transition 'to' references unknown task '{}'",
                            transition.to
                        ),
                        Some(task.id.clone()),
                        Some(format!(
                            "Ensure the transition target '{}' exists",
                            transition.to
                        )),
                    ));
                }
            }
        }
        results
    }
}

#[derive(Default)]
pub struct UnreachableTaskRule;

impl WorkflowLintRule for UnreachableTaskRule {
    fn validate(&self, workflow: &WorkflowDocument, _engine: &ExpressionEngine) -> Vec<LintResult> {
        let (graph, node_map) = build_task_graph(workflow);
        let entry_id = &workflow.workflow.settings.entry_task;
        let entry_node = match node_map.get(entry_id) {
            Some(&node) => node,
            None => return Vec::new(),
        };

        let reachable = reachable_nodes(&graph, entry_node);
        let mut results = Vec::new();
        for (task_id, &node) in &node_map {
            if task_id == entry_id {
                continue;
            }
            if !reachable.contains(&node) {
                results.push(LintResult::new(
                    "WFG-LINT-003",
                    LintSeverity::Warning,
                    format!("task '{}' is not reachable from entry_task", task_id),
                    Some(task_id.clone()),
                    Some("Ensure the task is reachable or remove it".to_string()),
                ));
            }
        }
        results
    }
}

#[derive(Default)]
pub struct AssertCompletedRequireRule;

impl WorkflowLintRule for AssertCompletedRequireRule {
    fn validate(&self, workflow: &WorkflowDocument, _engine: &ExpressionEngine) -> Vec<LintResult> {
        let known_ids = task_id_set(workflow);
        let mut results = Vec::new();
        for task in &workflow.workflow.tasks {
            if task.operator != "AssertCompletedOperator" {
                continue;
            }
            if let Some(require) = task.params.get("require").and_then(Value::as_array) {
                for value in require {
                    if let Some(id) = value.as_str() {
                        if !known_ids.contains(id) {
                            results.push(LintResult::new(
                                "WFG-LINT-004",
                                LintSeverity::Error,
                                format!(
                                    "AssertCompletedOperator '{}' references unknown task '{}'",
                                    task.id, id
                                ),
                                Some(task.id.clone()),
                                Some("Use valid task ids in 'require'".to_string()),
                            ));
                        }
                    }
                }
            }
        }
        results
    }
}

struct ExpressionEntry {
    expr: String,
    location: Option<String>,
}

#[derive(Default)]
pub struct ExpressionParseRule;

impl WorkflowLintRule for ExpressionParseRule {
    fn validate(&self, workflow: &WorkflowDocument, engine: &ExpressionEngine) -> Vec<LintResult> {
        let mut expressions = Vec::new();
        collect_expression_strings(&workflow.workflow.context, None, &mut expressions);
        for task in &workflow.workflow.tasks {
            collect_expression_strings(&task.params, Some(task.id.clone()), &mut expressions);
            for transition in &task.transitions {
                if let Some(condition) = &transition.when {
                    if let Some(expr) = condition.expression() {
                        expressions.push(ExpressionEntry {
                            expr: expr.to_string(),
                            location: Some(task.id.clone()),
                        });
                    }
                }
            }
        }

        let mut results = Vec::new();
        for entry in expressions {
            if let Err(err) = engine.compile(&entry.expr) {
                results.push(LintResult::new(
                    "WFG-LINT-005",
                    LintSeverity::Error,
                    format!(
                        "failed to parse expression '{}': {}",
                        entry.expr, err.message
                    ),
                    entry.location.clone(),
                    Some("Fix expression syntax".to_string()),
                ));
            }
        }
        results
    }
}

#[derive(Default)]
pub struct WhenConditionBooleanRule;

impl WorkflowLintRule for WhenConditionBooleanRule {
    fn validate(&self, workflow: &WorkflowDocument, engine: &ExpressionEngine) -> Vec<LintResult> {
        let context = evaluation_context_from_document(workflow);
        let mut results = Vec::new();
        for task in &workflow.workflow.tasks {
            for transition in &task.transitions {
                if let Some(Condition::Expr { expr }) = &transition.when {
                    if expression_depends_on_tasks(expr) {
                        continue;
                    }
                    if let Ok(value) = engine.evaluate(expr, &context) {
                        if !value.is_boolean() {
                            results.push(LintResult::new(
                                "WFG-LINT-006",
                                LintSeverity::Error,
                                format!("when expression '{}' did not evaluate to boolean", expr),
                                Some(task.id.clone()),
                                Some("Ensure the condition returns a boolean".to_string()),
                            ));
                        }
                    }
                }
            }
        }
        results
    }
}

#[derive(Default)]
pub struct SuspiciousLoopRule;

impl WorkflowLintRule for SuspiciousLoopRule {
    fn validate(&self, workflow: &WorkflowDocument, _engine: &ExpressionEngine) -> Vec<LintResult> {
        let (graph, _) = build_task_graph(workflow);
        let task_lookup: HashMap<String, &crate::core::workflow_graph::schema::WorkflowTask> =
            workflow
                .workflow
                .tasks
                .iter()
                .map(|task| (task.id.clone(), task))
                .collect();
        let mut results = Vec::new();
        let mut flagged = HashSet::new();

        for component in tarjan_scc(&graph) {
            let cyclical = component.len() > 1
                || component
                    .iter()
                    .any(|&node| graph.find_edge(node, node).is_some());
            if !cyclical {
                continue;
            }
            for node in component {
                if let Some(task_id) = graph.node_weight(node) {
                    let id = task_id.clone();
                    if !flagged.insert(id.clone()) {
                        continue;
                    }
                    if let Some(task) = task_lookup.get(&id) {
                        if task.max_iterations.is_none() {
                            results.push(LintResult::new(
                                    "WFG-LINT-007",
                                    LintSeverity::Info,
                                    format!(
                                        "task '{}' participates in a cycle without per-task max_iterations",
                                        id
                                    ),
                                    Some(task.id.clone()),
                                    Some("Set task.max_iterations to guard loops".to_string()),
                                ));
                        }
                    }
                }
            }
        }
        results
    }
}

#[derive(Default)]
pub struct CommandOperatorShellRule;

impl WorkflowLintRule for CommandOperatorShellRule {
    fn validate(&self, workflow: &WorkflowDocument, _engine: &ExpressionEngine) -> Vec<LintResult> {
        if workflow.workflow.settings.command_operator.allow_shell {
            return Vec::new();
        }
        let mut results = Vec::new();
        for task in &workflow.workflow.tasks {
            if task.operator != "CommandOperator" {
                continue;
            }
            if shell_enabled(&task.params) {
                results.push(LintResult::new(
                    "WFG-LINT-008",
                    LintSeverity::Error,
                    format!(
                        "CommandOperator '{}' enables shell execution without allow_shell opt-in",
                        task.id
                    ),
                    Some(task.id.clone()),
                    Some(
                        "Set settings.command_operator.allow_shell to true only when shell usage is intended"
                            .to_string(),
                    ),
                ));
            }
        }
        results
    }
}

fn task_id_set(workflow: &WorkflowDocument) -> HashSet<String> {
    workflow
        .workflow
        .tasks
        .iter()
        .map(|task| task.id.clone())
        .collect()
}

fn build_task_graph(
    workflow: &WorkflowDocument,
) -> (DiGraph<String, ()>, HashMap<String, NodeIndex>) {
    let mut graph = DiGraph::new();
    let mut node_map = HashMap::new();
    for task in &workflow.workflow.tasks {
        let idx = graph.add_node(task.id.clone());
        node_map.insert(task.id.clone(), idx);
    }
    for task in &workflow.workflow.tasks {
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

fn reachable_nodes(graph: &DiGraph<String, ()>, entry: NodeIndex) -> HashSet<NodeIndex> {
    let mut bfs = Bfs::new(graph, entry);
    let mut reachable = HashSet::new();
    while let Some(node) = bfs.next(graph) {
        reachable.insert(node);
    }
    reachable
}

fn collect_expression_strings(
    value: &Value,
    location: Option<String>,
    output: &mut Vec<ExpressionEntry>,
) {
    match value {
        Value::Object(map) => {
            if map.len() == 1 && map.contains_key("$expr") {
                if let Some(Value::String(expr)) = map.get("$expr") {
                    output.push(ExpressionEntry {
                        expr: expr.clone(),
                        location,
                    });
                    return;
                }
            }
            for child in map.values() {
                collect_expression_strings(child, location.clone(), output);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_expression_strings(item, location.clone(), output);
            }
        }
        _ => {}
    }
}

fn expression_depends_on_tasks(expr: &str) -> bool {
    expr.contains("tasks.") || expr.contains("tasks[")
}

fn evaluation_context_from_document(workflow: &WorkflowDocument) -> EvaluationContext {
    let context = workflow.workflow.context.clone();
    let tasks = build_tasks_placeholder(workflow);
    let triggers = Value::Object(Map::new());
    EvaluationContext::new(context, tasks, triggers)
}

fn build_tasks_placeholder(workflow: &WorkflowDocument) -> Value {
    let mut map = Map::new();
    for task in &workflow.workflow.tasks {
        map.insert(task.id.clone(), Value::Object(Map::new()));
    }
    Value::Object(map)
}

fn shell_enabled(params: &Value) -> bool {
    params
        .get("shell")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}
