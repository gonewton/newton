use crate::core::workflow_graph::schema::{Condition, Transition, WorkflowDocument};

pub fn workflow_to_dot(document: &WorkflowDocument) -> String {
    let mut lines = vec!["digraph workflow_graph {".to_string()];
    for task in &document.workflow.tasks {
        let label = format!("{}\\n{}", task.id, task.operator);
        lines.push(format!(
            "  \"{id}\" [label=\"{label}\"];",
            id = task.id,
            label = label
        ));
    }

    for task in &document.workflow.tasks {
        for transition in &task.transitions {
            let label = format_transition_label(transition);
            lines.push(format!(
                "  \"{from}\" -> \"{to}\" [label=\"{label}\"];",
                from = task.id,
                to = transition.to,
                label = label
            ));
        }
    }

    lines.push("}".to_string());
    lines.join("\n")
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
