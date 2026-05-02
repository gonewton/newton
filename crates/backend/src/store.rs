use crate::models::*;
use crate::{err_conflict, err_internal, err_not_found, err_validation, BackendStore};
use chrono::Utc;
use newton_types::ApiError;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::FromRow;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet, VecDeque};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Clone)]
pub struct SqliteBackendStore {
    pool: SqlitePool,
}

impl SqliteBackendStore {
    pub async fn new(database_url: &str) -> Result<Self, ApiError> {
        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(|e| err_internal(&format!("invalid database URL: {e}")))?
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|e| err_internal(&format!("failed to connect to database: {e}")))?;

        sqlx::query(include_str!("../migrations/001_init.sql"))
            .execute(&pool)
            .await
            .map_err(|e| err_internal(&format!("migration failed: {e}")))?;

        Ok(Self { pool })
    }

    pub async fn new_in_memory() -> Result<Self, ApiError> {
        Self::new("sqlite::memory:").await
    }

    fn now_iso() -> String {
        Utc::now().to_rfc3339()
    }
}

#[derive(Debug, FromRow)]
struct ProductRow {
    id: String,
    name: String,
    component_count: i64,
}

#[derive(Debug, FromRow)]
struct ComponentRow {
    id: String,
    name: String,
    domain: String,
    repos: i64,
    modules: i64,
    health: i64,
    trend: i64,
    owner: String,
    criticality: String,
    autonomy: String,
    open_plans: i64,
    open_requests: i64,
    last_eval: String,
    product_id: String,
}

#[derive(Debug, FromRow)]
struct PendingApprovalRow {
    id: String,
    title: String,
    item_type: String,
    component_id: Option<String>,
    repo_name: Option<String>,
    risk: String,
    expected_value: String,
    waiting_since: String,
    reviewer: String,
    status: String,
    confidence: i64,
    agent_generated: i64,
}

#[derive(Debug, FromRow)]
struct RegressionRow {
    repo: String,
    indicator: String,
    delta: f64,
    severity: String,
    since: String,
    trend: String,
}

#[derive(Debug, FromRow)]
struct IndicatorRow {
    id: String,
    name: String,
    description: String,
    scope: String,
    weight: f64,
    threshold: f64,
    current: f64,
    trend: f64,
    reports: i64,
    mode: String,
    last_run: String,
}

#[derive(Debug, FromRow)]
struct RecentActionRow {
    time: String,
    action: String,
    subject: String,
    item_type: String,
}

#[derive(Debug, FromRow)]
struct RepoRow {
    id: String,
    name: String,
    component_id: String,
    owner: String,
    criticality: String,
    autonomy: String,
    quality_score: i64,
    regressions: i64,
    open_plans: i64,
    exec_status: String,
    last_eval: String,
    coverage: i64,
    sec_score: i64,
}

#[derive(Debug, FromRow)]
struct RepoDepTargetRow {
    target_repo: String,
}

#[derive(Debug, FromRow)]
struct ModuleDepRow {
    id: String,
    #[allow(dead_code)]
    from_module_id: String,
    #[allow(dead_code)]
    to_module_id: String,
    dep_type: String,
    label: String,
    from_module_name: String,
    from_module_kind: String,
    from_repo_id: String,
    to_module_name: String,
    to_module_kind: String,
    to_repo_id: String,
}

#[derive(Debug, FromRow)]
struct SavedViewRow {
    id: String,
    label: String,
    filters: Option<String>,
    sort: Option<String>,
    sort_dir: Option<String>,
}

#[derive(Debug, FromRow)]
struct SavedViewKindRow {
    id: String,
    kind: String,
    label: String,
    filters: Option<String>,
    sort: Option<String>,
    sort_dir: Option<String>,
}

#[derive(Debug, FromRow)]
struct OpportunityRow {
    id: String,
    title: String,
    origin: String,
    component_id: Option<String>,
    module: Option<String>,
    repo_id: Option<String>,
    indicator: Option<String>,
    confidence: Option<f64>,
    risk: String,
    expected_value: f64,
    effort: Option<String>,
    status: String,
    age: Option<String>,
    rationale: Option<String>,
    depends_on: Option<String>,
    blocks: Option<String>,
}

#[derive(Debug, FromRow)]
struct RequestRow {
    id: String,
    title: String,
    description: Option<String>,
    component_id: Option<String>,
    repo_id: Option<String>,
    requested_by: String,
    status: String,
    linked_opportunity_id: Option<String>,
    created_at: String,
}

#[derive(Debug, FromRow)]
struct PlanRow {
    id: String,
    title: String,
    component_id: Option<String>,
    repo_id: Option<String>,
    status: String,
    linked_request_id: Option<String>,
    confidence: i64,
    risk: String,
    expected_value: Option<String>,
    agent_generated: i64,
    waiting_since: Option<String>,
    created_at: String,
}

#[derive(Debug, FromRow)]
struct PlanSectionRow {
    id: String,
    label: String,
    content: String,
}

#[derive(Debug, FromRow)]
struct PlanPolicyCheckRow {
    rule: String,
    status: String,
    met: i64,
}

#[derive(Debug, FromRow)]
struct PlanApproverRow {
    role: String,
    name: String,
    status: String,
}

