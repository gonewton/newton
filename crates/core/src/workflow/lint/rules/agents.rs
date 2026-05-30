use super::super::{LintResult, LintSeverity, WorkflowLintRule};
use crate::workflow::schema::WorkflowDocument;
use regex::Regex;
use serde_json::Value;

struct AgentNoEngineRule;

impl WorkflowLintRule for AgentNoEngineRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let mut out = Vec::new();
        let has_default_engine = workflow.workflow.settings.default_engine.is_some();

        for task in workflow.workflow.tasks() {
            if task.operator != "AgentOperator" {
                continue;
            }
            let has_engine_in_params = task
                .params
                .get("engine")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty());

            if !has_engine_in_params && !has_default_engine {
                out.push(LintResult::new(
                    "WFG-LINT-110",
                    LintSeverity::Warning,
                    format!(
                        "AgentOperator task '{}' has no engine in params.engine or \
                         settings.default_engine; workspace coding_agent config not checked at lint time",
                        task.id
                    ),
                    Some(task.id.clone()),
                    Some(
                        "set params.engine or settings.default_engine to resolve the engine"
                            .to_string(),
                    ),
                ));
            }
        }
        out
    }
}

struct AgentInvalidSignalRegexRule;

impl WorkflowLintRule for AgentInvalidSignalRegexRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let mut out = Vec::new();

        for task in workflow.workflow.tasks() {
            if task.operator != "AgentOperator" {
                continue;
            }
            let Some(signals_obj) = task.params.get("signals").and_then(Value::as_object) else {
                continue;
            };
            for (signal_name, pattern_val) in signals_obj {
                let Some(pattern) = pattern_val.as_str() else {
                    continue;
                };
                if pattern.contains('\n') {
                    out.push(LintResult::new(
                        "WFG-LINT-111",
                        LintSeverity::Warning,
                        format!(
                            "AgentOperator task '{}' signal '{}' contains \\n; \
                             cross-line matching is not supported",
                            task.id, signal_name
                        ),
                        Some(task.id.clone()),
                        Some(
                            "remove \\n from signal pattern; patterns match single lines only"
                                .to_string(),
                        ),
                    ));
                    continue;
                }
                if let Err(err) = Regex::new(pattern) {
                    out.push(LintResult::new(
                        "WFG-LINT-111",
                        LintSeverity::Warning,
                        format!(
                            "AgentOperator task '{}' signal '{}' has invalid regex: {}",
                            task.id, signal_name, err
                        ),
                        Some(task.id.clone()),
                        Some("fix the regex pattern so it compiles".to_string()),
                    ));
                }
            }
        }
        out
    }
}

struct AgentUnboundedLoopRule;

impl WorkflowLintRule for AgentUnboundedLoopRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let mut out = Vec::new();

        for task in workflow.workflow.tasks() {
            if task.operator != "AgentOperator" {
                continue;
            }
            let loop_mode = task
                .params
                .get("loop")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !loop_mode {
                continue;
            }
            let has_max_iterations = task.params.get("max_iterations").is_some();
            if !has_max_iterations {
                out.push(LintResult::new(
                    "WFG-LINT-113",
                    LintSeverity::Warning,
                    format!(
                        "AgentOperator task '{}' has loop: true but no max_iterations; \
                         loop may run indefinitely",
                        task.id
                    ),
                    Some(task.id.clone()),
                    Some("set params.max_iterations to bound the loop".to_string()),
                ));
            }
        }
        out
    }
}

struct AgentCommandNoEngineCommandRule;

impl WorkflowLintRule for AgentCommandNoEngineCommandRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let mut out = Vec::new();

        for task in workflow.workflow.tasks() {
            if task.operator != "AgentOperator" {
                continue;
            }
            let engine = task
                .params
                .get("engine")
                .and_then(Value::as_str)
                .or(workflow.workflow.settings.default_engine.as_deref());
            if engine != Some("command") {
                continue;
            }
            let has_engine_command = task
                .params
                .get("engine_command")
                .is_some_and(serde_json::Value::is_array);
            if !has_engine_command {
                out.push(LintResult::new(
                    "WFG-LINT-114",
                    LintSeverity::Warning,
                    format!(
                        "AgentOperator task '{}' uses engine: command but has no engine_command in params",
                        task.id
                    ),
                    Some(task.id.clone()),
                    Some("add engine_command to params when using engine: command".to_string()),
                ));
            }
        }
        out
    }
}

struct AgentNamedDriverNoPromptRule;

impl WorkflowLintRule for AgentNamedDriverNoPromptRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let mut out = Vec::new();

        for task in workflow.workflow.tasks() {
            if task.operator != "AgentOperator" {
                continue;
            }
            let engine = task
                .params
                .get("engine")
                .and_then(Value::as_str)
                .or(workflow.workflow.settings.default_engine.as_deref());
            let Some(engine_name) = engine else {
                continue;
            };
            if engine_name == "command" {
                continue;
            }
            let has_prompt =
                task.params.get("prompt").is_some() || task.params.get("prompt_file").is_some();
            if !has_prompt {
                out.push(LintResult::new(
                    "WFG-LINT-115",
                    LintSeverity::Warning,
                    format!(
                        "AgentOperator task '{}' uses engine '{}' but has neither \
                         prompt_file nor prompt in params",
                        task.id, engine_name
                    ),
                    Some(task.id.clone()),
                    Some(
                        "add prompt_file or prompt to params for named engine drivers".to_string(),
                    ),
                ));
            }
        }
        out
    }
}

pub(super) fn rules() -> Vec<Box<dyn WorkflowLintRule>> {
    vec![
        Box::new(AgentNoEngineRule),
        Box::new(AgentInvalidSignalRegexRule),
        Box::new(AgentUnboundedLoopRule),
        Box::new(AgentCommandNoEngineCommandRule),
        Box::new(AgentNamedDriverNoPromptRule),
    ]
}
