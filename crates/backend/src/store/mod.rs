mod catalog;
mod eval;
mod finding;
mod helpers;
mod migration;
mod optimize_run;
mod plan;
mod rows;
mod workflow_runtime;

use crate::err_internal;
use crate::models::*;
use crate::BackendStore;
use chrono::{DateTime, Utc};
use newton_types::ApiError;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

#[derive(Clone)]
pub struct SqliteBackendStore {
    pub(super) pool: SqlitePool,
}

impl SqliteBackendStore {
    pub async fn new(database_url: &str) -> Result<Self, ApiError> {
        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(|e| err_internal(&format!("invalid database URL: {e}")))?
            .create_if_missing(true)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|e| err_internal(&format!("failed to connect to database: {e}")))?;

        Self::run_migration(
            &pool,
            include_str!("../../migrations/001_init.sql"),
            "migration 001",
        )
        .await?;

        migration::upgrade_legacy_grade_schema(&pool).await?;

        Self::run_migration(
            &pool,
            include_str!("../../migrations/002_grades.sql"),
            "migration 002",
        )
        .await?;
        Self::run_migration(
            &pool,
            include_str!("../../migrations/003_workflow_runtime.sql"),
            "migration 003",
        )
        .await?;
        Self::run_migration(
            &pool,
            include_str!("../../migrations/004_grading.sql"),
            "migration 004",
        )
        .await?;
        migration::upgrade_eval_run_raw_assessment(&pool).await?;

        migration::upgrade_legacy_indicator_schema(&pool).await?;
        migration::upgrade_legacy_component_schema(&pool).await?;
        migration::upgrade_legacy_repo_schema(&pool).await?;

        migration::upgrade_plan_optimize(&pool).await?;
        migration::upgrade_optimize_run(&pool).await?;
        migration::upgrade_finding_blocked_by_plan(&pool).await?;

