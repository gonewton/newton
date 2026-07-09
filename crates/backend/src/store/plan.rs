use super::helpers::{query_err, tx_err};

const PLAN_STATUSES: &[&str] = &[
    "draft",
    "ready",
    "running",
    "complete",
    "failed",
    "abandoned",
];

fn validate_plan_status(status: &str) -> Result<(), newton_types::ApiError> {
    if PLAN_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(crate::err_validation(&format!(
            "invalid Plan status '{}'; must be one of: {}",
            status,
            PLAN_STATUSES.join(", ")
        )))
    }
}

pub(super) const PLAN_SELECT: &str =
    "SELECT p.id, p.title, p.componentId as component_id, c.name as component_name, \
     p.repoId as repo_id, r.name as repo_name, p.status, p.linkedChangeRequestId as linked_change_request_id, \
     p.confidence, p.risk, p.expectedValue as expected_value, p.agentGenerated as agent_generated, \
     p.waitingSince as waiting_since, p.body, p.executionId as execution_id, \
     COALESCE(p.attempts, 0) as attempts, p.lastError as last_error, p.module, p.createdAt as created_at \
     FROM Plan p LEFT JOIN Component c ON p.componentId = c.id LEFT JOIN Repo r ON p.repoId = r.id";
use super::rows::*;
use crate::err_conflict;
use crate::err_internal;
use crate::err_not_found;
use newton_types::ApiError;
use newton_types::*;
use uuid::Uuid;

