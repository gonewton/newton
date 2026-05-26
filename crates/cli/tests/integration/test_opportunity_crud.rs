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

fn post_opportunity(dir: &TempDir, body: serde_json::Value) -> serde_json::Value {
    let out = newton()
        .args([
            "data",
            "post",
            "opportunity",
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
fn opportunity_post_happy_path() {
    let dir = setup_workspace_with_db();

    let body = serde_json::json!({
        "id": "test-001",
        "title": "Test opportunity",
        "origin": "test",
        "risk": "low",
        "expectedValue": 1.0
    });
    let created = post_opportunity(&dir, body);

    assert_eq!(created["id"].as_str().unwrap(), "test-001");
    assert_eq!(created["title"].as_str().unwrap(), "Test opportunity");
    assert_eq!(created["status"].as_str().unwrap(), "awaiting_triage");
    assert_eq!(created["risk"].as_str().unwrap(), "low");
}

// ── Test 2: POST duplicate id → upsert, count remains 1 ──────────────────────

#[test]
fn opportunity_post_duplicate_upserts() {
    let dir = setup_workspace_with_db();

    let body1 = serde_json::json!({
        "id": "test-002",
        "title": "Original title",
        "origin": "test",
        "risk": "low",
        "expectedValue": 1.0
    });
    post_opportunity(&dir, body1);

    let body2 = serde_json::json!({
        "id": "test-002",
        "title": "Updated title",
        "origin": "test",
        "risk": "medium",
        "expectedValue": 2.0
    });
    let updated = post_opportunity(&dir, body2);

    assert_eq!(updated["id"].as_str().unwrap(), "test-002");
    assert_eq!(updated["title"].as_str().unwrap(), "Updated title");

    // Verify only one record with that id exists
    let out = newton()
        .args([
            "data",
            "get",
            "opportunities",
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
        .filter(|o| o["id"].as_str() == Some("test-002"))
        .count();
    assert_eq!(count, 1, "expected exactly one record with id test-002");
}

// ── Test 3: PATCH still works after opportunity arms added ────────────────────

#[test]
fn opportunity_patch_still_works() {
    let dir = setup_workspace_with_db();

    let body = serde_json::json!({
        "id": "test-003",
        "title": "Patchable",
        "origin": "test",
        "risk": "low",
        "expectedValue": 0.5
    });
    post_opportunity(&dir, body);

    let patch_body = serde_json::json!({"status": "triaged"});
    let out = newton()
        .args([
            "data",
            "patch",
            "opportunity",
            "test-003",
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

// ── Test 4: POST invalid status → validation error ────────────────────────────

#[test]
fn opportunity_post_invalid_status() {
    let dir = setup_workspace_with_db();

    let body_str = serde_json::to_string(&serde_json::json!({
        "id": "test-004",
        "title": "Bad status",
        "origin": "test",
        "risk": "low",
        "expectedValue": 1.0,
        "status": "invalid-value"
    }))
    .unwrap();

    let out = newton()
        .args([
            "data",
            "post",
            "opportunity",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &body_str,
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ERR_VALIDATION"),
        "expected ERR_VALIDATION in stderr, got: {stderr}"
    );
}

// ── Test 5: POST with unknown component → succeeds (soft warn) ────────────────

#[test]
fn opportunity_post_unknown_component_soft_warn() {
    let dir = setup_workspace_with_db();

    let body = serde_json::json!({
        "id": "test-005",
        "title": "Unknown component",
        "origin": "test",
        "risk": "low",
        "expectedValue": 0.0,
        "component": "nonexistent-component-xyz"
    });
    let created = post_opportunity(&dir, body);

    assert_eq!(created["id"].as_str().unwrap(), "test-005");
    assert_eq!(created["component"].as_str().unwrap(), "");
}

// ── Test 6: GET opportunities (list) exits 0 ─────────────────────────────────

#[test]
fn opportunity_get_list_exits_ok() {
    let dir = setup_workspace_with_db();

    newton()
        .args([
            "data",
            "get",
            "opportunities",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success();
}

// ── Test 7: POST confidence > 1.0 → validation error ─────────────────────────

#[test]
fn opportunity_post_invalid_confidence() {
    let dir = setup_workspace_with_db();

    let body_str = serde_json::to_string(&serde_json::json!({
        "id": "test-007",
        "title": "Bad confidence",
        "origin": "test",
        "risk": "low",
        "expectedValue": 1.0,
        "confidence": 1.5
    }))
    .unwrap();

    let out = newton()
        .args([
            "data",
            "post",
            "opportunity",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &body_str,
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ERR_VALIDATION"),
        "expected ERR_VALIDATION in stderr, got: {stderr}"
    );
}

// ── Test 8: POST expectedValue < 0 → validation error ────────────────────────

#[test]
fn opportunity_post_negative_expected_value() {
    let dir = setup_workspace_with_db();

    let body_str = serde_json::to_string(&serde_json::json!({
        "id": "test-008",
        "title": "Negative value",
        "origin": "test",
        "risk": "low",
        "expectedValue": -1.0
    }))
    .unwrap();

    let out = newton()
        .args([
            "data",
            "post",
            "opportunity",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &body_str,
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ERR_VALIDATION"),
        "expected ERR_VALIDATION in stderr, got: {stderr}"
    );
}
