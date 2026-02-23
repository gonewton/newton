use newton::core::workflow_graph::{schema, transform};
use std::fs;
use tempfile::NamedTempFile;

fn write_workflow(yaml: &str) -> NamedTempFile {
    let file = NamedTempFile::new().expect("temp file");
    fs::write(file.path(), yaml).expect("write workflow");
    file
}

#[test]
fn f1_macro_expansion_generates_unique_ids() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
macros:
  - name: gate
    tasks:
      - id: "{{ prefix }}_scan"
        operator: NoOpOperator
        params: {}
workflow:
  context: {}
  settings:
    entry_task: start_scan
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
  tasks:
    - macro: gate
      with:
        prefix: start
"#;
    let file = write_workflow(workflow);
    let raw = schema::parse_workflow(file.path()).expect("parse");
    let transformed = transform::apply_default_pipeline(raw).expect("transform");
    assert!(transformed
        .workflow
        .tasks()
        .any(|task| task.id == "start_scan"));
}

#[test]
fn f2_macro_expansion_id_collision_returns_wfg_macro_001() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
macros:
  - name: gate
    tasks:
      - id: "dup"
        operator: NoOpOperator
        params: {}
workflow:
  context: {}
  settings:
    entry_task: dup
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
  tasks:
    - macro: gate
      with: {}
    - id: dup
      operator: NoOpOperator
      params: {}
"#;
    let file = write_workflow(workflow);
    let raw = schema::parse_workflow(file.path()).expect("parse");
    let err = transform::apply_default_pipeline(raw).expect_err("collision should fail");
    assert_eq!(err.code, "WFG-MACRO-001");
}

#[test]
fn f3_include_if_false_removes_task() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: keep
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
  tasks:
    - id: keep
      operator: NoOpOperator
      params: {}
      transitions:
        - to: maybe
    - id: maybe
      operator: NoOpOperator
      include_if:
        $expr: "false"
      params: {}
"#;
    let file = write_workflow(workflow);
    let raw = schema::parse_workflow(file.path()).expect("parse");
    let transformed = transform::apply_default_pipeline(raw).expect("transform");
    assert!(transformed.workflow.tasks().all(|task| task.id != "maybe"));
    let keep = transformed
        .workflow
        .tasks()
        .find(|task| task.id == "keep")
        .expect("keep task");
    assert!(keep
        .transitions
        .iter()
        .all(|transition| transition.to != "maybe"));
}

#[test]
fn f4_include_if_tasks_reference_rejected() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
  tasks:
    - id: start
      operator: NoOpOperator
      include_if:
        $expr: "tasks.start.status == 'success'"
      params: {}
"#;
    let file = write_workflow(workflow);
    let raw = schema::parse_workflow(file.path()).expect("parse");
    let err = transform::apply_default_pipeline(raw).expect_err("tasks.* in include_if");
    assert_eq!(err.code, "WFG-INCLUDE-001");
}

#[test]
fn f5_template_interpolation_works() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
triggers:
  type: manual
  schema_version: "1"
  payload:
    pr_number: 42
workflow:
  context:
    env: dev
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
  tasks:
    - id: start
      operator: NoOpOperator
      params:
        msg: "PR {{ triggers.pr_number }} env={{ context.env }}"
"#;
    let file = write_workflow(workflow);
    let raw = schema::parse_workflow(file.path()).expect("parse");
    let transformed = transform::apply_default_pipeline(raw).expect("transform");
    let start = transformed
        .workflow
        .tasks()
        .find(|task| task.id == "start")
        .expect("start");
    assert_eq!(start.params["msg"], "PR 42 env=dev");
}

#[test]
fn f6_template_parse_error_returns_wfg_tpl_001() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
  tasks:
    - id: start
      operator: NoOpOperator
      params:
        msg: "{{ context.foo "
"#;
    let file = write_workflow(workflow);
    let raw = schema::parse_workflow(file.path()).expect("parse");
    let err = transform::apply_default_pipeline(raw).expect_err("invalid template");
    assert_eq!(err.code, "WFG-TPL-001");
}

#[test]
fn f7_expr_precompile_reports_wfg_lint_005() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
  tasks:
    - id: start
      operator: NoOpOperator
      params:
        bad:
          $expr: "1 +"
"#;
    let file = write_workflow(workflow);
    let raw = schema::parse_workflow(file.path()).expect("parse");
    let err = transform::apply_default_pipeline(raw).expect_err("invalid expr");
    assert_eq!(err.code, "WFG-LINT-005");
}

#[test]
fn f8_transform_output_is_deterministic() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
macros:
  - name: gate
    tasks:
      - id: "{{ prefix }}_task"
        operator: NoOpOperator
        params:
          msg: "{{ prefix }}"
workflow:
  context: {}
  settings:
    entry_task: a_task
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
  tasks:
    - macro: gate
      with:
        prefix: a
"#;
    let file = write_workflow(workflow);
    let raw_a = schema::parse_workflow(file.path()).expect("parse");
    let raw_b = schema::parse_workflow(file.path()).expect("parse");
    let a = transform::apply_default_pipeline(raw_a).expect("transform a");
    let b = transform::apply_default_pipeline(raw_b).expect("transform b");
    let a_json = serde_json::to_string(&a).expect("serialize");
    let b_json = serde_json::to_string(&b).expect("serialize");
    assert_eq!(a_json, b_json);
}
