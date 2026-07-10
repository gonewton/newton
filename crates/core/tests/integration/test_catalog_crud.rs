use axum::{
    body::Body,
    http::{header, method::Method, Request, StatusCode},
};
use newton_backend::{BackendStore, SqliteBackendStore};
use newton_core::api::state::AppState;
use newton_types::OperatorDescriptor;
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

async fn create_catalog_test_state() -> AppState {
    let store = SqliteBackendStore::new_in_memory().await.unwrap();
    store.reset().await.unwrap();
    let store_arc: Arc<dyn newton_backend::BackendStore> = Arc::new(store);
    let operators = vec![OperatorDescriptor {
        operator_type: "noop".to_string(),
        description: "No-operation operator".to_string(),
        params_schema: json!({}),
    }];
    AppState::new(operators, store_arc)
}

// ── Product ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_product_found() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .uri("/products/prod-1")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(item["id"], "prod-1");
    assert!(item["name"].is_string());
}

#[tokio::test]
async fn test_get_product_not_found() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .uri("/products/nonexistent")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["code"], "ERR_NOT_FOUND");
}

#[tokio::test]
async fn test_create_product_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({"name": "New Product"});
    let req = Request::builder()
        .method(Method::POST)
        .uri("/products")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(item["name"], "New Product");
    assert!(item["id"].is_string());
}

#[tokio::test]
async fn test_put_product_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({"name": "Updated Product"});
    let req = Request::builder()
        .method(Method::PUT)
        .uri("/products/prod-1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(item["name"], "Updated Product");
}

#[tokio::test]
async fn test_put_product_not_found() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({"name": "Ghost"});
    let req = Request::builder()
        .method(Method::PUT)
        .uri("/products/nonexistent")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_patch_product_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({"name": "Patched Product"});
    let req = Request::builder()
        .method(Method::PATCH)
        .uri("/products/prod-1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(item["name"], "Patched Product");
}

#[tokio::test]
async fn test_delete_product_conflict_has_components() {
    // prod-1 has comp-1 in fixtures → expect 409
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/products/prod-1")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["code"], "ERR_CONFLICT");
}

#[tokio::test]
async fn test_delete_product_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    // Create a standalone product with no components
    let create_body = json!({"name": "Orphan Product"});
    let create_req = Request::builder()
        .method(Method::POST)
        .uri("/products")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
        .unwrap();
    let create_resp = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);
    let create_body_bytes = axum::body::to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&create_body_bytes).unwrap();
    let new_id = created["id"].as_str().unwrap().to_string();

    let del_req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/products/{new_id}"))
        .body(Body::empty())
        .unwrap();
    let del_resp = app.oneshot(del_req).await.unwrap();
    assert_eq!(del_resp.status(), StatusCode::OK);
    let del_body = axum::body::to_bytes(del_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let deleted: serde_json::Value = serde_json::from_slice(&del_body).unwrap();
    assert_eq!(deleted["id"], new_id);
}

// ── Component ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_component_found() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .uri("/components/comp-1")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(item["id"], "comp-1");
}