#[derive(Debug, FromRow)]
struct ExecutionRow {
    id: String,
    instance_id: Option<String>,
    plan_id: Option<String>,
    workflow_id: Option<String>,
    plan_title: Option<String>,
    repo_id: Option<String>,
    component_id: Option<String>,
    stage: Option<String>,
    status: String,
    policy_level: Option<String>,
    started_by: Option<String>,
    waiting_on: Option<String>,
    test_result: Option<String>,
    pr_status: Option<String>,
    pr_link: Option<String>,
    deploy_status: Option<String>,
    created_at: String,
    started: Option<String>,
}

#[derive(Debug, FromRow)]
struct OperatorRow {
    operator_type: String,
    description: String,
    params_schema: Option<String>,
    palette_label: Option<String>,
    palette_icon: Option<String>,
}

#[derive(Debug, FromRow)]
struct DepEdge {
    from_id: String,
    to_id: String,
}

#[derive(Debug, FromRow)]
struct CountRow {
    count: i64,
}

#[derive(Debug, FromRow)]
struct IdRow {
    id: String,
}

#[derive(Debug, FromRow)]
struct NameRow {
    name: String,
}

#[derive(Debug, FromRow)]
struct ComponentIdRow {
    component_id: Option<String>,
}

#[derive(Debug, FromRow)]
struct StringValueRow {
    value: Option<String>,
}

#[derive(Debug, FromRow)]
struct ExpectedDeltaRow {
    expected_delta: Option<String>,
}

