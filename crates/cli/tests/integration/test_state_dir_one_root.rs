//! PR-2 · B4 — one state root (spec 074, tranche 1).
//!
//! `--state-dir` (and its NEWTON_STATE_DIR / newton.toml precedents) MUST
//! relocate the durable store, checkpoints, and artifacts as one tree, so
//! that the executor's DbSink, the grading operators, `newton data`, and
//! `newton workflow runs` all read and write the SAME database. Before this
//! fix three consumers hard-coded the workspace default and silently
//! split-brained against any `--state-dir` override:
//!   - grading operators (commands/mod.rs open_workspace_store)
//!   - `newton data` (commands/data.rs)
//!   - `newton workflow runs list`/`show` (commands/log.rs)
//!
//! These tests exercise the CLI seam end-to-end (prior art:
//! tests/integration/test_data_post_grade_local_store.rs).

#[path = "../support/mod.rs"]
mod support;

use serde_json::{json, Value};

fn run_json(cmd: &mut assert_cmd::Command) -> Value {
    let out = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&out).expect("stdout is valid JSON")
}

/// Grading workflow run with `--state-dir X`: backend.sqlite is created and
/// written under X, the workspace-default location is never touched, and
/// `newton data get ... --state-dir X` reads back what the run wrote.
#[test]
fn grading_run_with_state_dir_writes_isolated_store_and_data_reads_it_back() {
    let ws = support::TempWorkspace::new();
    let ws_path = ws.path().to_string_lossy().to_string();

    let override_dir = tempfile::tempdir().expect("override state dir");
    let override_path = override_dir.path().to_string_lossy().to_string();

    // Seed the FK chain (product -> component -> repo) directly into the
    // OVERRIDE store, via `data post --state-dir`, so the grading workflow's
    // create_eval_run FK check (scope=repo, scope_id) resolves against the
    // same database the run will use.
    let product = run_json(support::newton().args([
        "data",
        "post",
        "product",
        "--workspace",
        &ws_path,
        "--state-dir",
        &override_path,
        "--body",
        r#"{"name":"Product SD"}"#,
    ]));
    let product_id = product["id"].as_str().expect("product id").to_string();

    let component_body = json!({
        "name": "Component SD",
        "productId": product_id,
        "domain": "backend",
        "owner": "team-a",
        "criticality": "high",
        "autonomy": "full",
        "lastEval": "2026-05-26T00:00:00Z"
    });
    let component = run_json(support::newton().args([
        "data",
        "post",
        "component",
        "--workspace",
        &ws_path,
        "--state-dir",
        &override_path,
        "--body",
        &component_body.to_string(),
    ]));
    let component_id = component["id"].as_str().expect("component id").to_string();

    let repo_body = json!({
        "name": "repo-sd",
        "componentId": component_id,
        "owner": "team-a",
        "criticality": "high",
        "autonomy": "full",
        "qualityScore": 0,
        "coverage": 0,
        "secScore": 0,
        "execStatus": "unknown",
        "lastEval": "2026-05-26T00:00:00Z"
    });
    let repo = run_json(support::newton().args([
        "data",
        "post",
        "repo",
        "--workspace",
        &ws_path,
        "--state-dir",
        &override_path,
        "--body",
        &repo_body.to_string(),
    ]));
    let repo_id = repo["id"].as_str().expect("repo id").to_string();

    // A single-task grading workflow using GraderCommandOperator, which
    // registers only when a grading operators' backend store resolves —
    // exercising the commands/mod.rs::open_state_store fix.
    let workflow_yaml = r#"version: "2.0"
mode: workflow_graph
metadata:
  name: "grading state-dir test"
workflow:
  settings:
    entry_task: grade_repo
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 1
    max_workflow_iterations: 10
  tasks:
    - id: grade_repo
      operator: GraderCommandOperator
      terminal: success
      params:
        grader: "state-dir-test-grader"
        scope: "repo"
        scope_id: "__REPO_ID__"
        cmd: "printf '%s' '{\"overall_score\": 80, \"verdict\": \"approve\", \"summary\": \"ok\", \"scores\": [{\"dimension\": \"tests\", \"score\": 80}]}'"
"#
    .replace("__REPO_ID__", &repo_id);
    let workflow_path = ws.write_workflow("grading_state_dir.yaml", &workflow_yaml);

    support::newton()
        .arg("workflow")
        .arg("run")
        .arg(&workflow_path)
        .arg("--workspace")
        .arg(&ws_path)
        .arg("--state-dir")
        .arg(&override_path)
        .assert()
        .success();

    // Backend store landed under the override root...
    let override_db = override_dir.path().join("backend.sqlite");
    assert!(
        override_db.exists(),
        "expected backend.sqlite under override state dir at {}",
        override_db.display()
    );

    // ...and the workspace-default state root was never created: the run
    // and every `data post` above used --state-dir throughout, so if any
    // consumer had silently fallen back to the workspace default it would
    // have created this path as a side effect of opening it (sqlite
    // mode=rwc creates the file on open).
    let default_db = ws.path().join(".newton/state/backend.sqlite");
    assert!(
        !default_db.exists(),
        "workspace-default backend.sqlite must not exist — the run must have used ONLY the \
         override state dir, not split-brained against the workspace default: {}",
        default_db.display()
    );

    // `newton data get grades --state-dir X` reads back what the grading
    // operator wrote to X.
    let grades = run_json(support::newton().args([
        "data",
        "get",
        "grades",
        "--workspace",
        &ws_path,
        "--state-dir",
        &override_path,
        "--json",
    ]));
    let grades_arr = grades.as_array().expect("grades response is an array");
    assert_eq!(
        grades_arr.len(),
        1,
        "expected exactly one grade written by the grading run; got {grades:?}"
    );
    assert_eq!(grades_arr[0]["dimension"], "tests");
    assert_eq!(grades_arr[0]["score"], 80.0);

    // Same for eval-runs: the EvalRun the operator persisted is visible
    // through `data get eval-runs --state-dir X`.
    let eval_runs = run_json(support::newton().args([
        "data",
        "get",
        "eval-runs",
        "--workspace",
        &ws_path,
        "--state-dir",
        &override_path,
        "--json",
    ]));
    let eval_runs_arr = eval_runs
        .as_array()
        .expect("eval-runs response is an array");
    assert_eq!(eval_runs_arr.len(), 1);
    assert_eq!(eval_runs_arr[0]["scopeId"], repo_id);
    assert_eq!(eval_runs_arr[0]["score"], 80.0);
}