#[tokio::test]
async fn test_get_component_not_found() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .uri("/components/nonexistent")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_create_component_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({
        "name": "New Component",
        "productId": "prod-1",
        "domain": "backend",
        "owner": "team-a",
        "criticality": "high",
        "autonomy": "full",
        "health": 80,
        "trend": 1,
        "lastEval": "2026-01-01T00:00:00Z"
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/components")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(item["name"], "New Component");
    assert!(item["id"].is_string());
}

#[tokio::test]
async fn test_put_component_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({
        "name": "Updated Component",
        "productId": "prod-1",
        "domain": "frontend",
        "owner": "team-b",
        "criticality": "low",
        "autonomy": "partial",
        "health": 70,
        "trend": 0,
        "lastEval": "2026-01-01T00:00:00Z"
    });
    let req = Request::builder()
        .method(Method::PUT)
        .uri("/components/comp-1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(item["name"], "Updated Component");
}

#[tokio::test]
async fn test_patch_component_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({"name": "Patched Component"});
    let req = Request::builder()
        .method(Method::PATCH)
        .uri("/components/comp-1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(item["name"], "Patched Component");
}

#[tokio::test]
async fn test_delete_component_conflict_has_repos() {
    // comp-1 has repo-1 in fixtures → expect 409
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/components/comp-1")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["code"], "ERR_CONFLICT");
}

#[tokio::test]
async fn test_delete_component_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    // Create a standalone product, then a component with no repos
    let prod_body = json!({"name": "Orphan Product For Comp"});
    let prod_req = Request::builder()
        .method(Method::POST)
        .uri("/products")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&prod_body).unwrap()))
        .unwrap();
    let prod_resp = app.clone().oneshot(prod_req).await.unwrap();
    assert_eq!(prod_resp.status(), StatusCode::CREATED);
    let prod_bytes = axum::body::to_bytes(prod_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let prod: serde_json::Value = serde_json::from_slice(&prod_bytes).unwrap();
    let pid = prod["id"].as_str().unwrap().to_string();

    let comp_body = json!({
        "name": "Orphan Comp",
        "productId": pid,
        "domain": "d",
        "owner": "o",
        "criticality": "low",
        "autonomy": "low",
        "health": 0,
        "trend": 0,
        "lastEval": "2026-01-01T00:00:00Z"
    });
    let comp_req = Request::builder()
        .method(Method::POST)
        .uri("/components")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&comp_body).unwrap()))
        .unwrap();
    let comp_resp = app.clone().oneshot(comp_req).await.unwrap();
    assert_eq!(comp_resp.status(), StatusCode::CREATED);
    let comp_bytes = axum::body::to_bytes(comp_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let comp: serde_json::Value = serde_json::from_slice(&comp_bytes).unwrap();
    let cid = comp["id"].as_str().unwrap().to_string();

    let del_req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/components/{cid}"))
        .body(Body::empty())
        .unwrap();
    let del_resp = app.oneshot(del_req).await.unwrap();
    assert_eq!(del_resp.status(), StatusCode::OK);
    let del_body = axum::body::to_bytes(del_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let deleted: serde_json::Value = serde_json::from_slice(&del_body).unwrap();
    assert_eq!(deleted["id"], cid);
}

// ── Repo ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_repo_found() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .uri("/repos/repo-1")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(item["id"], "repo-1");
}

#[tokio::test]
async fn test_get_repo_not_found() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .uri("/repos/nonexistent")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_create_repo_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({
        "name": "new-repo",
        "componentId": "comp-1",
        "owner": "team-a",
        "criticality": "high",
        "autonomy": "full",
        "qualityScore": 85,
        "coverage": 80,
        "secScore": 90,
        "execStatus": "idle",
        "lastEval": "2026-01-01T00:00:00Z"
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/repos")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(item["name"], "new-repo");
    assert!(item["id"].is_string());
}

#[tokio::test]
async fn test_put_repo_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    // Keep the same name (auth-api) to avoid FK violation on Regression(repoName)
    let body = json!({
        "name": "auth-api",
        "componentId": "comp-1",
        "owner": "team-b",
        "criticality": "low",
        "autonomy": "partial",
        "qualityScore": 75,
        "coverage": 70,
        "secScore": 80,
        "execStatus": "idle",
        "lastEval": "2026-01-01T00:00:00Z"
    });
    let req = Request::builder()
        .method(Method::PUT)
        .uri("/repos/repo-1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    // name kept as auth-api because Regression table has FK on Repo(name)
    assert_eq!(item["owner"], "team-b");
}

#[tokio::test]
async fn test_patch_repo_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({"owner": "new-owner"});
    let req = Request::builder()
        .method(Method::PATCH)
        .uri("/repos/repo-1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(item["owner"], "new-owner");
}

#[tokio::test]
async fn test_delete_repo_conflict_has_modules() {
    // repo-1 has mod-1, mod-2 in fixtures → expect 409
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/repos/repo-1")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["code"], "ERR_CONFLICT");
}

#[tokio::test]
async fn test_delete_repo_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    // Create a repo with no modules under comp-1
    let body = json!({
        "name": "orphan-repo-for-delete",
        "componentId": "comp-1",
        "owner": "team-a",
        "criticality": "low",
        "autonomy": "full",
        "qualityScore": 70,
        "coverage": 60,
        "secScore": 75,
        "execStatus": "idle",
        "lastEval": "2026-01-01T00:00:00Z"
    });
    let create_req = Request::builder()
        .method(Method::POST)
        .uri("/repos")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let create_resp = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);
    let create_bytes = axum::body::to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&create_bytes).unwrap();
    let new_id = created["id"].as_str().unwrap().to_string();

    let del_req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/repos/{new_id}"))
        .body(Body::empty())
        .unwrap();
    let del_resp = app.oneshot(del_req).await.unwrap();
    assert_eq!(del_resp.status(), StatusCode::OK);
    let del_body = axum::body::to_bytes(del_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let deleted: serde_json::Value = serde_json::from_slice(&del_body).unwrap();
    assert_eq!(deleted["id"], new_id);
}