        Ok(Self { pool })
    }

    pub async fn new_in_memory() -> Result<Self, ApiError> {
        Self::new("sqlite::memory:").await
    }

    pub(super) fn now_iso() -> String {
        Utc::now().to_rfc3339()
    }

    pub(super) async fn row_exists(
        &self,
        table: &str,
        id: &str,
    ) -> Result<bool, newton_types::ApiError> {
        let sql = format!("SELECT COUNT(*) FROM {table} WHERE id = ?");
        let n = sqlx::query_scalar::<_, i64>(&sql)
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| crate::err_internal(&format!("query error: {e}")))?;
        Ok(n > 0)
    }

    async fn run_migration(pool: &SqlitePool, sql: &str, label: &str) -> Result<(), ApiError> {
        sqlx::query(sql)
            .execute(pool)
            .await
            .map_err(|e| err_internal(&format!("{label} failed: {e}")))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl BackendStore for SqliteBackendStore {
    async fn list_products(&self) -> Result<Vec<ProductItem>, ApiError> {
        self.list_products_db().await
    }
    async fn list_components(&self) -> Result<Vec<ComponentItem>, ApiError> {
        self.list_components_db().await
    }
    async fn list_pending_approvals(&self) -> Result<Vec<PendingApprovalItem>, ApiError> {
        self.list_pending_approvals_db().await
    }
    async fn list_regressions(&self) -> Result<Vec<RegressionItem>, ApiError> {
        self.list_regressions_db().await
    }
    async fn list_kpis(&self) -> Result<Vec<KpiItem>, ApiError> {
        self.list_kpis_db().await
    }
    async fn list_recent_actions(&self, limit: u32) -> Result<Vec<RecentActionItem>, ApiError> {
        self.list_recent_actions_db(limit).await
    }
    async fn list_repos(&self) -> Result<Vec<RepoItem>, ApiError> {
        self.list_repos_db().await
    }
    async fn list_repo_dependencies(&self) -> Result<Vec<RepoDependencyItem>, ApiError> {
        self.list_repo_dependencies_db().await
    }
    async fn list_module_dependencies(&self) -> Result<Vec<ModuleDependencyItem>, ApiError> {
        self.list_module_dependencies_db().await
    }
    async fn create_module_dependency(
        &self,
        body: CreateModuleDependencyBody,
    ) -> Result<ModuleDependencyItem, ApiError> {
        self.create_module_dependency_db(body).await
    }
    async fn list_saved_views(&self, kind: Option<String>) -> Result<serde_json::Value, ApiError> {
        self.list_saved_views_db(kind).await
    }
    async fn list_findings(
        &self,
        status: Option<String>,
        scope: Option<String>,
        scope_id: Option<String>,
    ) -> Result<Vec<FindingItem>, ApiError> {
        self.list_findings_db(status, scope, scope_id).await
    }
    async fn get_finding(&self, id: &str) -> Result<FindingItem, ApiError> {
        self.get_finding_db(id).await
    }
    async fn create_finding(&self, body: CreateFindingBody) -> Result<FindingItem, ApiError> {
        self.create_finding_db(body).await
    }
    async fn patch_finding(
        &self,
        id: &str,
        body: PatchFindingBody,
    ) -> Result<FindingItem, ApiError> {
        self.patch_finding_db(id, body).await
    }
    async fn list_change_requests(
        &self,
        status: Option<String>,
    ) -> Result<Vec<ChangeRequestItem>, ApiError> {
        self.list_change_requests_db(status).await
    }
    async fn get_change_request(&self, id: &str) -> Result<ChangeRequestItem, ApiError> {
        self.get_change_request_db(id).await
    }
    async fn create_change_request(
        &self,
        body: CreateChangeRequestBody,
    ) -> Result<ChangeRequestItem, ApiError> {
        self.create_change_request_db(body).await
    }
    async fn patch_change_request(
        &self,
        id: &str,
        body: PatchChangeRequestBody,
    ) -> Result<ChangeRequestItem, ApiError> {
        self.patch_change_request_db(id, body).await
    }
    async fn list_plans(
        &self,
        status: Option<String>,
        scope: Option<String>,
        scope_id: Option<String>,
    ) -> Result<Vec<PlanItem>, ApiError> {
        self.list_plans_db(status, scope, scope_id).await
    }
    async fn get_plan(&self, id: &str) -> Result<PlanDetail, ApiError> {
        self.get_plan_db(id).await
    }
    async fn create_plan(&self, body: CreatePlanBody) -> Result<PlanItem, ApiError> {
        self.create_plan_db(body).await
    }
    async fn patch_plan(&self, id: &str, body: PatchPlanBody) -> Result<PlanItem, ApiError> {
        self.patch_plan_db(id, body).await
    }
    async fn approve_plan(&self, id: &str) -> Result<ApprovedPlan, ApiError> {
        self.approve_plan_db(id).await
    }
    async fn reject_plan(&self, id: &str) -> Result<PlanItem, ApiError> {
        self.reject_plan_db(id).await
    }
    async fn unblock_finding(&self, id: &str) -> Result<FindingItem, ApiError> {
        self.unblock_finding_db(id).await
    }
    async fn list_optimize_runs(&self) -> Result<Vec<OptimizeRunItem>, ApiError> {
        self.list_optimize_runs_db().await
    }
    async fn get_optimize_run(&self, id: &str) -> Result<OptimizeRunDetail, ApiError> {
        self.get_optimize_run_db(id).await
    }
    async fn create_optimize_run(
        &self,
        body: CreateOptimizeRunBody,
    ) -> Result<OptimizeRunItem, ApiError> {
        self.create_optimize_run_db(body).await
    }
    async fn patch_optimize_run(
        &self,
        id: &str,
        body: PatchOptimizeRunBody,
    ) -> Result<OptimizeRunItem, ApiError> {
        self.patch_optimize_run_db(id, body).await
    }
    async fn create_optimize_cycle(
        &self,
        body: CreateOptimizeCycleBody,
    ) -> Result<OptimizeCycleItem, ApiError> {
        self.create_optimize_cycle_db(body).await
    }
    async fn list_optimize_cycles(&self, run_id: &str) -> Result<Vec<OptimizeCycleItem>, ApiError> {
        self.list_optimize_cycles_db(run_id).await
    }
    async fn list_executions(
        &self,
        plan_id: Option<String>,
    ) -> Result<Vec<ExecutionItem>, ApiError> {
        self.list_executions_db(plan_id).await
    }
    async fn list_operators(&self) -> Result<Vec<OperatorItem>, ApiError> {
        self.list_operators_db().await
    }
    async fn get_persistence(&self, key: &str) -> Result<serde_json::Value, ApiError> {
        self.get_persistence_db(key).await
    }
    async fn put_persistence(&self, key: &str, value: serde_json::Value) -> Result<(), ApiError> {
        self.put_persistence_db(key, value).await
    }
    async fn delete_persistence(&self, key: &str) -> Result<(), ApiError> {
        self.delete_persistence_db(key).await
    }
    async fn reset(&self) -> Result<(), ApiError> {
        self.reset_db().await
    }
    async fn get_product(&self, id: &str) -> Result<ProductItem, ApiError> {
        self.get_product_db(id).await
    }
    async fn create_product(&self, body: CreateProductBody) -> Result<ProductItem, ApiError> {
        self.create_product_db(body).await
    }
    async fn put_product(&self, id: &str, body: PutProductBody) -> Result<ProductItem, ApiError> {
        self.put_product_db(id, body).await
    }
    async fn patch_product(
        &self,
        id: &str,
        body: PatchProductBody,
    ) -> Result<ProductItem, ApiError> {
        self.patch_product_db(id, body).await
    }
    async fn delete_product(&self, id: &str) -> Result<String, ApiError> {
        self.delete_product_db(id).await
    }
    async fn get_component(&self, id: &str) -> Result<ComponentItem, ApiError> {
        self.get_component_db(id).await
    }
    async fn create_component(&self, body: CreateComponentBody) -> Result<ComponentItem, ApiError> {
        self.create_component_db(body).await
    }
    async fn put_component(
        &self,
        id: &str,
        body: PutComponentBody,
    ) -> Result<ComponentItem, ApiError> {
        self.put_component_db(id, body).await
    }
    async fn patch_component(
        &self,
        id: &str,
        body: PatchComponentBody,
    ) -> Result<ComponentItem, ApiError> {
        self.patch_component_db(id, body).await
    }
    async fn delete_component(&self, id: &str) -> Result<String, ApiError> {
        self.delete_component_db(id).await
    }
    async fn get_repo(&self, id: &str) -> Result<RepoItem, ApiError> {
        self.get_repo_db(id).await
    }
    async fn create_repo(&self, body: CreateRepoBody) -> Result<RepoItem, ApiError> {
        self.create_repo_db(body).await
    }
    async fn put_repo(&self, id: &str, body: PutRepoBody) -> Result<RepoItem, ApiError> {
        self.put_repo_db(id, body).await
    }
    async fn patch_repo(&self, id: &str, body: PatchRepoBody) -> Result<RepoItem, ApiError> {
        self.patch_repo_db(id, body).await
    }
    async fn delete_repo(&self, id: &str) -> Result<String, ApiError> {
        self.delete_repo_db(id).await
    }
    async fn list_modules(&self) -> Result<Vec<ModuleItem>, ApiError> {
        self.list_modules_db().await
    }
    async fn get_module(&self, id: &str) -> Result<ModuleItem, ApiError> {
        self.get_module_db(id).await
    }
    async fn create_module(&self, body: CreateModuleBody) -> Result<ModuleItem, ApiError> {
        self.create_module_db(body).await
    }
    async fn put_module(&self, id: &str, body: PutModuleBody) -> Result<ModuleItem, ApiError> {
        self.put_module_db(id, body).await
    }
    async fn patch_module(&self, id: &str, body: PatchModuleBody) -> Result<ModuleItem, ApiError> {
        self.patch_module_db(id, body).await
    }
    async fn delete_module(&self, id: &str) -> Result<String, ApiError> {
        self.delete_module_db(id).await
    }
    async fn get_module_dependency(&self, id: &str) -> Result<ModuleDependencyItem, ApiError> {
        self.get_module_dependency_db(id).await
    }
    async fn patch_module_dependency(
        &self,
        id: &str,
        body: PatchModuleDependencyBody,
    ) -> Result<ModuleDependencyItem, ApiError> {
        self.patch_module_dependency_db(id, body).await
    }
    async fn delete_module_dependency(&self, id: &str) -> Result<String, ApiError> {
        self.delete_module_dependency_db(id).await
    }
    async fn create_kpi(&self, body: CreateKpiBody) -> Result<KpiItem, ApiError> {
        self.create_kpi_db(body).await
    }
    async fn get_kpi(&self, id: &str) -> Result<KpiItem, ApiError> {
        self.get_kpi_db(id).await
    }
    async fn create_eval_run(&self, body: CreateEvalRunBody) -> Result<EvalRunItem, ApiError> {
        self.create_eval_run_db(body).await
    }
    async fn list_eval_runs(
        &self,
        scope: Option<String>,
        scope_id: Option<String>,
        source: Option<String>,
        limit: Option<u32>,
    ) -> Result<Vec<EvalRunItem>, ApiError> {
        self.list_eval_runs_db(scope, scope_id, source, limit).await
    }
    async fn get_eval_run(&self, id: &str) -> Result<EvalRunItem, ApiError> {
        self.get_eval_run_db(id).await
    }
    async fn create_grade(&self, body: CreateGradeBody) -> Result<GradeItem, ApiError> {
        self.create_grade_db(body).await
    }
    async fn list_grades(
        &self,
        run_id: Option<String>,
        kpi_id: Option<String>,
    ) -> Result<Vec<GradeItem>, ApiError> {
        self.list_grades_db(run_id, kpi_id).await
    }
    async fn get_grade(&self, id: &str) -> Result<GradeItem, ApiError> {
        self.get_grade_db(id).await
    }
    async fn get_workflow_instance(
        &self,
        instance_id: &str,
    ) -> Result<newton_types::WorkflowInstance, ApiError> {
        self.get_workflow_instance_db(instance_id).await
    }
    async fn list_workflow_instances(
        &self,
        status: Option<newton_types::WorkflowStatus>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<newton_types::WorkflowInstance>, ApiError> {
        self.list_workflow_instances_db(status, limit, offset).await
    }
    async fn upsert_workflow_instance(
        &self,
        instance: &newton_types::WorkflowInstance,
    ) -> Result<(), ApiError> {
        self.upsert_workflow_instance_db(instance).await
    }
    async fn delete_workflow_instance(&self, instance_id: &str) -> Result<(), ApiError> {
        self.delete_workflow_instance_db(instance_id).await
    }
    async fn get_node_state(
        &self,
        instance_id: &str,
        node_id: &str,
    ) -> Result<newton_types::NodeState, ApiError> {
        self.get_node_state_db(instance_id, node_id).await
    }
    async fn list_node_states_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<Vec<newton_types::NodeState>, ApiError> {
        self.list_node_states_for_instance_db(instance_id).await
    }
    async fn upsert_node_state(
        &self,
        instance_id: &str,
        node: &newton_types::NodeState,
    ) -> Result<(), ApiError> {
        self.upsert_node_state_db(instance_id, node).await
    }
    async fn update_workflow_status(
        &self,
        instance_id: &str,
        status: newton_types::WorkflowStatus,
        ended_at: DateTime<Utc>,
    ) -> Result<(), ApiError> {
        self.update_workflow_status_db(instance_id, status, ended_at)
            .await
    }
    async fn get_hil_event(&self, event_id: &str) -> Result<newton_types::HilEvent, ApiError> {
        self.get_hil_event_db(event_id).await
    }
    async fn list_hil_events_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<Vec<newton_types::HilEvent>, ApiError> {
        self.list_hil_events_for_instance_db(instance_id).await
    }
    async fn list_hil_instances(&self) -> Result<Vec<String>, ApiError> {
        self.list_hil_instances_db().await
    }
    async fn insert_hil_event(&self, event: &newton_types::HilEvent) -> Result<(), ApiError> {
        self.insert_hil_event_db(event).await
    }
    async fn update_hil_event_status(
        &self,
        event_id: &str,
        status: newton_types::HilStatus,
    ) -> Result<newton_types::HilEvent, ApiError> {
        self.update_hil_event_status_db(event_id, status).await
    }
    async fn append_log_line(
        &self,
        instance_id: &str,
        node_id: &str,
        line: &newton_types::LogLine,
    ) -> Result<(), ApiError> {
        self.append_log_line_db(instance_id, node_id, line).await
    }
    async fn list_log_lines(
        &self,
        instance_id: &str,
        node_id: &str,
        since_seq: i64,
    ) -> Result<Vec<newton_types::LogLine>, ApiError> {
        self.list_log_lines_db(instance_id, node_id, since_seq)
            .await
    }
}

#[cfg(test)]
mod store_tests {
    use super::*;
    use chrono::Utc;
    use newton_types::{
        HilEventType, HilStatus, NodeState, NodeStatus, WorkflowInstance, WorkflowStatus,
    };

    fn make_instance(id: &str) -> WorkflowInstance {
        WorkflowInstance {
            instance_id: id.to_string(),
            workflow_id: "wf-1".to_string(),
            status: WorkflowStatus::Running,
            nodes: vec![],
            started_at: Utc::now(),
            ended_at: None,
            linked_plan_id: None,
            definition: None,
        }
    }

    fn make_node(node_id: &str) -> NodeState {
        NodeState {
            node_id: node_id.to_string(),
            status: NodeStatus::Running,
            started_at: Some(Utc::now()),
            ended_at: None,
            operator_type: Some("command".to_string()),
        }
    }

    fn make_hil(event_id: &str, instance_id: &str) -> newton_types::HilEvent {
        newton_types::HilEvent {
            event_id: event_id.to_string(),
            instance_id: instance_id.to_string(),
            node_id: Some("node-1".to_string()),
            channel: "slack".to_string(),
            event_type: HilEventType::Question,
            question: "Continue?".to_string(),
            choices: vec!["yes".to_string(), "no".to_string()],
            timeout_seconds: Some(300),
            correlation_id: None,
            status: HilStatus::Pending,
            timestamp: Utc::now(),
        }
    }

    fn make_log(instance_id: &str, node_id: &str, msg: &str) -> newton_types::LogLine {
        newton_types::LogLine {
            instance_id: instance_id.to_string(),
            node_id: node_id.to_string(),
            level: "info".to_string(),
            message: msg.to_string(),
            timestamp: Utc::now(),
        }
    }

    #[tokio::test]
    async fn upsert_and_get_workflow_instance_round_trip() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let inst = make_instance("inst-1");

        store.upsert_workflow_instance(&inst).await.unwrap();
        let fetched = store.get_workflow_instance("inst-1").await.unwrap();

        assert_eq!(fetched.instance_id, inst.instance_id);
        assert_eq!(fetched.workflow_id, inst.workflow_id);
    }

    #[tokio::test]
    async fn upsert_node_state_is_idempotent() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let inst = make_instance("inst-2");
        store.upsert_workflow_instance(&inst).await.unwrap();

        let node = make_node("node-a");
        store.upsert_node_state("inst-2", &node).await.unwrap();
        let mut node2 = node.clone();
        node2.status = NodeStatus::Succeeded;
        store.upsert_node_state("inst-2", &node2).await.unwrap();

        let nodes = store.list_node_states_for_instance("inst-2").await.unwrap();
        assert_eq!(
            nodes.len(),
            1,
            "idempotent upsert must not create duplicate rows"
        );
        assert_eq!(nodes[0].status, NodeStatus::Succeeded);
    }

    #[tokio::test]
    async fn insert_hil_event_and_update_status_round_trip() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let inst = make_instance("inst-3");
        store.upsert_workflow_instance(&inst).await.unwrap();

        let hil = make_hil("hil-1", "inst-3");
        store.insert_hil_event(&hil).await.unwrap();

        let fetched = store.get_hil_event("hil-1").await.unwrap();
        assert_eq!(fetched.status, HilStatus::Pending);

        let updated = store
            .update_hil_event_status("hil-1", HilStatus::Resolved)
            .await
            .unwrap();
        assert_eq!(updated.status, HilStatus::Resolved);
    }

    #[tokio::test]
    async fn append_log_line_seq_is_monotonic() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let inst = make_instance("inst-4");
        store.upsert_workflow_instance(&inst).await.unwrap();

        for i in 0..5 {
            let line = make_log("inst-4", "node-1", &format!("msg-{i}"));
            store
                .append_log_line("inst-4", "node-1", &line)
                .await
                .unwrap();
        }

        let rows: Vec<(i64,)> = sqlx::query_as(
            "SELECT seq FROM WorkflowLog WHERE instanceId = ? AND nodeId = ? ORDER BY seq ASC",
        )
        .bind("inst-4")
        .bind("node-1")
        .fetch_all(&store.pool)
        .await
        .unwrap();

        let seqs: Vec<i64> = rows.into_iter().map(|(s,)| s).collect();
        assert_eq!(
            seqs,
            vec![1, 2, 3, 4, 5],
            "seq must be strictly monotonic starting at 1"
        );
    }

    #[tokio::test]
    async fn list_log_lines_with_since_seq_filter() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let inst = make_instance("inst-5");
        store.upsert_workflow_instance(&inst).await.unwrap();

        for i in 0..10 {
            let line = make_log("inst-5", "node-x", &format!("line-{i}"));
            store
                .append_log_line("inst-5", "node-x", &line)
                .await
                .unwrap();
        }

        let all = store.list_log_lines("inst-5", "node-x", 0).await.unwrap();
        assert_eq!(all.len(), 10);

        let tail = store.list_log_lines("inst-5", "node-x", 5).await.unwrap();
        assert_eq!(tail.len(), 5);
        assert_eq!(tail[0].message, "line-5");
        assert_eq!(tail[4].message, "line-9");
    }
}

