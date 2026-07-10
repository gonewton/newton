use super::helpers::query_err;
use super::rows::{OptimizeCycleRow, OptimizeRunRow};
use crate::err_internal;
use crate::err_not_found;
use newton_types::ApiError;
use newton_types::*;

const RUN_SELECT: &str =
    "SELECT id, projectId as project_id, scope, scopeId as scope_id, status, cycle, \
     maxCycles as max_cycles, graders, latestGrades as latest_grades, \
     openFindings as open_findings, blockedFindings as blocked_findings, \
     outcomeReason as outcome_reason, startedAt as started_at, updatedAt as updated_at \
     FROM OptimizeRun";

const CYCLE_SELECT: &str =
    "SELECT id, runId as run_id, cycle, grades, gradeMin as grade_min, decision, \
     changeRequestId as change_request_id, planId as plan_id, executionId as execution_id, \
     developStatus as develop_status, openFindings as open_findings, \
     resolvedThisCycle as resolved_this_cycle, createdAt as created_at \
     FROM OptimizeCycle";

fn run_row_to_item(row: OptimizeRunRow) -> OptimizeRunItem {
    OptimizeRunItem {
        id: row.id,
        project_id: row.project_id,
        scope: row.scope,
        scope_id: row.scope_id,
        status: row.status,
        cycle: row.cycle,
        max_cycles: row.max_cycles,
        graders: serde_json::from_str(&row.graders).unwrap_or_default(),
        latest_grades: serde_json::from_str(&row.latest_grades)
            .unwrap_or(serde_json::Value::Object(Default::default())),
        open_findings: row.open_findings,
        blocked_findings: row.blocked_findings,
        started_at: row.started_at,
        updated_at: row.updated_at,
    }
}

fn cycle_row_to_item(row: OptimizeCycleRow) -> OptimizeCycleItem {
    OptimizeCycleItem {
        id: row.id,
        run_id: row.run_id,
        cycle: row.cycle,
        grades: serde_json::from_str(&row.grades)
            .unwrap_or(serde_json::Value::Object(Default::default())),
        grade_min: row.grade_min,
        decision: row.decision,
        change_request_id: row.change_request_id,
        plan_id: row.plan_id,
        execution_id: row.execution_id,
        develop_status: row.develop_status,
        open_findings: row.open_findings,
        resolved_this_cycle: row.resolved_this_cycle,
        created_at: row.created_at,
    }
}

impl super::SqliteBackendStore {
    pub(super) async fn list_optimize_runs_db(&self) -> Result<Vec<OptimizeRunItem>, ApiError> {
        let rows =
            sqlx::query_as::<_, OptimizeRunRow>(&format!("{RUN_SELECT} ORDER BY startedAt DESC"))
                .fetch_all(&self.pool)
                .await
                .map_err(query_err)?;
        Ok(rows.into_iter().map(run_row_to_item).collect())
    }

    pub(super) async fn get_optimize_run_db(
        &self,
        id: &str,
    ) -> Result<OptimizeRunDetail, ApiError> {
        let row = sqlx::query_as::<_, OptimizeRunRow>(&format!("{RUN_SELECT} WHERE id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(query_err)?
            .ok_or_else(|| err_not_found("OptimizeRun not found"))?;

        let outcome_reason = row
            .outcome_reason
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        let run = run_row_to_item(row);
        Ok(OptimizeRunDetail {
            run,
            outcome_reason,
        })
    }

    pub(super) async fn create_optimize_run_db(
        &self,
        body: CreateOptimizeRunBody,
    ) -> Result<OptimizeRunItem, ApiError> {
        let now = Self::now_iso();
        let graders_json =
            serde_json::to_string(&body.graders).unwrap_or_else(|_| "[]".to_string());
        sqlx::query(
            "INSERT INTO OptimizeRun \
             (id, projectId, scope, scopeId, status, cycle, maxCycles, graders, \
              latestGrades, openFindings, blockedFindings, outcomeReason, startedAt, updatedAt) \
             VALUES (?, ?, ?, ?, 'running', 0, ?, ?, '{}', 0, 0, NULL, ?, ?)",
        )
        .bind(&body.id)
        .bind(&body.project_id)
        .bind(&body.scope)
        .bind(&body.scope_id)
        .bind(body.max_cycles)
        .bind(&graders_json)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("insert OptimizeRun error: {e}")))?;

        let row = sqlx::query_as::<_, OptimizeRunRow>(&format!("{RUN_SELECT} WHERE id = ?"))
            .bind(&body.id)
            .fetch_one(&self.pool)
            .await
            .map_err(query_err)?;
        Ok(run_row_to_item(row))
    }

