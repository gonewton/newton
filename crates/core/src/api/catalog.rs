use crate::api::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{delete, get, patch, post, put},
    Router,
};
use newton_backend::{
    CreateComponentBody, CreateEvalRunBody, CreateGradeBody, CreateModuleBody, CreateProductBody,
    CreateRepoBody, DeletedItem, PatchComponentBody, PatchModuleBody, PatchModuleDependencyBody,
    PatchProductBody, PatchRepoBody, PutComponentBody, PutModuleBody, PutProductBody, PutRepoBody,
};
use newton_types::ApiError;
use serde::Deserialize;
use std::sync::Arc;

fn status_from_error(e: &ApiError) -> StatusCode {
    match e.code.as_str() {
        "ERR_NOT_FOUND" => StatusCode::NOT_FOUND,
        "ERR_CONFLICT" => StatusCode::CONFLICT,
        "ERR_VALIDATION" => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        // KPI (read-only)
        .route("/kpis", get(list_kpis))
        .route("/kpis/{id}", get(get_kpi))
        // EvalRun
        .route("/eval-runs", get(list_eval_runs).post(create_eval_run))
        .route("/eval-runs/{id}", get(get_eval_run))
        // Product
        .route("/products/{id}", get(get_product))
        .route("/products", post(create_product))
        .route("/products/{id}", put(put_product))
        .route("/products/{id}", patch(patch_product))
        .route("/products/{id}", delete(delete_product))
        // Component
        .route("/components/{id}", get(get_component))
        .route("/components", post(create_component))
        .route("/components/{id}", put(put_component))
        .route("/components/{id}", patch(patch_component))
        .route("/components/{id}", delete(delete_component))
        // Repo
        .route("/repos/{id}", get(get_repo))
        .route("/repos", post(create_repo))
        .route("/repos/{id}", put(put_repo))
        .route("/repos/{id}", patch(patch_repo))
        .route("/repos/{id}", delete(delete_repo))
        // Module
        .route("/modules", get(list_modules))
        .route("/modules/{id}", get(get_module))
        .route("/modules", post(create_module))
        .route("/modules/{id}", put(put_module))
        .route("/modules/{id}", patch(patch_module))
        .route("/modules/{id}", delete(delete_module))
        // ModuleDependency
        .route("/module-dependencies/{id}", get(get_module_dependency))
        .route("/module-dependencies/{id}", patch(patch_module_dependency))
        .route(
            "/module-dependencies/{id}",
            delete(delete_module_dependency),
        )
        // Grade
        .route("/grades", get(list_grades).post(create_grade))
        .route("/grades/{id}", get(get_grade))
        .with_state(state)
}

// ── KPI ───────────────────────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/kpis",
    tag = "dashboard",
    responses(
        (status = 200, description = "KPI list", body = [newton_backend::KpiItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_kpis(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_kpis().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/kpis/{id}",
    tag = "dashboard",
    params(("id" = String, Path, description = "KPI ID")),
    responses(
        (status = 200, description = "KPI", body = newton_backend::KpiItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn get_kpi(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.get_kpi(&id).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

// ── EvalRun ───────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListEvalRunsQuery {
    scope: Option<String>,
    scope_id: Option<String>,
    source: Option<String>,
    limit: Option<u32>,
}

#[utoipa::path(
    post,
    path = "/eval-runs",
    tag = "catalog",
    request_body = CreateEvalRunBody,
    responses(
        (status = 201, description = "Created EvalRun", body = newton_backend::EvalRunItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 409, description = "Conflict", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn create_eval_run(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateEvalRunBody>,
) -> Response {
    match state.backend.create_eval_run(body).await {
        Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/eval-runs",
    tag = "catalog",
    params(
        ("scope" = Option<String>, Query, description = "Filter by scope"),
        ("scopeId" = Option<String>, Query, description = "Filter by scope id"),
        ("source" = Option<String>, Query, description = "Filter by source"),
        ("limit" = Option<u32>, Query, description = "Limit results")
    ),
    responses(
        (status = 200, description = "EvalRun list", body = [newton_backend::EvalRunItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_eval_runs(
    Query(query): Query<ListEvalRunsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state
        .backend
        .list_eval_runs(query.scope, query.scope_id, query.source, query.limit)
        .await
    {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/eval-runs/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "EvalRun ID")),
    responses(
        (status = 200, description = "EvalRun", body = newton_backend::EvalRunItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn get_eval_run(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.get_eval_run(&id).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

// ── Product ───────────────────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/products/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Product ID")),
    responses(
        (status = 200, description = "Product", body = newton_backend::ProductItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn get_product(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.get_product(&id).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/products",
    tag = "catalog",
    request_body = CreateProductBody,
    responses(
        (status = 201, description = "Created product", body = newton_backend::ProductItem),
        (status = 409, description = "Conflict", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn create_product(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateProductBody>,
) -> Response {
    match state.backend.create_product(body).await {
        Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    put,
    path = "/products/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Product ID")),
    request_body = PutProductBody,
    responses(
        (status = 200, description = "Updated product", body = newton_backend::ProductItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 409, description = "Conflict", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn put_product(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<PutProductBody>,
) -> Response {
    match state.backend.put_product(&id, body).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/products/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Product ID")),
    request_body = PatchProductBody,
    responses(
        (status = 200, description = "Patched product", body = newton_backend::ProductItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn patch_product(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<PatchProductBody>,
) -> Response {
    match state.backend.patch_product(&id, body).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/products/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Product ID")),
    responses(
        (status = 200, description = "Deleted", body = DeletedItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 409, description = "Conflict", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn delete_product(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.delete_product(&id).await {
        Ok(deleted_id) => (StatusCode::OK, Json(DeletedItem { id: deleted_id })).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

// ── Component ─────────────────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/components/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Component ID")),
    responses(
        (status = 200, description = "Component", body = newton_backend::ComponentItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn get_component(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.get_component(&id).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/components",
    tag = "catalog",
    request_body = CreateComponentBody,
    responses(
        (status = 201, description = "Created component", body = newton_backend::ComponentItem),
        (status = 404, description = "Referenced product not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn create_component(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateComponentBody>,
) -> Response {
    match state.backend.create_component(body).await {
        Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    put,
    path = "/components/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Component ID")),
    request_body = PutComponentBody,
    responses(
        (status = 200, description = "Updated component", body = newton_backend::ComponentItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn put_component(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<PutComponentBody>,
) -> Response {
    match state.backend.put_component(&id, body).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/components/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Component ID")),
    request_body = PatchComponentBody,
    responses(
        (status = 200, description = "Patched component", body = newton_backend::ComponentItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn patch_component(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<PatchComponentBody>,
) -> Response {
    match state.backend.patch_component(&id, body).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/components/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Component ID")),
    responses(
        (status = 200, description = "Deleted", body = DeletedItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 409, description = "Conflict", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn delete_component(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.delete_component(&id).await {
        Ok(deleted_id) => (StatusCode::OK, Json(DeletedItem { id: deleted_id })).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

// ── Repo ──────────────────────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/repos/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Repo ID")),
    responses(
        (status = 200, description = "Repo", body = newton_backend::RepoItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn get_repo(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.get_repo(&id).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/repos",
    tag = "catalog",
    request_body = CreateRepoBody,
    responses(
        (status = 201, description = "Created repo", body = newton_backend::RepoItem),
        (status = 404, description = "Referenced component not found", body = ApiError),
        (status = 409, description = "Conflict", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn create_repo(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRepoBody>,
) -> Response {
    match state.backend.create_repo(body).await {
        Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    put,
    path = "/repos/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Repo ID")),
    request_body = PutRepoBody,
    responses(
        (status = 200, description = "Updated repo", body = newton_backend::RepoItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn put_repo(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<PutRepoBody>,
) -> Response {
    match state.backend.put_repo(&id, body).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/repos/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Repo ID")),
    request_body = PatchRepoBody,
    responses(
        (status = 200, description = "Patched repo", body = newton_backend::RepoItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn patch_repo(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<PatchRepoBody>,
) -> Response {
    match state.backend.patch_repo(&id, body).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/repos/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Repo ID")),
    responses(
        (status = 200, description = "Deleted", body = DeletedItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 409, description = "Conflict", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn delete_repo(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.delete_repo(&id).await {
        Ok(deleted_id) => (StatusCode::OK, Json(DeletedItem { id: deleted_id })).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

// ── Module ────────────────────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/modules",
    tag = "catalog",
    responses(
        (status = 200, description = "Module list", body = [newton_backend::ModuleItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_modules(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_modules().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/modules/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Module ID")),
    responses(
        (status = 200, description = "Module", body = newton_backend::ModuleItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn get_module(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.get_module(&id).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/modules",
    tag = "catalog",
    request_body = CreateModuleBody,
    responses(
        (status = 201, description = "Created module", body = newton_backend::ModuleItem),
        (status = 404, description = "Referenced repo not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn create_module(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateModuleBody>,
) -> Response {
    match state.backend.create_module(body).await {
        Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    put,
    path = "/modules/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Module ID")),
    request_body = PutModuleBody,
    responses(
        (status = 200, description = "Updated module", body = newton_backend::ModuleItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn put_module(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<PutModuleBody>,
) -> Response {
    match state.backend.put_module(&id, body).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/modules/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Module ID")),
    request_body = PatchModuleBody,
    responses(
        (status = 200, description = "Patched module", body = newton_backend::ModuleItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn patch_module(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<PatchModuleBody>,
) -> Response {
    match state.backend.patch_module(&id, body).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/modules/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Module ID")),
    responses(
        (status = 200, description = "Deleted", body = DeletedItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 409, description = "Conflict", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn delete_module(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.delete_module(&id).await {
        Ok(deleted_id) => (StatusCode::OK, Json(DeletedItem { id: deleted_id })).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

// ── ModuleDependency ──────────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/module-dependencies/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "ModuleDependency ID")),
    responses(
        (status = 200, description = "ModuleDependency", body = newton_backend::ModuleDependencyItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn get_module_dependency(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.get_module_dependency(&id).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/module-dependencies/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "ModuleDependency ID")),
    request_body = PatchModuleDependencyBody,
    responses(
        (status = 200, description = "Patched module dependency", body = newton_backend::ModuleDependencyItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn patch_module_dependency(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<PatchModuleDependencyBody>,
) -> Response {
    match state.backend.patch_module_dependency(&id, body).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/module-dependencies/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "ModuleDependency ID")),
    responses(
        (status = 200, description = "Deleted", body = DeletedItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn delete_module_dependency(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.delete_module_dependency(&id).await {
        Ok(deleted_id) => (StatusCode::OK, Json(DeletedItem { id: deleted_id })).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

// ── Grade ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListGradesQuery {
    run_id: Option<String>,
    kpi_id: Option<String>,
}

#[utoipa::path(
    get,
    path = "/grades",
    tag = "catalog",
    params(
        ("runId" = Option<String>, Query, description = "Filter by EvalRun id"),
        ("kpiId" = Option<String>, Query, description = "Filter by KPI id")
    ),
    responses(
        (status = 200, description = "List of grades", body = [newton_backend::GradeItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_grades(
    Query(query): Query<ListGradesQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.list_grades(query.run_id, query.kpi_id).await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/grades",
    tag = "catalog",
    request_body = CreateGradeBody,
    responses(
        (status = 201, description = "Created grade", body = newton_backend::GradeItem),
        (status = 404, description = "Not found", body = ApiError),
        (status = 409, description = "Conflict", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn create_grade(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateGradeBody>,
) -> Response {
    match state.backend.create_grade(body).await {
        Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/grades/{id}",
    tag = "catalog",
    params(("id" = String, Path, description = "Grade id")),
    responses(
        (status = 200, description = "Grade", body = newton_backend::GradeItem),
        (status = 404, description = "Not found", body = ApiError)
    )
)]
pub(crate) async fn get_grade(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.get_grade(&id).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => (status_from_error(&e), Json(e)).into_response(),
    }
}
