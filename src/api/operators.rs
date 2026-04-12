use crate::api::state::AppState;
use axum::{extract::State, routing::get, Json, Router};
use newton_types::OperatorDescriptor;
use std::sync::Arc;

/// Routes for the operators API resource.
pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/operators", get(list_operators))
        .with_state(state)
}

/// Return the configured operator descriptors as a typed JSON array.
pub async fn list_operators(State(state): State<Arc<AppState>>) -> Json<Vec<OperatorDescriptor>> {
    Json(state.operators.as_ref().clone())
}
