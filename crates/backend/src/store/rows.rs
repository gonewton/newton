use crate::models::*;

use sqlx::FromRow;

#[derive(Debug, FromRow)]
pub(super) struct ProductRow {
    pub id: String,
    pub name: String,
    pub component_count: i64,
}

#[derive(Debug, FromRow)]
pub(super) struct ComponentRow {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub repos: i64,
    pub modules: i64,
    pub trend: i64,
    pub owner: String,
    pub criticality: String,
    pub autonomy: String,
    pub open_plans: i64,
    pub open_requests: i64,
    pub last_eval: String,
    pub product_id: String,
    pub product_name: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, FromRow)]
pub(super) struct PendingApprovalRow {
    pub id: String,
    pub title: String,
    pub item_type: String,
    pub component_id: Option<String>,
    pub component_name: Option<String>,
    pub repo_name: Option<String>,
    pub risk: String,
    pub expected_value: String,
    pub waiting_since: String,
    pub reviewer: String,
    pub status: String,
    pub confidence: i64,
    pub agent_generated: bool,
}

#[derive(Debug, FromRow)]
pub(super) struct RegressionRow {
    pub repo: String,
    pub kpi_id: Option<String>,
    pub delta: f64,
    pub severity: String,
    pub since: String,
    pub trend: String,
}

#[derive(Debug, FromRow)]
pub(super) struct KpiRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub scope_level: String,
    pub threshold: f64,
    pub weight: f64,
    pub agg_fn: String,
    pub created_at: String,
    pub updated_at: String,
}

