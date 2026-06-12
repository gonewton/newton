use super::helpers::tx_err;
use crate::err_internal;
use newton_types::ApiError;
use sqlx::FromRow;
use sqlx::SqlitePool;

pub(super) async fn upgrade_eval_run_raw_assessment(pool: &SqlitePool) -> Result<(), ApiError> {
    let has_col: bool = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pragma_table_info('EvalRun') WHERE name='rawAssessment'",
    )
    .fetch_one(pool)
    .await
    .map(|n| n > 0)
    .unwrap_or(false);

    if !has_col {
        sqlx::query("ALTER TABLE EvalRun ADD COLUMN rawAssessment TEXT NULL")
            .execute(pool)
            .await
            .map_err(|e| err_internal(&format!("add rawAssessment column: {e}")))?;
    }
    Ok(())
}

pub(super) async fn upgrade_legacy_grade_schema(pool: &SqlitePool) -> Result<(), ApiError> {
    #[derive(Debug, FromRow)]
    struct TableInfoRow {
        name: String,
    }

    let info: Vec<TableInfoRow> = sqlx::query_as::<_, TableInfoRow>("PRAGMA table_info(Grade)")
        .fetch_all(pool)
        .await
        .map_err(|e| err_internal(&format!("schema check failed: {e}")))?;

    if info.is_empty() {
        return Ok(());
    }

    let has_run_id = info.iter().any(|r| r.name == "runId");
    let has_dimension = info.iter().any(|r| r.name == "dimension");
    if has_run_id && has_dimension {
        return Ok(());
    }

    let mut tx = pool.begin().await.map_err(tx_err)?;

    sqlx::query("PRAGMA foreign_keys = OFF;")
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("pragma error: {e}")))?;

    sqlx::query("DROP TABLE IF EXISTS Grade;")
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("drop Grade failed: {e}")))?;

    sqlx::query(
        "CREATE TABLE Grade (\
          id          TEXT PRIMARY KEY,\
          runId       TEXT NOT NULL,\
          kpiId       TEXT NULL,\
          dimension   TEXT NOT NULL,\
          score       REAL NOT NULL CHECK(score >= 0 AND score <= 100),\
          evidence    TEXT NULL,\
          evaluatedAt TEXT NOT NULL,\
          ingestedAt  TEXT NOT NULL,\
          UNIQUE(runId, dimension),\
          FOREIGN KEY(runId) REFERENCES EvalRun(id) ON DELETE CASCADE,\
          FOREIGN KEY(kpiId) REFERENCES KPI(id)\
        );",
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| err_internal(&format!("create Grade failed: {e}")))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_grade_runId ON Grade(runId);")
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("create index failed: {e}")))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_grade_kpiId ON Grade(kpiId);")
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("create index failed: {e}")))?;

    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("pragma error: {e}")))?;

    tx.commit().await.map_err(tx_err)?;

    Ok(())
}

