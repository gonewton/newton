use newton_core::workflow::{
    lint::{LintRegistry, LintSeverity},
    schema, transform,
};
use std::fs;
use std::path::PathBuf;
use tempfile::NamedTempFile;

const VALID_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 2
    max_workflow_iterations: 5
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: done
          when:
            $expr: "true"
    - id: done
      operator: NoOpOperator
      params: {}
"#;

const INVALID_TRANSITION: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 2
    max_workflow_iterations: 5
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: missing
          when:
            $expr: "true"
"#;

#[test]
fn valid_workflow_parses_and_validates() {
    let file = NamedTempFile::new().expect("temp file");
    let path = file.path().to_owned();
    drop(file);
    fs::write(&path, VALID_WORKFLOW).unwrap();
    let workflow = schema::load_workflow(&path);
    assert!(workflow.is_ok());
}

#[test]
fn invalid_transition_reports_error() {
    let file = NamedTempFile::new().expect("temp file");
    let path = file.path().to_owned();
    drop(file);
    fs::write(&path, INVALID_TRANSITION).unwrap();
    let workflow = schema::load_workflow(&path);
    assert!(workflow.is_err());
    let err = workflow.err().unwrap();
    assert!(err.message.contains("unknown task"));
}

/// Every workflow shipped in the `newton init` template must pass
/// `newton workflow validate` and `newton workflow lint` (no error-severity
/// results), mirroring what those two commands run.
#[test]
fn template_workflows_validate_and_lint_cleanly() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/newton-template/newton/workflows");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("yaml" | "yml")
            )
        })
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no workflows found in {}", dir.display());

    let mut failures = Vec::new();
    for path in &paths {
        if let Err(err) = schema::load_workflow(path) {
            failures.push(format!("{}: validate: {err}", path.display()));
            continue;
        }
        let linted = schema::parse_workflow(path)
            .and_then(|doc| transform::apply_default_pipeline(doc, false))
            .map(|doc| LintRegistry::new().run(&doc));
        match linted {
            Ok(results) => {
                for result in results
                    .iter()
                    .filter(|result| result.severity == LintSeverity::Error)
                {
                    failures.push(format!("{}: lint: {result:?}", path.display()));
                }
            }
            Err(err) => failures.push(format!("{}: lint: {err}", path.display())),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
