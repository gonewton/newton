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
}