pub(super) async fn upgrade_legacy_indicator_schema(pool: &SqlitePool) -> Result<(), ApiError> {
    #[derive(Debug, FromRow)]
    struct TableInfoRow {
        name: String,
        notnull: i64,
    }

    async fn table_info(pool: &SqlitePool, table: &str) -> Result<Vec<TableInfoRow>, ApiError> {
        sqlx::query_as::<_, TableInfoRow>(&format!("PRAGMA table_info({table})"))
            .fetch_all(pool)
            .await
            .map_err(|e| err_internal(&format!("schema check failed: {e}")))
    }

    let opportunity_info = table_info(pool, "Opportunity").await?;
    let has_opportunity_indicator = opportunity_info.iter().any(|r| r.name == "indicator");
    let has_opportunity_kpi = opportunity_info.iter().any(|r| r.name == "kpiId");

    let regression_info = table_info(pool, "Regression").await?;
    let has_regression_indicator = regression_info.iter().any(|r| r.name == "indicator");
    let has_regression_kpi = regression_info.iter().any(|r| r.name == "kpiId");
    let regression_kpi_not_null = regression_info
        .iter()
        .find(|r| r.name == "kpiId")
        .map(|r| r.notnull != 0)
        .unwrap_or(false);

    let mut tx = pool.begin().await.map_err(tx_err)?;

    sqlx::query("PRAGMA foreign_keys = OFF;")
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("pragma error: {e}")))?;

    sqlx::query("DROP TABLE IF EXISTS Indicator;")
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("drop Indicator failed: {e}")))?;

    if has_opportunity_indicator {
        sqlx::query(
            "CREATE TABLE Opportunity_new (\
              id TEXT PRIMARY KEY,\
              title TEXT NOT NULL,\
              origin TEXT NOT NULL,\
              componentId TEXT NULL,\
              module TEXT NULL,\
              repoId TEXT NULL,\
              kpiId TEXT NULL,\
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
              FOREIGN KEY(repoId) REFERENCES Repo(id),\
              FOREIGN KEY(kpiId) REFERENCES KPI(id)\
            );",
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("create Opportunity_new failed: {e}")))?;

        sqlx::query(
            "INSERT INTO Opportunity_new (
                id, title, origin, componentId, module, repoId, kpiId,
                confidence, risk, expectedValue, effort, status, age, rationale,
                dependsOn, blocks, createdAt, updatedAt
            )
            SELECT
                id, title, origin, componentId, module, repoId, NULL as kpiId,
                confidence, risk, expectedValue, effort, status, age, rationale,
                dependsOn, blocks, createdAt, updatedAt
            FROM Opportunity;",
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("copy Opportunity failed: {e}")))?;

        sqlx::query("DROP TABLE Opportunity;")
            .execute(&mut *tx)
            .await
            .map_err(|e| err_internal(&format!("drop Opportunity failed: {e}")))?;

        sqlx::query("ALTER TABLE Opportunity_new RENAME TO Opportunity;")
            .execute(&mut *tx)
            .await
            .map_err(|e| err_internal(&format!("rename Opportunity failed: {e}")))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_opportunity_status ON Opportunity(status);")
            .execute(&mut *tx)
            .await
            .map_err(|e| err_internal(&format!("index Opportunity status failed: {e}")))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_opportunity_componentId ON Opportunity(componentId);",
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("index Opportunity component failed: {e}")))?;
    } else if has_opportunity_kpi {
        sqlx::query("UPDATE Opportunity SET kpiId = NULL WHERE kpiId IS NOT NULL;")
            .execute(&mut *tx)
            .await
            .map_err(|e| err_internal(&format!("clear Opportunity kpiId failed: {e}")))?;
    }

    if has_regression_indicator || (has_regression_kpi && regression_kpi_not_null) {
        sqlx::query(
            "CREATE TABLE Regression_new (\
              id TEXT PRIMARY KEY,\
              repoName TEXT NOT NULL,\
              kpiId TEXT NULL,\
              delta REAL NOT NULL,\
              severity TEXT NOT NULL,\
              since TEXT NOT NULL,\
              trend TEXT NOT NULL,\
              createdAt TEXT NOT NULL,\
              FOREIGN KEY(repoName) REFERENCES Repo(name),\
              FOREIGN KEY(kpiId) REFERENCES KPI(id)\
            );",
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("create Regression_new failed: {e}")))?;

        sqlx::query(
            "INSERT INTO Regression_new (
                id, repoName, kpiId, delta, severity, since, trend, createdAt
            )
            SELECT
                id, repoName, NULL as kpiId, delta, severity, since, trend, createdAt
            FROM Regression;",
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("copy Regression failed: {e}")))?;

        sqlx::query("DROP TABLE Regression;")
            .execute(&mut *tx)
            .await
            .map_err(|e| err_internal(&format!("drop Regression failed: {e}")))?;

        sqlx::query("ALTER TABLE Regression_new RENAME TO Regression;")
            .execute(&mut *tx)
            .await
            .map_err(|e| err_internal(&format!("rename Regression failed: {e}")))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_regression_repoName ON Regression(repoName);")
            .execute(&mut *tx)
            .await
            .map_err(|e| err_internal(&format!("index Regression repoName failed: {e}")))?;
    } else if has_regression_kpi {
        sqlx::query("UPDATE Regression SET kpiId = NULL WHERE kpiId IS NOT NULL;")
            .execute(&mut *tx)
            .await
            .map_err(|e| err_internal(&format!("clear Regression kpiId failed: {e}")))?;
    }

    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("pragma error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| err_internal(&format!("commit error: {e}")))?;

    Ok(())
}

