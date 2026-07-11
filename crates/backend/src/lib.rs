pub mod fixtures;
pub mod service;
pub mod store;

pub use store::SqliteBackendStore;

// The `BackendStore` trait and the domain DTOs it speaks live in
// `newton-types` (ADR-0015: `newton-core` must not depend on
// `newton-backend`/sqlx). This crate implements the trait over sqlx/SQLite;
// everything below is re-exported for backward compatibility so downstream
// imports of `newton_backend::{BackendStore, FindingItem, err_not_found, ...}`
// keep compiling unchanged.
pub use newton_types::{
    err_conflict, err_internal, err_not_found, err_testing_reset_disabled, err_validation,
    BackendStore,
};
pub use newton_types::{
    ApiError, ApprovedPlan, ChangeRequestItem, ComponentItem, CreateChangeRequestBody,
    CreateComponentBody, CreateEvalRunBody, CreateFindingBody, CreateGradeBody,
    CreateGradeInlineBody, CreateKpiBody, CreateModuleBody, CreateModuleDependencyBody,
    CreateOptimizeCycleBody, CreateOptimizeRunBody, CreatePlanBody, CreateProductBody,
    CreateRepoBody, DeletedItem, EvalRunItem, ExecutionItem, FindingItem, GradeItem, KpiItem,
    ModuleDependencyItem, ModuleItem, ModuleRef, OperatorItem, OptimizeCycleItem,
    OptimizeRunDetail, OptimizeRunItem, OptimizeRunTrajectory, PatchChangeRequestBody,
    PatchComponentBody, PatchFindingBody, PatchModuleBody, PatchModuleDependencyBody,
    PatchOptimizeRunBody, PatchPlanBody, PatchProductBody, PatchRepoBody, PendingApprovalItem,
    PlanApproverItem, PlanDetail, PlanItem, PlanPolicyCheckItem, PlanSectionItem, ProductItem,
    PutComponentBody, PutModuleBody, PutProductBody, PutRepoBody, RecentActionItem, RegressionItem,
    RepoDependencyItem, RepoItem, SavedViewItem,
};
