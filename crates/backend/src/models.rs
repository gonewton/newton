use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductItem {
    pub id: String,
    pub name: String,
    pub component_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentItem {
    pub id: String,
    pub name: String,
    pub product_id: String,
    pub product_name: String,
    pub domain: String,
    pub repos: i64,
    pub modules: i64,
    pub health: i64,
    pub trend: i64,
    pub owner: String,
    pub criticality: String,
    pub autonomy: String,
    pub open_plans: i64,
    pub open_requests: i64,
    pub last_eval: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionItem {
    pub repo: String,
    pub indicator: String,
    pub delta: f64,
    pub severity: String,
    pub since: String,
    pub trend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndicatorItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub scope: String,
    pub weight: f64,
    pub threshold: f64,
    pub current: f64,
    pub trend: f64,
    pub reports: i64,
    pub mode: String,
    pub last_run: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentActionItem {
    pub time: String,
    pub action: String,
    pub subject: String,
    #[serde(rename = "type")]
    pub item_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoItem {
    pub id: String,
    pub name: String,
    pub component: String,
    pub owner: String,
    pub criticality: String,
    pub autonomy: String,
    pub quality_score: i64,
    pub regressions: i64,
    pub open_plans: i64,
    pub exec_status: String,
    pub last_eval: String,
    pub coverage: i64,
    pub sec_score: i64,
    pub depends_on: Vec<String>,
    pub depended_on_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoDependencyItem {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub dep_type: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleRef {
    pub module: String,
    pub kind: String,
    pub repo: String,
    pub component: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleDependencyItem {
    pub id: String,
    pub from: ModuleRef,
    pub to: ModuleRef,
    #[serde(rename = "type")]
    pub dep_type: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateModuleDependencyBody {
    pub from_module_id: String,
    pub to_module_id: String,
    #[serde(rename = "type")]
    pub dep_type: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedViewItem {
    pub id: String,
    pub label: String,
    pub filters: Option<serde_json::Value>,
    pub sort: Option<String>,
    pub sort_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpportunityItem {
    pub id: String,
    pub title: String,
    pub origin: String,
    pub component: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    pub repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indicator: Option<String>,
    pub confidence: Option<f64>,
    pub risk: String,
    pub expected_value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    pub depends_on: Vec<String>,
    pub blocks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchOpportunityBody {
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestItem {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub component: String,
    pub repo: String,
    pub requested_by: String,
    pub status: String,
    pub linked_opportunity_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRequestBody {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub requested_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_opportunity_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub id: String,
    pub title: String,
    pub component: String,
    pub repo: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_request_id: Option<String>,
    pub execution_ids: Vec<String>,
    pub confidence: i64,
    pub risk: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_value: Option<String>,
    pub agent_generated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting_since: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSectionItem {
    pub id: String,
    pub label: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanPolicyCheckItem {
    pub rule: String,
    pub status: String,
    pub met: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanApproverItem {
    pub role: String,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