pub(super) async fn upgrade_legacy_component_schema(pool: &SqlitePool) -> Result<(), ApiError> {
    #[derive(Debug, FromRow)]
    struct ColRow {
        name: String,
    }

    let cols: Vec<ColRow> = sqlx::query_as::<_, ColRow>("PRAGMA table_info(Component)")
        .fetch_all(pool)
        .await
        .map_err(|e| err_internal(&format!("schema check failed: {e}")))?;

    if cols.is_empty() || !cols.iter().any(|r| r.name == "health") {
        return Ok(());
    }

    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| err_internal(&format!("acquire conn error: {e}")))?;

    sqlx::query("PRAGMA foreign_keys = OFF;")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("pragma error: {e}")))?;

    sqlx::query(
        "CREATE TABLE Component_new (\
          id TEXT PRIMARY KEY,\
          name TEXT NOT NULL,\
          domain TEXT NOT NULL,\
          repos INTEGER NOT NULL,\
          modules INTEGER NOT NULL,\
          trend INTEGER NOT NULL,\
          owner TEXT NOT NULL,\
          criticality TEXT NOT NULL,\
          autonomy TEXT NOT NULL,\
          openPlans INTEGER NOT NULL DEFAULT 0,\
          openRequests INTEGER NOT NULL DEFAULT 0,\
          lastEval TEXT NOT NULL,\
          productId TEXT NOT NULL,\
          createdAt TEXT NOT NULL,\
          updatedAt TEXT NOT NULL,\
          FOREIGN KEY(productId) REFERENCES Product(id)\
        );",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| err_internal(&format!("create Component_new failed: {e}")))?;

    sqlx::query(
        "INSERT INTO Component_new \
          (id, name, domain, repos, modules, trend, owner, criticality, autonomy, \
           openPlans, openRequests, lastEval, productId, createdAt, updatedAt) \
         SELECT \
          id, name, domain, repos, modules, trend, owner, criticality, autonomy, \
          openPlans, openRequests, lastEval, productId, createdAt, updatedAt \
         FROM Component;",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| err_internal(&format!("copy Component failed: {e}")))?;

    sqlx::query("DROP TABLE Component;")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("drop Component failed: {e}")))?;

    sqlx::query("ALTER TABLE Component_new RENAME TO Component;")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("rename Component failed: {e}")))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_component_productId ON Component(productId);")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("index Component productId failed: {e}")))?;

    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("pragma error: {e}")))?;

    Ok(())
}

pub(super) async fn upgrade_legacy_repo_schema(pool: &SqlitePool) -> Result<(), ApiError> {
    #[derive(Debug, FromRow)]
    struct ColRow {
        name: String,
    }

    let cols: Vec<ColRow> = sqlx::query_as::<_, ColRow>("PRAGMA table_info(Repo)")
        .fetch_all(pool)
        .await
        .map_err(|e| err_internal(&format!("schema check failed: {e}")))?;

    if cols.is_empty() || !cols.iter().any(|r| r.name == "qualityScore") {
        return Ok(());
    }

    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| err_internal(&format!("acquire conn error: {e}")))?;

    sqlx::query("PRAGMA foreign_keys = OFF;")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("pragma error: {e}")))?;

    sqlx::query(
        "CREATE TABLE Repo_new (\
          id TEXT PRIMARY KEY,\
          name TEXT NOT NULL UNIQUE,\
          componentId TEXT NOT NULL,\
          owner TEXT NOT NULL,\
          criticality TEXT NOT NULL,\
          autonomy TEXT NOT NULL,\
          regressions INTEGER NOT NULL DEFAULT 0,\
          openPlans INTEGER NOT NULL DEFAULT 0,\
          execStatus TEXT NOT NULL,\
          lastEval TEXT NOT NULL,\
          createdAt TEXT NOT NULL,\
          updatedAt TEXT NOT NULL,\
          FOREIGN KEY(componentId) REFERENCES Component(id)\
        );",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| err_internal(&format!("create Repo_new failed: {e}")))?;

    sqlx::query(
        "INSERT INTO Repo_new \
          (id, name, componentId, owner, criticality, autonomy, \
           regressions, openPlans, execStatus, lastEval, createdAt, updatedAt) \
         SELECT \
          id, name, componentId, owner, criticality, autonomy, \
          regressions, openPlans, execStatus, lastEval, createdAt, updatedAt \
         FROM Repo;",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| err_internal(&format!("copy Repo failed: {e}")))?;

    sqlx::query("DROP TABLE Repo;")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("drop Repo failed: {e}")))?;

    sqlx::query("ALTER TABLE Repo_new RENAME TO Repo;")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("rename Repo failed: {e}")))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_repo_componentId ON Repo(componentId);")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("index Repo componentId failed: {e}")))?;

    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("pragma error: {e}")))?;

    Ok(())
}

