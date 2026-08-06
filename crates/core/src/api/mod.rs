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
    extract::{rejection::JsonRejection, FromRequest, Request},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::get,
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

/// Drop-in replacement for `axum::extract::Json<T>` that maps extractor
/// failures (malformed JSON, a body that doesn't match `T` — including a
/// `#[serde(rename_all = ...)]` enum field with an unrecognized variant, e.g.
/// a typed Finding `status`/`severity`/`origin`) to this API's `ApiError`
/// JSON envelope instead of axum's plain-text default rejection body.
///
/// Every other 4xx in this API is a structured `ApiError{code, category,
/// message, details}` (see `err_validation` and friends in
/// `newton_types::store`); without this wrapper, a bad enum value on a
/// `Json<T>`-extracted body slips through as axum's default rejection,
/// breaking that consistency (tranche 4 code review, S3 follow-up).
///
/// Currently adopted only by the two Finding handlers that surfaced the bug
/// (`create_finding`/`patch_finding` in `findings.rs`). Swapping the rest of
/// this API's many `Json<T>` extractors (`catalog.rs`, `plans.rs`,
/// `workflow_files.rs`, etc.) over to `AppJson<T>` is a reasonable follow-up
/// but out of scope here — those endpoints don't have this specific
/// typed-enum-rejection bug, and blanket-swapping them is a much bigger,
/// untested diff than this fix calls for.
pub(crate) struct AppJson<T>(pub(crate) T);

impl<S, T> FromRequest<S> for AppJson<T>
where
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(AppJson(v)),
            Err(rejection) => {
                let e = ApiError {
                    code: "ERR_VALIDATION".to_string(),
                    category: "validation".to_string(),
                    message: rejection.body_text(),
                    details: None,
                };
                Err((api_status(&e), Json(e)).into_response())
            }
        }
    }
}

// lockstep: axum major version MUST match cli-framework (both 0.8)
///
/// `with_magic_tools` gates the `aikit_magictool` router (spec 074 P9): it
/// currently registers only a `newton/ping` smoke-test tool, with real
/// `ToolDef`s landing in a future Part B work item. Until then the router is
/// mounted only when explicitly opted in (`newton serve --with-magic-tools`)
/// so the surface isn't live-but-empty by default. Not part of the OpenAPI
/// doc — that's Part B's job, once the tools are real.
pub fn api_v1_router(state: AppState, with_magic_tools: bool) -> Router {
    let arc_state = Arc::new(state);
    let mut router = Router::new()
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
        .merge(workflow_files::routes(arc_state.clone()));
    if with_magic_tools {
        router = router.merge(aikit_magictool::router(magic_tools::build_state()));
    }
    router
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

/// Public, non-secret OIDC metadata the embedded SPA needs to drive its own
/// PKCE login flow. `newton serve` puts everything under `/api/**` behind the
/// OIDC auth layer (see `commands/serve.rs`), so a config endpoint under
/// `/api` would be unreachable before the SPA has a token -- a
/// chicken-and-egg problem. This struct is instead handed to
/// [`embedded_web_router`], which exposes it at the public `/auth-config`
/// route (deliberately *not* under `/api`).
///
/// Issuer, audience and client id are all public-by-design OIDC metadata (the
/// same values a `.well-known/openid-configuration` document or any OIDC
/// client-side app would need); nothing else from the server config is
/// exposed here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PublicAuthConfig {
    /// `None` means OIDC is not configured for this `newton serve` process
    /// (the API is unauthenticated); `Some` means it is enforced.
    pub oidc: Option<PublicOidcConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicOidcConfig {
    pub issuer: String,
    /// The first configured `--oidc-audience` value. The backend may accept
    /// several (`AudiencePolicy::RequireAny`), but the SPA's PKCE flow only
    /// ever needs one `audience` parameter to request a token for, so we
    /// report the first rather than the full list.
    pub audience: String,
    /// The public OAuth client id the SPA uses for its PKCE flow. Optional:
    /// the backend doesn't need it to validate tokens, so an operator can run
    /// OIDC-gated API-only deployments (or SPA deployments configured another
    /// way) without setting `--oidc-client-id`. When absent the SPA cannot
    /// self-configure a login and this is reported as `null`.
    pub client_id: Option<String>,
}

/// `GET /auth-config` response body. Two shapes rather than one struct with
/// `skip_serializing_if` throughout: when auth is disabled the body is just
/// `{"enabled":false}` (no `issuer`/`audience`/`clientId` keys at all), but
/// when enabled `clientId` must still appear as an explicit `null` if unset
/// -- those are different "absent vs. present-but-null" semantics that a
/// single flat struct can't express with static per-field attributes.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum AuthConfigResponse {
    Disabled {
        enabled: bool,
    },
    // `rename_all` on the enum only renames variant *names* (irrelevant here
    // since this is `untagged`); it must be repeated per-variant to rename
    // this variant's fields (`client_id` -> `clientId`).
    #[serde(rename_all = "camelCase")]
    Enabled {
        enabled: bool,
        issuer: String,
        audience: String,
        client_id: Option<String>,
    },
}

impl From<&PublicAuthConfig> for AuthConfigResponse {
    fn from(cfg: &PublicAuthConfig) -> Self {
        match &cfg.oidc {
            None => AuthConfigResponse::Disabled { enabled: false },
            Some(oidc) => AuthConfigResponse::Enabled {
                enabled: true,
                issuer: oidc.issuer.clone(),
                audience: oidc.audience.clone(),
                client_id: oidc.client_id.clone(),
            },
        }
    }
}

async fn auth_config_handler(auth_config: Arc<PublicAuthConfig>) -> Json<AuthConfigResponse> {
    Json(AuthConfigResponse::from(auth_config.as_ref()))
}

