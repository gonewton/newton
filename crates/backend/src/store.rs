use crate::models::*;
use crate::{err_conflict, err_internal, err_not_found, err_validation, BackendStore};
use chrono::{DateTime, Utc};
use newton_types::{ApiError, HilEventType, HilStatus, NodeStatus, WorkflowStatus};
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
            .create_if_missing(true)
            // Enforce FK declarations on every connection. SQLite defaults
            // foreign_keys=OFF per connection, so the migration's PRAGMA
            // doesn't propagate — without this, ON DELETE CASCADE and FK
            // constraints across the schema are silently inert.
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|e| err_internal(&format!("failed to connect to database: {e}")))?;

        sqlx::query(include_str!("../migrations/001_init.sql"))
            .execute(&pool)
            .await
            .map_err(|e| err_internal(&format!("migration failed: {e}")))?;

        sqlx::query(include_str!("../migrations/002_grades.sql"))
            .execute(&pool)
            .await
            .map_err(|e| err_internal(&format!("migration 002 failed: {e}")))?;

        sqlx::query(include_str!("../migrations/003_workflow_runtime.sql"))
            .execute(&pool)
            .await
            .map_err(|e| err_internal(&format!("migration 003 failed: {e}")))?;

        Self::upgrade_legacy_indicator_schema(&pool).await?;
        Self::upgrade_legacy_grade_schema(&pool).await?;

        Ok(Self { pool })
    }

    pub async fn new_in_memory() -> Result<Self, ApiError> {
        Self::new("sqlite::memory:").await
    }

    fn now_iso() -> String {
        Utc::now().to_rfc3339()
    }

    async fn upgrade_legacy_grade_schema(pool: &SqlitePool) -> Result<(), ApiError> {
        #[derive(Debug, FromRow)]
        struct TableInfoRow {
            name: String,
        }

        let info: Vec<TableInfoRow> = sqlx::query_as::<_, TableInfoRow>("PRAGMA table_info(Grade)")
            .fetch_all(pool)
            .await
            .map_err(|e| err_internal(&format!("schema check failed: {e}")))?;

        // If the table doesn't exist yet, PRAGMA returns empty. 002_grades.sql should have
        // created it, but treat this as non-fatal.
        if info.is_empty() {
            return Ok(());
        }

        let has_run_id = info.iter().any(|r| r.name == "runId");
        let has_dimension = info.iter().any(|r| r.name == "dimension");
        if has_run_id && has_dimension {
            return Ok(());
        }

        // Legacy Grade schema detected. Rebuild to the new append-only schema.
        // This intentionally drops any existing Grade data because it cannot be mapped
        // losslessly to (runId, dimension) evidence rows.
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| err_internal(&format!("begin tx error: {e}")))?;

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

        tx.commit()
            .await
            .map_err(|e| err_internal(&format!("commit tx error: {e}")))?;

        Ok(())
    }

    async fn upgrade_legacy_indicator_schema(pool: &SqlitePool) -> Result<(), ApiError> {
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

        let mut tx = pool
            .begin()
            .await
            .map_err(|e| err_internal(&format!("begin tx error: {e}")))?;

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

            sqlx::query(
                "CREATE INDEX IF NOT EXISTS idx_opportunity_status ON Opportunity(status);",
            )
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
            // Nothing to do. Leave any existing kpiId values intact.
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

            sqlx::query(
                "CREATE INDEX IF NOT EXISTS idx_regression_repoName ON Regression(repoName);",
            )
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
    kpi_id: Option<String>,
    delta: f64,
    severity: String,
    since: String,
    trend: String,
}

#[derive(Debug, FromRow)]
struct KpiRow {
    id: String,
    name: String,
    description: String,
    scope_level: String,
    threshold: f64,
    weight: f64,
    agg_fn: String,
    created_at: String,
    updated_at: String,
}

