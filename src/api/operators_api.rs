use crate::api::state::AppState;
use axum::{extract::State, Json};
use newton_types::OperatorDescriptor;

pub async fn list_operators(State(state): State<AppState>) -> Json<Vec<OperatorDescriptor>> {
    Json(state.operators.as_ref().clone())
}
