pub mod catalog;
pub mod change_requests;
pub mod dashboard;
pub mod findings;
pub mod hil;
pub mod magic_tools;
pub mod openapi;
pub mod operators;
pub mod optimize_run;
pub mod persistence;
pub mod plans;
pub mod portfolio;
pub mod state;
pub mod streaming;
pub mod testing_reset;
pub mod workflow_files;
pub mod workflows;

use crate::api::state::AppState;
use axum::{
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    Router,
};
use newton_types::ApiError;
use serde::Serialize;
use std::io::Read as _;
use std::sync::Arc;

pub(crate) fn api_status(e: &ApiError) -> StatusCode {
    match e.code.as_str() {
        "ERR_NOT_FOUND" => StatusCode::NOT_FOUND,
        "ERR_CONFLICT" => StatusCode::CONFLICT,
        "ERR_VALIDATION" => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub(crate) fn ok_json<T: Serialize>(r: Result<T, ApiError>) -> Response {
    match r {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err(e) => (api_status(&e), Json(e)).into_response(),
    }
}

pub(crate) fn created_json<T: Serialize>(r: Result<T, ApiError>) -> Response {
    match r {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(e) => (api_status(&e), Json(e)).into_response(),
    }
}

// lockstep: axum major version MUST match cli-framework (both 0.8)
pub fn api_v1_router(state: AppState) -> Router {
    let arc_state = Arc::new(state);
    Router::new()
        .merge(workflows::routes(arc_state.clone()))
        .merge(hil::routes(arc_state.clone()))
        .merge(streaming::routes(arc_state.clone()))
        .merge(operators::routes(arc_state.clone()))
        .merge(dashboard::routes(arc_state.clone()))
        .merge(portfolio::routes(arc_state.clone()))
        .merge(findings::routes(arc_state.clone()))
        .merge(change_requests::routes(arc_state.clone()))
        .merge(plans::routes(arc_state.clone()))
        .merge(persistence::routes(arc_state.clone()))
        .merge(catalog::routes(arc_state.clone()))
        .merge(optimize_run::routes(arc_state.clone()))
        .merge(testing_reset::routes(arc_state.clone()))
        .merge(workflow_files::routes(arc_state.clone()))
        .merge(aikit_magictool::router(magic_tools::build_state()))
}

/// The Newton web UI, vendored as a single self-contained, gzip-compressed
/// `index.html` (see `scripts/vendor-web.sh`). Compiled into the binary so
/// `newton serve` ships the whole UI with no external files.
static WEB_BUNDLE_GZ: &[u8] = include_bytes!("../../assets/web/index.html.gz");

fn client_accepts_gzip(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().contains("gzip"))
        .unwrap_or(false)
}

/// Serve the embedded single-file UI for any unmatched document request. The
/// SPA owns client-side routing, so deep links (`/optimize`, `/findings`, …)
/// must all return the same `index.html` with a clean `200` — unlike a
/// `ServeDir` fallback, which leaks a `404` status for unknown sub-paths.
///
/// Only `GET`/`HEAD` are answered: SPA navigation is always a document GET, so
/// an unknown `POST`/`PUT`/… (e.g. an API typo, or `/mcp` when MCP is off)
/// still gets a proper `404` instead of an HTML body.
async fn serve_embedded_web(method: axum::http::Method, headers: HeaderMap) -> Response {
    use axum::http::Method;
    if !matches!(method, Method::GET | Method::HEAD) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if client_accepts_gzip(&headers) {
        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CONTENT_ENCODING, "gzip"),
            ],
            WEB_BUNDLE_GZ,
        )
            .into_response()
    } else {
        // Rare path (e.g. bare `curl` without --compressed): decode once.
        let mut html = Vec::new();
        match flate2::read::GzDecoder::new(WEB_BUNDLE_GZ).read_to_end(&mut html) {
            Ok(_) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                html,
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to decode embedded web bundle: {e}"),
            )
                .into_response(),
        }
    }
}

/// Router serving the embedded UI bundle for every unmatched path.
/// Mounted as the host `root_fallback`, so real API/MCP/ailoop routes win.
pub fn embedded_web_router() -> Router {
    Router::new().fallback(serve_embedded_web)
}

pub fn openapi_json() -> serde_json::Value {
    use utoipa::OpenApi;
    serde_json::to_value(openapi::ApiDoc::openapi()).expect("OpenAPI doc serialization failed")
}

#[cfg(test)]
mod web_ui_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn gz_accept() -> Request<Body> {
        Request::builder()
            .uri("/optimize")
            .header(header::ACCEPT_ENCODING, "gzip, deflate, br")
            .body(Body::empty())
            .unwrap()
    }

    async fn body_bytes(resp: Response) -> Vec<u8> {
        axum::body::to_bytes(resp.into_body(), 4 << 20)
            .await
            .unwrap()
            .to_vec()
    }

    #[test]
    fn embedded_bundle_is_valid_gzip_html() {
        let mut html = Vec::new();
        flate2::read::GzDecoder::new(WEB_BUNDLE_GZ)
            .read_to_end(&mut html)
            .expect("vendored bundle must be valid gzip");
        let head = String::from_utf8_lossy(&html[..html.len().min(64)]).to_lowercase();
        assert!(
            head.contains("<!doctype html"),
            "decoded bundle should be HTML, got: {head}"
        );
    }

    #[tokio::test]
    async fn embedded_router_serves_deeplinks_gzip_200() {
        for path in ["/", "/optimize", "/findings", "/change-requests"] {
            let req = Request::builder()
                .uri(path)
                .header(header::ACCEPT_ENCODING, "gzip")
                .body(Body::empty())
                .unwrap();
            let resp = embedded_web_router().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "path {path} should be 200");
            assert_eq!(
                resp.headers().get(header::CONTENT_ENCODING).unwrap(),
                "gzip",
                "path {path} should be gzip-encoded"
            );
            assert!(resp
                .headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("text/html"));
            assert_eq!(
                body_bytes(resp).await,
                WEB_BUNDLE_GZ,
                "gzip path should return the raw vendored bytes"
            );
        }
    }

    #[tokio::test]
    async fn embedded_router_decodes_when_gzip_not_accepted() {
        let req = Request::builder()
            .uri("/optimize")
            .body(Body::empty())
            .unwrap();
        let resp = embedded_web_router().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(header::CONTENT_ENCODING).is_none());
        let body = body_bytes(resp).await;
        let head = String::from_utf8_lossy(&body[..body.len().min(64)]).to_lowercase();
        assert!(head.contains("<!doctype html"), "got: {head}");
    }

    #[tokio::test]
    async fn embedded_router_returns_bundle_for_gzip_accept() {
        let resp = embedded_web_router().oneshot(gz_accept()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn embedded_router_404s_non_get_methods() {
        // Unknown POST/PUT (e.g. an API typo, or /mcp when MCP is off) must not
        // be masked by the SPA shell — they get a proper 404.
        for method in ["POST", "PUT", "DELETE", "PATCH"] {
            let req = Request::builder()
                .method(method)
                .uri("/mcp")
                .body(Body::empty())
                .unwrap();
            let resp = embedded_web_router().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "{method} should 404, not serve the SPA"
            );
        }
    }
}
