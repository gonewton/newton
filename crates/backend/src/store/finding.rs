use super::helpers::query_err;
use super::rows::*;
use crate::err_internal;
use crate::err_not_found;
use crate::models::*;
use newton_types::ApiError;

const FINDING_SELECT: &str =
    "SELECT f.id, f.source, f.origin, f.componentId as component_id, c.name as component_name, \
     f.module, f.repoId as repo_id, r.name as repo_name, f.kpiId as kpi_id, \
     f.dimension, f.location, f.fingerprint, f.title, \
     f.whyItMatters as why_it_matters, f.recommendedAction as recommended_action, \
     f.severity, f.risk, f.confidence, f.evidence, f.expectedValue as expected_value, \
     f.effort, f.status, f.lastSeenAt as last_seen_at, \
     f.dependsOn as depends_on, f.blocks, f.blockedByPlanId as blocked_by_plan_id, \
     p.attempts as blocked_plan_attempts, p.lastError as blocked_last_error, \
     p.linkedChangeRequestId as blocked_change_request_id, \
     f.createdAt as created_at, f.updatedAt as updated_at \
     FROM Finding f \
     LEFT JOIN Component c ON f.componentId = c.id \
     LEFT JOIN Repo r ON f.repoId = r.id \
     LEFT JOIN Plan p ON f.blockedByPlanId = p.id";

const CR_SELECT: &str = "SELECT cr.id, cr.title, cr.body, cr.origin, cr.author, \
     cr.componentId as component_id, c.name as component_name, \
     cr.repoId as repo_id, r.name as repo_name, \
     cr.status, cr.findingIds as finding_ids, \
     cr.risk, cr.confidence, \
     cr.createdAt as created_at, cr.updatedAt as updated_at \
     FROM ChangeRequest cr \
     LEFT JOIN Component c ON cr.componentId = c.id \
     LEFT JOIN Repo r ON cr.repoId = r.id";