/// Router serving the embedded UI bundle for every unmatched path, plus the
/// public `/auth-config` endpoint the SPA reads before login. Mounted as the
/// host `root_fallback`, so real API/MCP/ailoop routes win; unlike those, the
/// caller (`commands/serve.rs`) must NOT wrap this router in the OIDC auth
/// layer -- `/auth-config` has to answer without a bearer token.
pub fn embedded_web_router(auth_config: PublicAuthConfig) -> Router {
    let auth_config = Arc::new(auth_config);
    Router::new()
        .route(
            "/auth-config",
            get({
                let auth_config = auth_config.clone();
                move || auth_config_handler(auth_config)
            }),
        )
        .fallback(serve_embedded_web)
}

pub fn openapi_json() -> serde_json::Value {
    use utoipa::OpenApi;
    serde_json::to_value(openapi::ApiDoc::openapi()).expect("OpenAPI doc serialization failed")
}

#[cfg(test)]
mod magic_tools_gate_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use newton_types::OperatorDescriptor;
    use tower::ServiceExt;

    async fn test_state() -> AppState {
        let operators = vec![OperatorDescriptor {
            operator_type: "noop".to_string(),
            description: "No-operation operator".to_string(),
            params_schema: serde_json::json!({}),
        }];
        let store = newton_backend::SqliteBackendStore::new_in_memory()
            .await
            .expect("in-memory backend init");
        let backend: Arc<dyn newton_backend::BackendStore> = Arc::new(store);
        AppState::new(operators, backend)
    }

    #[tokio::test]
    async fn magic_tools_router_absent_by_default() {
        let app = api_v1_router(test_state().await, false);
        let req = Request::builder()
            .uri("/aitools")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "/aitools should be unmounted when with_magic_tools=false"
        );
    }

    #[tokio::test]
    async fn magic_tools_router_present_when_enabled() {
        let app = api_v1_router(test_state().await, true);
        let req = Request::builder()
            .uri("/aitools")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "/aitools should be mounted when with_magic_tools=true"
        );
    }
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
            let resp = embedded_web_router(PublicAuthConfig::default())
                .oneshot(req)
                .await
                .unwrap();
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
        let resp = embedded_web_router(PublicAuthConfig::default())
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(header::CONTENT_ENCODING).is_none());
        let body = body_bytes(resp).await;
        let head = String::from_utf8_lossy(&body[..body.len().min(64)]).to_lowercase();
        assert!(head.contains("<!doctype html"), "got: {head}");
    }

    #[tokio::test]
    async fn embedded_router_returns_bundle_for_gzip_accept() {
        let resp = embedded_web_router(PublicAuthConfig::default())
            .oneshot(gz_accept())
            .await
            .unwrap();
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
            let resp = embedded_web_router(PublicAuthConfig::default())
                .oneshot(req)
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "{method} should 404, not serve the SPA"
            );
        }
    }
}

#[cfg(test)]
mod auth_config_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn json_body(resp: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn disabled_reports_enabled_false_and_leaks_nothing() {
        let req = Request::builder()
            .uri("/auth-config")
            .body(Body::empty())
            .unwrap();
        let resp = embedded_web_router(PublicAuthConfig::default())
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body, serde_json::json!({ "enabled": false }));
        assert!(body.get("issuer").is_none(), "must not leak issuer: {body}");
        assert!(
            body.get("audience").is_none(),
            "must not leak audience: {body}"
        );
        assert!(
            body.get("clientId").is_none(),
            "must not leak clientId: {body}"
        );
    }

    #[tokio::test]
    async fn enabled_reports_issuer_audience_and_client_id() {
        let auth_config = PublicAuthConfig {
            oidc: Some(PublicOidcConfig {
                issuer: "https://issuer.example.com".to_string(),
                audience: "newton-api".to_string(),
                client_id: Some("newton-spa".to_string()),
            }),
        };
        let req = Request::builder()
            .uri("/auth-config")
            .body(Body::empty())
            .unwrap();
        let resp = embedded_web_router(auth_config).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(
            body,
            serde_json::json!({
                "enabled": true,
                "issuer": "https://issuer.example.com",
                "audience": "newton-api",
                "clientId": "newton-spa",
            })
        );
    }

    #[tokio::test]
    async fn enabled_without_client_id_reports_explicit_null() {
        let auth_config = PublicAuthConfig {
            oidc: Some(PublicOidcConfig {
                issuer: "https://issuer.example.com".to_string(),
                audience: "newton-api".to_string(),
                client_id: None,
            }),
        };
        let req = Request::builder()
            .uri("/auth-config")
            .body(Body::empty())
            .unwrap();
        let resp = embedded_web_router(auth_config).oneshot(req).await.unwrap();
        let body = json_body(resp).await;
        assert_eq!(
            body,
            serde_json::json!({
                "enabled": true,
                "issuer": "https://issuer.example.com",
                "audience": "newton-api",
                "clientId": null,
            })
        );
    }

    #[tokio::test]
    async fn auth_config_route_is_not_under_api() {
        // The whole point of this endpoint is that it lives outside `/api`,
        // since `/api/**` is behind the OIDC auth layer and unreachable
        // before login. Prove `/api/auth-config` is NOT what's served --
        // it falls through to the SPA fallback (text/html) rather than the
        // JSON auth-config handler.
        let req = Request::builder()
            .uri("/api/auth-config")
            .body(Body::empty())
            .unwrap();
        let resp = embedded_web_router(PublicAuthConfig::default())
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        assert!(
            content_type.starts_with("text/html"),
            "/api/auth-config must not be the JSON auth-config handler, got content-type {content_type:?}"
        );
    }
}