#[cfg(test)]
mod kpi_evalrun_grade_tests {
    use super::*;

    async fn seed_repo(store: &SqliteBackendStore) -> String {
        let now = SqliteBackendStore::now_iso();

        let product = store
            .create_product(CreateProductBody {
                name: "Test Product".to_string(),
            })
            .await
            .unwrap();
        let component = store
            .create_component(CreateComponentBody {
                name: "Test Component".to_string(),
                product_id: product.id,
                domain: "platform".to_string(),
                owner: "owner".to_string(),
                criticality: "low".to_string(),
                autonomy: "semi".to_string(),
                trend: 0,
                last_eval: now.clone(),
            })
            .await
            .unwrap();
        let repo = store
            .create_repo(CreateRepoBody {
                name: "test-repo".to_string(),
                component_id: component.id,
                owner: "owner".to_string(),
                criticality: "low".to_string(),
                autonomy: "semi".to_string(),
                exec_status: "idle".to_string(),
                last_eval: now,
            })
            .await
            .unwrap();
        repo.id
    }

    #[tokio::test]
    async fn create_two_eval_runs_for_same_scope_and_scope_id_preserves_history() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let repo_id = seed_repo(&store).await;

        let run1 = store
            .create_eval_run(CreateEvalRunBody {
                id: "evalrun.1".to_string(),
                source: "test".to_string(),
                scope: "repo".to_string(),
                scope_id: repo_id.clone(),
                score: Some(70.0),
                verdict: Some("ok".to_string()),
                summary: Some("first".to_string()),
                evaluated_at: Some("2026-05-26T00:00:00Z".to_string()),
                grades: None,
                raw_assessment: None,
            })
            .await
            .unwrap();
        let run2 = store
            .create_eval_run(CreateEvalRunBody {
                id: "evalrun.2".to_string(),
                source: "test".to_string(),
                scope: "repo".to_string(),
                scope_id: repo_id.clone(),
                score: Some(72.0),
                verdict: Some("ok".to_string()),
                summary: Some("second".to_string()),
                evaluated_at: Some("2026-05-26T00:05:00Z".to_string()),
                grades: None,
                raw_assessment: None,
            })
            .await
            .unwrap();
        assert_ne!(run1.id, run2.id);

