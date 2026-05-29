use super::rows::*;
use crate::err_conflict;
use crate::err_internal;
use crate::err_not_found;
use crate::err_validation;
use crate::models::*;
use newton_types::ApiError;
use uuid::Uuid;

impl super::SqliteBackendStore {
    pub(super) async fn list_pending_approvals_impl(
        &self,
    ) -> Result<Vec<PendingApprovalItem>, ApiError> {
        let rows = sqlx::query_as::<_, PendingApprovalRow>(
            "SELECT pa.id, pa.title, pa.type as item_type, pa.componentId as component_id, c.name as component_name, pa.repoName as repo_name, pa.risk, pa.expectedValue as expected_value, pa.waitingSince as waiting_since, pa.reviewer, pa.status, pa.confidence, pa.agentGenerated as agent_generated FROM PendingApproval pa LEFT JOIN Component c ON pa.componentId = c.id ORDER BY pa.id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

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
                agent_generated: row.agent_generated != 0,
            })
            .collect())
    }

    pub(super) async fn list_regressions_impl(&self) -> Result<Vec<RegressionItem>, ApiError> {
        let rows = sqlx::query_as::<_, RegressionRow>(
            "SELECT repoName as repo, kpiId as kpi_id, delta, severity, since, trend FROM Regression ORDER BY id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

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

    pub(super) async fn list_recent_actions_impl(
        &self,
        limit: u32,
    ) -> Result<Vec<RecentActionItem>, ApiError> {
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

    pub(super) async fn list_saved_views_impl(
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

    pub(super) async fn list_opportunities_impl(
        &self,
        status: Option<String>,
    ) -> Result<Vec<OpportunityItem>, ApiError> {
        let base_sql = "SELECT o.id, o.title, o.origin, o.componentId as component_id, c.name as component_name, o.module, o.repoId as repo_id, r.name as repo_name, o.kpiId as kpi_id, o.confidence, o.risk, o.expectedValue as expected_value, o.effort, o.status, o.age, o.rationale, o.dependsOn as depends_on, o.blocks FROM Opportunity o LEFT JOIN Component c ON o.componentId = c.id LEFT JOIN Repo r ON o.repoId = r.id";

        let rows = if let Some(ref s) = status {
            sqlx::query_as::<_, OpportunityRow>(&format!(
                "{base_sql} WHERE o.status = ? ORDER BY o.id ASC"
            ))
            .bind(s)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("query error: {e}")))?
        } else {
            sqlx::query_as::<_, OpportunityRow>(&format!("{base_sql} ORDER BY o.id ASC"))
                .fetch_all(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?
        };

        Ok(rows
            .into_iter()
            .map(|row| {
                let depends_on: Vec<String> = row
                    .depends_on
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();
                let blocks: Vec<String> = row
                    .blocks
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();

                OpportunityItem {
                    id: row.id,
                    title: row.title,
                    origin: row.origin,
                    component: row.component_name.unwrap_or_default(),
                    module: row.module,
                    repo: row.repo_name.unwrap_or_default(),
                    kpi_id: row.kpi_id,
                    confidence: row.confidence,
                    risk: row.risk,
                    expected_value: row.expected_value,
                    effort: row.effort,
                    status: row.status,
                    age: row.age,
                    rationale: row.rationale,
                    depends_on,
                    blocks,
                }
            })
            .collect())
    }

    pub(super) async fn patch_opportunity_impl(
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

        self.list_opportunities_impl(None)
            .await?
            .into_iter()
            .find(|o| o.id == id)
            .ok_or_else(|| err_internal("Failed to read back updated opportunity"))
    }

    pub(super) async fn create_opportunity_impl(
        &self,
        body: CreateOpportunityBody,
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
        if let Some(c) = body.confidence {
            if !(0.0..=1.0).contains(&c) {
                return Err(err_validation("confidence must be between 0.0 and 1.0"));
            }
        }
        if body.expected_value < 0.0 {
            return Err(err_validation("expectedValue must be non-negative"));
        }
        if body.id.trim().is_empty() {
            return Err(err_validation("id must not be empty"));
        }
        if body.title.trim().is_empty() {
            return Err(err_validation("title must not be empty"));
        }
        if body.origin.trim().is_empty() {
            return Err(err_validation("origin must not be empty"));
        }

        let component_id = if let Some(ref cid) = body.component {
            let count: Option<CountRow> = sqlx::query_as::<_, CountRow>(
                "SELECT COUNT(*) as count FROM Component WHERE id = ?",
            )
            .bind(cid)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("query error: {e}")))?;
            if count.map(|c| c.count).unwrap_or(0) == 0 {
                tracing::warn!("component '{}' not found, proceeding", cid);
                None
            } else {
                Some(cid.clone())
            }
        } else {
            None
        };
        let repo_id = if let Some(ref rid) = body.repo {
            let count: Option<CountRow> =
                sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Repo WHERE id = ?")
                    .bind(rid)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| err_internal(&format!("query error: {e}")))?;
            if count.map(|c| c.count).unwrap_or(0) == 0 {
                tracing::warn!("repo '{}' not found, proceeding", rid);
                None
            } else {
                Some(rid.clone())
            }
        } else {
            None
        };

        let now = Self::now_iso();
        let depends_on_json =
            serde_json::to_string(&body.depends_on).unwrap_or_else(|_| "[]".to_string());
        let blocks_json = serde_json::to_string(&body.blocks).unwrap_or_else(|_| "[]".to_string());

        sqlx::query(
            "INSERT INTO Opportunity (
                id, title, origin, componentId, module, repoId,
                kpiId, confidence, risk, expectedValue, effort,
                status, rationale, dependsOn, blocks, createdAt, updatedAt
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                title       = excluded.title,
                origin      = excluded.origin,
                componentId = excluded.componentId,
                module      = excluded.module,
                repoId      = excluded.repoId,
                kpiId       = excluded.kpiId,
                confidence  = excluded.confidence,
                risk        = excluded.risk,
                expectedValue = excluded.expectedValue,
                effort      = excluded.effort,
                status      = excluded.status,
                rationale   = excluded.rationale,
                dependsOn   = excluded.dependsOn,
                blocks      = excluded.blocks,
                updatedAt   = excluded.updatedAt",
        )
        .bind(&body.id)
        .bind(&body.title)
        .bind(&body.origin)
        .bind(&component_id)
        .bind(&body.module)
        .bind(&repo_id)
        .bind(&body.kpi_id)
        .bind(body.confidence)
        .bind(&body.risk)
        .bind(body.expected_value)
        .bind(&body.effort)
        .bind(&body.status)
        .bind(&body.rationale)
        .bind(&depends_on_json)
        .bind(&blocks_json)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("upsert error: {e}")))?;

        self.list_opportunities_impl(None)
            .await?
            .into_iter()
            .find(|o| o.id == body.id)
            .ok_or_else(|| err_internal("Failed to read back created opportunity"))
    }

    pub(super) async fn list_requests_impl(&self) -> Result<Vec<RequestItem>, ApiError> {
        let rows = sqlx::query_as::<_, RequestRow>(
            "SELECT req.id, req.title, req.description, req.componentId as component_id, c.name as component_name, req.repoId as repo_id, r.name as repo_name, req.requestedBy as requested_by, req.status, req.linkedOpportunityId as linked_opportunity_id, req.createdAt as created_at FROM Request req LEFT JOIN Component c ON req.componentId = c.id LEFT JOIN Repo r ON req.repoId = r.id ORDER BY req.id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|row| RequestItem {
                id: row.id,
                title: row.title,
                description: row.description,
                component: row.component_name.unwrap_or_default(),
                repo: row.repo_name.unwrap_or_default(),
                requested_by: row.requested_by,
                status: row.status,
                linked_opportunity_id: row.linked_opportunity_id,
                created_at: row.created_at,
            })
            .collect())
    }

    pub(super) async fn create_request_impl(
        &self,
        body: CreateRequestBody,
    ) -> Result<RequestItem, ApiError> {
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

        self.list_requests_impl()
            .await?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| err_internal("Failed to read back created request"))
    }

    pub(super) async fn list_plans_impl(&self) -> Result<Vec<PlanItem>, ApiError> {
        let rows = sqlx::query_as::<_, PlanRow>(
            "SELECT p.id, p.title, p.componentId as component_id, c.name as component_name, p.repoId as repo_id, r.name as repo_name, p.status, p.linkedRequestId as linked_request_id, p.confidence, p.risk, p.expectedValue as expected_value, p.agentGenerated as agent_generated, p.waitingSince as waiting_since, p.createdAt as created_at FROM Plan p LEFT JOIN Component c ON p.componentId = c.id LEFT JOIN Repo r ON p.repoId = r.id ORDER BY p.id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
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
                component: row.component_name.unwrap_or_default(),
                repo: row.repo_name.unwrap_or_default(),
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

    pub(super) async fn get_plan_impl(&self, id: &str) -> Result<PlanDetail, ApiError> {
        let plan = self
            .list_plans_impl()
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

    pub(super) async fn approve_plan_impl(&self, id: &str) -> Result<ApprovedPlan, ApiError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| err_internal(&format!("begin tx error: {e}")))?;

        let plan: Option<PlanRow> = sqlx::query_as::<_, PlanRow>(
            "SELECT p.id, p.title, p.componentId as component_id, c.name as component_name, p.repoId as repo_id, r.name as repo_name, p.status, p.linkedRequestId as linked_request_id, p.confidence, p.risk, p.expectedValue as expected_value, p.agentGenerated as agent_generated, p.waitingSince as waiting_since, p.createdAt as created_at FROM Plan p LEFT JOIN Component c ON p.componentId = c.id LEFT JOIN Repo r ON p.repoId = r.id WHERE p.id = ?"
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

    pub(super) async fn reject_plan_impl(&self, id: &str) -> Result<PlanItem, ApiError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| err_internal(&format!("begin tx error: {e}")))?;

        let plan: Option<PlanRow> = sqlx::query_as::<_, PlanRow>(
            "SELECT p.id, p.title, p.componentId as component_id, c.name as component_name, p.repoId as repo_id, r.name as repo_name, p.status, p.linkedRequestId as linked_request_id, p.confidence, p.risk, p.expectedValue as expected_value, p.agentGenerated as agent_generated, p.waitingSince as waiting_since, p.createdAt as created_at FROM Plan p LEFT JOIN Component c ON p.componentId = c.id LEFT JOIN Repo r ON p.repoId = r.id WHERE p.id = ?"
        )
        .bind(id)
        .fetch_optional(&mut *tx)
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
            .execute(&mut *tx)
            .await
            .map_err(|e| err_internal(&format!("update error: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| err_internal(&format!("commit error: {e}")))?;

        self.fetch_plan_item(id).await
    }

    pub(super) async fn list_executions_impl(
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
            .map_err(|e| err_internal(&format!("query error: {e}")))?
        } else {
            sqlx::query_as::<_, ExecutionRow>(&format!("{base_sql} ORDER BY e.id ASC"))
                .fetch_all(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?
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

    pub(super) async fn list_operators_impl(&self) -> Result<Vec<OperatorItem>, ApiError> {
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

    pub(super) async fn get_persistence_impl(
        &self,
        key: &str,
    ) -> Result<serde_json::Value, ApiError> {
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

    pub(super) async fn put_persistence_impl(
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

    pub(super) async fn delete_persistence_impl(&self, key: &str) -> Result<(), ApiError> {
        sqlx::query("DELETE FROM Persistence WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("delete error: {e}")))?;
        Ok(())
    }

    pub(super) async fn reset_impl(&self) -> Result<(), ApiError> {
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
