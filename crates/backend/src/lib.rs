pub mod fixtures;
pub mod models;
pub mod store;

pub use models::*;
pub use store::SqliteBackendStore;

use newton_types::ApiError;

pub fn err_not_found(message: &str) -> ApiError {
    ApiError {
        code: "ERR_NOT_FOUND".to_string(),
        category: "resource".to_string(),
        message: message.to_string(),
        details: None,
    }
}

pub fn err_conflict(message: &str) -> ApiError {
    ApiError {
        code: "ERR_CONFLICT".to_string(),
        category: "state".to_string(),
        message: message.to_string(),
        details: None,
    }
}

pub fn err_validation(message: &str) -> ApiError {
    ApiError {
        code: "ERR_VALIDATION".to_string(),
        category: "validation".to_string(),
        message: message.to_string(),
        details: None,
    }
}

pub fn err_forbidden_in_prod(message: &str) -> ApiError {
    ApiError {
        code: "ERR_FORBIDDEN_IN_PROD".to_string(),
        category: "authorization".to_string(),
        message: message.to_string(),
        details: None,
    }
}

pub fn err_internal(message: &str) -> ApiError {
    ApiError {
        code: "ERR_INTERNAL".to_string(),
        category: "internal".to_string(),
        message: message.to_string(),
        details: None,
    }
}

#[async_trait::async_trait]
pub trait BackendStore: Send + Sync {
    async fn list_products(&self) -> Result<Vec<ProductItem>, ApiError>;
    async fn list_components(&self) -> Result<Vec<ComponentItem>, ApiError>;
    async fn list_pending_approvals(&self) -> Result<Vec<PendingApprovalItem>, ApiError>;
    async fn list_regressions(&self) -> Result<Vec<RegressionItem>, ApiError>;
    async fn list_indicators(&self) -> Result<Vec<IndicatorItem>, ApiError>;
    async fn list_recent_actions(&self, limit: u32) -> Result<Vec<RecentActionItem>, ApiError>;

    async fn list_repos(&self) -> Result<Vec<RepoItem>, ApiError>;
    async fn list_repo_dependencies(&self) -> Result<Vec<RepoDependencyItem>, ApiError>;
    async fn list_module_dependencies(&self) -> Result<Vec<ModuleDependencyItem>, ApiError>;
    async fn create_module_dependency(
        &self,
        body: CreateModuleDependencyBody,
    ) -> Result<ModuleDependencyItem, ApiError>;
    async fn list_saved_views(&self, kind: Option<String>) -> Result<serde_json::Value, ApiError>;

    async fn list_opportunities(
        &self,
        status: Option<String>,
    ) -> Result<Vec<OpportunityItem>, ApiError>;
    async fn patch_opportunity(
        &self,
        id: &str,
        body: PatchOpportunityBody,
    ) -> Result<OpportunityItem, ApiError>;

    async fn list_requests(&self) -> Result<Vec<RequestItem>, ApiError>;
    async fn create_request(&self, body: CreateRequestBody) -> Result<RequestItem, ApiError>;

    async fn list_plans(&self) -> Result<Vec<PlanItem>, ApiError>;
    async fn get_plan(&self, id: &str) -> Result<PlanDetail, ApiError>;
    async fn approve_plan(&self, id: &str) -> Result<ApprovedPlan, ApiError>;
    async fn reject_plan(&self, id: &str) -> Result<PlanItem, ApiError>;

    async fn list_executions(
        &self,
        plan_id: Option<String>,
    ) -> Result<Vec<ExecutionItem>, ApiError>;

    async fn list_operators(&self) -> Result<Vec<OperatorItem>, ApiError>;

    async fn get_persistence(&self, key: &str) -> Result<serde_json::Value, ApiError>;
    async fn put_persistence(&self, key: &str, value: serde_json::Value) -> Result<(), ApiError>;
    async fn delete_persistence(&self, key: &str) -> Result<(), ApiError>;

    async fn reset(&self) -> Result<(), ApiError>;

    // ── Catalog CRUD ─────────────────────────────────────────────────────────

    // Product
    async fn get_product(&self, id: &str) -> Result<ProductItem, ApiError>;
    async fn create_product(&self, body: CreateProductBody) -> Result<ProductItem, ApiError>;
    async fn put_product(&self, id: &str, body: PutProductBody) -> Result<ProductItem, ApiError>;
    async fn patch_product(
        &self,
        id: &str,
        body: PatchProductBody,
    ) -> Result<ProductItem, ApiError>;
    async fn delete_product(&self, id: &str) -> Result<String, ApiError>;

    // Component
    async fn get_component(&self, id: &str) -> Result<ComponentItem, ApiError>;
    async fn create_component(&self, body: CreateComponentBody) -> Result<ComponentItem, ApiError>;
    async fn put_component(
        &self,
        id: &str,
        body: PutComponentBody,
    ) -> Result<ComponentItem, ApiError>;
    async fn patch_component(
        &self,
        id: &str,
        body: PatchComponentBody,
    ) -> Result<ComponentItem, ApiError>;
    async fn delete_component(&self, id: &str) -> Result<String, ApiError>;

    // Repo
    async fn get_repo(&self, id: &str) -> Result<RepoItem, ApiError>;
    async fn create_repo(&self, body: CreateRepoBody) -> Result<RepoItem, ApiError>;
    async fn put_repo(&self, id: &str, body: PutRepoBody) -> Result<RepoItem, ApiError>;
    async fn patch_repo(&self, id: &str, body: PatchRepoBody) -> Result<RepoItem, ApiError>;
    async fn delete_repo(&self, id: &str) -> Result<String, ApiError>;

    // Module
    async fn list_modules(&self) -> Result<Vec<ModuleItem>, ApiError>;
    async fn get_module(&self, id: &str) -> Result<ModuleItem, ApiError>;
    async fn create_module(&self, body: CreateModuleBody) -> Result<ModuleItem, ApiError>;
    async fn put_module(&self, id: &str, body: PutModuleBody) -> Result<ModuleItem, ApiError>;
    async fn patch_module(&self, id: &str, body: PatchModuleBody) -> Result<ModuleItem, ApiError>;
    async fn delete_module(&self, id: &str) -> Result<String, ApiError>;

    // ModuleDependency additions
    async fn get_module_dependency(&self, id: &str) -> Result<ModuleDependencyItem, ApiError>;
    async fn patch_module_dependency(
        &self,
        id: &str,
        body: PatchModuleDependencyBody,
    ) -> Result<ModuleDependencyItem, ApiError>;
    async fn delete_module_dependency(&self, id: &str) -> Result<String, ApiError>;

    // Grade
    async fn list_grades(&self) -> Result<Vec<GradeItem>, ApiError>;
    async fn get_grade(&self, id: &str) -> Result<GradeItem, ApiError>;
    async fn create_grade(&self, body: CreateGradeBody) -> Result<GradeItem, ApiError>;
    async fn patch_grade(&self, id: &str, body: PatchGradeBody) -> Result<GradeItem, ApiError>;
    async fn delete_grade(&self, id: &str) -> Result<String, ApiError>;
}
