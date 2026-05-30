use super::rows::*;
use crate::err_conflict;
use crate::err_internal;
use crate::err_not_found;
use crate::err_validation;
use crate::models::*;
use newton_types::ApiError;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

impl super::SqliteBackendStore {
    pub(super) async fn fetch_product_item(&self, id: &str) -> Result<ProductItem, ApiError> {
        let row: Option<ProductRow> = sqlx::query_as::<_, ProductRow>(
            "SELECT p.id, p.name, COUNT(c.id) as component_count FROM Product p LEFT JOIN Component c ON c.productId = p.id WHERE p.id = ? GROUP BY p.id"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        row.map(|r| ProductItem {
            id: r.id,
            name: r.name,
            component_count: r.component_count,
        })
        .ok_or_else(|| err_not_found("Product not found"))
    }

    pub(super) async fn fetch_component_item(&self, id: &str) -> Result<ComponentItem, ApiError> {
        let row: Option<ComponentRow> = sqlx::query_as::<_, ComponentRow>(
            "SELECT c.id, c.name, c.domain, c.repos, c.modules, c.trend, c.owner, c.criticality, c.autonomy, c.openPlans as open_plans, c.openRequests as open_requests, c.lastEval as last_eval, c.productId as product_id, p.name as product_name FROM Component c LEFT JOIN Product p ON c.productId = p.id WHERE c.id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("Component not found"))?;
        Ok(ComponentItem {
            id: row.id,
            name: row.name,
            product_id: row.product_id,
            product_name: row.product_name.unwrap_or_default(),
            domain: row.domain,
            repos: row.repos,
            modules: row.modules,
            trend: row.trend,
            owner: row.owner,
            criticality: row.criticality,
            autonomy: row.autonomy,
            open_plans: row.open_plans,
            open_requests: row.open_requests,
            last_eval: row.last_eval,
        })
    }

    pub(super) async fn fetch_repo_item(&self, id: &str) -> Result<RepoItem, ApiError> {
        let row: Option<RepoRow> = sqlx::query_as::<_, RepoRow>(
            "SELECT r.id, r.name, r.componentId as component_id, c.name as component_name, r.owner, r.criticality, r.autonomy, r.regressions, r.openPlans as open_plans, r.execStatus as exec_status, r.lastEval as last_eval FROM Repo r LEFT JOIN Component c ON r.componentId = c.id WHERE r.id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("Repo not found"))?;
        let depends_on = compute_repo_depends_on(&self.pool, &row.name).await?;
        let depended_on_by = compute_repo_depended_on_by(&self.pool, &row.name).await?;
        Ok(RepoItem {
            id: row.id,
            name: row.name,
            component: row.component_name.unwrap_or_default(),
            owner: row.owner,
            criticality: row.criticality,
            autonomy: row.autonomy,
            regressions: row.regressions,
            open_plans: row.open_plans,
            exec_status: row.exec_status,
            last_eval: row.last_eval,
            depends_on,
            depended_on_by,
        })
    }

    pub(super) async fn fetch_module_item(&self, id: &str) -> Result<ModuleItem, ApiError> {
        let row: Option<ModuleRow> = sqlx::query_as::<_, ModuleRow>(
            "SELECT m.id, m.name, m.kind, m.repoId as repo_id, r.name as repo_name, r.componentId as component_id, c.name as component_name FROM Module m LEFT JOIN Repo r ON m.repoId = r.id LEFT JOIN Component c ON r.componentId = c.id WHERE m.id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("Module not found"))?;
        Ok(ModuleItem {
            id: row.id,
            name: row.name,
            kind: row.kind,
            repo_id: row.repo_id,
            repo_name: row.repo_name.unwrap_or_default(),
            component_id: row.component_id.unwrap_or_default(),
            component_name: row.component_name.unwrap_or_default(),
        })
    }

    pub(super) async fn fetch_module_dependency_item(
        &self,
        id: &str,
    ) -> Result<ModuleDependencyItem, ApiError> {
        self.list_module_dependencies()
            .await?
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| err_not_found("ModuleDependency not found"))
    }

    pub(super) async fn check_cycle(&self, from: &str, to: &str) -> Result<bool, ApiError> {
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

    pub(super) async fn fetch_plan_item(&self, id: &str) -> Result<PlanItem, ApiError> {
        let row: Option<PlanRow> = sqlx::query_as::<_, PlanRow>(
            "SELECT p.id, p.title, p.componentId as component_id, c.name as component_name, p.repoId as repo_id, r.name as repo_name, p.status, p.linkedRequestId as linked_request_id, p.confidence, p.risk, p.expectedValue as expected_value, p.agentGenerated as agent_generated, p.waitingSince as waiting_since, p.createdAt as created_at FROM Plan p LEFT JOIN Component c ON p.componentId = c.id LEFT JOIN Repo r ON p.repoId = r.id WHERE p.id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("Plan not found"))?;

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
        })
    }
}

impl super::SqliteBackendStore {
    pub(super) async fn list_products(&self) -> Result<Vec<ProductItem>, ApiError> {
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

    pub(super) async fn create_product(
        &self,
        body: CreateProductBody,
    ) -> Result<ProductItem, ApiError> {
        let id = Uuid::new_v4().to_string();
        let now = Self::now_iso();
        sqlx::query("INSERT INTO Product (id, name, createdAt, updatedAt) VALUES (?, ?, ?, ?)")
            .bind(&id)
            .bind(&body.name)
            .bind(&now)
            .bind(&now)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                if e.to_string().contains("UNIQUE constraint failed") {
                    err_conflict("name already exists")
                } else {
                    err_internal(&format!("insert error: {e}"))
                }
            })?;
        self.fetch_product_item(&id).await
    }

    pub(super) async fn put_product(
        &self,
        id: &str,
        body: PutProductBody,
    ) -> Result<ProductItem, ApiError> {
        let now = Self::now_iso();
        let affected = sqlx::query("UPDATE Product SET name = ?, updatedAt = ? WHERE id = ?")
            .bind(&body.name)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                if e.to_string().contains("UNIQUE constraint failed") {
                    err_conflict("name already exists")
                } else {
                    err_internal(&format!("update error: {e}"))
                }
            })?;
        if affected.rows_affected() == 0 {
            return Err(err_not_found("Product not found"));
        }
        self.fetch_product_item(id).await
    }

