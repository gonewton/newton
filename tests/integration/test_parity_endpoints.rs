use axum::{
    body::Body,
    http::{header, method::Method, Request, StatusCode},
};
use newton::api::state::AppState;
use newton_backend::{BackendStore, SqliteBackendStore};
use newton_types::OperatorDescriptor;
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

async fn create_parity_test_state() -> AppState {
    let store = SqliteBackendStore::new_in_memory().await.unwrap();
    store.reset().await.unwrap();
    let store_arc: Arc<dyn newton_backend::BackendStore> = Arc::new(store);

    let operators = vec![OperatorDescriptor {
        operator_type: "noop".to_string(),
        description: "No-operation operator".to_string(),
        params_schema: json!({}),
    }];
    AppState::new(operators).with_backend(store_arc)
}

macro_rules! parity_test {
    ($name:ident, $method:expr, $path:expr, $expected_status:expr) => {
        #[tokio::test]
        async fn $name() {
            let state = create_parity_test_state().await;
            let app = newton::api::create_router(state, None);
            let request = Request::builder()
                .method($method)
                .uri($path)
                .body(Body::empty())
                .unwrap();
            let response = app.oneshot(request).await.unwrap();
            assert_eq!(response.status(), $expected_status);
        }
    };
}

parity_test!(test_health_ok, Method::GET, "/health", StatusCode::OK);
parity_test!(
    test_list_products,
    Method::GET,
    "/api/products",
    StatusCode::OK
);
parity_test!(
    test_list_components,
    Method::GET,
    "/api/components",
    StatusCode::OK
);
parity_test!(
    test_list_pending_approvals,
    Method::GET,
    "/api/pending-approvals",
    StatusCode::OK
);
parity_test!(
    test_list_regressions,
    Method::GET,
    "/api/regressions",
    StatusCode::OK
);
parity_test!(
    test_list_indicators,
    Method::GET,
    "/api/indicators",
    StatusCode::OK
);
parity_test!(
    test_list_recent_actions,
    Method::GET,
    "/api/recent-actions",
    StatusCode::OK
);
parity_test!(test_list_repos, Method::GET, "/api/repos", StatusCode::OK);
parity_test!(
    test_list_repo_deps,
    Method::GET,
    "/api/repo-dependencies",
    StatusCode::OK
);
parity_test!(
    test_list_module_deps,
    Method::GET,
    "/api/module-dependencies",
    StatusCode::OK
);
parity_test!(
    test_list_saved_views,
    Method::GET,
    "/api/saved-views",
    StatusCode::OK
);
parity_test!(
    test_list_opportunities,
    Method::GET,
    "/api/opportunities",
    StatusCode::OK
);
parity_test!(
    test_list_requests,
    Method::GET,
    "/api/requests",
    StatusCode::OK
);
parity_test!(test_list_plans, Method::GET, "/api/plans", StatusCode::OK);
parity_test!(
    test_list_executions,
    Method::GET,
    "/api/executions",
    StatusCode::OK
);
parity_test!(
    test_list_workflows_parity,
    Method::GET,
    "/api/workflows",
    StatusCode::OK
);
parity_test!(
    test_list_operators_parity,
    Method::GET,
    "/api/operators",
    StatusCode::OK
);

#[tokio::test]
async fn test_health_returns_ok() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let request = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
    assert!(json["version"].is_string());
}

#[tokio::test]
async fn test_products_returns_array() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let request = Request::builder()
        .uri("/api/products")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let products: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(!products.is_empty());
    assert!(products[0]["id"].is_string());
    assert!(products[0]["name"].is_string());
    assert!(products[0]["componentCount"].is_number());
}