impl KpiRow {
    fn into_item(self) -> KpiItem {
        KpiItem {
            id: self.id,
            name: self.name,
            description: self.description,
            scope_level: self.scope_level,
            threshold: self.threshold,
            weight: self.weight,
            agg_fn: self.agg_fn,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, FromRow)]
struct EvalRunRow {
    id: String,
    source: String,
    scope: String,
    scope_id: String,
    score: Option<f64>,
    verdict: Option<String>,
    summary: Option<String>,
    evaluated_at: String,
    ingested_at: String,
}

impl EvalRunRow {
    fn into_item(self) -> EvalRunItem {
        EvalRunItem {
            id: self.id,
            source: self.source,
            scope: self.scope,
            scope_id: self.scope_id,
            score: self.score,
            verdict: self.verdict,
            summary: self.summary,
            evaluated_at: self.evaluated_at,
            ingested_at: self.ingested_at,
        }
    }
}

#[derive(Debug, FromRow)]
struct GradeRow {
    id: String,
    run_id: String,
    kpi_id: Option<String>,
    dimension: String,
    score: f64,
    evidence: Option<String>,
    evaluated_at: String,
    ingested_at: String,
}

impl GradeRow {
    fn into_item(self) -> GradeItem {
        GradeItem {
            id: self.id,
            run_id: self.run_id,
            kpi_id: self.kpi_id,
            dimension: self.dimension,
            score: self.score,
            evidence: self.evidence.and_then(|s| serde_json::from_str(&s).ok()),
            evaluated_at: self.evaluated_at,
            ingested_at: self.ingested_at,
            warnings: vec![],
        }
    }
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
struct ModuleRow {
    id: String,
    name: String,
    kind: String,
    repo_id: String,
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
    kpi_id: Option<String>,
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

// ── Workflow runtime row types ───────────────────────────────────────────────

#[derive(Debug, FromRow)]
struct WorkflowInstanceRow {
    #[sqlx(rename = "instanceId")]
    instance_id: String,
    #[sqlx(rename = "workflowId")]
    workflow_id: String,
    status: String,
    #[sqlx(rename = "linkedPlanId")]
    linked_plan_id: Option<String>,
    #[sqlx(rename = "startedAt")]
    started_at: String,
    #[sqlx(rename = "endedAt")]
    ended_at: Option<String>,
    definition: Option<String>,
}

#[derive(Debug, FromRow)]
struct NodeStateRow {
    #[allow(dead_code)]
    #[sqlx(rename = "instanceId")]
    instance_id: String,
    #[sqlx(rename = "nodeId")]
    node_id: String,
    status: String,
    #[sqlx(rename = "startedAt")]
    started_at: Option<String>,
    #[sqlx(rename = "endedAt")]
    ended_at: Option<String>,
    #[sqlx(rename = "operatorType")]
    operator_type: Option<String>,
}

#[derive(Debug, FromRow)]
struct HilEventRow {
    #[sqlx(rename = "eventId")]
    event_id: String,
    #[sqlx(rename = "instanceId")]
    instance_id: String,
    #[sqlx(rename = "nodeId")]
    node_id: Option<String>,
    channel: String,
    #[sqlx(rename = "eventType")]
    event_type: String,
    question: String,
    choices: String,
    #[sqlx(rename = "timeoutSeconds")]
    timeout_seconds: Option<i64>,
    #[sqlx(rename = "correlationId")]
    correlation_id_str: Option<String>,
    status: String,
    timestamp: String,
}

#[derive(Debug, FromRow)]
struct WorkflowLogRow {
    #[allow(dead_code)]
    seq: i64,
    #[sqlx(rename = "instanceId")]
    instance_id: String,
    #[sqlx(rename = "nodeId")]
    node_id: String,
    ts: String,
    level: String,
    message: String,
}

#[derive(Debug, FromRow)]
struct InstanceIdRow {
    #[sqlx(rename = "instanceId")]
    instance_id: String,
}

// ── Workflow runtime conversion helpers ──────────────────────────────────────

fn parse_dt(s: &str) -> Result<DateTime<Utc>, ApiError> {
    s.parse::<DateTime<Utc>>()
        .map_err(|_| err_internal(&format!("invalid datetime: {s}")))
}

fn parse_opt_dt(s: Option<&str>) -> Result<Option<DateTime<Utc>>, ApiError> {
    match s {
        None => Ok(None),
        Some(v) => Ok(Some(parse_dt(v)?)),
    }
}

fn parse_workflow_status(s: &str) -> WorkflowStatus {
    match s {
        "running" => WorkflowStatus::Running,
        "succeeded" => WorkflowStatus::Succeeded,
        "failed" => WorkflowStatus::Failed,
        "paused" => WorkflowStatus::Paused,
        "cancelled" => WorkflowStatus::Cancelled,
        _ => WorkflowStatus::Running,
    }
}

fn workflow_status_str(s: &WorkflowStatus) -> &'static str {
    match s {
        WorkflowStatus::Running => "running",
        WorkflowStatus::Succeeded => "succeeded",
        WorkflowStatus::Failed => "failed",
        WorkflowStatus::Paused => "paused",
        WorkflowStatus::Cancelled => "cancelled",
    }
}

fn parse_node_status(s: &str) -> NodeStatus {
    match s {
        "pending" => NodeStatus::Pending,
        "running" => NodeStatus::Running,
        "succeeded" => NodeStatus::Succeeded,
        "failed" => NodeStatus::Failed,
        "timeout" => NodeStatus::Timeout,
        "cancelled" => NodeStatus::Cancelled,
        _ => NodeStatus::Pending,
    }
}

fn node_status_str(s: &NodeStatus) -> &'static str {
    match s {
        NodeStatus::Pending => "pending",
        NodeStatus::Running => "running",
        NodeStatus::Succeeded => "succeeded",
        NodeStatus::Failed => "failed",
        NodeStatus::Timeout => "timeout",
        NodeStatus::Cancelled => "cancelled",
    }
}

fn parse_hil_event_type(s: &str) -> HilEventType {
    match s {
        "authorization" => HilEventType::Authorization,
        _ => HilEventType::Question,
    }
}

fn hil_event_type_str(t: &HilEventType) -> &'static str {
    match t {
        HilEventType::Question => "question",
        HilEventType::Authorization => "authorization",
    }
}

fn parse_hil_status(s: &str) -> HilStatus {
    match s {
        "resolved" => HilStatus::Resolved,
        "timed_out" => HilStatus::TimedOut,
        "cancelled" => HilStatus::Cancelled,
        _ => HilStatus::Pending,
    }
}

fn hil_status_str(s: &HilStatus) -> &'static str {
    match s {
        HilStatus::Pending => "pending",
        HilStatus::Resolved => "resolved",
        HilStatus::TimedOut => "timed_out",
        HilStatus::Cancelled => "cancelled",
    }
}

fn wi_row_to_instance(
    row: WorkflowInstanceRow,
    nodes: Vec<newton_types::NodeState>,
) -> Result<newton_types::WorkflowInstance, ApiError> {
    Ok(newton_types::WorkflowInstance {
        instance_id: row.instance_id,
        workflow_id: row.workflow_id,
        status: parse_workflow_status(&row.status),
        linked_plan_id: row.linked_plan_id,
        started_at: parse_dt(&row.started_at)?,
        ended_at: parse_opt_dt(row.ended_at.as_deref())?,
        definition: row
            .definition
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| err_internal(&format!("definition json: {e}")))?,
        nodes,
    })
}

fn row_to_node_state(row: NodeStateRow) -> Result<newton_types::NodeState, ApiError> {
    Ok(newton_types::NodeState {
        node_id: row.node_id,
        status: parse_node_status(&row.status),
        started_at: parse_opt_dt(row.started_at.as_deref())?,
        ended_at: parse_opt_dt(row.ended_at.as_deref())?,
        operator_type: row.operator_type,
    })
}