#[async_trait::async_trait]
impl BackendStore for SqliteBackendStore {
    async fn list_products(&self) -> Result<Vec<ProductItem>, ApiError> {
        let rows = sqlx::query_as::<_, ProductRow>(
            "SELECT p.id, p.name, COUNT(c.id) as component_count FROM Product p LEFT JOIN Component c ON c.productId = p.id GROUP BY p.id ORDER BY p.id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| ProductItem {
                id: r.id,
                name: r.name,
                component_count: r.component_count,
            })
            .collect())
    }

    async fn list_components(&self) -> Result<Vec<ComponentItem>, ApiError> {
        let rows = sqlx::query_as::<_, ComponentRow>(
            "SELECT id, name, domain, repos, modules, health, trend, owner, criticality, autonomy, openPlans as open_plans, openRequests as open_requests, lastEval as last_eval, productId as product_id FROM Component ORDER BY id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            let product_name: Option<NameRow> =
                sqlx::query_as::<_, NameRow>("SELECT name FROM Product WHERE id = ?")
                    .bind(&row.product_id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| err_internal(&format!("query error: {e}")))?;
            result.push(ComponentItem {
                id: row.id,
                name: row.name,
                product_id: row.product_id,
                product_name: product_name.map(|p| p.name).unwrap_or_default(),
                domain: row.domain,
                repos: row.repos,
                modules: row.modules,
                health: row.health,
                trend: row.trend,
                owner: row.owner,
                criticality: row.criticality,
                autonomy: row.autonomy,
                open_plans: row.open_plans,
                open_requests: row.open_requests,
                last_eval: row.last_eval,
            });
        }
        Ok(result)
    }

    async fn list_pending_approvals(&self) -> Result<Vec<PendingApprovalItem>, ApiError> {
        let rows = sqlx::query_as::<_, PendingApprovalRow>(
            "SELECT id, title, type as item_type, componentId as component_id, repoName as repo_name, risk, expectedValue as expected_value, waitingSince as waiting_since, reviewer, status, confidence, agentGenerated as agent_generated FROM PendingApproval ORDER BY id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            let product = if let Some(ref cid) = row.component_id {
                let cn: Option<NameRow> =
                    sqlx::query_as::<_, NameRow>("SELECT name FROM Component WHERE id = ?")
                        .bind(cid)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| err_internal(&format!("query error: {e}")))?;
                cn.map(|c| c.name).unwrap_or_default()
            } else {
                String::new()
            };
            result.push(PendingApprovalItem {
                id: row.id,
                title: row.title,
                item_type: row.item_type,
                product,
                repo: row.repo_name.unwrap_or_default(),
                risk: row.risk,
                expected_value: row.expected_value,
                waiting_since: row.waiting_since,
                reviewer: row.reviewer,
                status: row.status,
                confidence: row.confidence,
                agent_generated: row.agent_generated != 0,
            });
        }
        Ok(result)
    }

    async fn list_regressions(&self) -> Result<Vec<RegressionItem>, ApiError> {
        let rows = sqlx::query_as::<_, RegressionRow>(
            "SELECT repoName as repo, indicator, delta, severity, since, trend FROM Regression ORDER BY id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| RegressionItem {
                repo: r.repo,
                indicator: r.indicator,
                delta: r.delta,
                severity: r.severity,
                since: r.since,
                trend: r.trend,
            })
            .collect())
    }

    async fn list_indicators(&self) -> Result<Vec<IndicatorItem>, ApiError> {
        let rows = sqlx::query_as::<_, IndicatorRow>(
            "SELECT id, name, description, scope, weight, threshold, current, trend, reports, mode, lastRun as last_run FROM Indicator ORDER BY id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| IndicatorItem {
                id: r.id,
                name: r.name,
                description: r.description,
                scope: r.scope,
                weight: r.weight,
                threshold: r.threshold,
                current: r.current,
                trend: r.trend,
                reports: r.reports,
                mode: r.mode,
                last_run: r.last_run,
            })
            .collect())
    }

    async fn list_recent_actions(&self, limit: u32) -> Result<Vec<RecentActionItem>, ApiError> {
        let rows = sqlx::query_as::<_, RecentActionRow>(
            "SELECT time, action, subject, type as item_type FROM RecentAction ORDER BY createdAt DESC LIMIT ?"
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| RecentActionItem {
                time: r.time,
                action: r.action,
                subject: r.subject,
                item_type: r.item_type,
            })
            .collect())
    }

    async fn list_repos(&self) -> Result<Vec<RepoItem>, ApiError> {
        let rows = sqlx::query_as::<_, RepoRow>(
            "SELECT r.id, r.name, r.componentId as component_id, r.owner, r.criticality, r.autonomy, r.qualityScore as quality_score, r.regressions, r.openPlans as open_plans, r.execStatus as exec_status, r.lastEval as last_eval, r.coverage, r.secScore as sec_score FROM Repo r ORDER BY r.id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let mut result = Vec::new();
        for row in &rows {
            let component: Option<NameRow> =
                sqlx::query_as::<_, NameRow>("SELECT name FROM Component WHERE id = ?")
                    .bind(&row.component_id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| err_internal(&format!("query error: {e}")))?;

            let depends_on = compute_repo_depends_on(&self.pool, &row.name).await?;
            let depended_on_by = compute_repo_depended_on_by(&self.pool, &row.name).await?;

            result.push(RepoItem {
                id: row.id.clone(),
                name: row.name.clone(),
                component: component.map(|c| c.name).unwrap_or_default(),
                owner: row.owner.clone(),
                criticality: row.criticality.clone(),
                autonomy: row.autonomy.clone(),
                quality_score: row.quality_score,
                regressions: row.regressions,
                open_plans: row.open_plans,
                exec_status: row.exec_status.clone(),
                last_eval: row.last_eval.clone(),
                coverage: row.coverage,
                sec_score: row.sec_score,
                depends_on,
                depended_on_by,
            });
        }
        Ok(result)
    }

    async fn list_repo_dependencies(&self) -> Result<Vec<RepoDependencyItem>, ApiError> {
        let deps = sqlx::query_as::<_, ModuleDepRow>(
            "SELECT md.id, md.fromModuleId as from_module_id, md.toModuleId as to_module_id, md.type as dep_type, md.label,
             fm.name as from_module_name, fm.kind as from_module_kind, fm.repoId as from_repo_id,
             tm.name as to_module_name, tm.kind as to_module_kind, tm.repoId as to_repo_id
             FROM ModuleDependency md
             JOIN Module fm ON fm.id = md.fromModuleId
             JOIN Module tm ON tm.id = md.toModuleId
             ORDER BY md.id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for dep in &deps {
            let from_repo_name = get_repo_name(&self.pool, &dep.from_repo_id).await?;
            let to_repo_name = get_repo_name(&self.pool, &dep.to_repo_id).await?;
            if from_repo_name == to_repo_name {
                continue;
            }
            let key = (from_repo_name.clone(), to_repo_name.clone());
            if seen.insert(key.clone()) {
                result.push(RepoDependencyItem {
                    from: key.0,
                    to: key.1,
                    dep_type: dep.dep_type.clone(),
                    label: dep.label.clone(),
                });
            }
        }
        Ok(result)
    }

    async fn list_module_dependencies(&self) -> Result<Vec<ModuleDependencyItem>, ApiError> {
        let deps = sqlx::query_as::<_, ModuleDepRow>(
            "SELECT md.id, md.fromModuleId as from_module_id, md.toModuleId as to_module_id, md.type as dep_type, md.label,
             fm.name as from_module_name, fm.kind as from_module_kind, fm.repoId as from_repo_id,
             tm.name as to_module_name, tm.kind as to_module_kind, tm.repoId as to_repo_id
             FROM ModuleDependency md
             JOIN Module fm ON fm.id = md.fromModuleId
             JOIN Module tm ON tm.id = md.toModuleId
             ORDER BY md.id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let mut result = Vec::new();
        for dep in &deps {
            let from_repo_name = get_repo_name(&self.pool, &dep.from_repo_id).await?;
            let from_component = get_component_name_for_repo(&self.pool, &dep.from_repo_id).await?;
            let to_repo_name = get_repo_name(&self.pool, &dep.to_repo_id).await?;
            let to_component = get_component_name_for_repo(&self.pool, &dep.to_repo_id).await?;

            result.push(ModuleDependencyItem {
                id: dep.id.clone(),
                from: ModuleRef {
                    module: dep.from_module_name.clone(),
                    kind: dep.from_module_kind.clone(),
                    repo: from_repo_name,
                    component: from_component,
                },
                to: ModuleRef {
                    module: dep.to_module_name.clone(),
                    kind: dep.to_module_kind.clone(),
                    repo: to_repo_name,
                    component: to_component,
                },
                dep_type: dep.dep_type.clone(),
                label: dep.label.clone(),
            });
        }
        Ok(result)
    }

    async fn create_module_dependency(
        &self,
        body: CreateModuleDependencyBody,
    ) -> Result<ModuleDependencyItem, ApiError> {
        let count: Option<CountRow> =
            sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Module WHERE id = ?")
                .bind(&body.from_module_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if count.map(|c| c.count).unwrap_or(0) == 0 {
            return Err(err_not_found("Source module not found"));
        }

        let count: Option<CountRow> =
            sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Module WHERE id = ?")
                .bind(&body.to_module_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if count.map(|c| c.count).unwrap_or(0) == 0 {
            return Err(err_not_found("Target module not found"));
        }

        if body.from_module_id == body.to_module_id {
            return Err(err_validation("Self-dependency is not allowed"));
        }

        let existing: Option<CountRow> = sqlx::query_as::<_, CountRow>(
            "SELECT COUNT(*) as count FROM ModuleDependency WHERE fromModuleId = ? AND toModuleId = ?"
        )
        .bind(&body.from_module_id)
        .bind(&body.to_module_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if existing.map(|c| c.count).unwrap_or(0) > 0 {
            return Err(err_conflict("Module dependency already exists"));
        }

        if self
            .check_cycle(&body.from_module_id, &body.to_module_id)
            .await?
        {
            return Err(err_conflict("Creating this dependency would form a cycle"));
        }

        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO ModuleDependency (id, fromModuleId, toModuleId, type, label) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&id).bind(&body.from_module_id).bind(&body.to_module_id).bind(&body.dep_type).bind(&body.label)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("insert error: {e}")))?;

        self.list_module_dependencies()
            .await?
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| err_internal("Failed to read back created dependency"))
    }

    async fn list_saved_views(&self, kind: Option<String>) -> Result<serde_json::Value, ApiError> {
        if let Some(ref k) = kind {
            let rows = sqlx::query_as::<_, SavedViewRow>(
                "SELECT id, label, filters, sort, sortDir as sort_dir FROM SavedView WHERE kind = ? ORDER BY id ASC"
            )
            .bind(k)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("query error: {e}")))?;

            let items: Vec<SavedViewItem> = rows
                .into_iter()
                .map(|r| SavedViewItem {
                    id: r.id,
                    label: r.label,
                    filters: r.filters.and_then(|s| serde_json::from_str(&s).ok()),
                    sort: r.sort,
                    sort_dir: r.sort_dir,
                })
                .collect();
            Ok(serde_json::to_value(items).unwrap_or(serde_json::Value::Null))
        } else {
            let rows = sqlx::query_as::<_, SavedViewKindRow>(
                "SELECT id, kind, label, filters, sort, sortDir as sort_dir FROM SavedView ORDER BY id ASC"
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("query error: {e}")))?;

            let mut grouped: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
            for row in rows {
                let item = SavedViewItem {
                    id: row.id,
                    label: row.label,
                    filters: row.filters.and_then(|s| serde_json::from_str(&s).ok()),
                    sort: row.sort,
                    sort_dir: row.sort_dir,
                };
                let val = serde_json::to_value(&item).unwrap_or(serde_json::Value::Null);
                grouped
                    .entry(row.kind)
                    .or_insert_with(|| serde_json::Value::Array(Vec::new()))
                    .as_array_mut()
                    .unwrap()
                    .push(val);
            }
            Ok(serde_json::Value::Object(grouped))
        }
    }

    async fn list_opportunities(
        &self,
        status: Option<String>,
    ) -> Result<Vec<OpportunityItem>, ApiError> {
        let rows = if let Some(ref s) = status {
            sqlx::query_as::<_, OpportunityRow>(
                "SELECT id, title, origin, componentId as component_id, module, repoId as repo_id, indicator, confidence, risk, expectedValue as expected_value, effort, status, age, rationale, dependsOn as depends_on, blocks FROM Opportunity WHERE status = ? ORDER BY id ASC"
            ).bind(s)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("query error: {e}")))?
        } else {
            sqlx::query_as::<_, OpportunityRow>(
                "SELECT id, title, origin, componentId as component_id, module, repoId as repo_id, indicator, confidence, risk, expectedValue as expected_value, effort, status, age, rationale, dependsOn as depends_on, blocks FROM Opportunity ORDER BY id ASC"
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("query error: {e}")))?
        };

        let mut result = Vec::new();
        for row in rows {
            let component = if let Some(ref cid) = row.component_id {
                let cn: Option<NameRow> =
                    sqlx::query_as::<_, NameRow>("SELECT name FROM Component WHERE id = ?")
                        .bind(cid)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| err_internal(&format!("query error: {e}")))?;
                cn.map(|c| c.name).unwrap_or_default()
            } else {
                String::new()
            };
            let repo = if let Some(ref rid) = row.repo_id {
                let rn: Option<NameRow> =
                    sqlx::query_as::<_, NameRow>("SELECT name FROM Repo WHERE id = ?")
                        .bind(rid)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| err_internal(&format!("query error: {e}")))?;
                rn.map(|r| r.name).unwrap_or_default()
            } else {
                String::new()
            };
            let depends_on: Vec<String> = row
                .depends_on
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            let blocks: Vec<String> = row
                .blocks
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

            result.push(OpportunityItem {
                id: row.id,
                title: row.title,
                origin: row.origin,
                component,
                module: row.module,
                repo,
                indicator: row.indicator,
                confidence: row.confidence,
                risk: row.risk,
                expected_value: row.expected_value,
                effort: row.effort,
                status: row.status,
                age: row.age,
                rationale: row.rationale,
                depends_on,
                blocks,
            });
        }
        Ok(result)
    }

    async fn patch_opportunity(
        &self,
        id: &str,
        body: PatchOpportunityBody,
    ) -> Result<OpportunityItem, ApiError> {
        let valid_statuses = [
            "awaiting_triage",
            "triaged",
            "approved_for_planning",
            "structured",
            "deferred",
            "rejected",
        ];
        if !valid_statuses.contains(&body.status.as_str()) {
            return Err(err_validation("Invalid opportunity status"));
        }

        let count: Option<CountRow> =
            sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Opportunity WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if count.map(|c| c.count).unwrap_or(0) == 0 {
            return Err(err_not_found("Opportunity not found"));
        }

        let now = Self::now_iso();
        sqlx::query("UPDATE Opportunity SET status = ?, updatedAt = ? WHERE id = ?")
            .bind(&body.status)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("update error: {e}")))?;

        self.list_opportunities(None)
            .await?
            .into_iter()
            .find(|o| o.id == id)
            .ok_or_else(|| err_internal("Failed to read back updated opportunity"))
    }

    async fn list_requests(&self) -> Result<Vec<RequestItem>, ApiError> {
        let rows = sqlx::query_as::<_, RequestRow>(
            "SELECT id, title, description, componentId as component_id, repoId as repo_id, requestedBy as requested_by, status, linkedOpportunityId as linked_opportunity_id, createdAt as created_at FROM Request ORDER BY id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            let component = if let Some(ref cid) = row.component_id {
                let cn: Option<NameRow> =
                    sqlx::query_as::<_, NameRow>("SELECT name FROM Component WHERE id = ?")
                        .bind(cid)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| err_internal(&format!("query error: {e}")))?;
                cn.map(|c| c.name).unwrap_or_default()
            } else {
                String::new()
            };
            let repo = if let Some(ref rid) = row.repo_id {
                let rn: Option<NameRow> =
                    sqlx::query_as::<_, NameRow>("SELECT name FROM Repo WHERE id = ?")
                        .bind(rid)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| err_internal(&format!("query error: {e}")))?;
                rn.map(|r| r.name).unwrap_or_default()
            } else {
                String::new()
            };
            result.push(RequestItem {
                id: row.id,
                title: row.title,
                description: row.description,
                component,
                repo,
                requested_by: row.requested_by,
                status: row.status,
                linked_opportunity_id: row.linked_opportunity_id,
                created_at: row.created_at,
            });
        }
        Ok(result)
    }

    async fn create_request(&self, body: CreateRequestBody) -> Result<RequestItem, ApiError> {
        let id = Uuid::new_v4().to_string();
        let now = Self::now_iso();

        let component_id = if let Some(ref name) = body.component {
            let r: Option<IdRow> =
                sqlx::query_as::<_, IdRow>("SELECT id FROM Component WHERE name = ?")
                    .bind(name)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| err_internal(&format!("query error: {e}")))?;
            r.map(|r| r.id)
        } else {
            None
        };
        let repo_id = if let Some(ref name) = body.repo {
            let r: Option<IdRow> = sqlx::query_as::<_, IdRow>("SELECT id FROM Repo WHERE name = ?")
                .bind(name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
            r.map(|r| r.id)
        } else {
            None
        };

        sqlx::query(
            "INSERT INTO Request (id, title, description, componentId, repoId, requestedBy, status, linkedOpportunityId, createdAt, updatedAt) VALUES (?, ?, ?, ?, ?, ?, 'draft', ?, ?, ?)"
        )
        .bind(&id).bind(&body.title).bind(&body.description).bind(&component_id).bind(&repo_id).bind(&body.requested_by).bind(&body.linked_opportunity_id).bind(&now).bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("insert error: {e}")))?;

        self.list_requests()
            .await?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| err_internal("Failed to read back created request"))
    }

    async fn list_plans(&self) -> Result<Vec<PlanItem>, ApiError> {
        let rows = sqlx::query_as::<_, PlanRow>(
            "SELECT id, title, componentId as component_id, repoId as repo_id, status, linkedRequestId as linked_request_id, confidence, risk, expectedValue as expected_value, agentGenerated as agent_generated, waitingSince as waiting_since, createdAt as created_at FROM Plan ORDER BY id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            let component = if let Some(ref cid) = row.component_id {
                let cn: Option<NameRow> =
                    sqlx::query_as::<_, NameRow>("SELECT name FROM Component WHERE id = ?")
                        .bind(cid)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| err_internal(&format!("query error: {e}")))?;
                cn.map(|c| c.name).unwrap_or_default()
            } else {
                String::new()
            };
            let repo = if let Some(ref rid) = row.repo_id {
                let rn: Option<NameRow> =
                    sqlx::query_as::<_, NameRow>("SELECT name FROM Repo WHERE id = ?")
                        .bind(rid)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| err_internal(&format!("query error: {e}")))?;
                rn.map(|r| r.name).unwrap_or_default()
            } else {
                String::new()
            };
            let exec_ids: Vec<IdRow> = sqlx::query_as::<_, IdRow>(
                "SELECT id FROM ExecutionRecord WHERE planId = ? ORDER BY id ASC",
            )
            .bind(&row.id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("query error: {e}")))?;

            result.push(PlanItem {
                id: row.id,
                title: row.title,
                component,
                repo,
                status: row.status,
                linked_request_id: row.linked_request_id,
                execution_ids: exec_ids.into_iter().map(|e| e.id).collect(),
                confidence: row.confidence,
                risk: row.risk,
                expected_value: row.expected_value,
                agent_generated: row.agent_generated != 0,
                waiting_since: row.waiting_since,
                created_at: row.created_at,
            });
        }
        Ok(result)
    }

    async fn get_plan(&self, id: &str) -> Result<PlanDetail, ApiError> {
        let plan = self
            .list_plans()
            .await?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| err_not_found("Plan not found"))?;

        let delta: Option<ExpectedDeltaRow> = sqlx::query_as::<_, ExpectedDeltaRow>(
            "SELECT expectedDelta as expected_delta FROM Plan WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let sections = sqlx::query_as::<_, PlanSectionRow>(
            "SELECT id, label, content FROM PlanSection WHERE planId = ? ORDER BY sortOrder ASC",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let policy_checks = sqlx::query_as::<_, PlanPolicyCheckRow>(
            "SELECT rule, status, met FROM PlanPolicyCheck WHERE planId = ?",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let approvers = sqlx::query_as::<_, PlanApproverRow>(
            "SELECT role, name, approverStatus as status FROM PlanApprover WHERE planId = ?",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        Ok(PlanDetail {
            plan,
            expected_delta: delta.and_then(|d| d.expected_delta),
            sections: sections
                .into_iter()
                .map(|s| PlanSectionItem {
                    id: s.id,
                    label: s.label,
                    content: s.content,
                })
                .collect(),
            policy_checks: policy_checks
                .into_iter()
                .map(|p| PlanPolicyCheckItem {
                    rule: p.rule,
                    status: p.status,
                    met: p.met != 0,
                })
                .collect(),
            approvers: approvers
                .into_iter()
                .map(|a| PlanApproverItem {
                    role: a.role,
                    name: a.name,
                    status: a.status,
                })
                .collect(),
        })
    }

    async fn approve_plan(&self, id: &str) -> Result<ApprovedPlan, ApiError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| err_internal(&format!("begin tx error: {e}")))?;

        let plan: Option<PlanRow> = sqlx::query_as::<_, PlanRow>(
            "SELECT id, title, componentId as component_id, repoId as repo_id, status, linkedRequestId as linked_request_id, confidence, risk, expectedValue as expected_value, agentGenerated as agent_generated, waitingSince as waiting_since, createdAt as created_at FROM Plan WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let plan = plan.ok_or_else(|| err_not_found("Plan not found"))?;

        if plan.status != "awaiting_approval" && plan.status != "needs_revision" {
            return Err(err_conflict("Plan is not in an approvable state"));
        }

        let now = Self::now_iso();
        sqlx::query("UPDATE Plan SET status = 'approved', updatedAt = ? WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| err_internal(&format!("update error: {e}")))?;

        let exec_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO ExecutionRecord (id, planId, planTitle, repoId, componentId, status, startedBy, createdAt) VALUES (?, ?, ?, ?, ?, 'running', 'Newton Agent', ?)"
        )
        .bind(&exec_id).bind(id).bind(&plan.title).bind(&plan.repo_id).bind(&plan.component_id).bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("insert error: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| err_internal(&format!("commit error: {e}")))?;

        let plan_item = self.fetch_plan_item(id).await?;
        Ok(ApprovedPlan {
            plan: plan_item,
            execution_id: exec_id,
            created_at: now,
        })
    }

    async fn reject_plan(&self, id: &str) -> Result<PlanItem, ApiError> {
        let plan: Option<PlanRow> = sqlx::query_as::<_, PlanRow>(
            "SELECT id, title, componentId as component_id, repoId as repo_id, status, linkedRequestId as linked_request_id, confidence, risk, expectedValue as expected_value, agentGenerated as agent_generated, waitingSince as waiting_since, createdAt as created_at FROM Plan WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let plan = plan.ok_or_else(|| err_not_found("Plan not found"))?;

        if plan.status != "awaiting_approval" && plan.status != "needs_revision" {
            return Err(err_conflict("Plan is not in a rejectable state"));
        }

        let now = Self::now_iso();
        sqlx::query("UPDATE Plan SET status = 'rejected', updatedAt = ? WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("update error: {e}")))?;

        self.fetch_plan_item(id).await
    }

    async fn list_executions(
        &self,
        plan_id: Option<String>,
    ) -> Result<Vec<ExecutionItem>, ApiError> {
        let rows = if let Some(ref pid) = plan_id {
            sqlx::query_as::<_, ExecutionRow>(
                "SELECT id, instanceId as instance_id, planId as plan_id, workflowId as workflow_id, planTitle as plan_title, repoId as repo_id, componentId as component_id, stage, status, policyLevel as policy_level, startedBy as started_by, waitingOn as waiting_on, testResult as test_result, prStatus as pr_status, prLink as pr_link, deployStatus as deploy_status, createdAt as created_at, startedAt as started FROM ExecutionRecord WHERE planId = ? ORDER BY id ASC"
            ).bind(pid)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("query error: {e}")))?
        } else {
            sqlx::query_as::<_, ExecutionRow>(
                "SELECT id, instanceId as instance_id, planId as plan_id, workflowId as workflow_id, planTitle as plan_title, repoId as repo_id, componentId as component_id, stage, status, policyLevel as policy_level, startedBy as started_by, waitingOn as waiting_on, testResult as test_result, prStatus as pr_status, prLink as pr_link, deployStatus as deploy_status, createdAt as created_at, startedAt as started FROM ExecutionRecord ORDER BY id ASC"
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("query error: {e}")))?
        };

        let mut result = Vec::new();
        for r in rows {
            let repo = if let Some(ref rid) = r.repo_id {
                let rn: Option<NameRow> =
                    sqlx::query_as::<_, NameRow>("SELECT name FROM Repo WHERE id = ?")
                        .bind(rid)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| err_internal(&format!("query error: {e}")))?;
                rn.map(|r| r.name)
            } else {
                None
            };
            let component = if let Some(ref cid) = r.component_id {
                let cn: Option<NameRow> =
                    sqlx::query_as::<_, NameRow>("SELECT name FROM Component WHERE id = ?")
                        .bind(cid)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| err_internal(&format!("query error: {e}")))?;
                cn.map(|c| c.name)
            } else {
                None
            };
            result.push(ExecutionItem {
                instance_id: r.instance_id.unwrap_or_else(|| r.id.clone()),
                plan_id: r.plan_id.clone(),
                linked_plan_id: r.plan_id,
                workflow_id: r.workflow_id,
                plan_title: r.plan_title,
                repo,
                component,
                stage: r.stage,
                status: r.status,
                policy_level: r.policy_level,
                started_by: r.started_by,
                waiting_on: r.waiting_on,
                test_result: r.test_result,
                pr_status: r.pr_status,
                pr_link: r.pr_link,
                deploy_status: r.deploy_status,
                created_at: r.created_at,
                started: r.started,
            });
        }
        Ok(result)
    }

    async fn list_operators(&self) -> Result<Vec<OperatorItem>, ApiError> {
        let rows = sqlx::query_as::<_, OperatorRow>(
            "SELECT operatorType as operator_type, description, paramsSchema as params_schema, paletteLabel as palette_label, paletteIcon as palette_icon FROM Operator ORDER BY id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| OperatorItem {
                operator_type: r.operator_type,
                description: r.description,
                params_schema: r
                    .params_schema
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
                palette_label: r.palette_label,
                palette_icon: r.palette_icon,
            })
            .collect())
    }

    async fn get_persistence(&self, key: &str) -> Result<serde_json::Value, ApiError> {
        let row: Option<StringValueRow> =
            sqlx::query_as::<_, StringValueRow>("SELECT value FROM Persistence WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;

        match row.and_then(|r| r.value) {
            Some(v) => serde_json::from_str(&v)
                .map_err(|e| err_internal(&format!("corrupt persistence value: {e}"))),
            None => Err(err_not_found("Key not found")),
        }
    }

    async fn put_persistence(&self, key: &str, value: serde_json::Value) -> Result<(), ApiError> {
        let now = Self::now_iso();
        let value_str = serde_json::to_string(&value)
            .map_err(|e| err_internal(&format!("serialize error: {e}")))?;

        sqlx::query(
            "INSERT INTO Persistence (key, value, createdAt, updatedAt) VALUES (?, ?, ?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updatedAt = excluded.updatedAt"
        )
        .bind(key).bind(&value_str).bind(&now).bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("upsert error: {e}")))?;

        Ok(())
    }

    async fn delete_persistence(&self, key: &str) -> Result<(), ApiError> {
        sqlx::query("DELETE FROM Persistence WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("delete error: {e}")))?;
        Ok(())
    }

    async fn reset(&self) -> Result<(), ApiError> {
        use sqlx::Executor;
        let tables = [
            "ExecutionRecord",
            "PlanApprover",
            "PlanPolicyCheck",
            "PlanSection",
            "Plan",
            "Request",
            "ModuleDependency",
            "Opportunity",
            "PendingApproval",
            "Regression",
            "Indicator",
            "Module",
            "Repo",
            "Component",
            "Product",
            "RecentAction",
            "SavedView",
            "Operator",
            "Persistence",
        ];

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| err_internal(&format!("begin tx error: {e}")))?;

        for table in &tables {
            tx.execute(sqlx::query(&format!("DELETE FROM {table}")))
                .await
                .map_err(|e| err_internal(&format!("truncate {table} error: {e}")))?;
        }

        crate::fixtures::load_fixtures(&mut tx).await?;

        tx.commit()
            .await
            .map_err(|e| err_internal(&format!("commit error: {e}")))?;

        Ok(())
    }
}