pub(super) async fn upgrade_optimize_run(pool: &SqlitePool) -> Result<(), ApiError> {
    let has_table: bool = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='OptimizeRun'",
    )
    .fetch_one(pool)
    .await
    .map(|n| n > 0)
    .unwrap_or(false);

    if !has_table {
        sqlx::query(
            "CREATE TABLE OptimizeRun (\
              id TEXT PRIMARY KEY,\
              projectId TEXT NOT NULL,\
              scope TEXT NOT NULL DEFAULT 'repo',\
              scopeId TEXT NOT NULL,\
              status TEXT NOT NULL DEFAULT 'running',\
              cycle INTEGER NOT NULL DEFAULT 0,\
              maxCycles INTEGER NOT NULL DEFAULT 8,\
              graders TEXT NOT NULL DEFAULT '[]',\
              latestGrades TEXT NOT NULL DEFAULT '{}',\
              openFindings INTEGER NOT NULL DEFAULT 0,\
              blockedFindings INTEGER NOT NULL DEFAULT 0,\
              outcomeReason TEXT NULL,\
              startedAt TEXT NOT NULL,\
              updatedAt TEXT NOT NULL\
            )",
        )
        .execute(pool)
        .await
        .map_err(|e| err_internal(&format!("create OptimizeRun failed: {e}")))?;
    }

    let has_cycle_table: bool = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='OptimizeCycle'",
    )
    .fetch_one(pool)
    .await
    .map(|n| n > 0)
    .unwrap_or(false);

    if !has_cycle_table {
        sqlx::query(
            "CREATE TABLE OptimizeCycle (\
              id TEXT PRIMARY KEY,\
              runId TEXT NOT NULL,\
              cycle INTEGER NOT NULL,\
              grades TEXT NOT NULL DEFAULT '{}',\
              gradeMin REAL NULL,\
              decision TEXT NOT NULL DEFAULT 'none',\
              changeRequestId TEXT NULL,\
              planId TEXT NULL,\
              executionId TEXT NULL,\
              developStatus TEXT NULL,\
              openFindings INTEGER NOT NULL DEFAULT 0,\
              resolvedThisCycle INTEGER NOT NULL DEFAULT 0,\
              createdAt TEXT NOT NULL,\
              FOREIGN KEY(runId) REFERENCES OptimizeRun(id)\
            )",
        )
        .execute(pool)
        .await
        .map_err(|e| err_internal(&format!("create OptimizeCycle failed: {e}")))?;
    }

    Ok(())
}

pub(super) async fn upgrade_finding_blocked_by_plan(pool: &SqlitePool) -> Result<(), ApiError> {
    let has_col: bool = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pragma_table_info('Finding') WHERE name='blockedByPlanId'",
    )
    .fetch_one(pool)
    .await
    .map(|n| n > 0)
    .unwrap_or(false);

    if !has_col {
        sqlx::query("ALTER TABLE Finding ADD COLUMN blockedByPlanId TEXT NULL")
            .execute(pool)
            .await
            .map_err(|e| err_internal(&format!("add blockedByPlanId column: {e}")))?;
    }
    Ok(())
}