        let list = store
            .list_eval_runs(Some("repo".to_string()), Some(repo_id), None, None)
            .await
            .unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn create_eval_run_missing_required_fields_returns_validation() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let repo_id = seed_repo(&store).await;

        let err = store
            .create_eval_run(CreateEvalRunBody {
                id: "".to_string(),
                source: "test".to_string(),
                scope: "repo".to_string(),
                scope_id: repo_id.clone(),
                score: None,
                verdict: None,
                summary: None,
                evaluated_at: None,
                grades: None,
                raw_assessment: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, "ERR_VALIDATION");

        let err = store
            .create_eval_run(CreateEvalRunBody {
                id: "evalrun.missing-source".to_string(),
                source: "   ".to_string(),
                scope: "repo".to_string(),
                scope_id: repo_id.clone(),
                score: None,
                verdict: None,
                summary: None,
                evaluated_at: None,
                grades: None,
                raw_assessment: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, "ERR_VALIDATION");

        let err = store
            .create_eval_run(CreateEvalRunBody {
                id: "evalrun.missing-scope-id".to_string(),
                source: "test".to_string(),
                scope: "repo".to_string(),
                scope_id: "".to_string(),
                score: None,
                verdict: None,
                summary: None,
                evaluated_at: None,
                grades: None,
                raw_assessment: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, "ERR_VALIDATION");
    }

    #[tokio::test]
    async fn create_grade_missing_run_id_returns_not_found() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let err = store
            .create_grade(CreateGradeBody {
                id: "grade.1".to_string(),
                run_id: "no-such-run".to_string(),
                kpi_id: None,
                dimension: "tests".to_string(),
                score: 50.0,
                evidence: None,
                evaluated_at: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, "ERR_NOT_FOUND");
    }

    #[tokio::test]
    async fn create_grade_missing_required_fields_returns_validation() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();

        let err = store
            .create_grade(CreateGradeBody {
                id: "".to_string(),
                run_id: "some-run".to_string(),
                kpi_id: None,
                dimension: "tests".to_string(),
                score: 50.0,
                evidence: None,
                evaluated_at: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, "ERR_VALIDATION");

        let err = store
            .create_grade(CreateGradeBody {
                id: "grade.missing-run".to_string(),
                run_id: "   ".to_string(),
                kpi_id: None,
                dimension: "tests".to_string(),
                score: 50.0,
                evidence: None,
                evaluated_at: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, "ERR_VALIDATION");
    }

    #[tokio::test]
    async fn create_grade_out_of_range_score_returns_validation() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let repo_id = seed_repo(&store).await;
        store
            .create_eval_run(CreateEvalRunBody {
                id: "evalrun.score".to_string(),
                source: "test".to_string(),
                scope: "repo".to_string(),
                scope_id: repo_id,
                score: None,
                verdict: None,
                summary: None,
                evaluated_at: Some("2026-05-26T00:00:00Z".to_string()),
                grades: None,
                raw_assessment: None,
            })
            .await
            .unwrap();

        let err = store
            .create_grade(CreateGradeBody {
                id: "grade.bad-score".to_string(),
                run_id: "evalrun.score".to_string(),
                kpi_id: None,
                dimension: "tests".to_string(),
                score: 101.0,
                evidence: None,
                evaluated_at: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, "ERR_VALIDATION");
    }

    #[tokio::test]
    async fn create_grade_duplicate_dimension_returns_conflict_and_does_not_overwrite() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let repo_id = seed_repo(&store).await;
        store
            .create_eval_run(CreateEvalRunBody {
                id: "evalrun.dupe".to_string(),
                source: "test".to_string(),
                scope: "repo".to_string(),
                scope_id: repo_id,
                score: None,
                verdict: None,
                summary: None,
                evaluated_at: Some("2026-05-26T00:00:00Z".to_string()),
                grades: None,
                raw_assessment: None,
            })
            .await
            .unwrap();

        let first = store
            .create_grade(CreateGradeBody {
                id: "grade.dupe.1".to_string(),
                run_id: "evalrun.dupe".to_string(),
                kpi_id: None,
                dimension: "tests".to_string(),
                score: 60.0,
                evidence: Some(serde_json::json!({"findings": 1})),
                evaluated_at: Some("2026-05-26T00:00:00Z".to_string()),
            })
            .await
            .unwrap();

        let err = store
            .create_grade(CreateGradeBody {
                id: "grade.dupe.2".to_string(),
                run_id: "evalrun.dupe".to_string(),
                kpi_id: None,
                dimension: "tests".to_string(),
                score: 10.0,
                evidence: Some(serde_json::json!({"findings": 999})),
                evaluated_at: Some("2026-05-26T00:00:00Z".to_string()),
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, "ERR_CONFLICT");

        let fetched = store.get_grade(&first.id).await.unwrap();
        assert_eq!(fetched.score, 60.0);
    }
}

#[cfg(test)]
mod fk_tests {
    use super::*;

    #[tokio::test]
    async fn pragma_foreign_keys_is_on() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let row: (i64,) = sqlx::query_as("PRAGMA foreign_keys")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(row.0, 1, "foreign_keys must be ON for every connection");
    }

    #[tokio::test]
    async fn raw_insert_with_missing_fk_target_is_rejected() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let result = sqlx::query(
            "INSERT INTO Module (id, name, kind, repoId) VALUES ('m-x', 'x', 'lib', 'no-such-repo')",
        )
        .execute(&store.pool)
        .await;
        assert!(
            result.is_err(),
            "raw insert with missing FK target must fail; FK enforcement is off"
        );
    }
}

#[cfg(test)]
mod finding_store_tests {
    use super::*;
    use crate::models::CreateFindingBody;

