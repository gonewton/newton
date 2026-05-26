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

async fn create_test_state() -> AppState {
    let store = SqliteBackendStore::new_in_memory().await.unwrap();
    store.reset().await.unwrap();
    let store_arc: Arc<dyn BackendStore> = Arc::new(store);
    let operators = vec![OperatorDescriptor {
        operator_type: "noop".to_string(),
        description: "No-operation operator".to_string(),
        params_schema: json!({}),
    }];
    AppState::new(operators, store_arc)
}

#[tokio::test]
async fn test_grade_roundtrip_get_by_id() {
    let state = create_test_state().await;
    let app = newton_core::api::api_v1_router(state);

    let run_id = "evalrun.dk-review.repo.repo-1.2026-05-26T00:00:00Z";
    let eval_run_body = json!({
        "id": run_id,
        "source": "dk-review",
        "scope": "repo",
        "scopeId": "repo-1",
        "score": 70,
        "verdict": "approve_with_comments",
        "summary": "Tests dimension had findings",
        "evaluatedAt": "2026-05-26T00:00:00Z"
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/eval-runs")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&eval_run_body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let grade_id = format!("grade.{run_id}.tests");
    let grade_body = json!({
        "id": grade_id,
        "runId": run_id,
        "kpiId": null,
        "dimension": "tests",
        "score": 60,
        "evidence": { "findings": 3 },
        "evaluatedAt": "2026-05-26T00:00:00Z"
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/grades")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&grade_body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let req = Request::builder()
        .uri(format!("/grades/{grade_id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let item: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(item["id"], grade_id);
    assert_eq!(item["runId"], run_id);
    assert_eq!(item["dimension"], "tests");
    assert_eq!(item["score"], 60);
}

#[tokio::test]
async fn test_list_grades_filters_by_run_id() {
    let state = create_test_state().await;
    let app = newton_core::api::api_v1_router(state);

    // Two runs for the same repo (fixtures include repo-1).
    for (run_id, dimension, evaluated_at) in [
        (
            "evalrun.dk-review.repo.repo-1.2026-05-26T00:00:00Z",
            "tests",
            "2026-05-26T00:00:00Z",
        ),
        (
            "evalrun.dk-review.repo.repo-1.2026-05-27T00:00:00Z",
            "security",
            "2026-05-27T00:00:00Z",
        ),
    ] {
        let eval_run_body = json!({
            "id": run_id,
            "source": "dk-review",
            "scope": "repo",
            "scopeId": "repo-1",
            "evaluatedAt": evaluated_at
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri("/eval-runs")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&eval_run_body).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let grade_body = json!({
            "id": format!("grade.{run_id}.{dimension}"),
            "runId": run_id,
            "kpiId": null,
            "dimension": dimension,
            "score": 50,
            "evaluatedAt": "2026-05-26T00:00:00Z"
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri("/grades")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&grade_body).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // Filter by runId (camelCase query param).
    let req = Request::builder()
        .uri("/grades?runId=evalrun.dk-review.repo.repo-1.2026-05-27T00:00:00Z")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let items: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(items.as_array().is_some());
    let arr = items.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(
        arr[0]["runId"],
        "evalrun.dk-review.repo.repo-1.2026-05-27T00:00:00Z"
    );
    assert_eq!(arr[0]["dimension"], "security");
}
