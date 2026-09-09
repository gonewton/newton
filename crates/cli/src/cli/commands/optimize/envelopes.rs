//! Explicit software-strategy workflow outputs (independent of task IDs).

use newton_types::optimization::{Candidate, CandidateEvaluation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GradeOutput {
    pub candidate: Candidate,
    pub evaluation: CandidateEvaluation,
    #[serde(default)]
    pub change_request_id: Option<String>,
    #[serde(default)]
    pub open_findings: Option<std::collections::BTreeMap<String, u64>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum PlanOutput {
    Propose {
        plan_id: String,
        #[serde(default)]
        change_request_id: Option<String>,
    },
    None,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DevelopOutput {
    #[serde(default)]
    pub candidate: Option<Candidate>,
    #[serde(default)]
    pub failure: Option<super::software_work::ReconciledFailure>,
}