    fn make_finding(id: &str) -> CreateFindingBody {
        CreateFindingBody {
            id: id.to_string(),
            source: "test".to_string(),
            origin: "system".to_string(),
            component_id: None,
            module: None,
            repo_id: None,
            kpi_id: None,
            dimension: "tests".to_string(),
            location: None,
            fingerprint: format!("fp-{id}"),
            title: "Test finding".to_string(),
            why_it_matters: "Coverage gap detected".to_string(),
            recommended_action: "Add more tests".to_string(),
            severity: "medium".to_string(),
            risk: "low".to_string(),
            confidence: None,
            evidence: None,
            expected_value: None,
            effort: None,
            status: "awaiting_triage".to_string(),
            last_seen_at: None,
            depends_on: vec![],
            blocks: vec![],
        }
    }

    #[tokio::test]
    async fn create_finding_happy_path() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let body = make_finding("find-001");
        let item = store.create_finding(body).await.unwrap();
        assert_eq!(item.id, "find-001");
        assert_eq!(item.status, "awaiting_triage");
        assert_eq!(item.dimension, "tests");
        assert_eq!(item.risk, "low");
    }

    #[tokio::test]
    async fn create_finding_upsert_updates_title() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        store
            .create_finding(make_finding("find-002"))
            .await
            .unwrap();
        let mut body2 = make_finding("find-002");
        body2.title = "Updated title".to_string();
        let item = store.create_finding(body2).await.unwrap();
        assert_eq!(item.title, "Updated title");
        let all = store.list_findings(None, None, None).await.unwrap();
        assert_eq!(all.iter().filter(|f| f.id == "find-002").count(), 1);
    }
}

#[cfg(test)]
mod legacy_indicator_migration_tests {
    use super::*;
    use tempfile::tempdir;

