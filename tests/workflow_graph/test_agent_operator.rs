/// Integration tests for AgentOperator and engine drivers (017-h spec).
use newton::core::workflow_graph::{
    lint::{LintRegistry, LintSeverity},
    schema::{self, ContextFidelity, ModelStylesheet, WorkflowSettings},
};
use std::fs;
use tempfile::NamedTempFile;

// ── H3: Engine resolved from settings.default_engine ────────────────────────

#[test]
fn h3_engine_resolution_from_settings_default_engine() {
    // Workflow YAML with default_engine in settings; no engine in task params.
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: agent
    max_time_seconds: 60
    default_engine: opencode
    model_stylesheet:
      model: gpt-4o
  tasks:
    - id: agent
      operator: "AgentOperator"
      params:
        prompt: "do the thing"
      terminal: success
"#;
    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), workflow).unwrap();
    let doc = schema::parse_workflow(file.path()).unwrap();
    // Verify settings parsed correctly
    assert_eq!(
        doc.workflow.settings.default_engine.as_deref(),
        Some("opencode")
    );
    assert_eq!(
        doc.workflow
            .settings
            .model_stylesheet
            .as_ref()
            .map(|ms| ms.model.as_str()),
        Some("gpt-4o")
    );
}

// ── ModelStylesheet schema ────────────────────────────────────────────────────

#[test]
fn model_stylesheet_context_fidelity_defaults_to_summary() {
    let ms = ModelStylesheet {
        model: "gpt-4o".to_string(),
        context_fidelity: ContextFidelity::default(),
    };
    // Default context_fidelity is Summary
    assert!(matches!(ms.context_fidelity, ContextFidelity::Summary));
}

#[test]
fn workflow_settings_default_engine_and_stylesheet_default_to_none() {
    let settings = WorkflowSettings::default();
    assert!(settings.default_engine.is_none());
    assert!(settings.model_stylesheet.is_none());
}

#[test]
fn model_stylesheet_round_trips_yaml() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: start
    max_time_seconds: 60
    default_engine: opencode
    model_stylesheet:
      model: test-model
      context_fidelity: full
  tasks:
    - id: start
      operator: "NoOpOperator"
      terminal: success
"#;
    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), workflow).unwrap();
    let doc = schema::parse_workflow(file.path()).unwrap();
    let ms = doc.workflow.settings.model_stylesheet.as_ref().unwrap();
    assert_eq!(ms.model, "test-model");
    assert!(matches!(ms.context_fidelity, ContextFidelity::Full));
}

// ── H9: WFG-LINT-111 invalid regex in signals ─────────────────────────────

#[test]
fn h9_lint_111_invalid_signal_regex() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: agent
    max_time_seconds: 60
    default_engine: command
  tasks:
    - id: agent
      operator: "AgentOperator"
      params:
        engine: command
        engine_command: ["echo", "hi"]
        signals:
          bad: "["
      terminal: success
"#;
    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), workflow).unwrap();
    let doc = schema::parse_workflow(file.path()).unwrap();
    let results = LintRegistry::new().run(&doc);
    let lint_111: Vec<_> = results
        .iter()
        .filter(|r| r.code == "WFG-LINT-111")
        .collect();
    assert!(
        !lint_111.is_empty(),
        "expected WFG-LINT-111 for invalid regex"
    );
    assert_eq!(lint_111[0].severity, LintSeverity::Warning);
}

// ── H9b: WFG-LINT-111 signal with \\n ────────────────────────────────────────

#[test]
fn h9b_lint_111_newline_in_signal_pattern() {
    let workflow = "version: \"2.0\"\nmode: workflow_graph\nworkflow:\n  settings:\n    entry_task: agent\n    max_time_seconds: 60\n    default_engine: command\n  tasks:\n    - id: agent\n      operator: \"AgentOperator\"\n      params:\n        engine: command\n        engine_command: [\"echo\", \"hi\"]\n        signals:\n          bad: \"foo\\nbar\"\n      terminal: success\n";
    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), workflow).unwrap();
    let doc = schema::parse_workflow(file.path()).unwrap();
    let results = LintRegistry::new().run(&doc);
    let lint_111: Vec<_> = results
        .iter()
        .filter(|r| r.code == "WFG-LINT-111")
        .collect();
    assert!(
        !lint_111.is_empty(),
        "expected WFG-LINT-111 for \\n in signal pattern"
    );
}

// ── H10: WFG-LINT-110 no engine ──────────────────────────────────────────────

