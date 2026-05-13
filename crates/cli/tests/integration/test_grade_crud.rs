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
    let product_body = serde_json::json!({"name": "GradeTestProduct"});
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
        "name": "GradeTestComp",
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

/// Helper: post a grade and return the parsed response.
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

// ── Test 1: POST happy path → correct fields ──────────────────────────────────

#[test]
fn grade_post_happy_path() {
    let dir = setup_workspace_with_db();
    let (_, comp_id) = create_component(&dir);

    let body = serde_json::json!({
        "scope": "component",
        "scopeId": comp_id,
        "indicator": "code-health",
        "score": 75.5,
        "metrics": {"coverage": 0.92}
    });
    let created = post_grade(&dir, body);

    let expected_id = format!("component.{}.code-health", comp_id);
    assert_eq!(created["id"].as_str().unwrap(), expected_id);
    assert!((created["score"].as_f64().unwrap() - 75.5).abs() < 0.001);
    assert_eq!(created["scope"].as_str().unwrap(), "component");
    assert_eq!(created["scopeId"].as_str().unwrap(), comp_id);
    assert_eq!(created["indicator"].as_str().unwrap(), "code-health");
}

// ── Test 2: POST score > 100 → validation error ───────────────────────────────

#[test]
fn grade_post_score_too_high() {
    let dir = setup_workspace_with_db();
    let (_, comp_id) = create_component(&dir);

    let body = serde_json::json!({
        "scope": "component",
        "scopeId": comp_id,
        "indicator": "code-health",
        "score": 101.0
    });
    let body_str = serde_json::to_string(&body).unwrap();
    let out = newton()
        .args([
            "data",
            "post",
            "grade",
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

// ── Test 3: POST score < 0 → validation error ─────────────────────────────────

#[test]
fn grade_post_score_negative() {
    let dir = setup_workspace_with_db();
    let (_, comp_id) = create_component(&dir);

    let body = serde_json::json!({
        "scope": "component",
        "scopeId": comp_id,
        "indicator": "code-health",
        "score": -1.0
    });
    let body_str = serde_json::to_string(&body).unwrap();
    let out = newton()
        .args([
            "data",
            "post",
            "grade",
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

// ── Test 4: POST unknown scope_id → ERR_NOT_FOUND ─────────────────────────────

#[test]
fn grade_post_unknown_scope_id() {
    let dir = setup_workspace_with_db();

    let body = serde_json::json!({
        "scope": "component",
        "scopeId": "nonexistent-component-xyz",
        "indicator": "code-health",
        "score": 50.0
    });
    let body_str = serde_json::to_string(&body).unwrap();
    let out = newton()
        .args([
            "data",
            "post",
            "grade",
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
        stderr.contains("ERR_NOT_FOUND"),
        "expected ERR_NOT_FOUND in stderr, got: {stderr}"
    );
}

// ── Test 5: POST unknown indicator → 201 + warnings populated ─────────────────

#[test]
fn grade_post_unknown_indicator_warns() {
    let dir = setup_workspace_with_db();
    let (_, comp_id) = create_component(&dir);

    let body = serde_json::json!({
        "scope": "component",
        "scopeId": comp_id,
        "indicator": "totally-unknown-indicator-xyz",
        "score": 50.0
    });
    let created = post_grade(&dir, body);

    let warnings = created["warnings"].as_array();
    assert!(
        warnings.is_some() && !warnings.unwrap().is_empty(),
        "expected non-empty warnings for unknown indicator, got: {created}"
    );
}

// ── Test 6: POST twice same (scope, scopeId, indicator) → upsert ──────────────

#[test]
fn grade_post_upsert_no_duplicate() {
    let dir = setup_workspace_with_db();
    let (_, comp_id) = create_component(&dir);

    let body = serde_json::json!({
        "scope": "component",
        "scopeId": comp_id,
        "indicator": "code-health",
        "score": 60.0
    });
    post_grade(&dir, body.clone());

    // Second post with updated score
    let body2 = serde_json::json!({
        "scope": "component",
        "scopeId": comp_id,
        "indicator": "code-health",
        "score": 80.0
    });
    let updated = post_grade(&dir, body2);
    assert!((updated["score"].as_f64().unwrap() - 80.0).abs() < 0.001);

    // Only one grade in list
    let out = newton()
        .args([
            "data",
            "get",
            "grades",
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
    assert_eq!(
        list.as_array().unwrap().len(),
        1,
        "upsert should not create duplicates"
    );
}

// ── Test 7: GET grades → JSON array ───────────────────────────────────────────

#[test]
fn grade_list_empty_then_populated() {
    let dir = setup_workspace_with_db();

    // Empty list
    let out = newton()
        .args([
            "data",
            "get",
            "grades",
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
    assert!(list.is_array(), "grades list should be array");

    // Add one grade
    let (_, comp_id) = create_component(&dir);
    let body = serde_json::json!({
        "scope": "component",
        "scopeId": comp_id,
        "indicator": "code-health",
        "score": 70.0
    });
    post_grade(&dir, body);

    let out2 = newton()
        .args([
            "data",
            "get",
            "grades",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let list2: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out2.stdout)).unwrap();
    assert_eq!(list2.as_array().unwrap().len(), 1);
}

// ── Test 8: GET grade by id → single item ─────────────────────────────────────

#[test]
fn grade_get_by_id() {
    let dir = setup_workspace_with_db();
    let (_, comp_id) = create_component(&dir);

    let body = serde_json::json!({
        "scope": "component",
        "scopeId": comp_id,
        "indicator": "code-health",
        "score": 88.0
    });
    let created = post_grade(&dir, body);
    let id = created["id"].as_str().unwrap().to_string();

    let out = newton()
        .args([
            "data",
            "get",
            "grade",
            &id,
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
    assert_eq!(item["id"].as_str().unwrap(), id);
}

// ── Test 9: GET grade unknown id → error ──────────────────────────────────────

#[test]
fn grade_get_unknown_id() {
    let dir = setup_workspace_with_db();

    let out = newton()
        .args([
            "data",
            "get",
            "grade",
            "component.nonexistent.no-such-indicator",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ERR_NOT_FOUND"),
        "expected ERR_NOT_FOUND, got: {stderr}"
    );
}

// ── Test 10: PATCH grade score → updated item ─────────────────────────────────

#[test]
fn grade_patch_score() {
    let dir = setup_workspace_with_db();
    let (_, comp_id) = create_component(&dir);

    let body = serde_json::json!({
        "scope": "component",
        "scopeId": comp_id,
        "indicator": "code-health",
        "score": 55.0
    });
    let created = post_grade(&dir, body);
    let id = created["id"].as_str().unwrap().to_string();

    let patch_body = serde_json::json!({"score": 90.0});
    let out = newton()
        .args([
            "data",
            "patch",
            "grade",
            &id,
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
    assert!((patched["score"].as_f64().unwrap() - 90.0).abs() < 0.001);
}

// ── Test 11: DELETE grade → {"id": "..."} ─────────────────────────────────────

#[test]
fn grade_delete() {
    let dir = setup_workspace_with_db();
    let (_, comp_id) = create_component(&dir);

    let body = serde_json::json!({
        "scope": "component",
        "scopeId": comp_id,
        "indicator": "code-health",
        "score": 40.0
    });
    let created = post_grade(&dir, body);
    let id = created["id"].as_str().unwrap().to_string();

    let out = newton()
        .args([
            "data",
            "delete",
            "grade",
            &id,
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let result: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(result["id"].as_str().unwrap(), id);

    // Verify grade is gone
    let out2 = newton()
        .args([
            "data",
            "get",
            "grades",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let list: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out2.stdout)).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 0);
}

// ── Test 12: GET /api/indicators after POST grade → live data (G7) ───────────

#[test]
fn grade_live_list_indicators() {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn pick_free_port() -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        l.local_addr().unwrap().port()
    }

    let dir = setup_workspace_with_db();

    // Initialize the DB by running a CLI command so backend.sqlite is created.
    newton()
        .args([
            "data",
            "get",
            "grades",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success();

    // Seed two indicator rows directly into the SQLite DB.
    // - "ind-graded": will receive a grade → expect live score/lastRun
    // - "ind-static": no grade → expect static values from Indicator table
    let db_path = dir
        .path()
        .join(".newton/state/backend.sqlite")
        .to_string_lossy()
        .to_string();
    let now = "2025-01-01T00:00:00Z";
    let insert_sql = format!(
        "INSERT INTO Indicator (id, name, description, scope, weight, threshold, current, trend, reports, mode, lastRun, createdAt, updatedAt) VALUES \
         ('ind-graded', 'Graded Ind', 'desc', 'component', 1.0, 70.0, 50.0, 0.0, 3, 'auto', '2024-01-01T00:00:00Z', '{now}', '{now}'), \
         ('ind-static', 'Static Ind', 'desc', 'component', 1.0, 70.0, 42.0, 0.0, 1, 'auto', '2024-06-01T00:00:00Z', '{now}', '{now}');",
        now = now
    );
    let status = std::process::Command::new("sqlite3")
        .args([&db_path, &insert_sql])
        .status()
        .expect("sqlite3 should be available");
    assert!(status.success(), "sqlite3 insert should succeed");

    // Create a component for FK validation.
    let (_, comp_id) = create_component(&dir);

    // Post a grade targeting "ind-graded" with score=88 and a known evaluatedAt.
    let evaluated_at = "2026-01-01T00:00:00Z";
    let grade_body = serde_json::json!({
        "scope": "component",
        "scopeId": comp_id,
        "indicator": "ind-graded",
        "score": 88.0,
        "evaluatedAt": evaluated_at
    });
    let created = post_grade(&dir, grade_body);
    assert!((created["score"].as_f64().unwrap() - 88.0).abs() < 0.001);

    // Start newton serve in the workspace directory so it uses the same DB.
    let port = pick_free_port();
    let bin = assert_cmd::cargo::cargo_bin("newton");
    let mut child = Command::new(&bin)
        .current_dir(dir.path())
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--with-mcp",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn newton serve");

    // Wait for the structured startup log line (up to 30s for SQLite init).
    let stderr = child.stderr.take().expect("stderr pipe");
    let mut reader = BufReader::new(stderr);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut server_ready = false;
    while Instant::now() < deadline {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.contains("mcp_serve_started") || line.contains("listening") {
                    server_ready = true;
                    break;
                }
            }
            Err(_) => break,
        }
    }
    if !server_ready {
        let _ = child.kill();
        let _ = child.wait();
        panic!("newton serve did not emit a readiness signal within 30s");
    }

    // Hit GET /api/indicators via reqwest and verify G7 live data.
    let result = (|| -> Result<(), String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("tokio runtime: {e}"))?;
        rt.block_on(async {
            let client = reqwest::Client::new();
            let url = format!("http://127.0.0.1:{}/api/indicators", port);
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("GET /api/indicators: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("/api/indicators returned {}", resp.status()));
            }
            let body: serde_json::Value =
                resp.json().await.map_err(|e| format!("parse JSON: {e}"))?;
            let indicators = body.as_array().ok_or("expected JSON array")?;

            // AC #21: ind-graded should reflect live grade data.
            let graded = indicators
                .iter()
                .find(|i| i["id"].as_str() == Some("ind-graded"))
                .ok_or("ind-graded not found in /api/indicators response")?;
            let current = graded["current"].as_f64().ok_or("current not a number")?;
            if (current - 88.0).abs() > 0.001 {
                return Err(format!(
                    "AC#21: expected current=88.0 for ind-graded, got {current}"
                ));
            }
            let last_run = graded["lastRun"]
                .as_str()
                .ok_or("lastRun missing for ind-graded")?;
            if last_run != evaluated_at {
                return Err(format!(
                    "AC#21: expected lastRun={evaluated_at}, got {last_run}"
                ));
            }

            // AC #22: ind-static (no grade) should return static Indicator values.
            let static_ind = indicators
                .iter()
                .find(|i| i["id"].as_str() == Some("ind-static"))
                .ok_or("ind-static not found in /api/indicators response")?;
            let static_current = static_ind["current"]
                .as_f64()
                .ok_or("static current not a number")?;
            if (static_current - 42.0).abs() > 0.001 {
                return Err(format!(
                    "AC#22: expected static current=42.0 for ind-static, got {static_current}"
                ));
            }
            let static_last_run = static_ind["lastRun"]
                .as_str()
                .ok_or("static lastRun missing")?;
            if static_last_run != "2024-06-01T00:00:00Z" {
                return Err(format!(
                    "AC#22: expected static lastRun=2024-06-01T00:00:00Z, got {static_last_run}"
                ));
            }

            Ok(())
        })
    })();

    let _ = child.kill();
    let _ = child.wait();
    result.expect("G7 live list_indicators assertions failed");
}
