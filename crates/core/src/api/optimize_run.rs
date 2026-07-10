use crate::api::ok_json;
use crate::api::state::AppState;
use axum::{
    extract::{Path, State},
    response::Response,
    routing::get,
    Router,
};
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/optimize-runs", get(list_optimize_runs))
        .route("/optimize-runs/{id}", get(get_optimize_run))
        .route(
            "/optimize-runs/{id}/trajectory",
            get(get_optimize_run_trajectory),
        )
        .route("/optimize-runs/{id}/cycles", get(list_optimize_cycles))
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/optimize-runs",
    tag = "optimize",
    responses(
        (status = 200, description = "List of optimize runs", body = [newton_types::OptimizeRunItem]),
        (status = 500, description = "Internal error", body = newton_types::ApiError)
    )
)]
pub(crate) async fn list_optimize_runs(State(state): State<Arc<AppState>>) -> Response {
    ok_json(state.backend.list_optimize_runs().await)
}

#[utoipa::path(
    get,
    path = "/optimize-runs/{id}",
    tag = "optimize",
    params(("id" = String, Path, description = "Optimize run id")),
    responses(
        (status = 200, description = "Optimize run detail", body = newton_types::OptimizeRunDetail),
        (status = 404, description = "Not found", body = newton_types::ApiError),
        (status = 500, description = "Internal error", body = newton_types::ApiError)
    )
)]
pub(crate) async fn get_optimize_run(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    ok_json(state.backend.get_optimize_run(&id).await)
}

#[utoipa::path(
    get,
    path = "/optimize-runs/{id}/trajectory",
    tag = "optimize",
    params(("id" = String, Path, description = "Optimize run id")),
    responses(
        (status = 200, description = "Run detail with cycles", body = newton_types::OptimizeRunTrajectory),
        (status = 404, description = "Not found", body = newton_types::ApiError),
        (status = 500, description = "Internal error", body = newton_types::ApiError)
    )
)]
pub(crate) async fn get_optimize_run_trajectory(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let detail = match state.backend.get_optimize_run(&id).await {
        Ok(d) => d,
        Err(e) => return ok_json::<()>(Err(e)),
    };
    let cycles = match state.backend.list_optimize_cycles(&id).await {
        Ok(c) => c,
        Err(e) => return ok_json::<()>(Err(e)),
    };
    ok_json(Ok::<_, newton_types::ApiError>(
        newton_types::OptimizeRunTrajectory { detail, cycles },
    ))
}

#[utoipa::path(
    get,
    path = "/optimize-runs/{id}/cycles",
    tag = "optimize",
    params(("id" = String, Path, description = "Optimize run id")),
    responses(
        (status = 200, description = "Cycles for run", body = [newton_types::OptimizeCycleItem]),
        (status = 500, description = "Internal error", body = newton_types::ApiError)
    )
)]
pub(crate) async fn list_optimize_cycles(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    ok_json(state.backend.list_optimize_cycles(&id).await)
}
