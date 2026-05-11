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

#[test]
fn data_get_products_empty() {
    let dir = setup_workspace_with_db();
    let out = newton()
        .args([
            "data",
            "get",
            "products",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("data get products must emit JSON");
    assert!(
        parsed.is_array(),
        "empty products should be array; got: {stdout}"
    );
}

#[test]
fn data_get_products_empty_output_format_json() {
    let dir = setup_workspace_with_db();
    let out = newton()
        .args([
            "data",
            "get",
            "products",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--output-format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("--output-format json must emit JSON");
    assert!(
        parsed.is_array(),
        "empty products with --output-format json should be array; got: {stdout}"
    );
}

#[test]
fn data_post_and_get_product() {
    let dir = setup_workspace_with_db();
    let body = serde_json::json!({"name": "TestProduct"});
    let body_str = serde_json::to_string(&body).unwrap();

    // Create product
    let out = newton()
        .args([
            "data",
            "post",
            "product",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &body_str,
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let created: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .expect("post product must emit JSON");
    let id = created["id"]
        .as_str()
        .expect("created product must have id");
    assert_eq!(created["name"].as_str().unwrap(), "TestProduct");

    // Get single product
    let out2 = newton()
        .args([
            "data",
            "get",
            "product",
            id,
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let fetched: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&out2.stdout))
        .expect("get product must emit JSON");
    assert_eq!(fetched["id"].as_str().unwrap(), id);
}

#[test]
fn data_get_product_not_found_exits_1() {
    let dir = setup_workspace_with_db();
    let out = newton()
        .args([
            "data",
            "get",
            "product",
            "nonexistent-id",
            "--workspace",
            &dir.path().to_string_lossy(),
        ])
        .output()
        .expect("newton should execute");
    assert_ne!(out.status.code(), Some(0), "nonexistent id should exit 1");
}

#[test]
fn data_delete_product_success() {
    let dir = setup_workspace_with_db();
    let body_str = r#"{"name":"ToDelete"}"#;

    // Create
    let out = newton()
        .args([
            "data",
            "post",
            "product",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            body_str,
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let created: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    let id = created["id"].as_str().unwrap().to_string();

    // Delete
    let out2 = newton()
        .args([
            "data",
            "delete",
            "product",
            &id,
            "--workspace",
            &dir.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let deleted: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out2.stdout)).unwrap();
    assert_eq!(deleted["id"].as_str().unwrap(), id);
}

#[test]
fn data_delete_product_with_child_exits_1() {
    let dir = setup_workspace_with_db();

    // Create product
    let out = newton()
        .args([
            "data",
            "post",
            "product",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            r#"{"name":"ParentP"}"#,
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let product: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    let pid = product["id"].as_str().unwrap().to_string();

    // Create component under product
    let comp_body = serde_json::json!({"name":"ChildC","productId":pid,"domain":"d","owner":"o","criticality":"low","autonomy":"low","lastEval":"2024-01-01T00:00:00Z"});
    newton()
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
        .success();

    // Delete product should fail with 409
    let out3 = newton()
        .args([
            "data",
            "delete",
            "product",
            &pid,
            "--workspace",
            &dir.path().to_string_lossy(),
        ])
        .output()
        .expect("newton should run");
    assert_ne!(
        out3.status.code(),
        Some(0),
        "deleting product with child component should fail"
    );
    let stderr = String::from_utf8_lossy(&out3.stderr);
    assert!(
        stderr.contains("ERR_CONFLICT")
            || stderr.contains("conflict")
            || stderr.contains("dependent"),
        "stderr: {stderr}"
    );
}

#[test]
fn data_delete_product_not_found_exits_1() {
    let dir = setup_workspace_with_db();
    let out = newton()
        .args([
            "data",
            "delete",
            "product",
            "nonexistent-id-for-delete",
            "--workspace",
            &dir.path().to_string_lossy(),
        ])
        .output()
        .expect("newton should execute");
    assert_ne!(
        out.status.code(),
        Some(0),
        "deleting nonexistent product should exit 1"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.is_empty() || !String::from_utf8_lossy(&out.stdout).is_empty(),
        "should print error for nonexistent delete"
    );
}

#[test]
fn data_dry_run_component_does_not_create() {
    let dir = setup_workspace_with_db();

    // Create a product to satisfy FK reference
    let out = newton()
        .args([
            "data",
            "post",
            "product",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            r#"{"name":"DryRunParent"}"#,
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let product: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    let pid = product["id"].as_str().unwrap().to_string();

    // Dry-run a component post (FK ref to product)
    let comp_body = serde_json::json!({"name":"DryComp","productId":pid,"domain":"d","owner":"o","criticality":"low","autonomy":"low","lastEval":"2024-01-01T00:00:00Z"});
    newton()
        .args([
            "data",
            "post",
            "component",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            &serde_json::to_string(&comp_body).unwrap(),
            "--dry-run",
        ])
        .assert()
        .success();

    // Verify no component was created
    let out2 = newton()
        .args([
            "data",
            "get",
            "components",
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
    assert_eq!(
        list.as_array().unwrap().len(),
        0,
        "dry-run component post should not persist any component"
    );
}

#[test]
fn data_dry_run_does_not_create() {
    let dir = setup_workspace_with_db();
    // Dry-run a post
    newton()
        .args([
            "data",
            "post",
            "product",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            r#"{"name":"DryRun"}"#,
            "--dry-run",
        ])
        .assert()
        .success();

    // Verify nothing was created
    let out = newton()
        .args([
            "data",
            "get",
            "products",
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
        0,
        "dry-run should not persist"
    );
}

#[test]
fn data_post_product_with_file() {
    let dir = setup_workspace_with_db();
    let body_file = dir.path().join("product_body.json");
    fs::write(&body_file, r#"{"name":"FileProduct"}"#).unwrap();

    let out = newton()
        .args([
            "data",
            "post",
            "product",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--file",
            &body_file.to_string_lossy(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let created: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .expect("post product with -f must emit JSON");
    assert_eq!(
        created["name"].as_str().unwrap(),
        "FileProduct",
        "created product name should match file body"
    );
    assert!(created["id"].is_string(), "created product must have an id");
}

#[test]
fn data_error_both_file_and_body() {
    let dir = setup_workspace_with_db();
    let tmp_file = dir.path().join("body.json");
    fs::write(&tmp_file, r#"{"name":"X"}"#).unwrap();

    let out = newton()
        .args([
            "data",
            "post",
            "product",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            r#"{"name":"X"}"#,
            "--file",
            &tmp_file.to_string_lossy(),
        ])
        .output()
        .expect("newton should run");
    assert_ne!(
        out.status.code(),
        Some(0),
        "should exit 1 when --file and --body are both provided"
    );
    // The framework may catch the conflict before our code (E005) or our code catches it (DATA-001)
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("DATA-001") || stderr.contains("conflict") || stderr.contains("E005"),
        "stderr should mention conflict; got: {stderr}"
    );
}
