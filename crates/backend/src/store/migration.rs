use super::helpers::tx_err;
use crate::err_internal;
use newton_types::ApiError;
use sqlx::FromRow;
use sqlx::SqlitePool;

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