    async fn create_legacy_db(url: &str) {
        let options = SqliteConnectOptions::from_str(url)
            .unwrap()
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();

        sqlx::query(include_str!("../../migrations/001_init.sql"))
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("PRAGMA foreign_keys = OFF;")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("DROP TABLE Component;")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE Component (\
              id TEXT PRIMARY KEY,\
              name TEXT NOT NULL,\
              domain TEXT NOT NULL,\
              repos INTEGER NOT NULL,\
              modules INTEGER NOT NULL,\
              health INTEGER NOT NULL,\
              trend INTEGER NOT NULL,\
              owner TEXT NOT NULL,\
              criticality TEXT NOT NULL,\
              autonomy TEXT NOT NULL,\
              openPlans INTEGER NOT NULL DEFAULT 0,\
              openRequests INTEGER NOT NULL DEFAULT 0,\
              lastEval TEXT NOT NULL,\
              productId TEXT NOT NULL,\
              createdAt TEXT NOT NULL,\
              updatedAt TEXT NOT NULL\
            );",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_component_productId ON Component(productId);")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("DROP TABLE Repo;")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE Repo (\
              id TEXT PRIMARY KEY,\
              name TEXT NOT NULL UNIQUE,\
              componentId TEXT NOT NULL,\
              owner TEXT NOT NULL,\
              criticality TEXT NOT NULL,\
              autonomy TEXT NOT NULL,\
              qualityScore INTEGER NOT NULL,\
              regressions INTEGER NOT NULL DEFAULT 0,\
              openPlans INTEGER NOT NULL DEFAULT 0,\
              execStatus TEXT NOT NULL,\
              lastEval TEXT NOT NULL,\
              coverage INTEGER NOT NULL,\
              secScore INTEGER NOT NULL,\
              createdAt TEXT NOT NULL,\
              updatedAt TEXT NOT NULL\
            );",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_repo_componentId ON Repo(componentId);")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("PRAGMA foreign_keys = ON;")
            .execute(&pool)
            .await
            .unwrap();

        let mut tx = pool.begin().await.unwrap();
        sqlx::query("PRAGMA foreign_keys = OFF;")
            .execute(&mut *tx)
            .await
            .unwrap();

        sqlx::query("DROP TABLE Opportunity;")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE Opportunity (\
              id TEXT PRIMARY KEY,\
              title TEXT NOT NULL,\
              origin TEXT NOT NULL,\
              componentId TEXT NULL,\
              module TEXT NULL,\
              repoId TEXT NULL,\
              indicator TEXT NULL,\
              confidence REAL NULL,\
              risk TEXT NOT NULL,\
              expectedValue REAL NOT NULL,\
              effort TEXT NULL,\
              status TEXT NOT NULL,\
              age TEXT NULL,\
              rationale TEXT NULL,\
              dependsOn TEXT NOT NULL DEFAULT '[]',\
              blocks TEXT NOT NULL DEFAULT '[]',\
              createdAt TEXT NOT NULL,\
              updatedAt TEXT NOT NULL,\
              FOREIGN KEY(componentId) REFERENCES Component(id),\
              FOREIGN KEY(repoId) REFERENCES Repo(id)\
            );",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_opportunity_status ON Opportunity(status);")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_opportunity_componentId ON Opportunity(componentId);",
        )
        .execute(&mut *tx)
        .await
        .unwrap();

        sqlx::query("DROP TABLE Regression;")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE Regression (\
              id TEXT PRIMARY KEY,\
              repoName TEXT NOT NULL,\
              indicator TEXT NOT NULL,\
              delta REAL NOT NULL,\
              severity TEXT NOT NULL,\
              since TEXT NOT NULL,\
              trend TEXT NOT NULL,\
              createdAt TEXT NOT NULL,\
              FOREIGN KEY(repoName) REFERENCES Repo(name)\
            );",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_regression_repoName ON Regression(repoName);")
            .execute(&mut *tx)
            .await
            .unwrap();

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS Indicator (\
              id TEXT PRIMARY KEY,\
              name TEXT NOT NULL UNIQUE,\
              description TEXT NOT NULL,\
              scope TEXT NOT NULL,\
              weight REAL NOT NULL,\
              threshold REAL NOT NULL,\
              current REAL NOT NULL,\
              trend REAL NOT NULL,\
              reports INTEGER NOT NULL,\
              mode TEXT NOT NULL,\
              lastRun TEXT NOT NULL,\
              createdAt TEXT NOT NULL,\
              updatedAt TEXT NOT NULL\
            );",
        )
        .execute(&mut *tx)
        .await
        .unwrap();

        sqlx::query("PRAGMA foreign_keys = ON;")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        sqlx::query("INSERT INTO Product (id, name, createdAt, updatedAt) VALUES (?, ?, ?, ?);")
            .bind("prod-legacy")
            .bind("Legacy Product")
            .bind("2026-01-01T00:00:00Z")
            .bind("2026-01-01T00:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO Component (\
              id, name, domain, repos, modules, health, trend, owner, criticality, autonomy,\
              lastEval, productId, createdAt, updatedAt\
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
        )
        .bind("comp-legacy")
        .bind("Legacy Component")
        .bind("legacy")
        .bind(0_i64)
        .bind(0_i64)
        .bind(0_i64)
        .bind(0_i64)
        .bind("legacy-owner")
        .bind("low")
        .bind("high")
        .bind("2026-01-01T00:00:00Z")
        .bind("prod-legacy")
        .bind("2026-01-01T00:00:00Z")
        .bind("2026-01-01T00:00:00Z")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO Repo (\
              id, name, componentId, owner, criticality, autonomy, qualityScore, execStatus,\
              lastEval, coverage, secScore, createdAt, updatedAt\
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
        )
        .bind("repo-legacy")
        .bind("legacy-repo")
        .bind("comp-legacy")
        .bind("legacy-owner")
        .bind("low")
        .bind("high")
        .bind(0_i64)
        .bind("idle")
        .bind("2026-01-01T00:00:00Z")
        .bind(0_i64)
        .bind(0_i64)
        .bind("2026-01-01T00:00:00Z")
        .bind("2026-01-01T00:00:00Z")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO KPI (\
              id, name, description, scopeLevel, threshold, weight, aggFn, createdAt, updatedAt\
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?);",
        )
        .bind("kpi-legacy")
        .bind("Legacy KPI")
        .bind("legacy")
        .bind("repo")
        .bind(0.0_f64)
        .bind(1.0_f64)
        .bind("latest")
        .bind("2026-01-01T00:00:00Z")
        .bind("2026-01-01T00:00:00Z")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO Indicator (\
              id, name, description, scope, weight, threshold, current, trend, reports, mode,\
              lastRun, createdAt, updatedAt\
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
        )
        .bind("ind-legacy")
        .bind("Legacy Indicator")
        .bind("legacy")
        .bind("repo")
        .bind(1.0_f64)
        .bind(0.0_f64)
        .bind(0.0_f64)
        .bind(0.0_f64)
        .bind(0_i64)
        .bind("latest")
        .bind("2026-01-01T00:00:00Z")
        .bind("2026-01-01T00:00:00Z")
        .bind("2026-01-01T00:00:00Z")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO Opportunity (\
              id, title, origin, componentId, module, repoId, indicator, confidence, risk,\
              expectedValue, effort, status, age, rationale, dependsOn, blocks, createdAt, updatedAt\
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
        )
        .bind("opp-legacy")
        .bind("Legacy opportunity")
        .bind("legacy")
        .bind("comp-legacy")
        .bind(Option::<String>::None)
        .bind("repo-legacy")
        .bind("Legacy Indicator")
        .bind(Option::<f64>::None)
        .bind("low")
        .bind(1.0_f64)
        .bind(Option::<String>::None)
        .bind("awaiting_triage")
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind("[]")
        .bind("[]")
        .bind("2026-01-01T00:00:00Z")
        .bind("2026-01-01T00:00:00Z")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO Regression (\
              id, repoName, indicator, delta, severity, since, trend, createdAt\
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?);",
        )
        .bind("reg-legacy")
        .bind("legacy-repo")
        .bind("Legacy Indicator")
        .bind(-1.0_f64)
        .bind("low")
        .bind("2026-01-01T00:00:00Z")
        .bind("stable")
        .bind("2026-01-01T00:00:00Z")
        .execute(&pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn legacy_indicator_schema_is_migrated_and_cleared() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("legacy.sqlite");
        let url = format!("sqlite://{}", db_path.display());
        create_legacy_db(&url).await;

        let store = SqliteBackendStore::new(&url).await.unwrap();

        let indicator_table: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'Indicator'",
        )
        .fetch_optional(&store.pool)
        .await
        .unwrap();
        assert!(indicator_table.is_none(), "Indicator table must be dropped");

        // Post-spec-061: Opportunity table is replaced by Finding table.
        let opportunity_table: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'Opportunity'",
        )
        .fetch_optional(&store.pool)
        .await
        .unwrap();
        assert!(
            opportunity_table.is_none(),
            "Opportunity table must be dropped by migration 004"
        );

        let finding_table: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'Finding'",
        )
        .fetch_optional(&store.pool)
        .await
        .unwrap();
        assert!(
            finding_table.is_some(),
            "Finding table must exist after migration 004"
        );

        let (regression_has_kpi,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pragma_table_info('Regression') WHERE name = 'kpiId'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        let (regression_has_indicator,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pragma_table_info('Regression') WHERE name = 'indicator'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(regression_has_kpi, 1, "Regression.kpiId must exist");
        assert_eq!(
            regression_has_indicator, 0,
            "Regression.indicator must be removed"
        );

        let (reg_kpi_nulls,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM Regression WHERE kpiId IS NULL")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(reg_kpi_nulls, 1, "migrated Regression.kpiId must be NULL");

        let (component_has_health,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pragma_table_info('Component') WHERE name = 'health'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(
            component_has_health, 0,
            "Component.health must be removed after upgrade"
        );

        let (repo_has_quality_score,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pragma_table_info('Repo') WHERE name = 'qualityScore'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(
            repo_has_quality_score, 0,
            "Repo.qualityScore must be removed after upgrade"
        );

        let (repo_has_coverage,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pragma_table_info('Repo') WHERE name = 'coverage'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(
            repo_has_coverage, 0,
            "Repo.coverage must be removed after upgrade"
        );

        let (repo_has_sec_score,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pragma_table_info('Repo') WHERE name = 'secScore'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(
            repo_has_sec_score, 0,
            "Repo.secScore must be removed after upgrade"
        );
    }
}

#[cfg(test)]
mod kpi_create_inline_grade_tests {
    use super::*;

    async fn seed_component(store: &SqliteBackendStore) -> String {
        let now = SqliteBackendStore::now_iso();
        let product = store
            .create_product(CreateProductBody {
                name: "Product for KPI tests".to_string(),
            })
            .await
            .unwrap();
        let component = store
            .create_component(CreateComponentBody {
                name: "Component for KPI tests".to_string(),
                product_id: product.id,
                domain: "platform".to_string(),
                owner: "owner".to_string(),
                criticality: "low".to_string(),
                autonomy: "semi".to_string(),
                trend: 0,
                last_eval: now,
            })
            .await
            .unwrap();
        component.id
    }

    #[tokio::test]
    async fn create_kpi_inserts_and_returns_item() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let item = store
            .create_kpi(CreateKpiBody {
                id: "kpi-001".to_string(),
                name: "Test KPI".to_string(),
                description: "A test KPI".to_string(),
                scope_level: "component".to_string(),
                threshold: 70.0,
                weight: 1.0,
                agg_fn: "latest".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(item.id, "kpi-001");
        assert_eq!(item.agg_fn, "latest");
        assert_eq!(item.scope_level, "component");
        assert!(!item.created_at.is_empty());
        assert!(!item.updated_at.is_empty());
    }

    #[tokio::test]
    async fn create_kpi_upsert_updates_fields_preserves_created_at() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let first = store
            .create_kpi(CreateKpiBody {
                id: "kpi-upsert".to_string(),
                name: "Original Name".to_string(),
                description: "orig".to_string(),
                scope_level: "repo".to_string(),
                threshold: 50.0,
                weight: 1.0,
                agg_fn: "avg".to_string(),
            })
            .await
            .unwrap();
        let second = store
            .create_kpi(CreateKpiBody {
                id: "kpi-upsert".to_string(),
                name: "Original Name".to_string(),
                description: "updated".to_string(),
                scope_level: "repo".to_string(),
                threshold: 60.0,
                weight: 2.0,
                agg_fn: "p90".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(second.description, "updated");
        assert_eq!(second.threshold, 60.0);
        assert_eq!(second.agg_fn, "p90");
        assert_eq!(
            first.created_at, second.created_at,
            "createdAt must not change on upsert"
        );
    }

    #[tokio::test]
    async fn create_kpi_invalid_agg_fn_returns_validation() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let err = store
            .create_kpi(CreateKpiBody {
                id: "kpi-bad".to_string(),
                name: "Bad KPI".to_string(),
                description: "".to_string(),
                scope_level: "repo".to_string(),
                threshold: 50.0,
                weight: 1.0,
                agg_fn: "median".to_string(),
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, "ERR_VALIDATION");
        assert!(err.message.contains("aggFn"));
    }

    #[tokio::test]
    async fn create_kpi_invalid_scope_level_returns_validation() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let err = store
            .create_kpi(CreateKpiBody {
                id: "kpi-bad2".to_string(),
                name: "Bad KPI 2".to_string(),
                description: "".to_string(),
                scope_level: "team".to_string(),
                threshold: 50.0,
                weight: 1.0,
                agg_fn: "latest".to_string(),
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, "ERR_VALIDATION");
        assert!(err.message.contains("scopeLevel"));
    }

    #[tokio::test]
    async fn create_kpi_threshold_out_of_range_returns_validation() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let err = store
            .create_kpi(CreateKpiBody {
                id: "kpi-bad3".to_string(),
                name: "Bad KPI 3".to_string(),
                description: "".to_string(),
                scope_level: "repo".to_string(),
                threshold: 150.0,
                weight: 1.0,
                agg_fn: "latest".to_string(),
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, "ERR_VALIDATION");
        assert!(err.message.contains("threshold"));
    }

    #[tokio::test]
    async fn create_kpi_zero_weight_returns_validation() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let err = store
            .create_kpi(CreateKpiBody {
                id: "kpi-bad4".to_string(),
                name: "Bad KPI 4".to_string(),
                description: "".to_string(),
                scope_level: "repo".to_string(),
                threshold: 50.0,
                weight: 0.0,
                agg_fn: "latest".to_string(),
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, "ERR_VALIDATION");
        assert!(err.message.contains("weight"));
    }

    #[tokio::test]
    async fn create_eval_run_with_inline_grades_inserts_atomically() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let component_id = seed_component(&store).await;

        let item = store
            .create_eval_run(CreateEvalRunBody {
                id: "run-inline-001".to_string(),
                source: "test".to_string(),
                scope: "component".to_string(),
                scope_id: component_id.clone(),
                score: Some(75.0),
                verdict: None,
                summary: None,
                evaluated_at: None,
                grades: Some(vec![
                    CreateGradeInlineBody {
                        kpi_id: None,
                        dimension: "tests".to_string(),
                        score: 80.0,
                        evidence: None,
                        evaluated_at: None,
                    },
                    CreateGradeInlineBody {
                        kpi_id: None,
                        dimension: "security".to_string(),
                        score: 70.0,
                        evidence: Some(serde_json::json!({"findings": []})),
                        evaluated_at: None,
                    },
                ]),
                raw_assessment: None,
            })
            .await
            .unwrap();

        assert_eq!(item.id, "run-inline-001");

        let grades = store
            .list_grades(Some("run-inline-001".to_string()), None)
            .await
            .unwrap();
        assert_eq!(grades.len(), 2, "must have exactly 2 grade rows");
        let dims: std::collections::HashSet<&str> =
            grades.iter().map(|g| g.dimension.as_str()).collect();
        assert!(dims.contains("tests"));
        assert!(dims.contains("security"));
        for g in &grades {
            assert_eq!(g.run_id, "run-inline-001");
            assert!(!g.id.is_empty(), "grade id must be server-generated");
        }
    }

    #[tokio::test]
    async fn create_eval_run_inline_grade_rollback_on_bad_kpi() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let component_id = seed_component(&store).await;

        let err = store
            .create_eval_run(CreateEvalRunBody {
                id: "run-rollback-001".to_string(),
                source: "test".to_string(),
                scope: "component".to_string(),
                scope_id: component_id,
                score: None,
                verdict: None,
                summary: None,
                evaluated_at: None,
                grades: Some(vec![CreateGradeInlineBody {
                    kpi_id: Some("nonexistent-kpi".to_string()),
                    dimension: "tests".to_string(),
                    score: 50.0,
                    evidence: None,
                    evaluated_at: None,
                }]),
                raw_assessment: None,
            })
            .await
            .unwrap_err();

        assert_eq!(err.code, "ERR_NOT_FOUND");

        let run_err = store.get_eval_run("run-rollback-001").await.unwrap_err();
        assert_eq!(run_err.code, "ERR_NOT_FOUND");

        let grades = store
            .list_grades(Some("run-rollback-001".to_string()), None)
            .await
            .unwrap();
        assert_eq!(grades.len(), 0, "rollback must leave no grade rows");
    }

    #[tokio::test]
    async fn create_eval_run_inline_grade_duplicate_dimension_returns_conflict() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let component_id = seed_component(&store).await;

        let err = store
            .create_eval_run(CreateEvalRunBody {
                id: "run-dup-dim".to_string(),
                source: "test".to_string(),
                scope: "component".to_string(),
                scope_id: component_id,
                score: None,
                verdict: None,
                summary: None,
                evaluated_at: None,
                grades: Some(vec![
                    CreateGradeInlineBody {
                        kpi_id: None,
                        dimension: "tests".to_string(),
                        score: 50.0,
                        evidence: None,
                        evaluated_at: None,
                    },
                    CreateGradeInlineBody {
                        kpi_id: None,
                        dimension: "tests".to_string(),
                        score: 60.0,
                        evidence: None,
                        evaluated_at: None,
                    },
                ]),
                raw_assessment: None,
            })
            .await
            .unwrap_err();

        assert_eq!(err.code, "ERR_CONFLICT");
    }

    #[tokio::test]
    async fn create_eval_run_inline_grade_empty_dimension_returns_validation() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let component_id = seed_component(&store).await;

        let err = store
            .create_eval_run(CreateEvalRunBody {
                id: "run-empty-dim".to_string(),
                source: "test".to_string(),
                scope: "component".to_string(),
                scope_id: component_id,
                score: None,
                verdict: None,
                summary: None,
                evaluated_at: None,
                grades: Some(vec![CreateGradeInlineBody {
                    kpi_id: None,
                    dimension: "  ".to_string(),
                    score: 50.0,
                    evidence: None,
                    evaluated_at: None,
                }]),
                raw_assessment: None,
            })
            .await
            .unwrap_err();

        assert_eq!(err.code, "ERR_VALIDATION");
    }

    #[tokio::test]
    async fn create_eval_run_without_grades_preserves_current_behavior() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let component_id = seed_component(&store).await;

        let item = store
            .create_eval_run(CreateEvalRunBody {
                id: "run-no-grades".to_string(),
                source: "test".to_string(),
                scope: "component".to_string(),
                scope_id: component_id,
                score: None,
                verdict: None,
                summary: None,
                evaluated_at: None,
                grades: None,
                raw_assessment: None,
            })
            .await
            .unwrap();

        assert_eq!(item.id, "run-no-grades");
        let grades = store
            .list_grades(Some("run-no-grades".to_string()), None)
            .await
            .unwrap();
        assert_eq!(
            grades.len(),
            0,
            "no grades must be created when grades is None"
        );
    }
}