    pub(super) async fn patch_optimize_run_db(
        &self,
        id: &str,
        body: PatchOptimizeRunBody,
    ) -> Result<OptimizeRunItem, ApiError> {
        let row = sqlx::query_as::<_, OptimizeRunRow>(&format!("{RUN_SELECT} WHERE id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(query_err)?
            .ok_or_else(|| err_not_found("OptimizeRun not found"))?;
        let _ = row;

        let now = Self::now_iso();

        if let Some(ref status) = body.status {
            sqlx::query("UPDATE OptimizeRun SET status = ?, updatedAt = ? WHERE id = ?")
                .bind(status)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update run status: {e}")))?;
        }
        if let Some(cycle) = body.cycle {
            sqlx::query("UPDATE OptimizeRun SET cycle = ?, updatedAt = ? WHERE id = ?")
                .bind(cycle)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update run cycle: {e}")))?;
        }
        if let Some(ref grades) = body.latest_grades {
            let grades_json = serde_json::to_string(grades).unwrap_or_else(|_| "{}".to_string());
            sqlx::query("UPDATE OptimizeRun SET latestGrades = ?, updatedAt = ? WHERE id = ?")
                .bind(&grades_json)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update run latestGrades: {e}")))?;
        }
        if let Some(of) = body.open_findings {
            sqlx::query("UPDATE OptimizeRun SET openFindings = ?, updatedAt = ? WHERE id = ?")
                .bind(of)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update run openFindings: {e}")))?;
        }
        if let Some(bf) = body.blocked_findings {
            sqlx::query("UPDATE OptimizeRun SET blockedFindings = ?, updatedAt = ? WHERE id = ?")
                .bind(bf)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update run blockedFindings: {e}")))?;
        }
        if let Some(ref reason) = body.outcome_reason {
            let reason_json = serde_json::to_string(reason).unwrap_or_else(|_| "null".to_string());
            sqlx::query("UPDATE OptimizeRun SET outcomeReason = ?, updatedAt = ? WHERE id = ?")
                .bind(&reason_json)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update run outcomeReason: {e}")))?;
        }

        let updated = sqlx::query_as::<_, OptimizeRunRow>(&format!("{RUN_SELECT} WHERE id = ?"))
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(query_err)?;
        Ok(run_row_to_item(updated))
    }

    pub(super) async fn create_optimize_cycle_db(
        &self,
        body: CreateOptimizeCycleBody,
    ) -> Result<OptimizeCycleItem, ApiError> {
        let now = Self::now_iso();
        let grades_json = serde_json::to_string(&body.grades).unwrap_or_else(|_| "{}".to_string());
        sqlx::query(
            "INSERT INTO OptimizeCycle \
             (id, runId, cycle, grades, gradeMin, decision, changeRequestId, planId, \
              executionId, developStatus, openFindings, resolvedThisCycle, createdAt) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&body.id)
        .bind(&body.run_id)
        .bind(body.cycle)
        .bind(&grades_json)
        .bind(body.grade_min)
        .bind(&body.decision)
        .bind(&body.change_request_id)
        .bind(&body.plan_id)
        .bind(&body.execution_id)
        .bind(&body.develop_status)
        .bind(body.open_findings)
        .bind(body.resolved_this_cycle)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("insert OptimizeCycle error: {e}")))?;

        let row = sqlx::query_as::<_, OptimizeCycleRow>(&format!("{CYCLE_SELECT} WHERE id = ?"))
            .bind(&body.id)
            .fetch_one(&self.pool)
            .await
            .map_err(query_err)?;
        Ok(cycle_row_to_item(row))
    }

    pub(super) async fn list_optimize_cycles_db(
        &self,
        run_id: &str,
    ) -> Result<Vec<OptimizeCycleItem>, ApiError> {
        let rows = sqlx::query_as::<_, OptimizeCycleRow>(&format!(
            "{CYCLE_SELECT} WHERE runId = ? ORDER BY cycle ASC"
        ))
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map_err(query_err)?;
        Ok(rows.into_iter().map(cycle_row_to_item).collect())
    }
}
