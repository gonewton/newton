#[path = "../support/mod.rs"]
mod support;

use std::fs;
use support::newton;
use tempfile::TempDir;

fn setup_workspace_with_db() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".newton/state")).unwrap();
    dir
}

/// Helper: create a product, then a component inside it; returns (product_id, component_id).
fn create_component(dir: &TempDir) -> (String, String) {
    let product_body = serde_json::json!({"name": "EvalRunGradeTestProduct"});
    let out = newton()
        .args([
            "data",
            "post",
            "product",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &serde_json::to_string(&product_body).unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let p: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    let product_id = p["id"].as_str().unwrap().to_string();

    let comp_body = serde_json::json!({
        "name": "EvalRunGradeTestComp",
        "productId": product_id,
        "domain": "engineering",
        "owner": "test-owner",
        "criticality": "low",
        "autonomy": "low",
        "lastEval": "2026-01-01T00:00:00Z"
    });
    let out2 = newton()
        .args([
            "data",
            "post",
            "component",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &serde_json::to_string(&comp_body).unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let c: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out2.stdout)).unwrap();
    let comp_id = c["id"].as_str().unwrap().to_string();
    (product_id, comp_id)
}

fn post_eval_run(dir: &TempDir, body: serde_json::Value) -> serde_json::Value {
    let out = newton()
        .args([
            "data",
            "post",
            "eval-run",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &serde_json::to_string(&body).unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap()
}

fn post_grade(dir: &TempDir, body: serde_json::Value) -> serde_json::Value {
    let out = newton()
        .args([
            "data",
            "post",
            "grade",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &serde_json::to_string(&body).unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap()
}

// ── Acceptance 1: EvalRun history (two runs for same scope + scopeId) ─────────

#[test]
fn eval_run_history_preserved() {
    let dir = setup_workspace_with_db();
    let (_, comp_id) = create_component(&dir);

    let run1 = serde_json::json!({
        "id": "evalrun.test.component.comp.1",
        "source": "dk-review",
        "scope": "component",
        "scopeId": comp_id.clone(),
        "score": 70,
        "verdict": "approve_with_comments",
        "summary": "first run",
        "evaluatedAt": "2026-05-26T00:00:00Z"
    });
    let run2 = serde_json::json!({
        "id": "evalrun.test.component.comp.2",
        "source": "dk-review",
        "scope": "component",
        "scopeId": comp_id.clone(),
        "score": 72,
        "verdict": "approve_with_comments",
        "summary": "second run",
        "evaluatedAt": "2026-05-26T00:10:00Z"
    });
    post_eval_run(&dir, run1);
    post_eval_run(&dir, run2);

    let out = newton()
        .args([
            "data",
            "get",
            "eval-runs",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let runs: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    let matches = runs
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| {
            r["scope"].as_str() == Some("component")
                && r["scopeId"].as_str() == Some(comp_id.as_str())
        })
        .count();
    assert!(
        matches >= 2,
        "expected >= 2 eval runs for same (scope, scopeId), got {matches}"
    );
}

// ── Acceptance 2: Grade.runId must exist (ERR_NOT_FOUND) ──────────────────────

#[test]
fn grade_post_unknown_run_id_returns_not_found() {
    let dir = setup_workspace_with_db();

    let body = serde_json::json!({
        "id": "grade.unknown.run.tests",
        "runId": "evalrun.does.not.exist",
        "kpiId": null,
        "dimension": "tests",
        "score": 60,
        "evidence": {"findings": 1},
        "evaluatedAt": "2026-05-26T00:00:00Z"
    });

    newton()
        .args([
            "data",
            "post",
            "grade",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &serde_json::to_string(&body).unwrap(),
            "--json",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("ERR_NOT_FOUND"));
}

// ── Acceptance 3: Grade.score bounds (ERR_VALIDATION) ─────────────────────────

#[test]
fn grade_post_score_out_of_range_returns_validation_error() {
    let dir = setup_workspace_with_db();
    let (_, comp_id) = create_component(&dir);

    post_eval_run(
        &dir,
        serde_json::json!({
            "id": "evalrun.test.component.score-bounds",
            "source": "dk-review",
            "scope": "component",
            "scopeId": comp_id,
            "score": null,
            "verdict": null,
            "summary": null,
            "evaluatedAt": "2026-05-26T00:00:00Z"
        }),
    );

    let body = serde_json::json!({
        "id": "grade.evalrun.test.component.score-bounds.tests",
        "runId": "evalrun.test.component.score-bounds",
        "kpiId": null,
        "dimension": "tests",
        "score": 101,
        "evidence": null,
        "evaluatedAt": "2026-05-26T00:00:00Z"
    });

    newton()
        .args([
            "data",
            "post",
            "grade",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &serde_json::to_string(&body).unwrap(),
            "--json",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("ERR_VALIDATION"));
}

// ── Acceptance 4: UNIQUE(runId, dimension) conflict (ERR_CONFLICT) ────────────

#[test]
fn grade_duplicate_dimension_conflicts_and_does_not_overwrite() {
    let dir = setup_workspace_with_db();
    let (_, comp_id) = create_component(&dir);

    post_eval_run(
        &dir,
        serde_json::json!({
            "id": "evalrun.test.component.dup-dim",
            "source": "dk-review",
            "scope": "component",
            "scopeId": comp_id,
            "score": 70,
            "verdict": null,
            "summary": null,
            "evaluatedAt": "2026-05-26T00:00:00Z"
        }),
    );

    let first = post_grade(
        &dir,
        serde_json::json!({
            "id": "grade.evalrun.test.component.dup-dim.tests",
            "runId": "evalrun.test.component.dup-dim",
            "kpiId": null,
            "dimension": "tests",
            "score": 60,
            "evidence": {"findings": 3},
            "evaluatedAt": "2026-05-26T00:00:00Z"
        }),
    );
    assert_eq!(first["score"].as_f64().unwrap(), 60.0);

    let second = serde_json::json!({
        "id": "grade.evalrun.test.component.dup-dim.tests.v2",
        "runId": "evalrun.test.component.dup-dim",
        "kpiId": null,
        "dimension": "tests",
        "score": 10,
        "evidence": {"findings": 999},
        "evaluatedAt": "2026-05-26T00:00:00Z"
    });

    newton()
        .args([
            "data",
            "post",
            "grade",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &serde_json::to_string(&second).unwrap(),
            "--json",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("ERR_CONFLICT"));

    let out = newton()
        .args([
            "data",
            "get",
            "grade",
            "grade.evalrun.test.component.dup-dim.tests",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let fetched: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(fetched["score"].as_f64().unwrap(), 60.0);
}