#[test]
fn h10_lint_110_no_engine_in_params_or_settings() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: agent
    max_time_seconds: 60
  tasks:
    - id: agent
      operator: "AgentOperator"
      params:
        prompt: "test"
      terminal: success
"#;
    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), workflow).unwrap();
    let doc = schema::parse_workflow(file.path()).unwrap();
    let results = LintRegistry::new().run(&doc);
    let lint_110: Vec<_> = results
        .iter()
        .filter(|r| r.code == "WFG-LINT-110")
        .collect();
    assert!(
        !lint_110.is_empty(),
        "expected WFG-LINT-110 when no engine is resolvable"
    );
    assert_eq!(lint_110[0].severity, LintSeverity::Warning);
}

// ── H11: WFG-LINT-113 unbounded loop ─────────────────────────────────────────

#[test]
fn h11_lint_113_loop_true_no_max_iterations() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: agent
    max_time_seconds: 60
    default_engine: command
  tasks:
    - id: agent
      operator: "AgentOperator"
      params:
        engine: command
        engine_command: ["echo", "hi"]
        loop: true
      terminal: success
"#;
    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), workflow).unwrap();
    let doc = schema::parse_workflow(file.path()).unwrap();
    let results = LintRegistry::new().run(&doc);
    let lint_113: Vec<_> = results
        .iter()
        .filter(|r| r.code == "WFG-LINT-113")
        .collect();
    assert!(
        !lint_113.is_empty(),
        "expected WFG-LINT-113 for loop:true without max_iterations"
    );
}

// ── H14: WFG-LINT-114 command engine no engine_command ───────────────────────

#[test]
fn h14_lint_114_command_engine_no_engine_command() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: agent
    max_time_seconds: 60
  tasks:
    - id: agent
      operator: "AgentOperator"
      params:
        engine: command
      terminal: success
"#;
    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), workflow).unwrap();
    let doc = schema::parse_workflow(file.path()).unwrap();
    let results = LintRegistry::new().run(&doc);
    let lint_114: Vec<_> = results
        .iter()
        .filter(|r| r.code == "WFG-LINT-114")
        .collect();
    assert!(
        !lint_114.is_empty(),
        "expected WFG-LINT-114 for command engine without engine_command"
    );
}

// ── WFG-LINT-115: named engine without prompt ────────────────────────────────

#[test]
fn lint_115_named_engine_no_prompt() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: agent
    max_time_seconds: 60
    default_engine: opencode
    model_stylesheet:
      model: gpt-4o
  tasks:
    - id: agent
      operator: "AgentOperator"
      params: {}
      terminal: success
"#;
    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), workflow).unwrap();
    let doc = schema::parse_workflow(file.path()).unwrap();
    let results = LintRegistry::new().run(&doc);
    let lint_115: Vec<_> = results
        .iter()
        .filter(|r| r.code == "WFG-LINT-115")
        .collect();
    assert!(
        !lint_115.is_empty(),
        "expected WFG-LINT-115 for named engine without prompt"
    );
}

// ── No lint-110 when engine is in params ─────────────────────────────────────

#[test]
fn no_lint_110_when_engine_in_params() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: agent
    max_time_seconds: 60
  tasks:
    - id: agent
      operator: "AgentOperator"
      params:
        engine: command
        engine_command: ["echo", "hi"]
      terminal: success
"#;
    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), workflow).unwrap();
    let doc = schema::parse_workflow(file.path()).unwrap();
    let results = LintRegistry::new().run(&doc);
    let lint_110: Vec<_> = results
        .iter()
        .filter(|r| r.code == "WFG-LINT-110")
        .collect();
    assert!(
        lint_110.is_empty(),
        "should not have WFG-LINT-110 when engine is in params"
    );
}

// ── No lint-113 when max_iterations is set ───────────────────────────────────

#[test]
fn no_lint_113_when_max_iterations_set() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: agent
    max_time_seconds: 60
    default_engine: command
  tasks:
    - id: agent
      operator: "AgentOperator"
      params:
        engine: command
        engine_command: ["echo", "hi"]
        loop: true
        max_iterations: 10
      terminal: success
"#;
    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), workflow).unwrap();
    let doc = schema::parse_workflow(file.path()).unwrap();
    let results = LintRegistry::new().run(&doc);
    let lint_113: Vec<_> = results
        .iter()
        .filter(|r| r.code == "WFG-LINT-113")
        .collect();
    assert!(
        lint_113.is_empty(),
        "should not have WFG-LINT-113 when max_iterations is set"
    );
}
