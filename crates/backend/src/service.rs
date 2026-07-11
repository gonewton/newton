//! Shared FK pre-validation helpers.
//!
//! Spec 074 S5 audit finding: `dispatch_data` (`crates/cli/src/cli/commands/data.rs`)
//! hand-rolled its own FK-existence checks inside the `data --dry-run` block
//! so a dry run can report a friendly "productId 'x' not found" error
//! without attempting a real write. The settled fix asked for a shared
//! service layer that both the axum handlers and `dispatch_data` call.
//!
//! Investigation for this item found the premise needed narrowing: the
//! store-layer `create_*_db` functions for all five resources below
//! (`crates/backend/src/store/catalog.rs`, `crates/backend/src/store/eval.rs`)
//! already perform their own FK-existence checks before inserting — some
//! (`create_eval_run_db`, `create_grade_db`) do so inside a `BEGIN IMMEDIATE`
//! transaction specifically to avoid a TOCTOU race against the insert. Those
//! checks already return a clean `ApiError` (`ERR_NOT_FOUND`), which the
//! axum handlers in `crates/core/src/api/catalog.rs` already surface as a
//! clean 404 JSON body (via `created_json`) — not a raw SQL constraint
//! violation. `dispatch_data`'s real (non-dry-run) POST arms call the same
//! `store.create_*` methods, so they already get this validation too.
//!
//! The one real, still-live duplication is narrower: the CLI's `--dry-run`
//! block reimplements its own separate copy of "does this FK exist" (working
//! off the raw JSON body rather than a typed one, since a dry run should
//! validate whatever FK fields are present without requiring the rest of the
//! payload's required fields to be filled in) instead of sharing any logic
//! with the store's internal checks. That is what this module extracts: one
//! function per resource, each taking the already-extracted FK id(s) so the
//! CLI's dry-run block can keep working off partial raw JSON exactly as
//! before, while the checks themselves live in one place instead of being
//! hand-copied per resource in `data.rs`.
//!
//! Deliberately NOT wired into the axum handlers or `dispatch_data`'s real
//! write path: those already call `store.create_*`, whose internal checks
//! are correct and (for eval-run/grade) transactionally race-safe. Adding a
//! second, non-transactional pre-check ahead of them would reintroduce a
//! TOCTOU gap and cost an extra round trip for zero behavioral improvement.

use crate::{err_not_found, err_validation, BackendStore};
use newton_types::ApiError;

/// Validates a Component create/put payload's `productId` FK, if present.
/// Mirrors the CLI dry-run contract: `None` (FK field absent from the
/// payload) is not itself an error here — required-field enforcement for a
/// real write happens separately via serde on `CreateComponentBody`.
pub async fn validate_component_fks(
    store: &dyn BackendStore,
    product_id: Option<&str>,
) -> Result<(), ApiError> {
    let Some(product_id) = product_id else {
        return Ok(());
    };
    store
        .get_product(product_id)
        .await
        .map(|_| ())
        .map_err(|e| {
            err_not_found(&format!(
                "productId '{product_id}' not found: {}",
                e.message
            ))
        })
}

/// Validates a Repo create/put payload's `componentId` FK, if present.
pub async fn validate_repo_fks(
    store: &dyn BackendStore,
    component_id: Option<&str>,
) -> Result<(), ApiError> {
    let Some(component_id) = component_id else {
        return Ok(());
    };
    store
        .get_component(component_id)
        .await
        .map(|_| ())
        .map_err(|e| {
            err_not_found(&format!(
                "componentId '{component_id}' not found: {}",
                e.message
            ))
        })
}

/// Validates a Module create/put payload's `repoId` FK, if present.
pub async fn validate_module_fks(
    store: &dyn BackendStore,
    repo_id: Option<&str>,
) -> Result<(), ApiError> {
    let Some(repo_id) = repo_id else {
        return Ok(());
    };
    store
        .get_repo(repo_id)
        .await
        .map(|_| ())
        .map_err(|e| err_not_found(&format!("repoId '{repo_id}' not found: {}", e.message)))
}

/// Validates an EvalRun create payload's `scope`/`scopeId` FK pair.
///
/// Unlike the other four checks, `scope`/`scopeId` are jointly required —
/// mirrors the exact validation semantics of the CLI dry-run block this
/// replaces (`data.rs`, formerly ~line 204-233).
pub async fn validate_eval_run_fks(
    store: &dyn BackendStore,
    scope: Option<&str>,
    scope_id: Option<&str>,
) -> Result<(), ApiError> {
    let scope = scope.unwrap_or("");
    let scope_id = scope_id.unwrap_or("");
    if scope.is_empty() || scope_id.is_empty() {
        return Err(err_validation("scope and scopeId are required"));
    }
    let lookup: Result<(), ApiError> = match scope {
        "product" => store.get_product(scope_id).await.map(|_| ()),
        "component" => store.get_component(scope_id).await.map(|_| ()),
        "repo" => store.get_repo(scope_id).await.map(|_| ()),
        "module" => store.get_module(scope_id).await.map(|_| ()),
        _ => Err(err_validation(
            "scope must be one of: product, component, repo, module",
        )),
    };
    lookup.map_err(|e| err_not_found(&format!("{scope} '{scope_id}' not found: {}", e.message)))
}

