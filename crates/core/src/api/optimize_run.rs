use crate::api::ok_json;
use crate::api::state::AppState;
use axum::{
    extract::{Path, State},
    response::Response,
    routing::get,
    Router,
};
use newton_types::{
    ApiError, BroadcastEvent, CreateOptimizeCycleBody, CreateOptimizeRunBody, OptimizeCycleItem,
    OptimizeRunItem, PatchOptimizeRunBody,
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

// ── In-process write-and-broadcast primitives (spec 074 P3 / ADR-0013) ────────
//
// Deliberately NOT mounted as routes: ADR-0013 is explicit that "the 070 HTTP
// surface stays read-only ... no write routes are added" (upholding ADR-0004,
// no external ingress). These are the store-write-then-broadcast primitives
// the *in-process* `newton optimize` driver (spec 073, still draft/unbuilt)
// is meant to call directly, in-process, once it exists — mirroring exactly
// how `plans.rs`'s `approve_plan`/`reject_plan` call `state.backend...` then
// `state.events_tx.send(...)`.
//
// Judgment call (documented per the work-item instructions): as of this
// change, NO code path in the repository actually calls these. The only
// current mutator of `OptimizeRun`/`OptimizeCycle` rows is
// `crates/cli/src/cli/commands/data.rs`'s `dispatch_data()` (invoked via
// `newton data post/patch optimize-run|optimize-cycle`), which the external
// `optimize.sh` bash driver shells out to — a separate OS process with no
// access to `serve`'s `AppState.events_tx`, exactly the limitation ADR-0013
// describes for the *interim* driver. `crates/cli/src/cli/commands/optimize.rs`
// (the `newton optimize` command) is a plan-queue executor; it never touches
// the `OptimizeRun`/`OptimizeCycle` tables at all. The real in-process driver
// that owns Run/Cycle lifecycle end-to-end in `serve`'s own process is spec
// 073 (draft, not implemented) — building it is out of this work item's
// scope. Rather than fabricate a caller (e.g. a process-global broadcast
// channel bridging the CLI's `data` command into `serve`, or a write HTTP
// route/DB-polling bridge — both explicitly rejected in ADR-0013 "Considered
// and rejected"), this change ships the tested write-and-broadcast primitive
// spec 073 needs, ready to be called the moment that command lands. They are
// `pub` (not `pub(crate)`) deliberately: the future driver is expected to
// live in `crates/cli` (a different crate from this one), same as every
// other CLI command that already calls into `newton_core` today.

/// Create an `OptimizeRun` row and broadcast `OptimizeRunUpdate { cycle: None }`
/// on `state.events_tx`. See the module-level comment above for the current
/// (unwired) status of this primitive.
pub async fn create_optimize_run(
    state: &AppState,
    body: CreateOptimizeRunBody,
) -> Result<OptimizeRunItem, ApiError> {
    let item = state.backend.create_optimize_run(body).await?;
    let _ = state.events_tx.send(BroadcastEvent::OptimizeRunUpdate {
        run_id: item.id.clone(),
        cycle: None,
    });
    Ok(item)
}

/// Patch an `OptimizeRun` row and broadcast `OptimizeRunUpdate { cycle: None }`
/// on `state.events_tx`. See the module-level comment above for the current
/// (unwired) status of this primitive.
pub async fn patch_optimize_run(
    state: &AppState,
    id: &str,
    body: PatchOptimizeRunBody,
) -> Result<OptimizeRunItem, ApiError> {
    let item = state.backend.patch_optimize_run(id, body).await?;
    let _ = state.events_tx.send(BroadcastEvent::OptimizeRunUpdate {
        run_id: item.id.clone(),
        cycle: None,
    });
    Ok(item)
}

/// Append an `OptimizeCycle` row and broadcast
/// `OptimizeRunUpdate { cycle: Some(cycle_number) }` on `state.events_tx`.
/// See the module-level comment above for the current (unwired) status of
/// this primitive.
pub async fn create_optimize_cycle(
    state: &AppState,
    body: CreateOptimizeCycleBody,
) -> Result<OptimizeCycleItem, ApiError> {
    let item = state.backend.create_optimize_cycle(body).await?;
    let _ = state.events_tx.send(BroadcastEvent::OptimizeRunUpdate {
        run_id: item.run_id.clone(),
        cycle: Some(item.cycle),
    });
    Ok(item)
}

#[cfg(test)]
mod optimize_run_update_tests {
    use super::*;
    use newton_types::OperatorDescriptor;

    async fn test_state() -> AppState {
        let store = newton_backend::SqliteBackendStore::new_in_memory()
            .await
            .expect("in-memory backend init");
        let backend: Arc<dyn newton_backend::BackendStore> = Arc::new(store);
        AppState::new(Vec::<OperatorDescriptor>::new(), backend)
    }

    #[tokio::test]
    async fn create_optimize_run_broadcasts_optimize_run_update() {
        let state = test_state().await;
        let mut rx = state.events_tx.subscribe();

        let item = create_optimize_run(
            &state,
            CreateOptimizeRunBody {
                id: "run-1".to_string(),
                project_id: "proj-1".to_string(),
                scope: "repo".to_string(),
                scope_id: "repo-1".to_string(),
                max_cycles: 5,
                graders: vec![],
            },
        )
        .await
        .expect("create_optimize_run");

        let event = rx.try_recv().expect("event should be sent");
        match event {
            BroadcastEvent::OptimizeRunUpdate { run_id, cycle } => {
                assert_eq!(run_id, item.id);
                assert_eq!(cycle, None);
            }
            other => panic!("expected OptimizeRunUpdate, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn patch_optimize_run_broadcasts_optimize_run_update() {
        let state = test_state().await;
        create_optimize_run(
            &state,
            CreateOptimizeRunBody {
                id: "run-2".to_string(),
                project_id: "proj-1".to_string(),
                scope: "repo".to_string(),
                scope_id: "repo-1".to_string(),
                max_cycles: 5,
                graders: vec![],
            },
        )
        .await
        .expect("create_optimize_run");

        let mut rx = state.events_tx.subscribe();
        patch_optimize_run(
            &state,
            "run-2",
            PatchOptimizeRunBody {
                status: Some("converged".to_string()),
                cycle: None,
                latest_grades: None,
                open_findings: None,
                blocked_findings: None,
                outcome_reason: None,
            },
        )
        .await
        .expect("patch_optimize_run");

        let event = rx.try_recv().expect("event should be sent");
        match event {
            BroadcastEvent::OptimizeRunUpdate { run_id, cycle } => {
                assert_eq!(run_id, "run-2");
                assert_eq!(cycle, None);
            }
            other => panic!("expected OptimizeRunUpdate, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_optimize_cycle_broadcasts_optimize_run_update_with_cycle() {
        let state = test_state().await;
        create_optimize_run(
            &state,
            CreateOptimizeRunBody {
                id: "run-3".to_string(),
                project_id: "proj-1".to_string(),
                scope: "repo".to_string(),
                scope_id: "repo-1".to_string(),
                max_cycles: 5,
                graders: vec![],
            },
        )
        .await
        .expect("create_optimize_run");

        let mut rx = state.events_tx.subscribe();
        let cycle = create_optimize_cycle(
            &state,
            CreateOptimizeCycleBody {
                id: "cycle-1".to_string(),
                run_id: "run-3".to_string(),
                cycle: 1,
                grades: serde_json::json!({}),
                grade_min: None,
                decision: "continue".to_string(),
                change_request_id: None,
                plan_id: None,
                execution_id: None,
                develop_status: None,
                open_findings: 0,
                resolved_this_cycle: 0,
            },
        )
        .await
        .expect("create_optimize_cycle");

        let event = rx.try_recv().expect("event should be sent");
        match event {
            BroadcastEvent::OptimizeRunUpdate { run_id, cycle: c } => {
                assert_eq!(run_id, "run-3");
                assert_eq!(c, Some(cycle.cycle));
            }
            other => panic!("expected OptimizeRunUpdate, got {other:?}"),
        }
    }
}