impl super::SqliteBackendStore {
    pub(super) async fn list_pending_approvals_db(
        &self,
    ) -> Result<Vec<PendingApprovalItem>, ApiError> {
        let rows = sqlx::query_as::<_, PendingApprovalRow>(
            "SELECT pa.id, pa.title, pa.type as item_type, pa.componentId as component_id, c.name as component_name, pa.repoName as repo_name, pa.risk, pa.expectedValue as expected_value, pa.waitingSince as waiting_since, pa.reviewer, pa.status, pa.confidence, pa.agentGenerated as agent_generated FROM PendingApproval pa LEFT JOIN Component c ON pa.componentId = c.id ORDER BY pa.id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(query_err)?;

        Ok(rows
            .into_iter()
            .map(|row| PendingApprovalItem {
                id: row.id,
                title: row.title,
                item_type: row.item_type,
                product: row.component_name.unwrap_or_default(),
                repo: row.repo_name.unwrap_or_default(),
                risk: row.risk,
                expected_value: row.expected_value,
                waiting_since: row.waiting_since,
                reviewer: row.reviewer,
                status: row.status,
                confidence: row.confidence,
                agent_generated: row.agent_generated,
            })
            .collect())
    }

    pub(super) async fn list_regressions_db(&self) -> Result<Vec<RegressionItem>, ApiError> {
        let rows = sqlx::query_as::<_, RegressionRow>(
            "SELECT repoName as repo, kpiId as kpi_id, delta, severity, since, trend FROM Regression ORDER BY id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(query_err)?;

        Ok(rows
            .into_iter()
            .map(|r| RegressionItem {
                repo: r.repo,
                kpi_id: r.kpi_id,
                delta: r.delta,
                severity: r.severity,
                since: r.since,
                trend: r.trend,
            })
            .collect())
    }

    pub(super) async fn list_recent_actions_db(
        &self,
        limit: u32,
    ) -> Result<Vec<RecentActionItem>, ApiError> {
        let rows = sqlx::query_as::<_, RecentActionRow>(
            "SELECT time, action, subject, type as item_type FROM RecentAction ORDER BY createdAt DESC LIMIT ?"
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(query_err)?;

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

    pub(super) async fn list_saved_views_db(
        &self,
        kind: Option<String>,
    ) -> Result<serde_json::Value, ApiError> {
        if let Some(ref k) = kind {
            let rows = sqlx::query_as::<_, SavedViewRow>(
                "SELECT id, label, filters, sort, sortDir as sort_dir FROM SavedView WHERE kind = ? ORDER BY id ASC"
            )
            .bind(k)
            .fetch_all(&self.pool)
            .await
            .map_err(query_err)?;

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
            .map_err(query_err)?;

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

    pub(super) fn plan_row_to_item(row: PlanRow, exec_ids: Vec<String>) -> PlanItem {
        PlanItem {
            id: row.id,
            title: row.title,
            component: row.component_name.unwrap_or_default(),
            repo: row.repo_name.unwrap_or_default(),
            status: row.status,
            linked_change_request_id: row.linked_change_request_id,
            execution_ids: exec_ids,
            confidence: row.confidence,
            risk: row.risk,
            expected_value: row.expected_value,
            agent_generated: row.agent_generated,
            waiting_since: row.waiting_since,
            body: row.body,
            execution_id: row.execution_id,
            attempts: row.attempts,
            last_error: row.last_error,
            module: row.module,
            created_at: row.created_at,
        }
    }

    pub(super) async fn fetch_plan_item(&self, id: &str) -> Result<PlanItem, ApiError> {
        let row: Option<PlanRow> =
            sqlx::query_as::<_, PlanRow>(&format!("{PLAN_SELECT} WHERE p.id = ?"))
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(query_err)?;
        let row = row.ok_or_else(|| err_not_found("Plan not found"))?;
        let exec_ids: Vec<IdRow> = sqlx::query_as::<_, IdRow>(
            "SELECT id FROM ExecutionRecord WHERE planId = ? ORDER BY id ASC",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(query_err)?;
        Ok(Self::plan_row_to_item(
            row,
            exec_ids.into_iter().map(|e| e.id).collect(),
        ))
    }

    pub(super) async fn list_plans_db(
        &self,
        status: Option<String>,
        scope: Option<String>,
        scope_id: Option<String>,
    ) -> Result<Vec<PlanItem>, ApiError> {
        let mut conditions: Vec<String> = Vec::new();
        if status.is_some() {
            conditions.push("p.status = ?".to_string());
        }
        if scope_id.is_some() {
            let col = match scope.as_deref() {
                Some("component") => "p.componentId",
                _ => "p.repoId",
            };
            conditions.push(format!("{col} = ?"));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        let sql = format!("{PLAN_SELECT}{where_clause} ORDER BY p.createdAt DESC");
        let mut q = sqlx::query_as::<_, PlanRow>(&sql);
        if let Some(ref s) = status {
            q = q.bind(s);
        }
        if let Some(ref sid) = scope_id {
            q = q.bind(sid);
        }
        let rows = q.fetch_all(&self.pool).await.map_err(query_err)?;

        let mut result = Vec::new();
        for row in rows {
            let exec_ids: Vec<IdRow> = sqlx::query_as::<_, IdRow>(
                "SELECT id FROM ExecutionRecord WHERE planId = ? ORDER BY id ASC",
            )
            .bind(&row.id)
            .fetch_all(&self.pool)
            .await
            .map_err(query_err)?;
            result.push(Self::plan_row_to_item(
                row,
                exec_ids.into_iter().map(|e| e.id).collect(),
            ));
        }
        Ok(result)
    }

    pub(super) async fn create_plan_db(&self, body: CreatePlanBody) -> Result<PlanItem, ApiError> {
        let now = Self::now_iso();
        let status = if body.status.is_empty() {
            "draft".to_string()
        } else {
            body.status.clone()
        };
        validate_plan_status(&status)?;
        sqlx::query(
            "INSERT INTO Plan (
                id, title, linkedChangeRequestId, body, status,
                componentId, repoId, module, confidence, risk,
                expectedValue, expectedDelta,
                agentGenerated, createdAt, updatedAt
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        )
        .bind(&body.id)
        .bind(&body.title)
        .bind(&body.linked_change_request_id)
        .bind(&body.body)
        .bind(&status)
        .bind(&body.component_id)
        .bind(&body.repo_id)
        .bind(&body.module)
        .bind(body.confidence)
        .bind(&body.risk)
        .bind(&body.expected_value)
        .bind(&body.expected_delta)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                err_conflict("Plan id already exists")
            } else {
                err_internal(&format!("insert Plan error: {e}"))
            }
        })?;

        self.fetch_plan_item(&body.id).await
    }

    pub(super) async fn patch_plan_db(
        &self,
        id: &str,
        body: PatchPlanBody,
    ) -> Result<PlanItem, ApiError> {
        if !self.row_exists("Plan", id).await? {
            return Err(err_not_found("Plan not found"));
        }
        let now = Self::now_iso();
        if let Some(ref s) = body.status {
            validate_plan_status(s)?;
            sqlx::query("UPDATE Plan SET status = ?, updatedAt = ? WHERE id = ?")
                .bind(s)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update Plan status: {e}")))?;
        }
        if let Some(ref eid) = body.execution_id {
            sqlx::query("UPDATE Plan SET executionId = ?, updatedAt = ? WHERE id = ?")
                .bind(eid)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update Plan executionId: {e}")))?;
        }
        if let Some(attempts) = body.attempts {
            sqlx::query("UPDATE Plan SET attempts = ?, updatedAt = ? WHERE id = ?")
                .bind(attempts)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update Plan attempts: {e}")))?;
        }
        if let Some(ref le) = body.last_error {
            sqlx::query("UPDATE Plan SET lastError = ?, updatedAt = ? WHERE id = ?")
                .bind(le)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update Plan lastError: {e}")))?;
        }
        if let Some(ref b) = body.body {
            sqlx::query("UPDATE Plan SET body = ?, updatedAt = ? WHERE id = ?")
                .bind(b)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update Plan body: {e}")))?;
        }
        self.fetch_plan_item(id).await
    }

    pub(super) async fn get_plan_db(&self, id: &str) -> Result<PlanDetail, ApiError> {
        let plan = self.fetch_plan_item(id).await?;

        let delta: Option<ExpectedDeltaRow> = sqlx::query_as::<_, ExpectedDeltaRow>(
            "SELECT expectedDelta as expected_delta FROM Plan WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(query_err)?;

        let sections = sqlx::query_as::<_, PlanSectionRow>(
            "SELECT id, label, content FROM PlanSection WHERE planId = ? ORDER BY sortOrder ASC",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(query_err)?;

        let policy_checks = sqlx::query_as::<_, PlanPolicyCheckRow>(
            "SELECT rule, status, met FROM PlanPolicyCheck WHERE planId = ?",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(query_err)?;

        let approvers = sqlx::query_as::<_, PlanApproverRow>(
            "SELECT role, name, approverStatus as status FROM PlanApprover WHERE planId = ?",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(query_err)?;

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

    pub(super) async fn approve_plan_db(&self, id: &str) -> Result<ApprovedPlan, ApiError> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(tx_err)?;

        let plan: Option<PlanRow> =
            sqlx::query_as::<_, PlanRow>(&format!("{PLAN_SELECT} WHERE p.id = ?"))
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(query_err)?;

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

        tx.commit().await.map_err(tx_err)?;

        let plan_item = self.fetch_plan_item(id).await?;
        Ok(ApprovedPlan {
            plan: plan_item,
            execution_id: exec_id,
            created_at: now,
        })
    }

    pub(super) async fn reject_plan_db(&self, id: &str) -> Result<PlanItem, ApiError> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(tx_err)?;

        let plan: Option<PlanRow> =
            sqlx::query_as::<_, PlanRow>(&format!("{PLAN_SELECT} WHERE p.id = ?"))
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(query_err)?;

        let plan = plan.ok_or_else(|| err_not_found("Plan not found"))?;

        if plan.status != "awaiting_approval" && plan.status != "needs_revision" {
            return Err(err_conflict("Plan is not in a rejectable state"));
        }

        let now = Self::now_iso();
        sqlx::query("UPDATE Plan SET status = 'rejected', updatedAt = ? WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| err_internal(&format!("update error: {e}")))?;

        tx.commit().await.map_err(tx_err)?;

        self.fetch_plan_item(id).await
    }

    pub(super) async fn list_executions_db(
        &self,
        plan_id: Option<String>,
    ) -> Result<Vec<ExecutionItem>, ApiError> {
        let base_sql = "SELECT e.id, e.instanceId as instance_id, e.planId as plan_id, e.workflowId as workflow_id, e.planTitle as plan_title, e.repoId as repo_id, r.name as repo_name, e.componentId as component_id, c.name as component_name, e.stage, e.status, e.policyLevel as policy_level, e.startedBy as started_by, e.waitingOn as waiting_on, e.testResult as test_result, e.prStatus as pr_status, e.prLink as pr_link, e.deployStatus as deploy_status, e.createdAt as created_at, e.startedAt as started FROM ExecutionRecord e LEFT JOIN Repo r ON e.repoId = r.id LEFT JOIN Component c ON e.componentId = c.id";

        let rows = if let Some(ref pid) = plan_id {
            sqlx::query_as::<_, ExecutionRow>(&format!(
                "{base_sql} WHERE e.planId = ? ORDER BY e.id ASC"
            ))
            .bind(pid)
            .fetch_all(&self.pool)
            .await
            .map_err(query_err)?
        } else {
            sqlx::query_as::<_, ExecutionRow>(&format!("{base_sql} ORDER BY e.id ASC"))
                .fetch_all(&self.pool)
                .await
                .map_err(query_err)?
        };

        Ok(rows
            .into_iter()
            .map(|r| ExecutionItem {
                instance_id: r.instance_id.unwrap_or_else(|| r.id.clone()),
                plan_id: r.plan_id.clone(),
                linked_plan_id: r.plan_id,
                workflow_id: r.workflow_id,
                plan_title: r.plan_title,
                repo: r.repo_name,
                component: r.component_name,
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
            })
            .collect())
    }

    pub(super) async fn list_operators_db(&self) -> Result<Vec<OperatorItem>, ApiError> {
        let rows = sqlx::query_as::<_, OperatorRow>(
            "SELECT operatorType as operator_type, description, paramsSchema as params_schema, paletteLabel as palette_label, paletteIcon as palette_icon FROM Operator ORDER BY id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(query_err)?;

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

    pub(super) async fn get_persistence_db(
        &self,
        key: &str,
    ) -> Result<serde_json::Value, ApiError> {
        let row: Option<StringValueRow> =
            sqlx::query_as::<_, StringValueRow>("SELECT value FROM Persistence WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(query_err)?;

        match row.and_then(|r| r.value) {
            Some(v) => serde_json::from_str(&v)
                .map_err(|e| err_internal(&format!("corrupt persistence value: {e}"))),
            None => Err(err_not_found("Key not found")),
        }
    }

    pub(super) async fn put_persistence_db(
        &self,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), ApiError> {
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

    pub(super) async fn delete_persistence_db(&self, key: &str) -> Result<(), ApiError> {
        sqlx::query("DELETE FROM Persistence WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("delete error: {e}")))?;
        Ok(())
    }

    pub(super) async fn reset_db(&self) -> Result<(), ApiError> {
        use sqlx::Executor;
        let tables = [
            "ExecutionRecord",
            "PlanApprover",
            "PlanPolicyCheck",
            "PlanSection",
            "Plan",
            "ChangeRequest",
            "Finding",
            "ModuleDependency",
            "PendingApproval",
            "Regression",
            "Grade",
            "EvalRun",
            "KPI",
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
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(tx_err)?;

        for table in &tables {
            tx.execute(sqlx::query(&format!("DELETE FROM {table}")))
                .await
                .map_err(|e| err_internal(&format!("truncate {table} error: {e}")))?;
        }

        crate::fixtures::load_fixtures(&mut tx).await?;

        tx.commit().await.map_err(tx_err)?;

        Ok(())
    }
}