// ── Module ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_modules_ok() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .uri("/modules")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let items: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(!items.is_empty());
}

#[tokio::test]
async fn test_get_module_found() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .uri("/modules/mod-1")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(item["id"], "mod-1");
}

#[tokio::test]
async fn test_get_module_not_found() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .uri("/modules/nonexistent")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_create_module_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({
        "name": "new-module",
        "kind": "service",
        "repoId": "repo-1"
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/modules")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(item["name"], "new-module");
    assert!(item["id"].is_string());
}

#[tokio::test]
async fn test_put_module_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({
        "name": "mod-1-updated",
        "kind": "library",
        "repoId": "repo-1"
    });
    let req = Request::builder()
        .method(Method::PUT)
        .uri("/modules/mod-1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(item["name"], "mod-1-updated");
}

#[tokio::test]
async fn test_patch_module_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({"kind": "cli"});
    let req = Request::builder()
        .method(Method::PATCH)
        .uri("/modules/mod-1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(item["kind"], "cli");
}

#[tokio::test]
async fn test_delete_module_conflict_has_dependencies() {
    // mod-1 is part of dep-1 in fixtures → expect 409
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/modules/mod-1")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["code"], "ERR_CONFLICT");
}

#[tokio::test]
async fn test_delete_module_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    // Create a module with no dependencies
    let body = json!({
        "name": "orphan-module",
        "kind": "service",
        "repoId": "repo-1"
    });
    let create_req = Request::builder()
        .method(Method::POST)
        .uri("/modules")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let create_resp = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);
    let create_bytes = axum::body::to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&create_bytes).unwrap();
    let new_id = created["id"].as_str().unwrap().to_string();

    let del_req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/modules/{new_id}"))
        .body(Body::empty())
        .unwrap();
    let del_resp = app.oneshot(del_req).await.unwrap();
    assert_eq!(del_resp.status(), StatusCode::OK);
    let del_body = axum::body::to_bytes(del_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let deleted: serde_json::Value = serde_json::from_slice(&del_body).unwrap();
    assert_eq!(deleted["id"], new_id);
}

// ── ModuleDependency ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_module_dependency_found() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .uri("/module-dependencies/dep-1")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(item["id"], "dep-1");
}

#[tokio::test]
async fn test_get_module_dependency_not_found() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .uri("/module-dependencies/nonexistent")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_patch_module_dependency_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({"label": "updated-label"});
    let req = Request::builder()
        .method(Method::PATCH)
        .uri("/module-dependencies/dep-1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(item["label"], "updated-label");
}

#[tokio::test]
async fn test_patch_module_dependency_not_found() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let body = json!({"label": "ghost"});
    let req = Request::builder()
        .method(Method::PATCH)
        .uri("/module-dependencies/nonexistent")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_module_dependency_success() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/module-dependencies/dep-1")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let deleted: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(deleted["id"], "dep-1");
}

#[tokio::test]
async fn test_delete_module_dependency_not_found() {
    let state = create_catalog_test_state().await;
    let app = newton_core::api::api_v1_router(state, false);
    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/module-dependencies/nonexistent")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
