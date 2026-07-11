use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProductItem {
    pub id: String,
    pub name: String,
    pub component_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComponentItem {
    pub id: String,
    pub name: String,
    pub product_id: String,
    pub product_name: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PendingApprovalItem {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub product: String,
    pub repo: String,
    pub risk: String,
    pub expected_value: String,
    pub waiting_since: String,
    pub reviewer: String,
    pub status: String,
    pub confidence: i64,
    pub agent_generated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegressionItem {
    pub repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kpi_id: Option<String>,
    pub delta: f64,
    pub severity: String,
    pub since: String,
    pub trend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KpiItem {
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateKpiBody {
    pub id: String,
    pub name: String,
    pub description: String,
    pub scope_level: String,
    pub threshold: f64,
    pub weight: f64,
    pub agg_fn: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvalRunItem {
    pub id: String,
    pub source: String,
    pub scope: String,
    pub scope_id: String,
    pub score: Option<f64>,
    pub verdict: Option<String>,
    pub summary: Option<String>,
    pub evaluated_at: String,
    pub ingested_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_assessment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateGradeInlineBody {
    pub kpi_id: Option<String>,
    pub dimension: String,
    pub score: f64,
    pub evidence: Option<serde_json::Value>,
    pub evaluated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEvalRunBody {
    pub id: String,
    pub source: String,
    pub scope: String,
    pub scope_id: String,
    pub score: Option<f64>,
    pub verdict: Option<String>,
    pub summary: Option<String>,
    pub evaluated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grades: Option<Vec<CreateGradeInlineBody>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_assessment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GradeItem {
    pub id: String,
    pub run_id: String,
    pub kpi_id: Option<String>,
    pub dimension: String,
    pub score: f64,
    pub evidence: Option<serde_json::Value>,
    pub evaluated_at: String,
    pub ingested_at: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateGradeBody {
    pub id: String,
    pub run_id: String,
    pub kpi_id: Option<String>,
    pub dimension: String,
    pub score: f64,
    pub evidence: Option<serde_json::Value>,
    pub evaluated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RecentActionItem {
    pub time: String,
    pub action: String,
    pub subject: String,
    #[serde(rename = "type")]
    pub item_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RepoItem {
    pub id: String,
    pub name: String,
    pub component: String,
    pub owner: String,
    pub criticality: String,
    pub autonomy: String,
    pub regressions: i64,
    pub open_plans: i64,
    pub exec_status: String,
    pub last_eval: String,
    pub depends_on: Vec<String>,
    pub depended_on_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RepoDependencyItem {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub dep_type: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModuleRef {
    pub module: String,
    pub kind: String,
    pub repo: String,
    pub component: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModuleDependencyItem {
    pub id: String,
    pub from: ModuleRef,
    pub to: ModuleRef,
    #[serde(rename = "type")]
    pub dep_type: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateModuleDependencyBody {
    pub from_module_id: String,
    pub to_module_id: String,
    #[serde(rename = "type")]
    pub dep_type: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SavedViewItem {
    pub id: String,
    pub label: String,
    pub filters: Option<serde_json::Value>,
    pub sort: Option<String>,
    pub sort_dir: Option<String>,
}

fn default_finding_origin() -> String {
    "system".to_string()
}

fn default_finding_status() -> String {
    "awaiting_triage".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FindingItem {
    pub id: String,
    pub source: String,
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kpi_id: Option<String>,
    pub dimension: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<serde_json::Value>,
    pub fingerprint: String,
    pub title: String,
    pub why_it_matters: String,
    pub recommended_action: String,
    pub severity: String,
    pub risk: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    pub status: String,
    pub last_seen_at: String,
    pub depends_on: Vec<String>,
    pub blocks: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by_plan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_plan_attempts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_change_request_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateFindingBody {
    pub id: String,
    pub source: String,
    #[serde(default = "default_finding_origin")]
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kpi_id: Option<String>,
    pub dimension: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<serde_json::Value>,
    pub fingerprint: String,
    pub title: String,
    pub why_it_matters: String,
    pub recommended_action: String,
    pub severity: String,
    pub risk: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default = "default_finding_status")]
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub blocks: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchFindingBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by_plan_id: Option<String>,
}

// ── OptimizeRun ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeRunItem {
    pub id: String,
    pub project_id: String,
    pub scope: String,
    pub scope_id: String,
    pub status: String,
    pub cycle: i64,
    pub max_cycles: i64,
    pub graders: Vec<String>,
    pub latest_grades: serde_json::Value,
    pub open_findings: i64,
    pub blocked_findings: i64,
    pub started_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeRunDetail {
    #[serde(flatten)]
    pub run: OptimizeRunItem,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome_reason: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeCycleItem {
    pub id: String,
    pub run_id: String,
    pub cycle: i64,
    pub grades: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade_min: Option<f64>,
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub develop_status: Option<String>,
    pub open_findings: i64,
    pub resolved_this_cycle: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeRunTrajectory {
    #[serde(flatten)]
    pub detail: OptimizeRunDetail,
    pub cycles: Vec<OptimizeCycleItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOptimizeRunBody {
    pub id: String,
    pub project_id: String,
    #[serde(default = "default_run_scope")]
    pub scope: String,
    pub scope_id: String,
    #[serde(default = "default_max_cycles")]
    pub max_cycles: i64,
    #[serde(default)]
    pub graders: Vec<String>,
}

fn default_run_scope() -> String {
    "repo".to_string()
}

fn default_max_cycles() -> i64 {
    8
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchOptimizeRunBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_grades: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_findings: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_findings: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome_reason: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOptimizeCycleBody {
    pub id: String,
    pub run_id: String,
    pub cycle: i64,
    pub grades: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade_min: Option<f64>,
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub develop_status: Option<String>,
    pub open_findings: i64,
    pub resolved_this_cycle: i64,
}

fn default_cr_origin() -> String {
    "system".to_string()
}

fn default_cr_risk() -> String {
    "medium".to_string()
}

fn default_plan_status() -> String {
    "draft".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRequestItem {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    pub status: String,
    pub finding_ids: Vec<String>,
    pub risk: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateChangeRequestBody {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default = "default_cr_origin")]
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    #[serde(default)]
    pub finding_ids: Vec<String>,
    #[serde(default = "default_cr_risk")]
    pub risk: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchChangeRequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub id: String,
    pub title: String,
    pub component: String,
    pub repo: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_change_request_id: Option<String>,
    pub execution_ids: Vec<String>,
    pub confidence: i64,
    pub risk: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_value: Option<String>,
    pub agent_generated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting_since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    pub attempts: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatePlanBody {
    pub id: String,
    pub title: String,
    pub linked_change_request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default = "default_plan_status")]
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    pub confidence: i64,
    pub risk: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_delta: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchPlanBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlanSectionItem {
    pub id: String,
    pub label: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlanPolicyCheckItem {
    pub rule: String,
    pub status: String,
    pub met: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlanApproverItem {
    pub role: String,
    pub name: String,
    pub status: String,
}

/// Internal result of approving a plan: PlanItem plus the freshly-inserted
/// ExecutionRecord identity needed to publish the canonical `execution_update`
/// broadcast event. Not serialized over the wire.
#[derive(Debug, Clone)]
pub struct ApprovedPlan {
    pub plan: PlanItem,
    pub execution_id: String,
    /// Owning workflow instance id for the freshly-inserted ExecutionRecord.
    /// No workflow instance has attached to it yet at approval time (its
    /// `instanceId` column is NULL), so this mirrors `execution_id` per the
    /// same NULL-fallback convention `list_executions_db` applies when
    /// building `ExecutionItem::instance_id`.
    pub instance_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanDetail {
    #[serde(flatten)]
    pub plan: PlanItem,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_delta: Option<String>,
    pub sections: Vec<PlanSectionItem>,
    pub policy_checks: Vec<PlanPolicyCheckItem>,
    pub approvers: Vec<PlanApproverItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExecutionItem {
    pub instance_id: String,
    #[serde(rename = "planId")]
    pub plan_id: Option<String>,
    #[serde(rename = "linkedPlanId")]
    pub linked_plan_id: Option<String>,
    pub workflow_id: Option<String>,
    #[serde(rename = "planTitle")]
    pub plan_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    pub status: String,
    #[serde(rename = "policyLevel", skip_serializing_if = "Option::is_none")]
    pub policy_level: Option<String>,
    #[serde(rename = "startedBy", skip_serializing_if = "Option::is_none")]
    pub started_by: Option<String>,
    #[serde(rename = "waitingOn", skip_serializing_if = "Option::is_none")]
    pub waiting_on: Option<String>,
    #[serde(rename = "testResult", skip_serializing_if = "Option::is_none")]
    pub test_result: Option<String>,
    #[serde(rename = "prStatus", skip_serializing_if = "Option::is_none")]
    pub pr_status: Option<String>,
    #[serde(rename = "prLink", skip_serializing_if = "Option::is_none")]
    pub pr_link: Option<String>,
    #[serde(rename = "deployStatus", skip_serializing_if = "Option::is_none")]
    pub deploy_status: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OperatorItem {
    pub operator_type: String,
    pub description: String,
    #[serde(rename = "params_schema")]
    pub params_schema: serde_json::Value,
    #[serde(rename = "paletteLabel", skip_serializing_if = "Option::is_none")]
    pub palette_label: Option<String>,
    #[serde(rename = "paletteIcon", skip_serializing_if = "Option::is_none")]
    pub palette_icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModuleItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub repo_id: String,
    pub repo_name: String,
    pub component_id: String,
    pub component_name: String,
}

// ── Product ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateProductBody {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PutProductBody {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PatchProductBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// ── Component ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateComponentBody {
    pub name: String,
    pub product_id: String,
    pub domain: String,
    pub owner: String,
    pub criticality: String,
    pub autonomy: String,
    #[serde(default)]
    pub trend: i64,
    pub last_eval: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PutComponentBody {
    pub name: String,
    pub product_id: String,
    pub domain: String,
    pub owner: String,
    pub criticality: String,
    pub autonomy: String,
    pub trend: i64,
    pub last_eval: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchComponentBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub criticality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autonomy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trend: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_eval: Option<String>,
}

// ── Repo ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateRepoBody {
    pub name: String,
    pub component_id: String,
    pub owner: String,
    pub criticality: String,
    pub autonomy: String,
    pub exec_status: String,
    pub last_eval: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PutRepoBody {
    pub name: String,
    pub component_id: String,
    pub owner: String,
    pub criticality: String,
    pub autonomy: String,
    pub exec_status: String,
    pub last_eval: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchRepoBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub criticality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autonomy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_eval: Option<String>,
}

// ── Module ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateModuleBody {
    pub name: String,
    pub kind: String,
    pub repo_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PutModuleBody {
    pub name: String,
    pub kind: String,
    pub repo_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchModuleBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
}

// ── ModuleDependency ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchModuleDependencyBody {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub dep_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

// ── DELETE response ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeletedItem {
    pub id: String,
}
