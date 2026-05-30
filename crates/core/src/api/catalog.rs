use crate::api::state::AppState;
use crate::api::{api_status, created_json, ok_json};
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use newton_backend::{
    CreateComponentBody, CreateEvalRunBody, CreateGradeBody, CreateKpiBody, CreateModuleBody,
    CreateProductBody, CreateRepoBody, DeletedItem, PatchComponentBody, PatchModuleBody,
    PatchModuleDependencyBody, PatchProductBody, PatchRepoBody, PutComponentBody, PutModuleBody,
    PutProductBody, PutRepoBody,
};
use newton_types::ApiError;
use serde::Deserialize;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        // KPI
        .route("/kpis", get(list_kpis).post(create_kpi))
        .route("/kpis/{id}", get(get_kpi))
        // EvalRun
        .route("/eval-runs", get(list_eval_runs).post(create_eval_run))
        .route("/eval-runs/{id}", get(get_eval_run))
        .route("/eval-runs/{id}/grades", get(list_eval_run_grades))
        // Grade
        .route("/grades", get(list_grades).post(create_grade))
        .route("/grades/{id}", get(get_grade))
        // Product
        .route("/products", post(create_product))
        .route(
            "/products/{id}",
            get(get_product)
                .put(put_product)
                .patch(patch_product)
                .delete(delete_product),
        )
        // Component
        .route("/components", post(create_component))
        .route(
            "/components/{id}",
            get(get_component)
                .put(put_component)
                .patch(patch_component)
                .delete(delete_component),
        )
        // Repo
        .route("/repos", post(create_repo))
        .route(
            "/repos/{id}",
            get(get_repo)
                .put(put_repo)
                .patch(patch_repo)
                .delete(delete_repo),
        )
        // Module
        .route("/modules", get(list_modules).post(create_module))
        .route(
            "/modules/{id}",
            get(get_module)
                .put(put_module)
                .patch(patch_module)
                .delete(delete_module),
        )
        // ModuleDependency
        .route(
            "/module-dependencies/{id}",
            get(get_module_dependency)
                .patch(patch_module_dependency)
                .delete(delete_module_dependency),
        )
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/kpis",
    tag = "catalog",
    responses(
        (status = 200, description = "KPI list", body = [newton_backend::KpiItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_kpis(State(state): State<Arc<AppState>>) -> Response {
    ok_json(state.backend.list_kpis().await)
}

#[utoipa::path(
    get,
    path = "/kpis/{id}",
    tag = "catalog",
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
    ok_json(state.backend.get_kpi(&id).await)
}

#[utoipa::path(
    post,
    path = "/kpis",
    tag = "catalog",
    request_body = CreateKpiBody,
    responses(
        (status = 201, description = "Created or upserted KPI", body = newton_backend::KpiItem),
        (status = 409, description = "Conflict (name uniqueness violated)", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn create_kpi(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateKpiBody>,
) -> Response {
    created_json(state.backend.create_kpi(body).await)
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
    created_json(state.backend.create_eval_run(body).await)
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
    ok_json(
        state
            .backend
            .list_eval_runs(query.scope, query.scope_id, query.source, query.limit)
            .await,
    )
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
    ok_json(state.backend.get_eval_run(&id).await)
}

#[utoipa::path(
    get,
    path = "/eval-runs/{id}/grades",
    tag = "catalog",
    params(("id" = String, Path, description = "EvalRun ID")),
    responses(
        (status = 200, description = "Grades for the EvalRun", body = [newton_backend::GradeItem]),
        (status = 404, description = "EvalRun not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_eval_run_grades(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    if let Err(e) = state.backend.get_eval_run(&id).await {
        return (api_status(&e), Json(e)).into_response();
    }
    ok_json(state.backend.list_grades(Some(id), None).await)
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
    ok_json(state.backend.get_product(&id).await)
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
    created_json(state.backend.create_product(body).await)
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
    ok_json(state.backend.put_product(&id, body).await)
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
    ok_json(state.backend.patch_product(&id, body).await)
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
    ok_json(
        state
            .backend
            .delete_product(&id)
            .await
            .map(|id| DeletedItem { id }),
    )
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
    ok_json(state.backend.get_component(&id).await)
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
    created_json(state.backend.create_component(body).await)
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
    ok_json(state.backend.put_component(&id, body).await)
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
    ok_json(state.backend.patch_component(&id, body).await)
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
    ok_json(
        state
            .backend
            .delete_component(&id)
            .await
            .map(|id| DeletedItem { id }),
    )
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
    ok_json(state.backend.get_repo(&id).await)
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
    created_json(state.backend.create_repo(body).await)
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
    ok_json(state.backend.put_repo(&id, body).await)
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
    ok_json(state.backend.patch_repo(&id, body).await)
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
    ok_json(
        state
            .backend
            .delete_repo(&id)
            .await
            .map(|id| DeletedItem { id }),
    )
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
    ok_json(state.backend.list_modules().await)
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
    ok_json(state.backend.get_module(&id).await)
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
    created_json(state.backend.create_module(body).await)
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
    ok_json(state.backend.put_module(&id, body).await)
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
    ok_json(state.backend.patch_module(&id, body).await)
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
    ok_json(
        state
            .backend
            .delete_module(&id)
            .await
            .map(|id| DeletedItem { id }),
    )
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
    ok_json(state.backend.get_module_dependency(&id).await)
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
    ok_json(state.backend.patch_module_dependency(&id, body).await)
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
    ok_json(
        state
            .backend
            .delete_module_dependency(&id)
            .await
            .map(|id| DeletedItem { id }),
    )
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
    ok_json(state.backend.list_grades(query.run_id, query.kpi_id).await)
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
    created_json(state.backend.create_grade(body).await)
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
    ok_json(state.backend.get_grade(&id).await)
}