impl SqliteBackendStore {
    async fn fetch_plan_item(&self, id: &str) -> Result<PlanItem, ApiError> {
        let row: Option<PlanRow> = sqlx::query_as::<_, PlanRow>(
            "SELECT id, title, componentId as component_id, repoId as repo_id, status, linkedRequestId as linked_request_id, confidence, risk, expectedValue as expected_value, agentGenerated as agent_generated, waitingSince as waiting_since, createdAt as created_at FROM Plan WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("Plan not found"))?;

        let component = match row.component_id.as_deref() {
            Some(cid) => sqlx::query_as::<_, NameRow>("SELECT name FROM Component WHERE id = ?")
                .bind(cid)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?
                .map(|c| c.name)
                .unwrap_or_default(),
            None => String::new(),
        };
        let repo = match row.repo_id.as_deref() {
            Some(rid) => sqlx::query_as::<_, NameRow>("SELECT name FROM Repo WHERE id = ?")
                .bind(rid)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?
                .map(|r| r.name)
                .unwrap_or_default(),
            None => String::new(),
        };
        let exec_ids: Vec<IdRow> = sqlx::query_as::<_, IdRow>(
            "SELECT id FROM ExecutionRecord WHERE planId = ? ORDER BY id ASC",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        Ok(PlanItem {
            id: row.id,
            title: row.title,
            component,
            repo,
            status: row.status,
            linked_request_id: row.linked_request_id,
            execution_ids: exec_ids.into_iter().map(|e| e.id).collect(),
            confidence: row.confidence,
            risk: row.risk,
            expected_value: row.expected_value,
            agent_generated: row.agent_generated != 0,
            waiting_since: row.waiting_since,
            created_at: row.created_at,
        })
    }

    async fn check_cycle(&self, from: &str, to: &str) -> Result<bool, ApiError> {
        let all_deps: Vec<DepEdge> = sqlx::query_as::<_, DepEdge>(
            "SELECT fromModuleId as from_id, toModuleId as to_id FROM ModuleDependency",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for d in &all_deps {
            adj.entry(d.from_id.clone())
                .or_default()
                .push(d.to_id.clone());
        }
        adj.entry(from.to_string())
            .or_default()
            .push(to.to_string());

        const MAX_VISITED: usize = 10_000;
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(to.to_string());

        while let Some(node) = queue.pop_front() {
            if node == from {
                return Ok(true);
            }
            if !visited.insert(node.clone()) {
                continue;
            }
            if visited.len() >= MAX_VISITED {
                return Err(err_internal(
                    "Module dependency graph traversal limit exceeded",
                ));
            }
            if let Some(neighbors) = adj.get(&node) {
                for n in neighbors {
                    queue.push_back(n.clone());
                }
            }
        }
        Ok(false)
    }
}

async fn get_repo_name(pool: &SqlitePool, repo_id: &str) -> Result<String, ApiError> {
    let r: Option<NameRow> = sqlx::query_as::<_, NameRow>("SELECT name FROM Repo WHERE id = ?")
        .bind(repo_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;
    Ok(r.map(|r| r.name).unwrap_or_default())
}

async fn get_component_name_for_repo(pool: &SqlitePool, repo_id: &str) -> Result<String, ApiError> {
    let comp: Option<ComponentIdRow> = sqlx::query_as::<_, ComponentIdRow>(
        "SELECT componentId as component_id FROM Repo WHERE id = ?",
    )
    .bind(repo_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| err_internal(&format!("query error: {e}")))?;

    if let Some(cid) = comp.and_then(|c| c.component_id) {
        let cn: Option<NameRow> =
            sqlx::query_as::<_, NameRow>("SELECT name FROM Component WHERE id = ?")
                .bind(cid)
                .fetch_optional(pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        Ok(cn.map(|c| c.name).unwrap_or_default())
    } else {
        Ok(String::new())
    }
}

async fn compute_repo_depends_on(
    pool: &SqlitePool,
    repo_name: &str,
) -> Result<Vec<String>, ApiError> {
    let rows: Vec<RepoDepTargetRow> = sqlx::query_as::<_, RepoDepTargetRow>(
        "SELECT DISTINCT r2.name as target_repo FROM ModuleDependency md
         JOIN Module fm ON fm.id = md.fromModuleId
         JOIN Repo r1 ON r1.id = fm.repoId
         JOIN Module tm ON tm.id = md.toModuleId
         JOIN Repo r2 ON r2.id = tm.repoId
         WHERE r1.name = ? AND r2.name != ?",
    )
    .bind(repo_name)
    .bind(repo_name)
    .fetch_all(pool)
    .await
    .map_err(|e| err_internal(&format!("query error: {e}")))?;

    Ok(rows.into_iter().map(|r| r.target_repo).collect())
}

async fn compute_repo_depended_on_by(
    pool: &SqlitePool,
    repo_name: &str,
) -> Result<Vec<String>, ApiError> {
    let rows: Vec<RepoDepTargetRow> = sqlx::query_as::<_, RepoDepTargetRow>(
        "SELECT DISTINCT r1.name as target_repo FROM ModuleDependency md
         JOIN Module fm ON fm.id = md.fromModuleId
         JOIN Repo r1 ON r1.id = fm.repoId
         JOIN Module tm ON tm.id = md.toModuleId
         JOIN Repo r2 ON r2.id = tm.repoId
         WHERE r2.name = ? AND r1.name != ?",
    )
    .bind(repo_name)
    .bind(repo_name)
    .fetch_all(pool)
    .await
    .map_err(|e| err_internal(&format!("query error: {e}")))?;

    Ok(rows.into_iter().map(|r| r.target_repo).collect())
}
