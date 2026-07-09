pub mod fixtures;
pub mod models;
pub mod store;

pub use models::*;
pub use store::SqliteBackendStore;

use chrono::{DateTime, Utc};
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

pub fn err_testing_reset_disabled(message: &str) -> ApiError {
    ApiError {
        code: "ERR_TESTING_RESET_DISABLED".to_string(),
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
    async fn list_kpis(&self) -> Result<Vec<KpiItem>, ApiError>;
    async fn list_recent_actions(&self, limit: u32) -> Result<Vec<RecentActionItem>, ApiError>;

    async fn list_repos(&self) -> Result<Vec<RepoItem>, ApiError>;
    async fn list_repo_dependencies(&self) -> Result<Vec<RepoDependencyItem>, ApiError>;
    async fn list_module_dependencies(&self) -> Result<Vec<ModuleDependencyItem>, ApiError>;
    async fn create_module_dependency(
        &self,
        body: CreateModuleDependencyBody,
    ) -> Result<ModuleDependencyItem, ApiError>;
    async fn list_saved_views(&self, kind: Option<String>) -> Result<serde_json::Value, ApiError>;

    async fn list_findings(
        &self,
        status: Option<String>,
        scope: Option<String>,
        scope_id: Option<String>,
    ) -> Result<Vec<FindingItem>, ApiError>;
    async fn get_finding(&self, id: &str) -> Result<FindingItem, ApiError>;
    async fn create_finding(&self, body: CreateFindingBody) -> Result<FindingItem, ApiError>;
    async fn patch_finding(
        &self,
        id: &str,
        body: PatchFindingBody,
    ) -> Result<FindingItem, ApiError>;

    async fn list_change_requests(
        &self,
        status: Option<String>,
    ) -> Result<Vec<ChangeRequestItem>, ApiError>;
    async fn get_change_request(&self, id: &str) -> Result<ChangeRequestItem, ApiError>;
    async fn create_change_request(
        &self,
        body: CreateChangeRequestBody,
    ) -> Result<ChangeRequestItem, ApiError>;
    async fn patch_change_request(
        &self,
        id: &str,
        body: PatchChangeRequestBody,
    ) -> Result<ChangeRequestItem, ApiError>;

    async fn list_plans(
        &self,
        status: Option<String>,
        scope: Option<String>,
        scope_id: Option<String>,
    ) -> Result<Vec<PlanItem>, ApiError>;
    async fn get_plan(&self, id: &str) -> Result<PlanDetail, ApiError>;
    async fn create_plan(&self, body: CreatePlanBody) -> Result<PlanItem, ApiError>;
    async fn patch_plan(&self, id: &str, body: PatchPlanBody) -> Result<PlanItem, ApiError>;
    async fn approve_plan(&self, id: &str) -> Result<ApprovedPlan, ApiError>;
    async fn reject_plan(&self, id: &str) -> Result<PlanItem, ApiError>;

    async fn unblock_finding(&self, id: &str) -> Result<FindingItem, ApiError>;

    async fn list_optimize_runs(&self) -> Result<Vec<OptimizeRunItem>, ApiError>;
    async fn get_optimize_run(&self, id: &str) -> Result<OptimizeRunDetail, ApiError>;
    async fn create_optimize_run(
        &self,
        body: CreateOptimizeRunBody,
    ) -> Result<OptimizeRunItem, ApiError>;
    async fn patch_optimize_run(
        &self,
        id: &str,
        body: PatchOptimizeRunBody,
    ) -> Result<OptimizeRunItem, ApiError>;
    async fn create_optimize_cycle(
        &self,
        body: CreateOptimizeCycleBody,
    ) -> Result<OptimizeCycleItem, ApiError>;
    async fn list_optimize_cycles(&self, run_id: &str) -> Result<Vec<OptimizeCycleItem>, ApiError>;

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

    // KPI catalog
    async fn create_kpi(&self, body: CreateKpiBody) -> Result<KpiItem, ApiError>;
    async fn get_kpi(&self, id: &str) -> Result<KpiItem, ApiError>;

    // EvalRun
    async fn create_eval_run(&self, body: CreateEvalRunBody) -> Result<EvalRunItem, ApiError>;
    async fn list_eval_runs(
        &self,
        scope: Option<String>,
        scope_id: Option<String>,
        source: Option<String>,
        limit: Option<u32>,
    ) -> Result<Vec<EvalRunItem>, ApiError>;
    async fn get_eval_run(&self, id: &str) -> Result<EvalRunItem, ApiError>;

    // Grade (append-only)
    async fn create_grade(&self, body: CreateGradeBody) -> Result<GradeItem, ApiError>;
    async fn list_grades(
        &self,
        run_id: Option<String>,
        kpi_id: Option<String>,
    ) -> Result<Vec<GradeItem>, ApiError>;
    async fn get_grade(&self, id: &str) -> Result<GradeItem, ApiError>;

    // ── WorkflowInstance ────────────────────────────────────────────────────────

    async fn get_workflow_instance(
        &self,
        instance_id: &str,
    ) -> Result<newton_types::WorkflowInstance, ApiError>;

    async fn list_workflow_instances(
        &self,
        status: Option<newton_types::WorkflowStatus>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<newton_types::WorkflowInstance>, ApiError>;

    /// Inserts or fully replaces (upsert) the workflow instance row.
    /// Node rows are NOT touched by this method; use upsert_node_state for nodes.
    async fn upsert_workflow_instance(
        &self,
        instance: &newton_types::WorkflowInstance,
    ) -> Result<(), ApiError>;

    async fn delete_workflow_instance(&self, instance_id: &str) -> Result<(), ApiError>;

    // ── NodeState ────────────────────────────────────────────────────────────────

    async fn get_node_state(
        &self,
        instance_id: &str,
        node_id: &str,
    ) -> Result<newton_types::NodeState, ApiError>;

    async fn list_node_states_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<Vec<newton_types::NodeState>, ApiError>;

    /// Upserts on (instanceId, nodeId). Assigns a new UUID id on first insert.
    async fn upsert_node_state(
        &self,
        instance_id: &str,
        node: &newton_types::NodeState,
    ) -> Result<(), ApiError>;

    /// Update the status and ended_at of an existing workflow instance.
    async fn update_workflow_status(
        &self,
        instance_id: &str,
        status: newton_types::WorkflowStatus,
        ended_at: DateTime<Utc>,
    ) -> Result<(), ApiError>;

    // ── HilEvent ─────────────────────────────────────────────────────────────────

    async fn get_hil_event(&self, event_id: &str) -> Result<newton_types::HilEvent, ApiError>;

    async fn list_hil_events_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<Vec<newton_types::HilEvent>, ApiError>;

    async fn list_hil_instances(&self) -> Result<Vec<String>, ApiError>;

    async fn insert_hil_event(&self, event: &newton_types::HilEvent) -> Result<(), ApiError>;

    async fn update_hil_event_status(
        &self,
        event_id: &str,
        status: newton_types::HilStatus,
    ) -> Result<newton_types::HilEvent, ApiError>;

    // ── WorkflowLog ──────────────────────────────────────────────────────────────

    /// Appends a single log line; seq is MAX(seq)+1 for the (instance_id, node_id) pair,
    /// or 1 if no rows exist yet.
    async fn append_log_line(
        &self,
        instance_id: &str,
        node_id: &str,
        line: &newton_types::LogLine,
    ) -> Result<(), ApiError>;

    /// Returns log lines with seq > since_seq, ordered by seq ASC.
    /// since_seq = 0 returns all lines.
    async fn list_log_lines(
        &self,
        instance_id: &str,
        node_id: &str,
        since_seq: i64,
    ) -> Result<Vec<newton_types::LogLine>, ApiError>;
}