/// `newton workflow runs list` honors `--state-dir` the same way `run` does:
/// a run executed with `--state-dir X` is visible via `runs list --state-dir
/// X` and invisible via the workspace-default `runs list` (no split brain).
#[test]
fn runs_list_with_state_dir_sees_run_executed_with_state_dir() {
    let ws = support::TempWorkspace::new();
    let ws_path = ws.path().to_string_lossy().to_string();

    let override_dir = tempfile::tempdir().expect("override state dir");
    let override_path = override_dir.path().to_string_lossy().to_string();

    let workflow_yaml = r#"version: "2.0"
mode: workflow_graph
metadata:
  name: "runs list state-dir test"
workflow:
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 1
    max_workflow_iterations: 10
  tasks:
    - id: start
      operator: NoOpOperator
      terminal: success
      params: {}
"#;
    let workflow_path = ws.write_workflow("noop_state_dir.yaml", workflow_yaml);

    let envelope = run_json(
        support::newton()
            .arg("workflow")
            .arg("run")
            .arg(&workflow_path)
            .arg("--workspace")
            .arg(&ws_path)
            .arg("--state-dir")
            .arg(&override_path)
            .arg("--emit-completion-json"),
    );
    let execution_id = envelope["execution_id"]
        .as_str()
        .expect("completion envelope has execution_id")
        .to_string();

    // Visible with the same --state-dir override...
    let listed = run_json(support::newton().args([
        "workflow",
        "runs",
        "list",
        "--workspace",
        &ws_path,
        "--state-dir",
        &override_path,
        "--json",
    ]));
    let listed_arr = listed.as_array().expect("runs list is an array");
    assert!(
        listed_arr.iter().any(|e| e["execution_id"] == execution_id),
        "expected execution {execution_id} in --state-dir listing; got {listed_arr:?}"
    );

    // `runs show` also honors the override.
    support::newton()
        .args([
            "workflow",
            "runs",
            "show",
            "--run-id",
            &execution_id,
            "--workspace",
            &ws_path,
            "--state-dir",
            &override_path,
        ])
        .assert()
        .success();

    // ...invisible from the workspace-default listing (no --state-dir): the
    // run must not have split-brained into <ws>/.newton/state/workflows.
    let default_listed = run_json(support::newton().args([
        "workflow",
        "runs",
        "list",
        "--workspace",
        &ws_path,
        "--json",
    ]));
    let default_arr = default_listed
        .as_array()
        .expect("default runs list is an array");
    assert!(
        !default_arr
            .iter()
            .any(|e| e["execution_id"] == execution_id),
        "execution {execution_id} must NOT appear in the workspace-default runs list \
         (would indicate the run split-brained into the default state dir)"
    );
}