/// Validates a Grade create payload's `runId` (required) and `kpiId`
/// (optional) FKs. Mirrors the CLI dry-run block this replaces (`data.rs`,
/// formerly ~line 234-265).
pub async fn validate_grade_fks(
    store: &dyn BackendStore,
    run_id: Option<&str>,
    kpi_id: Option<&str>,
) -> Result<(), ApiError> {
    let Some(run_id) = run_id else {
        return Err(err_validation("runId is required"));
    };
    store
        .get_eval_run(run_id)
        .await
        .map(|_| ())
        .map_err(|e| err_not_found(&format!("runId '{run_id}' not found: {}", e.message)))?;
    if let Some(kpi_id) = kpi_id {
        store
            .get_kpi(kpi_id)
            .await
            .map(|_| ())
            .map_err(|e| err_not_found(&format!("kpiId '{kpi_id}' not found: {}", e.message)))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CreateComponentBody, CreateEvalRunBody, CreateProductBody, CreateRepoBody};

    async fn store() -> crate::SqliteBackendStore {
        crate::SqliteBackendStore::new_in_memory()
            .await
            .expect("in-memory store")
    }

    #[tokio::test]
    async fn component_fks_ok_when_absent() {
        let s = store().await;
        assert!(validate_component_fks(&s, None).await.is_ok());
    }

    #[tokio::test]
    async fn component_fks_ok_when_product_exists() {
        let s = store().await;
        let product = s
            .create_product(CreateProductBody {
                name: "p".to_string(),
            })
            .await
            .expect("seed product");
        assert!(validate_component_fks(&s, Some(&product.id)).await.is_ok());
    }

    #[tokio::test]
    async fn component_fks_err_when_product_missing() {
        let s = store().await;
        let err = validate_component_fks(&s, Some("ghost-product"))
            .await
            .expect_err("must fail");
        assert!(err.message.contains("productId 'ghost-product' not found"));
    }

    #[tokio::test]
    async fn repo_fks_ok_when_absent() {
        let s = store().await;
        assert!(validate_repo_fks(&s, None).await.is_ok());
    }

    #[tokio::test]
    async fn repo_fks_ok_when_component_exists() {
        let s = store().await;
        let product = s
            .create_product(CreateProductBody {
                name: "p".to_string(),
            })
            .await
            .expect("seed product");
        let component = s
            .create_component(CreateComponentBody {
                name: "c".to_string(),
                product_id: product.id,
                domain: "d".to_string(),
                owner: "o".to_string(),
                criticality: "low".to_string(),
                autonomy: "full".to_string(),
                trend: 0,
                last_eval: "2026-01-01T00:00:00Z".to_string(),
            })
            .await
            .expect("seed component");
        assert!(validate_repo_fks(&s, Some(&component.id)).await.is_ok());
    }

    #[tokio::test]
    async fn repo_fks_err_when_component_missing() {
        let s = store().await;
        let err = validate_repo_fks(&s, Some("ghost-component"))
            .await
            .expect_err("must fail");
        assert!(err
            .message
            .contains("componentId 'ghost-component' not found"));
    }

    #[tokio::test]
    async fn module_fks_ok_when_absent() {
        let s = store().await;
        assert!(validate_module_fks(&s, None).await.is_ok());
    }

    #[tokio::test]
    async fn module_fks_ok_when_repo_exists() {
        let s = store().await;
        let product = s
            .create_product(CreateProductBody {
                name: "p".to_string(),
            })
            .await
            .expect("seed product");
        let component = s
            .create_component(CreateComponentBody {
                name: "c".to_string(),
                product_id: product.id,
                domain: "d".to_string(),
                owner: "o".to_string(),
                criticality: "low".to_string(),
                autonomy: "full".to_string(),
                trend: 0,
                last_eval: "2026-01-01T00:00:00Z".to_string(),
            })
            .await
            .expect("seed component");
        let repo = s
            .create_repo(CreateRepoBody {
                name: "r".to_string(),
                component_id: component.id,
                owner: "o".to_string(),
                criticality: "low".to_string(),
                autonomy: "full".to_string(),
                exec_status: "idle".to_string(),
                last_eval: "2026-01-01T00:00:00Z".to_string(),
            })
            .await
            .expect("seed repo");
        assert!(validate_module_fks(&s, Some(&repo.id)).await.is_ok());
    }

    #[tokio::test]
    async fn module_fks_err_when_repo_missing() {
        let s = store().await;
        let err = validate_module_fks(&s, Some("ghost-repo"))
            .await
            .expect_err("must fail");
        assert!(err.message.contains("repoId 'ghost-repo' not found"));
    }

    #[tokio::test]
    async fn eval_run_fks_err_when_scope_or_scope_id_missing() {
        let s = store().await;
        let err = validate_eval_run_fks(&s, None, None)
            .await
            .expect_err("must fail");
        assert!(err.message.contains("scope and scopeId are required"));

        let err = validate_eval_run_fks(&s, Some("product"), None)
            .await
            .expect_err("must fail");
        assert!(err.message.contains("scope and scopeId are required"));
    }

    #[tokio::test]
    async fn eval_run_fks_ok_when_scope_target_exists() {
        let s = store().await;
        let product = s
            .create_product(CreateProductBody {
                name: "p".to_string(),
            })
            .await
            .expect("seed product");
        assert!(
            validate_eval_run_fks(&s, Some("product"), Some(&product.id))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn eval_run_fks_err_when_scope_target_missing() {
        let s = store().await;
        let err = validate_eval_run_fks(&s, Some("product"), Some("ghost-product"))
            .await
            .expect_err("must fail");
        assert!(err.message.contains("product 'ghost-product' not found"));
    }

    #[tokio::test]
    async fn grade_fks_err_when_run_id_missing() {
        let s = store().await;
        let err = validate_grade_fks(&s, None, None)
            .await
            .expect_err("must fail");
        assert!(err.message.contains("runId is required"));
    }

    #[tokio::test]
    async fn grade_fks_err_when_run_id_not_found() {
        let s = store().await;
        let err = validate_grade_fks(&s, Some("ghost-run"), None)
            .await
            .expect_err("must fail");
        assert!(err.message.contains("runId 'ghost-run' not found"));
    }

    #[tokio::test]
    async fn grade_fks_ok_when_run_exists_and_kpi_absent() {
        let s = store().await;
        let product = s
            .create_product(CreateProductBody {
                name: "p".to_string(),
            })
            .await
            .expect("seed product");
        let run = s
            .create_eval_run(CreateEvalRunBody {
                id: "run-1".to_string(),
                source: "test".to_string(),
                scope: "product".to_string(),
                scope_id: product.id,
                score: None,
                verdict: None,
                summary: None,
                evaluated_at: None,
                grades: None,
                raw_assessment: None,
            })
            .await
            .expect("seed eval-run");
        assert!(validate_grade_fks(&s, Some(&run.id), None).await.is_ok());
    }

    #[tokio::test]
    async fn grade_fks_err_when_kpi_missing() {
        let s = store().await;
        let product = s
            .create_product(CreateProductBody {
                name: "p".to_string(),
            })
            .await
            .expect("seed product");
        let run = s
            .create_eval_run(CreateEvalRunBody {
                id: "run-1".to_string(),
                source: "test".to_string(),
                scope: "product".to_string(),
                scope_id: product.id,
                score: None,
                verdict: None,
                summary: None,
                evaluated_at: None,
                grades: None,
                raw_assessment: None,
            })
            .await
            .expect("seed eval-run");
        let err = validate_grade_fks(&s, Some(&run.id), Some("ghost-kpi"))
            .await
            .expect_err("must fail");
        assert!(err.message.contains("kpiId 'ghost-kpi' not found"));
    }

    #[tokio::test]
    async fn grade_fks_ok_when_run_and_kpi_exist() {
        let s = store().await;
        let product = s
            .create_product(CreateProductBody {
                name: "p".to_string(),
            })
            .await
            .expect("seed product");
        let run = s
            .create_eval_run(CreateEvalRunBody {
                id: "run-1".to_string(),
                source: "test".to_string(),
                scope: "product".to_string(),
                scope_id: product.id,
                score: None,
                verdict: None,
                summary: None,
                evaluated_at: None,
                grades: None,
                raw_assessment: None,
            })
            .await
            .expect("seed eval-run");
        let kpi = s
            .create_kpi(crate::CreateKpiBody {
                id: "kpi-1".to_string(),
                name: "KPI One".to_string(),
                description: String::new(),
                scope_level: "product".to_string(),
                threshold: 50.0,
                weight: 1.0,
                agg_fn: "latest".to_string(),
            })
            .await
            .expect("seed kpi");
        assert!(validate_grade_fks(&s, Some(&run.id), Some(&kpi.id))
            .await
            .is_ok());
    }
}
