#[path = "../support/mod.rs"]
mod support;

use serde_json::json;

fn run_json(mut cmd: assert_cmd::Command) -> serde_json::Value {
    let out = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&out).expect("stdout is valid JSON")
}

#[test]
fn test_newton_data_post_grade_writes_local_store() {
    let ws = support::TempWorkspace::new();
    let ws_path = ws.path().to_string_lossy().to_string();

    let product = run_json(
        support::newton().args([
            "data",
            "post",
            "product",
            "--workspace",
            &ws_path,
            "--body",
            r#"{"name":"Product A"}"#,
        ]),
    );
    let product_id = product["id"].as_str().expect("product id").to_string();

    let component_body = json!({
        "name": "Component A",
        "productId": product_id,
        "domain": "backend",
        "owner": "team-a",
        "criticality": "high",
        "autonomy": "full",
        "lastEval": "2026-05-26T00:00:00Z"
    });
    let component = run_json(
        support::newton().args([
            "data",
            "post",
            "component",
            "--workspace",
            &ws_path,
            "--body",
            &component_body.to_string(),
        ]),
    );
    let component_id = component["id"].as_str().expect("component id").to_string();

    let repo_body = json!({
        "name": "repo-a",
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
    let repo = run_json(
        support::newton().args([
            "data",
            "post",
            "repo",
            "--workspace",
            &ws_path,
            "--body",
            &repo_body.to_string(),
        ]),
    );
    let repo_id = repo["id"].as_str().expect("repo id").to_string();

    let run_id = "evalrun.test.repo.repo-a.2026-05-26T00:00:00Z";
    let eval_run_body = json!({
        "id": run_id,
        "source": "dk-review",
        "scope": "repo",
        "scopeId": repo_id,
        "score": 70,
        "verdict": "approve_with_comments",
        "summary": "ok",
        "evaluatedAt": "2026-05-26T00:00:00Z"
    });
    let eval_run = run_json(
        support::newton().args([
            "data",
            "post",
            "eval-run",
            "--workspace",
            &ws_path,
            "--body",
            &eval_run_body.to_string(),
        ]),
    );
    assert_eq!(eval_run["id"], run_id);

    let grade_id = format!("grade.{run_id}.tests");
    let grade_body = json!({
        "id": grade_id,
        "runId": run_id,
        "dimension": "tests",
        "score": 60,
        "evidence": { "findings": 3 },
        "evaluatedAt": "2026-05-26T00:00:00Z"
    });
    let grade = run_json(
        support::newton().args([
            "data",
            "post",
            "grade",
            "--workspace",
            &ws_path,
            "--body",
            &grade_body.to_string(),
        ]),
    );
    assert_eq!(grade["id"], grade_id);

    let grade_get = run_json(
        support::newton().args([
            "data",
            "get",
            "grade",
            &grade_id,
            "--workspace",
            &ws_path,
        ]),
    );
    assert_eq!(grade_get["id"], grade_id);
    assert_eq!(grade_get["runId"], run_id);
    assert_eq!(grade_get["dimension"], "tests");
}