fn finding_row_to_item(row: FindingRow) -> FindingItem {
    let depends_on: Vec<String> = row
        .depends_on
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let blocks: Vec<String> = row
        .blocks
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    FindingItem {
        id: row.id,
        source: row.source,
        origin: row.origin,
        component_id: row.component_id,
        module: row.module,
        repo_id: row.repo_id,
        kpi_id: row.kpi_id,
        dimension: row.dimension,
        location: row.location.and_then(|s| serde_json::from_str(&s).ok()),
        fingerprint: row.fingerprint,
        title: row.title,
        why_it_matters: row.why_it_matters,
        recommended_action: row.recommended_action,
        severity: row.severity,
        risk: row.risk,
        confidence: row.confidence,
        evidence: row.evidence.and_then(|s| serde_json::from_str(&s).ok()),
        expected_value: row.expected_value,
        effort: row.effort,
        status: row.status,
        last_seen_at: row.last_seen_at,
        depends_on,
        blocks,
        blocked_by_plan_id: row.blocked_by_plan_id,
        blocked_plan_attempts: row.blocked_plan_attempts,
        blocked_last_error: row.blocked_last_error,
        blocked_change_request_id: row.blocked_change_request_id,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn cr_row_to_item(row: ChangeRequestRow) -> ChangeRequestItem {
    let finding_ids: Vec<String> = row
        .finding_ids
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    ChangeRequestItem {
        id: row.id,
        title: row.title,
        body: row.body,
        origin: row.origin,
        author: row.author,
        component_id: row.component_id,
        repo_id: row.repo_id,
        status: row.status,
        finding_ids,
        risk: row.risk,
        confidence: row.confidence,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

impl super::SqliteBackendStore {
    pub(super) async fn list_findings_db(
        &self,
        status: Option<String>,
        scope: Option<String>,
        scope_id: Option<String>,
    ) -> Result<Vec<FindingItem>, ApiError> {
        let mut conditions = Vec::new();
        if status.is_some() {
            conditions.push("f.status = ?".to_string());
        }
        let scope_condition = if scope_id.is_some() {
            let col = match scope.as_deref() {
                Some("component") => "f.componentId = ?",
                Some("repo") => "f.repoId = ?",
                Some("module") => "f.module = ?",
                _ => "(f.componentId = ? OR f.repoId = ?)",
            };
            conditions.push(col.to_string());
            col
        } else {
            ""
        };

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };

        let sql = format!("{FINDING_SELECT}{where_clause} ORDER BY f.createdAt DESC");

        let mut q = sqlx::query_as::<_, FindingRow>(&sql);
        if let Some(ref s) = status {
            q = q.bind(s);
        }
        if let Some(ref sid) = scope_id {
            // Multi-bind for the fallback OR clause; single bind for specific columns.
            if scope_condition == "(f.componentId = ? OR f.repoId = ?)" {
                q = q.bind(sid).bind(sid);
            } else {
                q = q.bind(sid);
            }
        }

        let rows = q.fetch_all(&self.pool).await.map_err(query_err)?;

        Ok(rows.into_iter().map(finding_row_to_item).collect())
    }

    pub(super) async fn get_finding_db(&self, id: &str) -> Result<FindingItem, ApiError> {
        let row = sqlx::query_as::<_, FindingRow>(&format!("{FINDING_SELECT} WHERE f.id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(query_err)?
            .ok_or_else(|| err_not_found("Finding not found"))?;

        Ok(finding_row_to_item(row))
    }

    pub(super) async fn create_finding_db(
        &self,
        body: CreateFindingBody,
    ) -> Result<FindingItem, ApiError> {
        let now = Self::now_iso();
        let last_seen_at = body.last_seen_at.unwrap_or_else(|| now.clone());
        let depends_on_json =
            serde_json::to_string(&body.depends_on).unwrap_or_else(|_| "[]".to_string());
        let blocks_json = serde_json::to_string(&body.blocks).unwrap_or_else(|_| "[]".to_string());
        let location_json = body
            .location
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());
        let evidence_json = body
            .evidence
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());

        sqlx::query(
            "INSERT INTO Finding (
                id, source, origin, componentId, module, repoId, kpiId,
                dimension, location, fingerprint, title, whyItMatters, recommendedAction,
                severity, risk, confidence, evidence, expectedValue, effort,
                status, lastSeenAt, dependsOn, blocks, createdAt, updatedAt
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                source          = excluded.source,
                origin          = excluded.origin,
                componentId     = excluded.componentId,
                module          = excluded.module,
                repoId          = excluded.repoId,
                kpiId           = excluded.kpiId,
                dimension       = excluded.dimension,
                location        = excluded.location,
                fingerprint     = excluded.fingerprint,
                title           = excluded.title,
                whyItMatters    = excluded.whyItMatters,
                recommendedAction = excluded.recommendedAction,
                severity        = excluded.severity,
                risk            = excluded.risk,
                confidence      = excluded.confidence,
                evidence        = excluded.evidence,
                expectedValue   = excluded.expectedValue,
                effort          = excluded.effort,
                status          = excluded.status,
                lastSeenAt      = excluded.lastSeenAt,
                dependsOn       = excluded.dependsOn,
                blocks          = excluded.blocks,
                updatedAt       = excluded.updatedAt",
        )
        .bind(&body.id)
        .bind(&body.source)
        .bind(&body.origin)
        .bind(&body.component_id)
        .bind(&body.module)
        .bind(&body.repo_id)
        .bind(&body.kpi_id)
        .bind(&body.dimension)
        .bind(&location_json)
        .bind(&body.fingerprint)
        .bind(&body.title)
        .bind(&body.why_it_matters)
        .bind(&body.recommended_action)
        .bind(&body.severity)
        .bind(&body.risk)
        .bind(body.confidence)
        .bind(&evidence_json)
        .bind(body.expected_value)
        .bind(&body.effort)
        .bind(&body.status)
        .bind(&last_seen_at)
        .bind(&depends_on_json)
        .bind(&blocks_json)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("upsert Finding error: {e}")))?;

        self.get_finding_db(&body.id).await
    }

    pub(super) async fn patch_finding_db(
        &self,
        id: &str,
        body: PatchFindingBody,
    ) -> Result<FindingItem, ApiError> {
        if !self.row_exists("Finding", id).await? {
            return Err(err_not_found("Finding not found"));
        }

        let now = Self::now_iso();

        if let Some(ref status) = body.status {
            sqlx::query("UPDATE Finding SET status = ?, updatedAt = ? WHERE id = ?")
                .bind(status)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update Finding status error: {e}")))?;
        }
        if let Some(ev) = body.expected_value {
            sqlx::query("UPDATE Finding SET expectedValue = ?, updatedAt = ? WHERE id = ?")
                .bind(ev)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update Finding expectedValue error: {e}")))?;
        }
        if let Some(ref effort) = body.effort {
            sqlx::query("UPDATE Finding SET effort = ?, updatedAt = ? WHERE id = ?")
                .bind(effort)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update Finding effort error: {e}")))?;
        }
        if let Some(ref risk) = body.risk {
            sqlx::query("UPDATE Finding SET risk = ?, updatedAt = ? WHERE id = ?")
                .bind(risk)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update Finding risk error: {e}")))?;
        }
        if let Some(ref last_seen_at) = body.last_seen_at {
            sqlx::query("UPDATE Finding SET lastSeenAt = ?, updatedAt = ? WHERE id = ?")
                .bind(last_seen_at)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update Finding lastSeenAt error: {e}")))?;
        }
        if let Some(ref plan_id) = body.blocked_by_plan_id {
            sqlx::query("UPDATE Finding SET blockedByPlanId = ?, updatedAt = ? WHERE id = ?")
                .bind(plan_id)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update Finding blockedByPlanId error: {e}")))?;
        }

        self.get_finding_db(id).await
    }

    pub(super) async fn unblock_finding_db(&self, id: &str) -> Result<FindingItem, ApiError> {
        let finding = self.get_finding_db(id).await?;
        if finding.status != "blocked" {
            return Err(crate::err_conflict("finding is not blocked"));
        }
        let now = Self::now_iso();
        sqlx::query(
            "UPDATE Finding SET status = 'awaiting_triage', blockedByPlanId = NULL, updatedAt = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("unblock Finding error: {e}")))?;
        self.get_finding_db(id).await
    }

    pub(super) async fn list_change_requests_db(
        &self,
        status: Option<String>,
    ) -> Result<Vec<ChangeRequestItem>, ApiError> {
        let sql = if status.is_some() {
            format!("{CR_SELECT} WHERE cr.status = ? ORDER BY cr.createdAt DESC")
        } else {
            format!("{CR_SELECT} ORDER BY cr.createdAt DESC")
        };

        let mut q = sqlx::query_as::<_, ChangeRequestRow>(&sql);
        if let Some(ref s) = status {
            q = q.bind(s);
        }

        let rows = q.fetch_all(&self.pool).await.map_err(query_err)?;

        Ok(rows.into_iter().map(cr_row_to_item).collect())
    }

    pub(super) async fn get_change_request_db(
        &self,
        id: &str,
    ) -> Result<ChangeRequestItem, ApiError> {
        let row = sqlx::query_as::<_, ChangeRequestRow>(&format!("{CR_SELECT} WHERE cr.id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(query_err)?
            .ok_or_else(|| err_not_found("ChangeRequest not found"))?;

        Ok(cr_row_to_item(row))
    }

    pub(super) async fn create_change_request_db(
        &self,
        body: CreateChangeRequestBody,
    ) -> Result<ChangeRequestItem, ApiError> {
        let now = Self::now_iso();
        let finding_ids_json =
            serde_json::to_string(&body.finding_ids).unwrap_or_else(|_| "[]".to_string());

        sqlx::query(
            "INSERT INTO ChangeRequest (
                id, title, body, origin, author, componentId, repoId,
                status, findingIds, risk, confidence, createdAt, updatedAt
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 'proposed', ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                title       = excluded.title,
                body        = excluded.body,
                origin      = excluded.origin,
                author      = excluded.author,
                componentId = excluded.componentId,
                repoId      = excluded.repoId,
                findingIds  = excluded.findingIds,
                risk        = excluded.risk,
                confidence  = excluded.confidence,
                updatedAt   = excluded.updatedAt",
        )
        .bind(&body.id)
        .bind(&body.title)
        .bind(&body.body)
        .bind(&body.origin)
        .bind(&body.author)
        .bind(&body.component_id)
        .bind(&body.repo_id)
        .bind(&finding_ids_json)
        .bind(&body.risk)
        .bind(body.confidence)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("upsert ChangeRequest error: {e}")))?;

        self.get_change_request_db(&body.id).await
    }

    pub(super) async fn patch_change_request_db(
        &self,
        id: &str,
        body: PatchChangeRequestBody,
    ) -> Result<ChangeRequestItem, ApiError> {
        if !self.row_exists("ChangeRequest", id).await? {
            return Err(err_not_found("ChangeRequest not found"));
        }

        let now = Self::now_iso();

        if let Some(ref status) = body.status {
            sqlx::query("UPDATE ChangeRequest SET status = ?, updatedAt = ? WHERE id = ?")
                .bind(status)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update ChangeRequest status error: {e}")))?;
        }

        self.get_change_request_db(id).await
    }
}

#[cfg(test)]
mod finding_tests {
    use super::super::SqliteBackendStore;
    use crate::models::{CreateChangeRequestBody, CreateFindingBody};

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
            why_it_matters: "Because tests matter".to_string(),
            recommended_action: "Add more tests".to_string(),
            severity: "medium".to_string(),
            risk: "medium".to_string(),
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
        let body = make_finding("finding-001");
        let item = store.create_finding_db(body).await.unwrap();
        assert_eq!(item.id, "finding-001");
        assert_eq!(item.status, "awaiting_triage");
        assert_eq!(item.dimension, "tests");
    }

    #[tokio::test]
    async fn create_change_request_happy_path() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let body = CreateChangeRequestBody {
            id: "cr-001".to_string(),
            title: "Add MFA".to_string(),
            body: None,
            origin: "system".to_string(),
            author: None,
            component_id: None,
            repo_id: None,
            finding_ids: vec![],
            risk: "medium".to_string(),
            confidence: None,
        };
        let item = store.create_change_request_db(body).await.unwrap();
        assert_eq!(item.id, "cr-001");
        assert_eq!(item.status, "proposed");
    }
}