/// Migrate the legacy Plan schema (`linkedRequestId` -> `Request`) to the
/// grading-era schema (`linkedChangeRequestId` -> `ChangeRequest`).
///
/// This is the one-time rebuild that used to live in `004_grading.sql`. Because
/// that file is re-run on every store open, performing the rebuild there wiped
/// every plan's `linkedChangeRequestId` (the `INSERT ... SELECT ..., NULL, ...`)
/// on each open. Here it is guarded: if the Plan table already has
/// `linkedChangeRequestId`, this is a no-op, so existing links are preserved.
pub(super) async fn upgrade_plan_change_request_link(pool: &SqlitePool) -> Result<(), ApiError> {
    #[derive(Debug, FromRow)]
    struct ColRow {
        name: String,
    }

    let cols: Vec<ColRow> = sqlx::query_as::<_, ColRow>("PRAGMA table_info(Plan)")
        .fetch_all(pool)
        .await
        .map_err(|e| err_internal(&format!("schema check Plan failed: {e}")))?;

    // No Plan table yet, or already migrated: nothing to do (and, crucially,
    // never rebuild a Plan that already has the column — that is what wiped links).
    if cols.is_empty() || cols.iter().any(|c| c.name == "linkedChangeRequestId") {
        return Ok(());
    }

    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| err_internal(&format!("acquire conn error: {e}")))?;

    sqlx::query("PRAGMA foreign_keys = OFF;")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("pragma error: {e}")))?;

    sqlx::query(
        "CREATE TABLE Plan_new (\
          id TEXT PRIMARY KEY,\
          title TEXT NOT NULL,\
          componentId TEXT NULL,\
          repoId TEXT NULL,\
          status TEXT NOT NULL,\
          linkedChangeRequestId TEXT NULL,\
          confidence INTEGER NOT NULL,\
          risk TEXT NOT NULL,\
          expectedValue TEXT NULL,\
          agentGenerated INTEGER NOT NULL DEFAULT 0,\
          waitingSince TEXT NULL,\
          expectedDelta TEXT NULL,\
          createdAt TEXT NOT NULL,\
          updatedAt TEXT NOT NULL,\
          FOREIGN KEY(componentId) REFERENCES Component(id),\
          FOREIGN KEY(repoId) REFERENCES Repo(id),\
          FOREIGN KEY(linkedChangeRequestId) REFERENCES ChangeRequest(id)\
        );",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| err_internal(&format!("create Plan_new failed: {e}")))?;

    // The legacy link pointed at the (now-dropped) Request table, so it cannot be
    // carried over; new links are populated going forward by create_plan.
    sqlx::query(
        "INSERT OR IGNORE INTO Plan_new \
          (id, title, componentId, repoId, status, linkedChangeRequestId, \
           confidence, risk, expectedValue, agentGenerated, waitingSince, \
           expectedDelta, createdAt, updatedAt) \
         SELECT \
          id, title, componentId, repoId, status, NULL, \
          confidence, risk, expectedValue, agentGenerated, waitingSince, \
          expectedDelta, createdAt, updatedAt \
         FROM Plan;",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| err_internal(&format!("copy Plan failed: {e}")))?;

    sqlx::query("DROP TABLE Plan;")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("drop Plan failed: {e}")))?;

    sqlx::query("ALTER TABLE Plan_new RENAME TO Plan;")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("rename Plan failed: {e}")))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_plan_status ON Plan(status);")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("index Plan status failed: {e}")))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_plan_componentId ON Plan(componentId);")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("index Plan componentId failed: {e}")))?;

    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&mut *conn)
        .await
        .map_err(|e| err_internal(&format!("pragma error: {e}")))?;

    Ok(())
}

pub(super) async fn upgrade_plan_optimize(pool: &SqlitePool) -> Result<(), ApiError> {
    #[derive(Debug, FromRow)]
    struct ColRow {
        name: String,
    }

    let plan_cols: Vec<ColRow> = sqlx::query_as::<_, ColRow>("PRAGMA table_info(Plan)")
        .fetch_all(pool)
        .await
        .map_err(|e| err_internal(&format!("schema check Plan failed: {e}")))?;

    let plan_col_names: std::collections::HashSet<String> =
        plan_cols.into_iter().map(|r| r.name).collect();

    for (col, ddl) in [
        ("body", "ALTER TABLE Plan ADD COLUMN body TEXT NULL"),
        (
            "executionId",
            "ALTER TABLE Plan ADD COLUMN executionId TEXT NULL",
        ),
        (
            "attempts",
            "ALTER TABLE Plan ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "lastError",
            "ALTER TABLE Plan ADD COLUMN lastError TEXT NULL",
        ),
        ("module", "ALTER TABLE Plan ADD COLUMN module TEXT NULL"),
    ] {
        if !plan_col_names.contains(col) {
            sqlx::query(ddl)
                .execute(pool)
                .await
                .map_err(|e| err_internal(&format!("add Plan.{col} failed: {e}")))?;
        }
    }

    let cr_cols: Vec<ColRow> = sqlx::query_as::<_, ColRow>("PRAGMA table_info(ChangeRequest)")
        .fetch_all(pool)
        .await
        .map_err(|e| err_internal(&format!("schema check ChangeRequest failed: {e}")))?;

    let cr_col_names: std::collections::HashSet<String> =
        cr_cols.into_iter().map(|r| r.name).collect();

    for (col, ddl) in [
        (
            "risk",
            "ALTER TABLE ChangeRequest ADD COLUMN risk TEXT NOT NULL DEFAULT 'medium'",
        ),
        (
            "confidence",
            "ALTER TABLE ChangeRequest ADD COLUMN confidence REAL NULL",
        ),
    ] {
        if !cr_col_names.contains(col) {
            sqlx::query(ddl)
                .execute(pool)
                .await
                .map_err(|e| err_internal(&format!("add ChangeRequest.{col} failed: {e}")))?;
        }
    }

    Ok(())
}
