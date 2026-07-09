use super::helpers::{query_err, tx_err, unique_err};
use super::rows::*;
use crate::err_conflict;
use crate::err_not_found;
use crate::err_validation;
use newton_types::ApiError;
use newton_types::*;
use uuid::Uuid;

const ALLOWED_AGG_FNS: &[&str] = &["latest", "avg", "p50", "p90"];
const ALLOWED_SCOPE_LEVELS: &[&str] = &["product", "component", "repo", "module"];

fn scope_table(scope: &str) -> Option<&'static str> {
    match scope {
        "product" => Some("Product"),
        "component" => Some("Component"),
        "repo" => Some("Repo"),
        "module" => Some("Module"),
        _ => None,
    }
}

impl super::SqliteBackendStore {
    pub(super) async fn list_kpis_db(&self) -> Result<Vec<KpiItem>, ApiError> {
        let rows = sqlx::query_as::<_, KpiRow>(
            "SELECT id, name, description, scopeLevel AS scope_level, threshold, weight, aggFn AS agg_fn, createdAt AS created_at, updatedAt AS updated_at \
             FROM KPI ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(query_err)?;

        Ok(rows.into_iter().map(|r| r.into_item()).collect())
    }

    pub(super) async fn create_kpi_db(&self, body: CreateKpiBody) -> Result<KpiItem, ApiError> {
        if body.id.trim().is_empty() {
            return Err(err_validation("id is required"));
        }
        if body.name.trim().is_empty() {
            return Err(err_validation("name is required"));
        }
        if !ALLOWED_AGG_FNS.contains(&body.agg_fn.as_str()) {
            return Err(err_validation(
                "aggFn must be one of: latest, avg, p50, p90",
            ));
        }
        if !ALLOWED_SCOPE_LEVELS.contains(&body.scope_level.as_str()) {
            return Err(err_validation(
                "scopeLevel must be one of: product, component, repo, module",
            ));
        }
        if !(0.0..=100.0).contains(&body.threshold) {
            return Err(err_validation("threshold must be between 0 and 100"));
        }
        if body.weight <= 0.0 {
            return Err(err_validation("weight must be greater than 0"));
        }

        let now = Self::now_iso();
        sqlx::query(
            "INSERT INTO KPI (id, name, description, scopeLevel, threshold, weight, aggFn, createdAt, updatedAt) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET \
               name = excluded.name, \
               description = excluded.description, \
               scopeLevel = excluded.scopeLevel, \
               threshold = excluded.threshold, \
               weight = excluded.weight, \
               aggFn = excluded.aggFn, \
               updatedAt = excluded.updatedAt",
        )
        .bind(&body.id)
        .bind(&body.name)
        .bind(&body.description)
        .bind(&body.scope_level)
        .bind(body.threshold)
        .bind(body.weight)
        .bind(&body.agg_fn)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| unique_err(e, "KPI name already exists", "insert error"))?;

        self.get_kpi_db(&body.id).await
    }