#[tokio::test]
async fn test_persistence_put_get_delete() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);

    let put_req = Request::builder()
        .method(Method::PUT)
        .uri("/api/persistence/test-key")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({"foo": "bar"})).unwrap(),
        ))
        .unwrap();
    let put_resp = app.clone().oneshot(put_req).await.unwrap();
    assert_eq!(put_resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(put_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let ok: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(ok["ok"], true);

    let get_req = Request::builder()
        .uri("/api/persistence/test-key")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);

    let del_req = Request::builder()
        .method(Method::DELETE)
        .uri("/api/persistence/test-key")
        .body(Body::empty())
        .unwrap();
    let del_resp = app.clone().oneshot(del_req).await.unwrap();
    assert_eq!(del_resp.status(), StatusCode::OK);

    let get_again = Request::builder()
        .uri("/api/persistence/test-key")
        .body(Body::empty())
        .unwrap();
    let get_resp2 = app.oneshot(get_again).await.unwrap();
    assert_eq!(get_resp2.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_persistence_get_not_found() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let req = Request::builder()
        .uri("/api/persistence/nonexistent")
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
async fn test_create_request_success() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let body = json!({
        "title": "Fix auth bug",
        "requestedBy": "alice"
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/requests")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(request["title"], "Fix auth bug");
    assert_eq!(request["status"], "draft");
    assert_eq!(request["requestedBy"], "alice");
}

#[tokio::test]
async fn test_create_module_dependency_self_dep() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let body = json!({
        "fromModuleId": "mod-1",
        "toModuleId": "mod-1",
        "type": "runtime",
        "label": "self"
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/module-dependencies")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["code"], "ERR_VALIDATION");
}

#[tokio::test]
async fn test_create_module_dependency_not_found() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let body = json!({
        "fromModuleId": "nonexistent",
        "toModuleId": "mod-1",
        "type": "runtime",
        "label": "test"
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/module-dependencies")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
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
async fn test_create_module_dependency_cycle() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let body = json!({
        "fromModuleId": "mod-1",
        "toModuleId": "mod-2",
        "type": "runtime",
        "label": "cycle-test"
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/module-dependencies")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
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
async fn test_create_module_dependency_success() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let body = json!({
        "fromModuleId": "mod-1",
        "toModuleId": "mod-2",
        "type": "dev",
        "label": "dev-dep"
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/module-dependencies")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_get_plan_not_found() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let req = Request::builder()
        .uri("/api/plans/nonexistent")
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
async fn test_approve_reject_plan_not_found() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);

    let approve_req = Request::builder()
        .method(Method::POST)
        .uri("/api/plans/nonexistent/approve")
        .body(Body::empty())
        .unwrap();
    let approve_resp = app.clone().oneshot(approve_req).await.unwrap();
    assert_eq!(approve_resp.status(), StatusCode::NOT_FOUND);

    let reject_req = Request::builder()
        .method(Method::POST)
        .uri("/api/plans/nonexistent/reject")
        .body(Body::empty())
        .unwrap();
    let reject_resp = app.oneshot(reject_req).await.unwrap();
    assert_eq!(reject_resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_patch_opportunity_not_found() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let body = json!({"status": "triaged"});
    let req = Request::builder()
        .method(Method::PATCH)
        .uri("/api/opportunities/nonexistent")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_patch_opportunity_invalid_status() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let body = json!({"status": "invalid_status"});
    let req = Request::builder()
        .method(Method::PATCH)
        .uri("/api/opportunities/nonexistent")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[serial_test::serial]
#[tokio::test]
async fn test_testing_reset_success() {
    std::env::remove_var("NEWTON_ENV");
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/testing/reset")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let ok: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(ok["ok"], true);
}

#[serial_test::serial]
#[tokio::test]
async fn test_testing_reset_forbidden_in_prod() {
    std::env::set_var("NEWTON_ENV", "production");
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/testing/reset")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    std::env::remove_var("NEWTON_ENV");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["code"], "ERR_FORBIDDEN_IN_PROD");
}

#[tokio::test]
async fn test_repos_include_dependency_arrays() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let req = Request::builder()
        .uri("/api/repos")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let repos: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(!repos.is_empty());
    assert!(repos[0]["dependsOn"].is_array());
    assert!(repos[0]["dependedOnBy"].is_array());
}

#[tokio::test]
async fn test_saved_views_grouped_without_kind() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let req = Request::builder()
        .uri("/api/saved-views")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_recent_actions_with_limit() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let req = Request::builder()
        .uri("/api/recent-actions?limit=5")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_executions_with_plan_id_filter() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let req = Request::builder()
        .uri("/api/executions?planId=some-plan")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_opportunity_status_filter() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let req = Request::builder()
        .uri("/api/opportunities?status=awaiting_triage")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_hil_list_events_by_workflow() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let req = Request::builder()
        .uri("/api/hil/workflows/test-instance-id")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_workflow_not_found_parity() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let uuid = uuid::Uuid::new_v4().to_string();
    let req = Request::builder()
        .uri(format!("/api/workflows/{uuid}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_persistence_idempotent() {
    let state = create_parity_test_state().await;
    let app = newton::api::create_router(state, None);
    let del_req = Request::builder()
        .method(Method::DELETE)
        .uri("/api/persistence/nonexistent-key")
        .body(Body::empty())
        .unwrap();
    let del_resp = app.oneshot(del_req).await.unwrap();
    assert_eq!(del_resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(del_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let ok: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(ok["ok"], true);
}
