//! Integration tests for the Finding and ChangeRequest data commands.
//! Replaces the old opportunity_crud suite (spec 061 clean-break rename).

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

fn minimal_finding_body(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "source": "test",
        "origin": "system",
        "dimension": "tests",
        "fingerprint": format!("fp-{id}"),
        "title": "Test finding",
        "whyItMatters": "Coverage gap detected",
        "recommendedAction": "Add more tests",
        "severity": "medium",
        "risk": "low",
        "status": "awaiting_triage",
        "dependsOn": [],
        "blocks": []
    })
}

fn post_finding(dir: &TempDir, body: serde_json::Value) -> serde_json::Value {
    let out = newton()
        .args([
            "data",
            "post",
            "finding",
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

// ── Test 1: POST happy path → 201 and correct id ─────────────────────────────

#[test]
fn finding_post_happy_path() {
    let dir = setup_workspace_with_db();
    let created = post_finding(&dir, minimal_finding_body("find-001"));

    assert_eq!(created["id"].as_str().unwrap(), "find-001");
    assert_eq!(created["status"].as_str().unwrap(), "awaiting_triage");
    assert_eq!(created["dimension"].as_str().unwrap(), "tests");
    assert_eq!(created["risk"].as_str().unwrap(), "low");
}

// ── Test 2: POST duplicate id → upsert, count remains 1 ──────────────────────

#[test]
fn finding_post_duplicate_upserts() {
    let dir = setup_workspace_with_db();

    post_finding(&dir, minimal_finding_body("find-002"));

    let mut body2 = minimal_finding_body("find-002");
    body2["title"] = serde_json::json!("Updated title");
    let updated = post_finding(&dir, body2);

    assert_eq!(updated["id"].as_str().unwrap(), "find-002");
    assert_eq!(updated["title"].as_str().unwrap(), "Updated title");

    let out = newton()
        .args([
            "data",
            "get",
            "findings",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let list: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    let count = list
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["id"].as_str() == Some("find-002"))
        .count();
    assert_eq!(count, 1, "upsert must not create duplicate records");
}

// ── Test 3: PATCH status works ────────────────────────────────────────────────

#[test]
fn finding_patch_still_works() {
    let dir = setup_workspace_with_db();
    post_finding(&dir, minimal_finding_body("find-003"));

    let patch_body = serde_json::json!({"status": "triaged"});
    let out = newton()
        .args([
            "data",
            "patch",
            "finding",
            "find-003",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &serde_json::to_string(&patch_body).unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let patched: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(patched["status"].as_str().unwrap(), "triaged");
}

// ── Test 4: GET findings (list) exits 0 ──────────────────────────────────────

#[test]
fn finding_get_list_exits_ok() {
    let dir = setup_workspace_with_db();

    newton()
        .args([
            "data",
            "get",
            "findings",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success();
}

// ── Test 5: POST finding then GET by id exits 0 ───────────────────────────────

#[test]
fn finding_get_by_id_exits_ok() {
    let dir = setup_workspace_with_db();
    post_finding(&dir, minimal_finding_body("find-005"));

    let out = newton()
        .args([
            "data",
            "get",
            "finding",
            "find-005",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let item: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(item["id"].as_str().unwrap(), "find-005");
}

// ── Test 6: PATCH unknown finding returns error ───────────────────────────────

#[test]
fn finding_patch_not_found_returns_error() {
    let dir = setup_workspace_with_db();

    let patch_body = serde_json::json!({"status": "triaged"});
    let out = newton()
        .args([
            "data",
            "patch",
            "finding",
            "no-such-id",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &serde_json::to_string(&patch_body).unwrap(),
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ERR_NOT_FOUND"),
        "expected ERR_NOT_FOUND in stderr, got: {stderr}"
    );
}

// ── Test 7: GET change-requests (list) exits 0 ────────────────────────────────

#[test]
fn change_request_get_list_exits_ok() {
    let dir = setup_workspace_with_db();

    newton()
        .args([
            "data",
            "get",
            "change-requests",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success();
}

// ── Test 8: POST change-request happy path ────────────────────────────────────

#[test]
fn change_request_post_happy_path() {
    let dir = setup_workspace_with_db();

    let body = serde_json::json!({
        "id": "cr-001",
        "title": "Add MFA to login flow",
        "origin": "system",
        "findingIds": []
    });
    let out = newton()
        .args([
            "data",
            "post",
            "change-request",
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
    let created: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(created["id"].as_str().unwrap(), "cr-001");
    assert_eq!(created["status"].as_str().unwrap(), "proposed");
}