fn row_to_hil_event(row: HilEventRow) -> Result<newton_types::HilEvent, ApiError> {
    let choices: Vec<String> = serde_json::from_str(&row.choices)
        .map_err(|e| err_internal(&format!("choices json: {e}")))?;
    let correlation_id = row
        .correlation_id_str
        .as_deref()
        .map(|s| Uuid::parse_str(s).map_err(|_| err_internal(&format!("invalid uuid: {s}"))))
        .transpose()?;
    Ok(newton_types::HilEvent {
        event_id: row.event_id,
        instance_id: row.instance_id,
        node_id: row.node_id,
        channel: row.channel,
        event_type: parse_hil_event_type(&row.event_type),
        question: row.question,
        choices,
        timeout_seconds: row.timeout_seconds.map(|v| v as u64),
        correlation_id,
        status: parse_hil_status(&row.status),
        timestamp: parse_dt(&row.timestamp)?,
    })
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

    async fn list_kpis(&self) -> Result<Vec<KpiItem>, ApiError> {
        let rows = sqlx::query_as::<_, KpiRow>(
            "SELECT id, name, description, scopeLevel AS scope_level, threshold, weight, aggFn AS agg_fn, createdAt AS created_at, updatedAt AS updated_at \
             FROM KPI ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        Ok(rows.into_iter().map(|r| r.into_item()).collect())
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
                "SELECT id, title, origin, componentId as component_id, module, repoId as repo_id, kpiId as kpi_id, confidence, risk, expectedValue as expected_value, effort, status, age, rationale, dependsOn as depends_on, blocks FROM Opportunity WHERE status = ? ORDER BY id ASC"
            ).bind(s)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("query error: {e}")))?
        } else {
            sqlx::query_as::<_, OpportunityRow>(
                "SELECT id, title, origin, componentId as component_id, module, repoId as repo_id, kpiId as kpi_id, confidence, risk, expectedValue as expected_value, effort, status, age, rationale, dependsOn as depends_on, blocks FROM Opportunity ORDER BY id ASC"
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

    async fn create_opportunity(
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

        self.list_opportunities(None)
            .await?
            .into_iter()
            .find(|o| o.id == body.id)
            .ok_or_else(|| err_internal("Failed to read back created opportunity"))
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

    async fn get_product(&self, id: &str) -> Result<ProductItem, ApiError> {
        self.fetch_product_item(id).await
    }

    async fn create_product(&self, body: CreateProductBody) -> Result<ProductItem, ApiError> {
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

    async fn put_product(&self, id: &str, body: PutProductBody) -> Result<ProductItem, ApiError> {
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

    async fn patch_product(
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

    async fn delete_product(&self, id: &str) -> Result<String, ApiError> {
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

    async fn get_component(&self, id: &str) -> Result<ComponentItem, ApiError> {
        self.fetch_component_item(id).await
    }

    async fn create_component(&self, body: CreateComponentBody) -> Result<ComponentItem, ApiError> {
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
            "INSERT INTO Component (id, name, domain, repos, modules, health, trend, owner, criticality, autonomy, openPlans, openRequests, lastEval, productId, createdAt, updatedAt) VALUES (?, ?, ?, 0, 0, ?, ?, ?, ?, ?, 0, 0, ?, ?, ?, ?)"
        )
        .bind(&id).bind(&body.name).bind(&body.domain)
        .bind(body.health).bind(body.trend)
        .bind(&body.owner).bind(&body.criticality).bind(&body.autonomy)
        .bind(&body.last_eval).bind(&body.product_id)
        .bind(&now).bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("insert error: {e}")))?;
        self.fetch_component_item(&id).await
    }

    async fn put_component(
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
            "UPDATE Component SET name = ?, domain = ?, repos = 0, modules = 0, health = ?, trend = ?, owner = ?, criticality = ?, autonomy = ?, openPlans = 0, openRequests = 0, lastEval = ?, productId = ?, updatedAt = ? WHERE id = ?"
        )
        .bind(&body.name).bind(&body.domain)
        .bind(body.health).bind(body.trend)
        .bind(&body.owner).bind(&body.criticality).bind(&body.autonomy)
        .bind(&body.last_eval).bind(&body.product_id)
        .bind(&now).bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("update error: {e}")))?;
        self.fetch_component_item(id).await
    }

    async fn patch_component(
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
        let health = body.health.unwrap_or(existing.health);
        let trend = body.trend.unwrap_or(existing.trend);
        let last_eval = body.last_eval.unwrap_or(existing.last_eval);
        let now = Self::now_iso();
        sqlx::query(
            "UPDATE Component SET name = ?, domain = ?, health = ?, trend = ?, owner = ?, criticality = ?, autonomy = ?, lastEval = ?, productId = ?, updatedAt = ? WHERE id = ?"
        )
        .bind(&name).bind(&domain).bind(health).bind(trend)
        .bind(&owner).bind(&criticality).bind(&autonomy)
        .bind(&last_eval).bind(&product_id)
        .bind(&now).bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("update error: {e}")))?;
        self.fetch_component_item(id).await
    }

    async fn delete_component(&self, id: &str) -> Result<String, ApiError> {
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

    async fn get_repo(&self, id: &str) -> Result<RepoItem, ApiError> {
        self.fetch_repo_item(id).await
    }

    async fn create_repo(&self, body: CreateRepoBody) -> Result<RepoItem, ApiError> {
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
            "INSERT INTO Repo (id, name, componentId, owner, criticality, autonomy, qualityScore, regressions, openPlans, execStatus, lastEval, coverage, secScore, createdAt, updatedAt) VALUES (?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&id).bind(&body.name).bind(&body.component_id)
        .bind(&body.owner).bind(&body.criticality).bind(&body.autonomy)
        .bind(body.quality_score).bind(&body.exec_status)
        .bind(&body.last_eval).bind(body.coverage).bind(body.sec_score)
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

    async fn put_repo(&self, id: &str, body: PutRepoBody) -> Result<RepoItem, ApiError> {
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
            "UPDATE Repo SET name = ?, componentId = ?, owner = ?, criticality = ?, autonomy = ?, qualityScore = ?, regressions = 0, openPlans = 0, execStatus = ?, lastEval = ?, coverage = ?, secScore = ?, updatedAt = ? WHERE id = ?"
        )
        .bind(&body.name).bind(&body.component_id)
        .bind(&body.owner).bind(&body.criticality).bind(&body.autonomy)
        .bind(body.quality_score).bind(&body.exec_status)
        .bind(&body.last_eval).bind(body.coverage).bind(body.sec_score)
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

    async fn patch_repo(&self, id: &str, body: PatchRepoBody) -> Result<RepoItem, ApiError> {
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
        // Get current component_id from DB since RepoItem.component is the name, not id
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
        let quality_score = body.quality_score.unwrap_or(existing.quality_score);
        let coverage = body.coverage.unwrap_or(existing.coverage);
        let sec_score = body.sec_score.unwrap_or(existing.sec_score);
        let exec_status = body.exec_status.unwrap_or(existing.exec_status);
        let last_eval = body.last_eval.unwrap_or(existing.last_eval);
        let now = Self::now_iso();
        sqlx::query(
            "UPDATE Repo SET name = ?, componentId = ?, owner = ?, criticality = ?, autonomy = ?, qualityScore = ?, execStatus = ?, lastEval = ?, coverage = ?, secScore = ?, updatedAt = ? WHERE id = ?"
        )
        .bind(&name).bind(&component_id)
        .bind(&owner).bind(&criticality).bind(&autonomy)
        .bind(quality_score).bind(&exec_status)
        .bind(&last_eval).bind(coverage).bind(sec_score)
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

    async fn delete_repo(&self, id: &str) -> Result<String, ApiError> {
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

    async fn list_modules(&self) -> Result<Vec<ModuleItem>, ApiError> {
        let rows = sqlx::query_as::<_, ModuleRow>(
            "SELECT id, name, kind, repoId as repo_id FROM Module ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(self.module_row_to_item(row).await?);
        }
        Ok(result)
    }

    async fn get_module(&self, id: &str) -> Result<ModuleItem, ApiError> {
        self.fetch_module_item(id).await
    }

    async fn create_module(&self, body: CreateModuleBody) -> Result<ModuleItem, ApiError> {
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

    async fn put_module(&self, id: &str, body: PutModuleBody) -> Result<ModuleItem, ApiError> {
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

    async fn patch_module(&self, id: &str, body: PatchModuleBody) -> Result<ModuleItem, ApiError> {
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

    async fn delete_module(&self, id: &str) -> Result<String, ApiError> {
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

    async fn get_module_dependency(&self, id: &str) -> Result<ModuleDependencyItem, ApiError> {
        self.fetch_module_dependency_item(id).await
    }

    async fn patch_module_dependency(
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

    async fn delete_module_dependency(&self, id: &str) -> Result<String, ApiError> {
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

    async fn get_kpi(&self, id: &str) -> Result<KpiItem, ApiError> {
        let row: Option<KpiRow> = sqlx::query_as::<_, KpiRow>(
            "SELECT id, name, description, scopeLevel AS scope_level, threshold, weight, aggFn AS agg_fn, createdAt AS created_at, updatedAt AS updated_at \
             FROM KPI WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;
        row.map(|r| r.into_item())
            .ok_or_else(|| err_not_found("KPI not found"))
    }

    async fn create_eval_run(&self, body: CreateEvalRunBody) -> Result<EvalRunItem, ApiError> {
        if body.id.trim().is_empty() {
            return Err(err_validation("id is required"));
        }
        if body.source.trim().is_empty() {
            return Err(err_validation("source is required"));
        }
        if body.scope_id.trim().is_empty() {
            return Err(err_validation("scopeId is required"));
        }
        let allowed_scopes = ["product", "component", "repo", "module"];
        if !allowed_scopes.contains(&body.scope.as_str()) {
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
            .begin()
            .await
            .map_err(|e| err_internal(&format!("begin tx error: {e}")))?;

        let scope_id_exists: bool = match body.scope.as_str() {
            "product" => {
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM Product WHERE id = ?")
                    .bind(&body.scope_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| err_internal(&format!("query error: {e}")))?
                    > 0
            }
            "component" => {
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM Component WHERE id = ?")
                    .bind(&body.scope_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| err_internal(&format!("query error: {e}")))?
                    > 0
            }
            "repo" => {
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM Repo WHERE id = ?")
                    .bind(&body.scope_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| err_internal(&format!("query error: {e}")))?
                    > 0
            }
            "module" => {
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM Module WHERE id = ?")
                    .bind(&body.scope_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| err_internal(&format!("query error: {e}")))?
                    > 0
            }
            _ => false,
        };
        if !scope_id_exists {
            return Err(err_not_found(&format!(
                "{} '{}' not found",
                body.scope, body.scope_id
            )));
        }

        sqlx::query(
            "INSERT INTO EvalRun (id, source, scope, scopeId, score, verdict, summary, evaluatedAt, ingestedAt) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if e.to_string().contains("UNIQUE constraint failed") {
                err_conflict("EvalRun id already exists")
            } else {
                err_internal(&format!("insert error: {e}"))
            }
        })?;

        tx.commit()
            .await
            .map_err(|e| err_internal(&format!("commit tx error: {e}")))?;

        self.get_eval_run(&body.id).await
    }

    async fn list_eval_runs(
        &self,
        scope: Option<String>,
        scope_id: Option<String>,
        source: Option<String>,
        limit: Option<u32>,
    ) -> Result<Vec<EvalRunItem>, ApiError> {
        let mut sql = String::from(
            "SELECT id, source, scope, scopeId AS scope_id, score, verdict, summary, evaluatedAt AS evaluated_at, ingestedAt AS ingested_at FROM EvalRun",
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

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("query error: {e}")))?;
        Ok(rows.into_iter().map(|r| r.into_item()).collect())
    }

    async fn get_eval_run(&self, id: &str) -> Result<EvalRunItem, ApiError> {
        let row: Option<EvalRunRow> = sqlx::query_as::<_, EvalRunRow>(
            "SELECT id, source, scope, scopeId AS scope_id, score, verdict, summary, evaluatedAt AS evaluated_at, ingestedAt AS ingested_at \
             FROM EvalRun WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;
        row.map(|r| r.into_item())
            .ok_or_else(|| err_not_found("EvalRun not found"))
    }

    async fn create_grade(&self, body: CreateGradeBody) -> Result<GradeItem, ApiError> {
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
            .begin()
            .await
            .map_err(|e| err_internal(&format!("begin tx error: {e}")))?;

        let run_exists: bool =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM EvalRun WHERE id = ?")
                .bind(&body.run_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?
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
                    .map_err(|e| err_internal(&format!("query error: {e}")))?
                    > 0;
            if !kpi_exists {
                return Err(err_not_found(&format!("KPI '{}' not found", kpi_id)));
            }
        }

        // Enforce append-only semantics at the application layer so we never overwrite
        // evidence for an existing (runId, dimension), regardless of SQLite conflict policy.
        let exists_for_dimension: bool = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM Grade WHERE runId = ? AND dimension = ?",
        )
        .bind(&body.run_id)
        .bind(&body.dimension)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?
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
        .map_err(|e| {
            if e.to_string().contains("UNIQUE constraint failed") {
                err_conflict("Grade already exists for (runId, dimension) or id")
            } else {
                err_internal(&format!("insert error: {e}"))
            }
        })?;

        tx.commit()
            .await
            .map_err(|e| err_internal(&format!("commit tx error: {e}")))?;

        self.get_grade(&body.id).await
    }

    async fn list_grades(
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

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("query error: {e}")))?;
        Ok(rows.into_iter().map(|r| r.into_item()).collect())
    }

    async fn get_grade(&self, id: &str) -> Result<GradeItem, ApiError> {
        let row: Option<GradeRow> = sqlx::query_as::<_, GradeRow>(
            "SELECT id, runId AS run_id, kpiId AS kpi_id, dimension, score, evidence, evaluatedAt AS evaluated_at, ingestedAt AS ingested_at \
             FROM Grade WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;
        row.map(|r| r.into_item())
            .ok_or_else(|| err_not_found("Grade not found"))
    }

    // ── Workflow runtime methods ───────────────────────────────────────────

    async fn get_workflow_instance(
        &self,
        instance_id: &str,
    ) -> Result<newton_types::WorkflowInstance, ApiError> {
        let row: Option<WorkflowInstanceRow> = sqlx::query_as::<_, WorkflowInstanceRow>(
            "SELECT instanceId, workflowId, status, linkedPlanId, startedAt, endedAt, definition FROM WorkflowInstance WHERE instanceId = ?"
        )
        .bind(instance_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("Workflow instance not found"))?;
        let nodes = self.list_node_states_for_instance(instance_id).await?;
        wi_row_to_instance(row, nodes)
    }

    async fn list_workflow_instances(
        &self,
        status: Option<newton_types::WorkflowStatus>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<newton_types::WorkflowInstance>, ApiError> {
        let rows: Vec<WorkflowInstanceRow> = match &status {
            Some(s) => {
                sqlx::query_as::<_, WorkflowInstanceRow>(
                    "SELECT instanceId, workflowId, status, linkedPlanId, startedAt, endedAt, definition FROM WorkflowInstance WHERE status = ? ORDER BY startedAt DESC LIMIT ? OFFSET ?"
                )
                .bind(workflow_status_str(s))
                .bind(limit.unwrap_or(100) as i64)
                .bind(offset.unwrap_or(0) as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?
            }
            None => {
                sqlx::query_as::<_, WorkflowInstanceRow>(
                    "SELECT instanceId, workflowId, status, linkedPlanId, startedAt, endedAt, definition FROM WorkflowInstance ORDER BY startedAt DESC LIMIT ? OFFSET ?"
                )
                .bind(limit.unwrap_or(100) as i64)
                .bind(offset.unwrap_or(0) as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?
            }
        };

        let mut instances = Vec::with_capacity(rows.len());
        for row in rows {
            let id = row.instance_id.clone();
            let nodes = self.list_node_states_for_instance(&id).await?;
            instances.push(wi_row_to_instance(row, nodes)?);
        }
        Ok(instances)
    }

    async fn upsert_workflow_instance(
        &self,
        instance: &newton_types::WorkflowInstance,
    ) -> Result<(), ApiError> {
        let now = Utc::now().to_rfc3339();
        let definition_json = instance
            .definition
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| err_internal(&format!("definition serialize: {e}")))?;

        sqlx::query(
            "INSERT INTO WorkflowInstance (instanceId, workflowId, status, linkedPlanId, startedAt, endedAt, definition, createdAt, updatedAt)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(instanceId) DO UPDATE SET
               workflowId = excluded.workflowId,
               status = excluded.status,
               linkedPlanId = excluded.linkedPlanId,
               startedAt = excluded.startedAt,
               endedAt = excluded.endedAt,
               definition = excluded.definition,
               updatedAt = excluded.updatedAt"
        )
        .bind(&instance.instance_id)
        .bind(&instance.workflow_id)
        .bind(workflow_status_str(&instance.status))
        .bind(&instance.linked_plan_id)
        .bind(instance.started_at.to_rfc3339())
        .bind(instance.ended_at.map(|dt| dt.to_rfc3339()))
        .bind(definition_json)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("upsert error: {e}")))?;

        Ok(())
    }

    async fn delete_workflow_instance(&self, instance_id: &str) -> Result<(), ApiError> {
        let affected = sqlx::query("DELETE FROM WorkflowInstance WHERE instanceId = ?")
            .bind(instance_id)
            .execute(&self.pool)
            .await
            .map_err(|e| err_internal(&format!("delete error: {e}")))?;
        if affected.rows_affected() == 0 {
            return Err(err_not_found("Workflow instance not found"));
        }
        Ok(())
    }

    async fn get_node_state(
        &self,
        instance_id: &str,
        node_id: &str,
    ) -> Result<newton_types::NodeState, ApiError> {
        let row: Option<NodeStateRow> = sqlx::query_as::<_, NodeStateRow>(
            "SELECT instanceId, nodeId, status, startedAt, endedAt, operatorType FROM NodeState WHERE instanceId = ? AND nodeId = ?"
        )
        .bind(instance_id)
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("Node state not found"))?;
        row_to_node_state(row)
    }

    async fn list_node_states_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<Vec<newton_types::NodeState>, ApiError> {
        let rows: Vec<NodeStateRow> = sqlx::query_as::<_, NodeStateRow>(
            "SELECT instanceId, nodeId, status, startedAt, endedAt, operatorType FROM NodeState WHERE instanceId = ? ORDER BY rowid ASC"
        )
        .bind(instance_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        rows.into_iter().map(row_to_node_state).collect()
    }

    async fn upsert_node_state(
        &self,
        instance_id: &str,
        node: &newton_types::NodeState,
    ) -> Result<(), ApiError> {
        let id = format!("{}-{}", instance_id, node.node_id);
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO NodeState (id, instanceId, nodeId, status, startedAt, endedAt, operatorType)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(instanceId, nodeId) DO UPDATE SET
               status = excluded.status,
               startedAt = excluded.startedAt,
               endedAt = excluded.endedAt,
               operatorType = excluded.operatorType"
        )
        .bind(&id)
        .bind(instance_id)
        .bind(&node.node_id)
        .bind(node_status_str(&node.status))
        .bind(node.started_at.map(|dt| dt.to_rfc3339()))
        .bind(node.ended_at.map(|dt| dt.to_rfc3339()))
        .bind(&node.operator_type)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("upsert node state error: {e}")))?;

        let _ = now;
        Ok(())
    }

    async fn update_workflow_status(
        &self,
        instance_id: &str,
        status: newton_types::WorkflowStatus,
        ended_at: DateTime<Utc>,
    ) -> Result<(), ApiError> {
        let now = Self::now_iso();
        let affected = sqlx::query(
            "UPDATE WorkflowInstance SET status = ?, endedAt = ?, updatedAt = ? WHERE instanceId = ?"
        )
        .bind(workflow_status_str(&status))
        .bind(ended_at.to_rfc3339())
        .bind(&now)
        .bind(instance_id)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("update_workflow_status error: {e}")))?;

        if affected.rows_affected() == 0 {
            return Err(err_not_found("Workflow instance not found"));
        }
        Ok(())
    }

    async fn get_hil_event(&self, event_id: &str) -> Result<newton_types::HilEvent, ApiError> {
        let row: Option<HilEventRow> = sqlx::query_as::<_, HilEventRow>(
            "SELECT eventId, instanceId, nodeId, channel, eventType, question, choices, timeoutSeconds, correlationId, status, timestamp FROM HilEvent WHERE eventId = ?"
        )
        .bind(event_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("HIL event not found"))?;
        row_to_hil_event(row)
    }

    async fn list_hil_events_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<Vec<newton_types::HilEvent>, ApiError> {
        let rows: Vec<HilEventRow> = sqlx::query_as::<_, HilEventRow>(
            "SELECT eventId, instanceId, nodeId, channel, eventType, question, choices, timeoutSeconds, correlationId, status, timestamp FROM HilEvent WHERE instanceId = ? ORDER BY rowid ASC"
        )
        .bind(instance_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        rows.into_iter().map(row_to_hil_event).collect()
    }

    async fn list_hil_instances(&self) -> Result<Vec<String>, ApiError> {
        let rows: Vec<InstanceIdRow> = sqlx::query_as::<_, InstanceIdRow>(
            "SELECT DISTINCT instanceId FROM HilEvent ORDER BY instanceId ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        Ok(rows.into_iter().map(|r| r.instance_id).collect())
    }

    async fn insert_hil_event(&self, event: &newton_types::HilEvent) -> Result<(), ApiError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let choices_json = serde_json::to_string(&event.choices)
            .map_err(|e| err_internal(&format!("choices serialize: {e}")))?;

        sqlx::query(
            "INSERT INTO HilEvent (id, eventId, instanceId, nodeId, channel, eventType, question, choices, timeoutSeconds, correlationId, status, timestamp, createdAt, updatedAt)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&id)
        .bind(&event.event_id)
        .bind(&event.instance_id)
        .bind(&event.node_id)
        .bind(&event.channel)
        .bind(hil_event_type_str(&event.event_type))
        .bind(&event.question)
        .bind(&choices_json)
        .bind(event.timeout_seconds.map(|v| v as i64))
        .bind(event.correlation_id.map(|u| u.to_string()))
        .bind(hil_status_str(&event.status))
        .bind(event.timestamp.to_rfc3339())
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("insert HIL event error: {e}")))?;

        Ok(())
    }

    async fn update_hil_event_status(
        &self,
        event_id: &str,
        status: newton_types::HilStatus,
    ) -> Result<newton_types::HilEvent, ApiError> {
        let now = Utc::now().to_rfc3339();
        let affected =
            sqlx::query("UPDATE HilEvent SET status = ?, updatedAt = ? WHERE eventId = ?")
                .bind(hil_status_str(&status))
                .bind(&now)
                .bind(event_id)
                .execute(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("update error: {e}")))?;

        if affected.rows_affected() == 0 {
            return Err(err_not_found("HIL event not found"));
        }
        self.get_hil_event(event_id).await
    }

    async fn append_log_line(
        &self,
        instance_id: &str,
        node_id: &str,
        line: &newton_types::LogLine,
    ) -> Result<(), ApiError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| err_internal(&format!("begin tx error: {e}")))?;

        sqlx::query(
            "INSERT INTO WorkflowLog (instanceId, nodeId, seq, ts, level, message)
             VALUES (?, ?, COALESCE((SELECT MAX(seq) FROM WorkflowLog WHERE instanceId = ? AND nodeId = ?), 0) + 1, ?, ?, ?)"
        )
        .bind(instance_id)
        .bind(node_id)
        .bind(instance_id)
        .bind(node_id)
        .bind(line.timestamp.to_rfc3339())
        .bind(&line.level)
        .bind(&line.message)
        .execute(&mut *tx)
        .await
        .map_err(|e| err_internal(&format!("append log line error: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| err_internal(&format!("commit tx error: {e}")))?;

        Ok(())
    }

    async fn list_log_lines(
        &self,
        instance_id: &str,
        node_id: &str,
        since_seq: i64,
    ) -> Result<Vec<newton_types::LogLine>, ApiError> {
        let rows: Vec<WorkflowLogRow> = sqlx::query_as::<_, WorkflowLogRow>(
            "SELECT seq, instanceId, nodeId, ts, level, message FROM WorkflowLog WHERE instanceId = ? AND nodeId = ? AND seq > ? ORDER BY seq ASC"
        )
        .bind(instance_id)
        .bind(node_id)
        .bind(since_seq)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        rows.into_iter()
            .map(|r| {
                Ok(newton_types::LogLine {
                    instance_id: r.instance_id,
                    node_id: r.node_id,
                    level: r.level,
                    message: r.message,
                    timestamp: parse_dt(&r.ts)?,
                })
            })
            .collect()
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

    async fn fetch_module_item(&self, id: &str) -> Result<ModuleItem, ApiError> {
        let row: Option<ModuleRow> = sqlx::query_as::<_, ModuleRow>(
            "SELECT id, name, kind, repoId as repo_id FROM Module WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("Module not found"))?;
        self.module_row_to_item(row).await
    }

    async fn module_row_to_item(&self, row: ModuleRow) -> Result<ModuleItem, ApiError> {
        let repo_name = get_repo_name(&self.pool, &row.repo_id).await?;
        let component_id: Option<ComponentIdRow> = sqlx::query_as::<_, ComponentIdRow>(
            "SELECT componentId as component_id FROM Repo WHERE id = ?",
        )
        .bind(&row.repo_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;
        let component_id_str = component_id
            .and_then(|c| c.component_id)
            .unwrap_or_default();
        let component_name = if component_id_str.is_empty() {
            String::new()
        } else {
            let cn: Option<NameRow> =
                sqlx::query_as::<_, NameRow>("SELECT name FROM Component WHERE id = ?")
                    .bind(&component_id_str)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| err_internal(&format!("query error: {e}")))?;
            cn.map(|c| c.name).unwrap_or_default()
        };
        Ok(ModuleItem {
            id: row.id,
            name: row.name,
            kind: row.kind,
            repo_id: row.repo_id,
            repo_name,
            component_id: component_id_str,
            component_name,
        })
    }

    async fn fetch_product_item(&self, id: &str) -> Result<ProductItem, ApiError> {
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

    async fn fetch_component_item(&self, id: &str) -> Result<ComponentItem, ApiError> {
        let row: Option<ComponentRow> = sqlx::query_as::<_, ComponentRow>(
            "SELECT id, name, domain, repos, modules, health, trend, owner, criticality, autonomy, openPlans as open_plans, openRequests as open_requests, lastEval as last_eval, productId as product_id FROM Component WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("Component not found"))?;
        let product_name: Option<NameRow> =
            sqlx::query_as::<_, NameRow>("SELECT name FROM Product WHERE id = ?")
                .bind(&row.product_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        Ok(ComponentItem {
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
        })
    }

    async fn fetch_repo_item(&self, id: &str) -> Result<RepoItem, ApiError> {
        let row: Option<RepoRow> = sqlx::query_as::<_, RepoRow>(
            "SELECT r.id, r.name, r.componentId as component_id, r.owner, r.criticality, r.autonomy, r.qualityScore as quality_score, r.regressions, r.openPlans as open_plans, r.execStatus as exec_status, r.lastEval as last_eval, r.coverage, r.secScore as sec_score FROM Repo r WHERE r.id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| err_internal(&format!("query error: {e}")))?;

        let row = row.ok_or_else(|| err_not_found("Repo not found"))?;
        let component: Option<NameRow> =
            sqlx::query_as::<_, NameRow>("SELECT name FROM Component WHERE id = ?")
                .bind(&row.component_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| err_internal(&format!("query error: {e}")))?;
        let depends_on = compute_repo_depends_on(&self.pool, &row.name).await?;
        let depended_on_by = compute_repo_depended_on_by(&self.pool, &row.name).await?;
        Ok(RepoItem {
            id: row.id,
            name: row.name,
            component: component.map(|c| c.name).unwrap_or_default(),
            owner: row.owner,
            criticality: row.criticality,
            autonomy: row.autonomy,
            quality_score: row.quality_score,
            regressions: row.regressions,
            open_plans: row.open_plans,
            exec_status: row.exec_status,
            last_eval: row.last_eval,
            coverage: row.coverage,
            sec_score: row.sec_score,
            depends_on,
            depended_on_by,
        })
    }

    async fn fetch_module_dependency_item(
        &self,
        id: &str,
    ) -> Result<ModuleDependencyItem, ApiError> {
        self.list_module_dependencies()
            .await?
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| err_not_found("ModuleDependency not found"))
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
        // First upsert
        store.upsert_node_state("inst-2", &node).await.unwrap();
        // Second upsert with different status — should update, not duplicate
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

        // since_seq=0 returns all 10
        let all = store.list_log_lines("inst-5", "node-x", 0).await.unwrap();
        assert_eq!(all.len(), 10);

        // since_seq=5 returns lines with seq > 5 (i.e., seq 6–10 → 5 lines)
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
                health: 0,
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
                quality_score: 0,
                coverage: 0,
                sec_score: 0,
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

    /// Verify that SQLite foreign-key enforcement is actually ON at the
    /// connection level. SQLite defaults foreign_keys=OFF per connection,
    /// so a missing `.foreign_keys(true)` on SqliteConnectOptions silently
    /// turns every FK declaration and ON DELETE CASCADE clause into a
    /// no-op. This test exercises the connection directly to defend
    /// against that regression — handler-level "not found" checks would
    /// mask it.
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
        // Bypass app-level validation entirely — go straight to SQL.
        // Module references Repo(id), so a non-existent repoId must fail.
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
mod opportunity_tests {
    use super::*;
    use crate::models::CreateOpportunityBody;

    fn make_opportunity(id: &str) -> CreateOpportunityBody {
        CreateOpportunityBody {
            id: id.to_string(),
            title: "Test opportunity".to_string(),
            origin: "test".to_string(),
            component: None,
            module: None,
            repo: None,
            kpi_id: None,
            confidence: None,
            risk: "low".to_string(),
            expected_value: 1.0,
            effort: None,
            status: "awaiting_triage".to_string(),
            rationale: None,
            depends_on: vec![],
            blocks: vec![],
        }
    }

    #[tokio::test]
    async fn create_opportunity_happy_path() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let body = make_opportunity("opp-001");
        let item = store.create_opportunity(body).await.unwrap();
        assert_eq!(item.id, "opp-001");
        assert_eq!(item.title, "Test opportunity");
        assert_eq!(item.origin, "test");
        assert_eq!(item.risk, "low");
        assert_eq!(item.status, "awaiting_triage");
    }

    #[tokio::test]
    async fn create_opportunity_duplicate_upsert_preserves_created_at() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let body1 = make_opportunity("opp-002");
        store.create_opportunity(body1).await.unwrap();

        let created_at_1: (String,) =
            sqlx::query_as("SELECT createdAt FROM Opportunity WHERE id = ?")
                .bind("opp-002")
                .fetch_one(&store.pool)
                .await
                .unwrap();

        let mut body2 = make_opportunity("opp-002");
        body2.title = "Updated title".to_string();
        let item2 = store.create_opportunity(body2).await.unwrap();

        assert_eq!(item2.id, "opp-002");
        assert_eq!(item2.title, "Updated title");

        let created_at_2: (String,) =
            sqlx::query_as("SELECT createdAt FROM Opportunity WHERE id = ?")
                .bind("opp-002")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(
            created_at_1.0, created_at_2.0,
            "createdAt must not change on upsert"
        );

        let all = store.list_opportunities(None).await.unwrap();
        let count = all.iter().filter(|o| o.id == "opp-002").count();
        assert_eq!(count, 1, "duplicate upsert must not create a second record");
    }

    #[tokio::test]
    async fn create_opportunity_invalid_status_returns_validation_error() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let mut body = make_opportunity("opp-003");
        body.status = "not-a-valid-status".to_string();
        let err = store.create_opportunity(body).await.unwrap_err();
        assert_eq!(err.code, "ERR_VALIDATION");
    }

    #[tokio::test]
    async fn create_opportunity_confidence_above_one_returns_validation_error() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let mut body = make_opportunity("opp-004");
        body.confidence = Some(1.5);
        let err = store.create_opportunity(body).await.unwrap_err();
        assert_eq!(err.code, "ERR_VALIDATION");
    }

    #[tokio::test]
    async fn create_opportunity_negative_expected_value_returns_validation_error() {
        let store = SqliteBackendStore::new_in_memory().await.unwrap();
        let mut body = make_opportunity("opp-005");
        body.expected_value = -1.0;
        let err = store.create_opportunity(body).await.unwrap_err();
        assert_eq!(err.code, "ERR_VALIDATION");
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

        // Start from the current schema for all non-legacy tables so indexes/constraints match
        // what `SqliteBackendStore::new()` will run, then rewrite Opportunity/Regression and add
        // the legacy Indicator table + columns.
        sqlx::query(include_str!("../migrations/001_init.sql"))
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

        // Seed required FK rows + legacy data.
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

        let (opportunity_has_kpi,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pragma_table_info('Opportunity') WHERE name = 'kpiId'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        let (opportunity_has_indicator,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pragma_table_info('Opportunity') WHERE name = 'indicator'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(opportunity_has_kpi, 1, "Opportunity.kpiId must exist");
        assert_eq!(
            opportunity_has_indicator, 0,
            "Opportunity.indicator must be removed"
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

        let (opp_kpi_nulls,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM Opportunity WHERE kpiId IS NULL")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(opp_kpi_nulls, 1, "migrated Opportunity.kpiId must be NULL");

        let (reg_kpi_nulls,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM Regression WHERE kpiId IS NULL")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(reg_kpi_nulls, 1, "migrated Regression.kpiId must be NULL");

        let (preserved_component_id,): (Option<String>,) =
            sqlx::query_as("SELECT componentId FROM Opportunity WHERE id = ?")
                .bind("opp-legacy")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(
            preserved_component_id.as_deref(),
            Some("comp-legacy"),
            "non-legacy columns must be preserved"
        );
    }
}