    pub(super) async fn get_kpi_db(&self, id: &str) -> Result<KpiItem, ApiError> {
        let row: Option<KpiRow> = sqlx::query_as::<_, KpiRow>(
            "SELECT id, name, description, scopeLevel AS scope_level, threshold, weight, aggFn AS agg_fn, createdAt AS created_at, updatedAt AS updated_at \
             FROM KPI WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(query_err)?;
        row.map(|r| r.into_item())
            .ok_or_else(|| err_not_found("KPI not found"))
    }

    pub(super) async fn create_eval_run_db(
        &self,
        body: CreateEvalRunBody,
    ) -> Result<EvalRunItem, ApiError> {
        if body.id.trim().is_empty() {
            return Err(err_validation("id is required"));
        }
        if body.source.trim().is_empty() {
            return Err(err_validation("source is required"));
        }
        if body.scope_id.trim().is_empty() {
            return Err(err_validation("scopeId is required"));
        }
        if !ALLOWED_SCOPE_LEVELS.contains(&body.scope.as_str()) {
            return Err(err_validation(
                "scope must be one of: product, component, repo, module",
            ));
        }
        if let Some(score) = body.score {
            if !(0.0..=100.0).contains(&score) {
                return Err(err_validation("score must be between 0 and 100"));
            }
        }

        let now = Self::now_iso();
        let evaluated_at = body.evaluated_at.clone().unwrap_or_else(|| now.clone());

        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(tx_err)?;

        let table = scope_table(&body.scope).ok_or_else(|| {
            err_validation("scope must be one of: product, component, repo, module")
        })?;
        let scope_id_exists: bool =
            sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table} WHERE id = ?"))
                .bind(&body.scope_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(query_err)?
                > 0;
        if !scope_id_exists {
            return Err(err_not_found(&format!(
                "{} '{}' not found",
                body.scope, body.scope_id
            )));
        }

        if let Some(ref grades) = body.grades {
            let mut seen_dims = std::collections::HashSet::new();
            for g in grades {
                if g.dimension.trim().is_empty() {
                    return Err(err_validation("inline grade dimension must not be empty"));
                }
                if !(0.0..=100.0).contains(&g.score) {
                    return Err(err_validation(
                        "inline grade score must be between 0 and 100",
                    ));
                }
                if !seen_dims.insert(g.dimension.trim().to_string()) {
                    return Err(err_conflict(&format!(
                        "duplicate dimension '{}' in inline grades",
                        g.dimension
                    )));
                }
            }
        }

        sqlx::query(
            "INSERT INTO EvalRun (id, source, scope, scopeId, score, verdict, summary, evaluatedAt, ingestedAt, rawAssessment) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&body.id)
        .bind(&body.source)
        .bind(&body.scope)
        .bind(&body.scope_id)
        .bind(body.score)
        .bind(&body.verdict)
        .bind(&body.summary)
        .bind(&evaluated_at)
        .bind(&now)
        .bind(&body.raw_assessment)
        .execute(&mut *tx)
        .await
        .map_err(|e| unique_err(e, "EvalRun id already exists", "insert error"))?;

        if let Some(grades) = body.grades {
            for g in grades {
                if let Some(ref kpi_id) = g.kpi_id {
                    let kpi_exists: bool =
                        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM KPI WHERE id = ?")
                            .bind(kpi_id)
                            .fetch_one(&mut *tx)
                            .await
                            .map_err(query_err)?
                            > 0;
                    if !kpi_exists {
                        return Err(err_not_found(&format!("KPI '{}' not found", kpi_id)));
                    }
                }

                let grade_id = Uuid::new_v4().to_string();
                let grade_evaluated_at = g
                    .evaluated_at
                    .as_deref()
                    .unwrap_or(&evaluated_at)
                    .to_string();
                let evidence_str = g
                    .evidence
                    .as_ref()
                    .map(|v| serde_json::to_string(v).unwrap_or_default());

                sqlx::query(
                    "INSERT INTO Grade (id, runId, kpiId, dimension, score, evidence, evaluatedAt, ingestedAt) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(&grade_id)
                .bind(&body.id)
                .bind(&g.kpi_id)
                .bind(g.dimension.trim())
                .bind(g.score)
                .bind(&evidence_str)
                .bind(&grade_evaluated_at)
                .bind(&now)
                .execute(&mut *tx)
                .await
                .map_err(|e| unique_err(e, &format!("Grade already exists for (runId, dimension={})", g.dimension), "insert grade error"))?;
            }
        }

        tx.commit().await.map_err(tx_err)?;

        self.get_eval_run_db(&body.id).await
    }

    pub(super) async fn list_eval_runs_db(
        &self,
        scope: Option<String>,
        scope_id: Option<String>,
        source: Option<String>,
        limit: Option<u32>,
    ) -> Result<Vec<EvalRunItem>, ApiError> {
        let mut sql = String::from(
            "SELECT id, source, scope, scopeId AS scope_id, score, verdict, summary, evaluatedAt AS evaluated_at, ingestedAt AS ingested_at, rawAssessment AS raw_assessment FROM EvalRun",
        );
        let mut conditions = Vec::new();
        if scope.is_some() {
            conditions.push("scope = ?");
        }
        if scope_id.is_some() {
            conditions.push("scopeId = ?");
        }
        if source.is_some() {
            conditions.push("source = ?");
        }
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY ingestedAt DESC");
        if limit.is_some() {
            sql.push_str(" LIMIT ?");
        }

        let mut q = sqlx::query_as::<_, EvalRunRow>(&sql);
        if let Some(scope) = scope {
            q = q.bind(scope);
        }
        if let Some(scope_id) = scope_id {
            q = q.bind(scope_id);
        }
        if let Some(source) = source {
            q = q.bind(source);
        }
        if let Some(limit) = limit {
            q = q.bind(limit as i64);
        }

        let rows = q.fetch_all(&self.pool).await.map_err(query_err)?;
        Ok(rows.into_iter().map(|r| r.into_item()).collect())
    }

    pub(super) async fn get_eval_run_db(&self, id: &str) -> Result<EvalRunItem, ApiError> {
        let row: Option<EvalRunRow> = sqlx::query_as::<_, EvalRunRow>(
            "SELECT id, source, scope, scopeId AS scope_id, score, verdict, summary, evaluatedAt AS evaluated_at, ingestedAt AS ingested_at, rawAssessment AS raw_assessment \
             FROM EvalRun WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(query_err)?;
        row.map(|r| r.into_item())
            .ok_or_else(|| err_not_found("EvalRun not found"))
    }

    pub(super) async fn create_grade_db(
        &self,
        body: CreateGradeBody,
    ) -> Result<GradeItem, ApiError> {
        if body.id.trim().is_empty() {
            return Err(err_validation("id is required"));
        }
        if body.run_id.trim().is_empty() {
            return Err(err_validation("runId is required"));
        }
        if let Some(kpi_id) = body.kpi_id.as_ref() {
            if kpi_id.trim().is_empty() {
                return Err(err_validation("kpiId must be non-empty when provided"));
            }
        }
        if body.dimension.trim().is_empty() {
            return Err(err_validation("dimension is required"));
        }
        if !(0.0..=100.0).contains(&body.score) {
            return Err(err_validation("score must be between 0 and 100"));
        }

        let now = Self::now_iso();
        let evaluated_at = body.evaluated_at.clone().unwrap_or_else(|| now.clone());
        let evidence_str = body
            .evidence
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_default());

        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(tx_err)?;

        let run_exists: bool =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM EvalRun WHERE id = ?")
                .bind(&body.run_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(query_err)?
                > 0;
        if !run_exists {
            return Err(err_not_found(&format!(
                "EvalRun '{}' not found",
                body.run_id
            )));
        }

        if let Some(ref kpi_id) = body.kpi_id {
            let kpi_exists: bool =
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM KPI WHERE id = ?")
                    .bind(kpi_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(query_err)?
                    > 0;
            if !kpi_exists {
                return Err(err_not_found(&format!("KPI '{}' not found", kpi_id)));
            }
        }

        let exists_for_dimension: bool = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM Grade WHERE runId = ? AND dimension = ?",
        )
        .bind(&body.run_id)
        .bind(&body.dimension)
        .fetch_one(&mut *tx)
        .await
        .map_err(query_err)?
            > 0;
        if exists_for_dimension {
            return Err(err_conflict("Grade already exists for (runId, dimension)"));
        }

        sqlx::query(
            "INSERT INTO Grade (id, runId, kpiId, dimension, score, evidence, evaluatedAt, ingestedAt) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&body.id)
        .bind(&body.run_id)
        .bind(&body.kpi_id)
        .bind(&body.dimension)
        .bind(body.score)
        .bind(&evidence_str)
        .bind(&evaluated_at)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| unique_err(e, "Grade already exists for (runId, dimension) or id", "insert error"))?;

        tx.commit().await.map_err(tx_err)?;

        self.get_grade_db(&body.id).await
    }

    pub(super) async fn list_grades_db(
        &self,
        run_id: Option<String>,
        kpi_id: Option<String>,
    ) -> Result<Vec<GradeItem>, ApiError> {
        let mut sql = String::from(
            "SELECT id, runId AS run_id, kpiId AS kpi_id, dimension, score, evidence, evaluatedAt AS evaluated_at, ingestedAt AS ingested_at FROM Grade",
        );
        let mut conditions = Vec::new();
        if run_id.is_some() {
            conditions.push("runId = ?");
        }
        if kpi_id.is_some() {
            conditions.push("kpiId = ?");
        }
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY ingestedAt DESC");

        let mut q = sqlx::query_as::<_, GradeRow>(&sql);
        if let Some(run_id) = run_id {
            q = q.bind(run_id);
        }
        if let Some(kpi_id) = kpi_id {
            q = q.bind(kpi_id);
        }

        let rows = q.fetch_all(&self.pool).await.map_err(query_err)?;
        Ok(rows.into_iter().map(|r| r.into_item()).collect())
    }

    pub(super) async fn get_grade_db(&self, id: &str) -> Result<GradeItem, ApiError> {
        let row: Option<GradeRow> = sqlx::query_as::<_, GradeRow>(
            "SELECT id, runId AS run_id, kpiId AS kpi_id, dimension, score, evidence, evaluatedAt AS evaluated_at, ingestedAt AS ingested_at \
             FROM Grade WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(query_err)?;
        row.map(|r| r.into_item())
            .ok_or_else(|| err_not_found("Grade not found"))
    }
}