    pub(super) async fn patch_product(
        &self,
        id: &str,
        body: PatchProductBody,
    ) -> Result<ProductItem, ApiError> {
        let existing = self.fetch_product_item(id).await?;
        let name = body.name.unwrap_or(existing.name);
        let now = Self::now_iso();
        sqlx::query("UPDATE Product SET name = ?, updatedAt = ? WHERE id = ?")
            .bind(&name)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                if e.to_string().contains("UNIQUE constraint failed") {
                    err_conflict("name already exists")
                } else {
                    err_internal(&format!("update error: {e}"))
                }
            })?;
        self.fetch_product_item(id).await
    }

    pub(super) async fn delete_product(&self, id: &str) -> Result<String, ApiError> {
        let count: Option<CountRow> = sqlx::query_as::<_, CountRow>(
            "SELECT COUNT(*) as count FROM Component WHERE productId = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if count.map(|c| c.count).unwrap_or(0) > 0 {
            return Err(err_conflict(
                "cannot delete product: it has dependent components; remove them first",
            ));
        }
        let affected = sqlx::query("DELETE FROM Product WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("delete error: {e}")))?;
        if affected.rows_affected() == 0 {
            return Err(err_not_found("Product not found"));
        }
        Ok(id.to_string())
    }

    pub(super) async fn list_components(&self) -> Result<Vec<ComponentItem>, ApiError> {
        let rows = sqlx::query_as::<_, ComponentRow>(
            "SELECT c.id, c.name, c.domain, c.repos, c.modules, c.trend, c.owner, c.criticality, c.autonomy, c.openPlans as open_plans, c.openRequests as open_requests, c.lastEval as last_eval, c.productId as product_id, p.name as product_name FROM Component c LEFT JOIN Product p ON c.productId = p.id ORDER BY c.id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| ComponentItem {
                id: r.id,
                name: r.name,
                product_id: r.product_id,
                product_name: r.product_name.unwrap_or_default(),
                domain: r.domain,
                repos: r.repos,
                modules: r.modules,
                trend: r.trend,
                owner: r.owner,
                criticality: r.criticality,
                autonomy: r.autonomy,
                open_plans: r.open_plans,
                open_requests: r.open_requests,
                last_eval: r.last_eval,
            })
            .collect())
    }

    pub(super) async fn create_component(
        &self,
        body: CreateComponentBody,
    ) -> Result<ComponentItem, ApiError> {
        let count: Option<CountRow> =
            sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Product WHERE id = ?")
                .bind(&body.product_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if count.map(|c| c.count).unwrap_or(0) == 0 {
            return Err(err_not_found("referenced product not found"));
        }
        let id = Uuid::new_v4().to_string();
        let now = Self::now_iso();
        sqlx::query(
            "INSERT INTO Component (id, name, domain, repos, modules, trend, owner, criticality, autonomy, openPlans, openRequests, lastEval, productId, createdAt, updatedAt) VALUES (?, ?, ?, 0, 0, ?, ?, ?, ?, 0, 0, ?, ?, ?, ?)"
        )
        .bind(&id).bind(&body.name).bind(&body.domain)
        .bind(body.trend)
        .bind(&body.owner).bind(&body.criticality).bind(&body.autonomy)
        .bind(&body.last_eval).bind(&body.product_id)
        .bind(&now).bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("insert error: {e}")))?;
        self.fetch_component_item(&id).await
    }

    pub(super) async fn put_component(
        &self,
        id: &str,
        body: PutComponentBody,
    ) -> Result<ComponentItem, ApiError> {
        let count: Option<CountRow> =
            sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Component WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if count.map(|c| c.count).unwrap_or(0) == 0 {
            return Err(err_not_found("Component not found"));
        }
        let pcount: Option<CountRow> =
            sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Product WHERE id = ?")
                .bind(&body.product_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if pcount.map(|c| c.count).unwrap_or(0) == 0 {
            return Err(err_not_found("referenced product not found"));
        }
        let now = Self::now_iso();
        sqlx::query(
            "UPDATE Component SET name = ?, domain = ?, repos = 0, modules = 0, trend = ?, owner = ?, criticality = ?, autonomy = ?, openPlans = 0, openRequests = 0, lastEval = ?, productId = ?, updatedAt = ? WHERE id = ?"
        )
        .bind(&body.name).bind(&body.domain)
        .bind(body.trend)
        .bind(&body.owner).bind(&body.criticality).bind(&body.autonomy)
        .bind(&body.last_eval).bind(&body.product_id)
        .bind(&now).bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("update error: {e}")))?;
        self.fetch_component_item(id).await
    }

    pub(super) async fn patch_component(
        &self,
        id: &str,
        body: PatchComponentBody,
    ) -> Result<ComponentItem, ApiError> {
        let existing = self.fetch_component_item(id).await?;
        if let Some(ref pid) = body.product_id {
            let pcount: Option<CountRow> =
                sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Product WHERE id = ?")
                    .bind(pid)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| err_internal(&format!("query error: {e}")))?;
            if pcount.map(|c| c.count).unwrap_or(0) == 0 {
                return Err(err_not_found("referenced product not found"));
            }
        }
        let name = body.name.unwrap_or(existing.name);
        let product_id = body.product_id.unwrap_or(existing.product_id);
        let domain = body.domain.unwrap_or(existing.domain);
        let owner = body.owner.unwrap_or(existing.owner);
        let criticality = body.criticality.unwrap_or(existing.criticality);
        let autonomy = body.autonomy.unwrap_or(existing.autonomy);
        let trend = body.trend.unwrap_or(existing.trend);
        let last_eval = body.last_eval.unwrap_or(existing.last_eval);
        let now = Self::now_iso();
        sqlx::query(
            "UPDATE Component SET name = ?, domain = ?, trend = ?, owner = ?, criticality = ?, autonomy = ?, lastEval = ?, productId = ?, updatedAt = ? WHERE id = ?"
        )
        .bind(&name).bind(&domain).bind(trend)
        .bind(&owner).bind(&criticality).bind(&autonomy)
        .bind(&last_eval).bind(&product_id)
        .bind(&now).bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("update error: {e}")))?;
        self.fetch_component_item(id).await
    }

    pub(super) async fn delete_component(&self, id: &str) -> Result<String, ApiError> {
        let count: Option<CountRow> = sqlx::query_as::<_, CountRow>(
            "SELECT COUNT(*) as count FROM Repo WHERE componentId = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if count.map(|c| c.count).unwrap_or(0) > 0 {
            return Err(err_conflict(
                "cannot delete component: it has dependent repos; remove them first",
            ));
        }
        let affected = sqlx::query("DELETE FROM Component WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("delete error: {e}")))?;
        if affected.rows_affected() == 0 {
            return Err(err_not_found("Component not found"));
        }
        Ok(id.to_string())
    }

    pub(super) async fn list_repos(&self) -> Result<Vec<RepoItem>, ApiError> {
        let rows = sqlx::query_as::<_, RepoRow>(
            "SELECT r.id, r.name, r.componentId as component_id, c.name as component_name, r.owner, r.criticality, r.autonomy, r.regressions, r.openPlans as open_plans, r.execStatus as exec_status, r.lastEval as last_eval FROM Repo r LEFT JOIN Component c ON r.componentId = c.id ORDER BY r.id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let mut result = Vec::new();
        for row in &rows {
            let depends_on = compute_repo_depends_on(&self.pool, &row.name).await?;
            let depended_on_by = compute_repo_depended_on_by(&self.pool, &row.name).await?;

            result.push(RepoItem {
                id: row.id.clone(),
                name: row.name.clone(),
                component: row.component_name.clone().unwrap_or_default(),
                owner: row.owner.clone(),
                criticality: row.criticality.clone(),
                autonomy: row.autonomy.clone(),
                regressions: row.regressions,
                open_plans: row.open_plans,
                exec_status: row.exec_status.clone(),
                last_eval: row.last_eval.clone(),
                depends_on,
                depended_on_by,
            });
        }
        Ok(result)
    }

    pub(super) async fn create_repo(&self, body: CreateRepoBody) -> Result<RepoItem, ApiError> {
        let count: Option<CountRow> =
            sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Component WHERE id = ?")
                .bind(&body.component_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if count.map(|c| c.count).unwrap_or(0) == 0 {
            return Err(err_not_found("referenced component not found"));
        }
        let id = Uuid::new_v4().to_string();
        let now = Self::now_iso();
        sqlx::query(
            "INSERT INTO Repo (id, name, componentId, owner, criticality, autonomy, regressions, openPlans, execStatus, lastEval, createdAt, updatedAt) VALUES (?, ?, ?, ?, ?, ?, 0, 0, ?, ?, ?, ?)"
        )
        .bind(&id).bind(&body.name).bind(&body.component_id)
        .bind(&body.owner).bind(&body.criticality).bind(&body.autonomy)
        .bind(&body.exec_status).bind(&body.last_eval)
        .bind(&now).bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("UNIQUE constraint failed") {
                err_conflict("name already exists")
            } else {
                err_internal(&format!("insert error: {e}"))
            }
        })?;
        self.fetch_repo_item(&id).await
    }

    pub(super) async fn put_repo(&self, id: &str, body: PutRepoBody) -> Result<RepoItem, ApiError> {
        let count: Option<CountRow> =
            sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Repo WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if count.map(|c| c.count).unwrap_or(0) == 0 {
            return Err(err_not_found("Repo not found"));
        }
        let ccount: Option<CountRow> =
            sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Component WHERE id = ?")
                .bind(&body.component_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if ccount.map(|c| c.count).unwrap_or(0) == 0 {
            return Err(err_not_found("referenced component not found"));
        }
        let now = Self::now_iso();
        sqlx::query(
            "UPDATE Repo SET name = ?, componentId = ?, owner = ?, criticality = ?, autonomy = ?, regressions = 0, openPlans = 0, execStatus = ?, lastEval = ?, updatedAt = ? WHERE id = ?"
        )
        .bind(&body.name).bind(&body.component_id)
        .bind(&body.owner).bind(&body.criticality).bind(&body.autonomy)
        .bind(&body.exec_status).bind(&body.last_eval)
        .bind(&now).bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("UNIQUE constraint failed") {
                err_conflict("name already exists")
            } else {
                err_internal(&format!("update error: {e}"))
            }
        })?;
        self.fetch_repo_item(id).await
    }

    pub(super) async fn patch_repo(
        &self,
        id: &str,
        body: PatchRepoBody,
    ) -> Result<RepoItem, ApiError> {
        let existing = self.fetch_repo_item(id).await?;
        if let Some(ref cid) = body.component_id {
            let ccount: Option<CountRow> = sqlx::query_as::<_, CountRow>(
                "SELECT COUNT(*) as count FROM Component WHERE id = ?",
            )
            .bind(cid)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("query error: {e}")))?;
            if ccount.map(|c| c.count).unwrap_or(0) == 0 {
                return Err(err_not_found("referenced component not found"));
            }
        }
        let current_component_row: Option<ComponentIdRow> = sqlx::query_as::<_, ComponentIdRow>(
            "SELECT componentId as component_id FROM Repo WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;
        let current_component_id = current_component_row
            .and_then(|c| c.component_id)
            .unwrap_or_default();
        let component_id = body.component_id.unwrap_or(current_component_id);
        let name = body.name.unwrap_or(existing.name);
        let owner = body.owner.unwrap_or(existing.owner);
        let criticality = body.criticality.unwrap_or(existing.criticality);
        let autonomy = body.autonomy.unwrap_or(existing.autonomy);
        let exec_status = body.exec_status.unwrap_or(existing.exec_status);
        let last_eval = body.last_eval.unwrap_or(existing.last_eval);
        let now = Self::now_iso();
        sqlx::query(
            "UPDATE Repo SET name = ?, componentId = ?, owner = ?, criticality = ?, autonomy = ?, execStatus = ?, lastEval = ?, updatedAt = ? WHERE id = ?"
        )
        .bind(&name).bind(&component_id)
        .bind(&owner).bind(&criticality).bind(&autonomy)
        .bind(&exec_status).bind(&last_eval)
        .bind(&now).bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("UNIQUE constraint failed") {
                err_conflict("name already exists")
            } else {
                err_internal(&format!("update error: {e}"))
            }
        })?;
        self.fetch_repo_item(id).await
    }

    pub(super) async fn delete_repo(&self, id: &str) -> Result<String, ApiError> {
        let count: Option<CountRow> =
            sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Module WHERE repoId = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if count.map(|c| c.count).unwrap_or(0) > 0 {
            return Err(err_conflict(
                "cannot delete repo: it has dependent modules; remove them first",
            ));
        }
        let affected = sqlx::query("DELETE FROM Repo WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("delete error: {e}")))?;
        if affected.rows_affected() == 0 {
            return Err(err_not_found("Repo not found"));
        }
        Ok(id.to_string())
    }

    pub(super) async fn list_modules(&self) -> Result<Vec<ModuleItem>, ApiError> {
        let rows = sqlx::query_as::<_, ModuleRow>(
            "SELECT m.id, m.name, m.kind, m.repoId as repo_id, r.name as repo_name, r.componentId as component_id, c.name as component_name FROM Module m LEFT JOIN Repo r ON m.repoId = r.id LEFT JOIN Component c ON r.componentId = c.id ORDER BY m.id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|row| ModuleItem {
                id: row.id,
                name: row.name,
                kind: row.kind,
                repo_id: row.repo_id,
                repo_name: row.repo_name.unwrap_or_default(),
                component_id: row.component_id.unwrap_or_default(),
                component_name: row.component_name.unwrap_or_default(),
            })
            .collect())
    }

    pub(super) async fn create_module(
        &self,
        body: CreateModuleBody,
    ) -> Result<ModuleItem, ApiError> {
        let count: Option<CountRow> =
            sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Repo WHERE id = ?")
                .bind(&body.repo_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if count.map(|c| c.count).unwrap_or(0) == 0 {
            return Err(err_not_found("referenced repo not found"));
        }
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO Module (id, name, kind, repoId) VALUES (?, ?, ?, ?)")
            .bind(&id)
            .bind(&body.name)
            .bind(&body.kind)
            .bind(&body.repo_id)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("insert error: {e}")))?;
        self.fetch_module_item(&id).await
    }

    pub(super) async fn put_module(
        &self,
        id: &str,
        body: PutModuleBody,
    ) -> Result<ModuleItem, ApiError> {
        let count: Option<CountRow> =
            sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Module WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if count.map(|c| c.count).unwrap_or(0) == 0 {
            return Err(err_not_found("Module not found"));
        }
        let rcount: Option<CountRow> =
            sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Repo WHERE id = ?")
                .bind(&body.repo_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if rcount.map(|c| c.count).unwrap_or(0) == 0 {
            return Err(err_not_found("referenced repo not found"));
        }
        sqlx::query("UPDATE Module SET name = ?, kind = ?, repoId = ? WHERE id = ?")
            .bind(&body.name)
            .bind(&body.kind)
            .bind(&body.repo_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("update error: {e}")))?;
        self.fetch_module_item(id).await
    }

    pub(super) async fn patch_module(
        &self,
        id: &str,
        body: PatchModuleBody,
    ) -> Result<ModuleItem, ApiError> {
        let existing = self.fetch_module_item(id).await?;
        if let Some(ref rid) = body.repo_id {
            let rcount: Option<CountRow> =
                sqlx::query_as::<_, CountRow>("SELECT COUNT(*) as count FROM Repo WHERE id = ?")
                    .bind(rid)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| err_internal(&format!("query error: {e}")))?;
            if rcount.map(|c| c.count).unwrap_or(0) == 0 {
                return Err(err_not_found("referenced repo not found"));
            }
        }
        let name = body.name.unwrap_or(existing.name);
        let kind = body.kind.unwrap_or(existing.kind);
        let repo_id = body.repo_id.unwrap_or(existing.repo_id);
        sqlx::query("UPDATE Module SET name = ?, kind = ?, repoId = ? WHERE id = ?")
            .bind(&name)
            .bind(&kind)
            .bind(&repo_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("update error: {e}")))?;
        self.fetch_module_item(id).await
    }

    pub(super) async fn delete_module(&self, id: &str) -> Result<String, ApiError> {
        let count: Option<CountRow> = sqlx::query_as::<_, CountRow>(
            "SELECT COUNT(*) as count FROM ModuleDependency WHERE fromModuleId = ? OR toModuleId = ?"
        )
        .bind(id).bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;
        if count.map(|c| c.count).unwrap_or(0) > 0 {
            return Err(err_conflict(
                "cannot delete module: it has dependent module-dependencies; remove them first",
            ));
        }
        let affected = sqlx::query("DELETE FROM Module WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("delete error: {e}")))?;
        if affected.rows_affected() == 0 {
            return Err(err_not_found("Module not found"));
        }
        Ok(id.to_string())
    }

    pub(super) async fn list_repo_dependencies(&self) -> Result<Vec<RepoDependencyItem>, ApiError> {
        let deps = sqlx::query_as::<_, ModuleDepRow>(
            "SELECT md.id, md.fromModuleId as from_module_id, md.toModuleId as to_module_id, md.type as dep_type, md.label,
             fm.name as from_module_name, fm.kind as from_module_kind, fm.repoId as from_repo_id,
             fr.name as from_repo_name, fc.name as from_component_name,
             tm.name as to_module_name, tm.kind as to_module_kind, tm.repoId as to_repo_id,
             tr.name as to_repo_name, tc.name as to_component_name
             FROM ModuleDependency md
             JOIN Module fm ON fm.id = md.fromModuleId
             JOIN Module tm ON tm.id = md.toModuleId
             JOIN Repo fr ON fr.id = fm.repoId
             JOIN Repo tr ON tr.id = tm.repoId
             LEFT JOIN Component fc ON fr.componentId = fc.id
             LEFT JOIN Component tc ON tr.componentId = tc.id
             ORDER BY md.id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for dep in &deps {
            let from_repo_name = dep.from_repo_name.clone().unwrap_or_default();
            let to_repo_name = dep.to_repo_name.clone().unwrap_or_default();
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

    pub(super) async fn list_module_dependencies(
        &self,
    ) -> Result<Vec<ModuleDependencyItem>, ApiError> {
        let deps = sqlx::query_as::<_, ModuleDepRow>(
            "SELECT md.id, md.fromModuleId as from_module_id, md.toModuleId as to_module_id, md.type as dep_type, md.label,
             fm.name as from_module_name, fm.kind as from_module_kind, fm.repoId as from_repo_id,
             fr.name as from_repo_name, fc.name as from_component_name,
             tm.name as to_module_name, tm.kind as to_module_kind, tm.repoId as to_repo_id,
             tr.name as to_repo_name, tc.name as to_component_name
             FROM ModuleDependency md
             JOIN Module fm ON fm.id = md.fromModuleId
             JOIN Module tm ON tm.id = md.toModuleId
             JOIN Repo fr ON fr.id = fm.repoId
             JOIN Repo tr ON tr.id = tm.repoId
             LEFT JOIN Component fc ON fr.componentId = fc.id
             LEFT JOIN Component tc ON tr.componentId = tc.id
             ORDER BY md.id ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let mut result = Vec::new();
        for dep in &deps {
            let from_repo_name = dep.from_repo_name.clone().unwrap_or_default();
            let from_component = dep.from_component_name.clone().unwrap_or_default();
            let to_repo_name = dep.to_repo_name.clone().unwrap_or_default();
            let to_component = dep.to_component_name.clone().unwrap_or_default();

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

    pub(super) async fn create_module_dependency(
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

    pub(super) async fn patch_module_dependency(
        &self,
        id: &str,
        body: PatchModuleDependencyBody,
    ) -> Result<ModuleDependencyItem, ApiError> {
        let existing = self.fetch_module_dependency_item(id).await?;
        let dep_type = body.dep_type.unwrap_or(existing.dep_type);
        let label = body.label.unwrap_or(existing.label);
        sqlx::query("UPDATE ModuleDependency SET type = ?, label = ? WHERE id = ?")
            .bind(&dep_type)
            .bind(&label)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("update error: {e}")))?;
        self.fetch_module_dependency_item(id).await
    }

    pub(super) async fn delete_module_dependency(&self, id: &str) -> Result<String, ApiError> {
        let affected = sqlx::query("DELETE FROM ModuleDependency WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("delete error: {e}")))?;
        if affected.rows_affected() == 0 {
            return Err(err_not_found("ModuleDependency not found"));
        }
        Ok(id.to_string())
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
