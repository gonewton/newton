use crate::core::workflow_graph::schema::{Condition, Transition, WorkflowDocument};
use petgraph::dot::Dot;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::Bfs;
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Node weight carrying task display information.
struct TaskNode {
    id: String,
    operator: String,
}

impl fmt::Display for TaskNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\\n{}", self.id, self.operator)
    }
}

/// Edge weight carrying a formatted transition label.
struct EdgeData {
    label: String,
}

impl fmt::Display for EdgeData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label)
    }
}

fn build_graph(
    document: &WorkflowDocument,
) -> (DiGraph<TaskNode, EdgeData>, HashMap<String, NodeIndex>) {
    let mut graph = DiGraph::new();
    let mut node_map: HashMap<String, NodeIndex> = HashMap::new();

    for task in document.workflow.tasks() {
        let idx = graph.add_node(TaskNode {
            id: task.id.clone(),
            operator: task.operator.clone(),
        });
        node_map.insert(task.id.clone(), idx);
    }

    for task in document.workflow.tasks() {
        let from = node_map[&task.id];
        for transition in &task.transitions {
            if let Some(&to) = node_map.get(&transition.to) {
                let label = format_transition_label(transition);
                graph.add_edge(from, to, EdgeData { label });
            }
        }
    }

    (graph, node_map)
}

/// Render the workflow graph as a Graphviz DOT string using petgraph.
pub fn workflow_to_dot(document: &WorkflowDocument) -> String {
    let (graph, _) = build_graph(document);
    format!("{}", Dot::new(&graph))
}

/// Returns the ids of tasks not reachable from the workflow's entry task.
pub fn reachability_warnings(document: &WorkflowDocument) -> Vec<String> {
    let (graph, node_map) = build_graph(document);
    let entry_id = &document.workflow.settings.entry_task;
    let entry_node = match node_map.get(entry_id) {
        Some(&n) => n,
        None => return Vec::new(),
    };

    let mut reachable = HashSet::new();
    let mut bfs = Bfs::new(&graph, entry_node);
    while let Some(nx) = bfs.next(&graph) {
        reachable.insert(nx);
    }

    let mut unreachable: Vec<String> = node_map
        .iter()
        .filter(|(_, &nx)| !reachable.contains(&nx))
        .map(|(id, _)| id.clone())
        .collect();
    unreachable.sort();
    unreachable
}

fn format_transition_label(transition: &Transition) -> String {
    let base = if let Some(label) = &transition.label {
        label.clone()
    } else if let Some(condition) = &transition.when {
        match condition {
            Condition::Bool(flag) => format!("when={} priority={}", flag, transition.priority),
            Condition::Expr { expr } => format!(
                "when:{} priority={}",
                truncate(expr, 60),
                transition.priority
            ),
        }
    } else {
        format!("priority={}", transition.priority)
    };
    escape_label(&truncate(&base, 80))
}

fn truncate(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        value.to_string()
    } else {
        format!("{}...", &value[..limit])
    }
}

fn escape_label(value: &str) -> String {
    value.replace('\"', "\\\"")
}