impl KpiRow {
    pub(super) fn into_item(self) -> KpiItem {
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
pub(super) struct EvalRunRow {
    pub id: String,
    pub source: String,
    pub scope: String,
    pub scope_id: String,
    pub score: Option<f64>,
    pub verdict: Option<String>,
    pub summary: Option<String>,
    pub evaluated_at: String,
    pub ingested_at: String,
    pub raw_assessment: Option<String>,
}

impl EvalRunRow {
    pub(super) fn into_item(self) -> EvalRunItem {
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
            raw_assessment: self.raw_assessment,
        }
    }
}

#[derive(Debug, FromRow)]
pub(super) struct GradeRow {
    pub id: String,
    pub run_id: String,
    pub kpi_id: Option<String>,
    pub dimension: String,
    pub score: f64,
    pub evidence: Option<String>,
    pub evaluated_at: String,
    pub ingested_at: String,
}

impl GradeRow {
    pub(super) fn into_item(self) -> GradeItem {
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
pub(super) struct RecentActionRow {
    pub time: String,
    pub action: String,
    pub subject: String,
    pub item_type: String,
}

#[allow(dead_code)]
#[derive(Debug, FromRow)]
pub(super) struct RepoRow {
    pub id: String,
    pub name: String,
    pub component_id: String,
    pub component_name: Option<String>,
    pub owner: String,
    pub criticality: String,
    pub autonomy: String,
    pub regressions: i64,
    pub open_plans: i64,
    pub exec_status: String,
    pub last_eval: String,
}

#[derive(Debug, FromRow)]
pub(super) struct RepoDepTargetRow {
    pub target_repo: String,
}

#[allow(dead_code)]
#[derive(Debug, FromRow)]
pub(super) struct ModuleDepRow {
    pub id: String,
    #[allow(dead_code)]
    pub from_module_id: String,
    #[allow(dead_code)]
    pub to_module_id: String,
    pub dep_type: String,
    pub label: String,
    pub from_module_name: String,
    pub from_module_kind: String,
    pub from_repo_id: String,
    pub from_repo_name: Option<String>,
    pub from_component_name: Option<String>,
    pub to_module_name: String,
    pub to_module_kind: String,
    pub to_repo_id: String,
    pub to_repo_name: Option<String>,
    pub to_component_name: Option<String>,
}

#[derive(Debug, FromRow)]
pub(super) struct ModuleRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub repo_id: String,
    pub repo_name: Option<String>,
    pub component_id: Option<String>,
    pub component_name: Option<String>,
}

#[derive(Debug, FromRow)]
pub(super) struct SavedViewRow {
    pub id: String,
    pub label: String,
    pub filters: Option<String>,
    pub sort: Option<String>,
    pub sort_dir: Option<String>,
}

#[derive(Debug, FromRow)]
pub(super) struct SavedViewKindRow {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub filters: Option<String>,
    pub sort: Option<String>,
    pub sort_dir: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, FromRow)]
pub(super) struct FindingRow {
    pub id: String,
    pub source: String,
    pub origin: String,
    pub component_id: Option<String>,
    pub component_name: Option<String>,
    pub module: Option<String>,
    pub repo_id: Option<String>,
    pub repo_name: Option<String>,
    pub kpi_id: Option<String>,
    pub dimension: String,
    pub location: Option<String>,
    pub fingerprint: String,
    pub title: String,
    pub why_it_matters: String,
    pub recommended_action: String,
    pub severity: String,
    pub risk: String,
    pub confidence: Option<f64>,
    pub evidence: Option<String>,
    pub expected_value: Option<f64>,
    pub effort: Option<String>,
    pub status: String,
    pub last_seen_at: String,
    pub depends_on: Option<String>,
    pub blocks: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[allow(dead_code)]
#[derive(Debug, FromRow)]
pub(super) struct ChangeRequestRow {
    pub id: String,
    pub title: String,
    pub body: Option<String>,
    pub origin: String,
    pub author: Option<String>,
    pub component_id: Option<String>,
    pub component_name: Option<String>,
    pub repo_id: Option<String>,
    pub repo_name: Option<String>,
    pub status: String,
    pub finding_ids: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, FromRow)]
pub(super) struct PlanRow {
    pub id: String,
    pub title: String,
    pub component_id: Option<String>,
    pub component_name: Option<String>,
    pub repo_id: Option<String>,
    pub repo_name: Option<String>,
    pub status: String,
    pub linked_change_request_id: Option<String>,
    pub confidence: i64,
    pub risk: String,
    pub expected_value: Option<String>,
    pub agent_generated: bool,
    pub waiting_since: Option<String>,
    pub created_at: String,
}

#[derive(Debug, FromRow)]
pub(super) struct PlanSectionRow {
    pub id: String,
    pub label: String,
    pub content: String,
}

#[derive(Debug, FromRow)]
pub(super) struct PlanPolicyCheckRow {
    pub rule: String,
    pub status: String,
    pub met: i64,
}

#[derive(Debug, FromRow)]
pub(super) struct PlanApproverRow {
    pub role: String,
    pub name: String,
    pub status: String,
}

#[allow(dead_code)]
#[derive(Debug, FromRow)]
pub(super) struct ExecutionRow {
    pub id: String,
    pub instance_id: Option<String>,
    pub plan_id: Option<String>,
    pub workflow_id: Option<String>,
    pub plan_title: Option<String>,
    pub repo_id: Option<String>,
    pub repo_name: Option<String>,
    pub component_id: Option<String>,
    pub component_name: Option<String>,
    pub stage: Option<String>,
    pub status: String,
    pub policy_level: Option<String>,
    pub started_by: Option<String>,
    pub waiting_on: Option<String>,
    pub test_result: Option<String>,
    pub pr_status: Option<String>,
    pub pr_link: Option<String>,
    pub deploy_status: Option<String>,
    pub created_at: String,
    pub started: Option<String>,
}

#[derive(Debug, FromRow)]
pub(super) struct OperatorRow {
    pub operator_type: String,
    pub description: String,
    pub params_schema: Option<String>,
    pub palette_label: Option<String>,
    pub palette_icon: Option<String>,
}

#[derive(Debug, FromRow)]
pub(super) struct DepEdge {
    pub from_id: String,
    pub to_id: String,
}

#[derive(Debug, FromRow)]
pub(super) struct IdRow {
    pub id: String,
}

#[derive(Debug, FromRow)]
pub(super) struct ComponentIdRow {
    pub component_id: Option<String>,
}

#[derive(Debug, FromRow)]
pub(super) struct StringValueRow {
    pub value: Option<String>,
}

#[derive(Debug, FromRow)]
pub(super) struct ExpectedDeltaRow {
    pub expected_delta: Option<String>,
}

#[derive(Debug, FromRow)]
pub(super) struct WorkflowInstanceRow {
    #[sqlx(rename = "instanceId")]
    pub instance_id: String,
    #[sqlx(rename = "workflowId")]
    pub workflow_id: String,
    pub status: String,
    #[sqlx(rename = "linkedPlanId")]
    pub linked_plan_id: Option<String>,
    #[sqlx(rename = "startedAt")]
    pub started_at: String,
    #[sqlx(rename = "endedAt")]
    pub ended_at: Option<String>,
    pub definition: Option<String>,
}

#[derive(Debug, FromRow)]
pub(super) struct NodeStateRow {
    #[allow(dead_code)]
    #[sqlx(rename = "instanceId")]
    pub instance_id: String,
    #[sqlx(rename = "nodeId")]
    pub node_id: String,
    pub status: String,
    #[sqlx(rename = "startedAt")]
    pub started_at: Option<String>,
    #[sqlx(rename = "endedAt")]
    pub ended_at: Option<String>,
    #[sqlx(rename = "operatorType")]
    pub operator_type: Option<String>,
}

#[derive(Debug, FromRow)]
pub(super) struct HilEventRow {
    #[sqlx(rename = "eventId")]
    pub event_id: String,
    #[sqlx(rename = "instanceId")]
    pub instance_id: String,
    #[sqlx(rename = "nodeId")]
    pub node_id: Option<String>,
    pub channel: String,
    #[sqlx(rename = "eventType")]
    pub event_type: String,
    pub question: String,
    pub choices: String,
    #[sqlx(rename = "timeoutSeconds")]
    pub timeout_seconds: Option<i64>,
    #[sqlx(rename = "correlationId")]
    pub correlation_id_str: Option<String>,
    pub status: String,
    pub timestamp: String,
}

#[derive(Debug, FromRow)]
pub(super) struct WorkflowLogRow {
    #[allow(dead_code)]
    pub seq: i64,
    #[sqlx(rename = "instanceId")]
    pub instance_id: String,
    #[sqlx(rename = "nodeId")]
    pub node_id: String,
    pub ts: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, FromRow)]
pub(super) struct InstanceIdRow {
    #[sqlx(rename = "instanceId")]
    pub instance_id: String,
}
