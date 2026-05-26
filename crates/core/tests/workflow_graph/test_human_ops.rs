use anyhow::Result;
use chrono::Utc;
use newton_core::workflow::{
    executor::{ExecutionOverrides, GraphHandle},
    human::{
        ApprovalResult, DecisionResult, Interviewer, InterviewerProvider, MockAiloopInterviewer,
    },
    operator::{ExecutionContext, Operator, OperatorRegistry, StateView},
    operators::{human_approval::HumanApprovalOperator, human_decision::HumanDecisionOperator},
    schema::HumanSettings,
};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;
use uuid::Uuid;

fn provider_from_mock(mock: Arc<MockAiloopInterviewer>) -> InterviewerProvider {
    let cloned = mock as Arc<dyn Interviewer>;
    Arc::new(move || Ok(cloned.clone()))
}

fn empty_provider() -> InterviewerProvider {
    let mock = Arc::new(MockAiloopInterviewer::new()) as Arc<dyn Interviewer>;
    Arc::new(move || Ok(mock.clone()))
}

fn build_execution_context(workspace: &TempDir, execution_id: String) -> ExecutionContext {
    let empty = Value::Object(Map::new());
    ExecutionContext {
        workspace_path: workspace.path().to_path_buf(),
        execution_id,
        task_id: "approval".to_string(),
        iteration: 1,
        state_view: StateView::new(empty.clone(), empty.clone(), empty),
        graph: GraphHandle::new(HashMap::new()),
        workflow_file: workspace.path().join("workflow.yaml"),
        nesting_depth: 0,
        execution_overrides: ExecutionOverrides {
            parallel_limit: None,
            max_time_seconds: None,
            checkpoint_base_path: None,
            artifact_base_path: None,
            max_nesting_depth: None,
            verbose: false,
            sink: None,
            pre_seed_nodes: true,
        },
        operator_registry: OperatorRegistry::new(),
    }
}

#[tokio::test]
async fn human_approval_logs_timeout_default() -> Result<()> {
    let workspace = TempDir::new()?;
    let execution_id = Uuid::new_v4().to_string();
    let approval_result = ApprovalResult {
        approved: true,
        reason: "fallback to default".to_string(),
        timestamp: Utc::now(),
        timeout_applied: true,
        default_used: true,
    };
    let mock = Arc::new(MockAiloopInterviewer::new());
    mock.push_approval(approval_result.clone());
    let operator = HumanApprovalOperator::new(
        provider_from_mock(mock),
        HumanSettings::default(),
        Arc::new(Vec::new()),
    );
    let ctx = build_execution_context(&workspace, execution_id.clone());
    let output = operator
        .execute(
            json!({
                "prompt": "Approve release?",
                "timeout_seconds": 1,
                "default_on_timeout": "approve",
            }),
            ctx,
        )
        .await?;
    assert_eq!(output["approved"], json!(true));
    assert_eq!(output["reason"], json!(approval_result.reason));

    let audit_path = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(&execution_id)
        .join("audit.jsonl");
    let contents = fs::read_to_string(audit_path)?;
    let entry_line = contents.lines().next().expect("audit entry missing");
    let entry: Value = serde_json::from_str(entry_line)?;
    assert_eq!(entry["execution_id"], json!(execution_id));
    assert_eq!(entry["task_id"], json!("approval"));
    assert_eq!(entry["interviewer_type"], json!("mock_ailoop"));
    assert_eq!(entry["prompt"], json!("Approve release?"));
    assert_eq!(entry["approved"], json!(true));
    assert_eq!(entry["default_used"], json!(true));
    assert_eq!(entry["timeout_applied"], json!(true));
    assert!(entry["response_text"].is_null());
    Ok(())
}

#[test]
fn human_approval_requires_default() -> Result<()> {
    let operator = HumanApprovalOperator::new(
        empty_provider(),
        HumanSettings::default(),
        Arc::new(Vec::new()),
    );
    let err = operator
        .validate_params(&json!({
            "prompt": "Confirm?",
            "timeout_seconds": 10
        }))
        .expect_err("missing default_on_timeout should fail");
    assert_eq!(err.code, "WFG-HUMAN-001");
    Ok(())
}

#[test]
fn human_decision_requires_default_choice() -> Result<()> {
    let operator = HumanDecisionOperator::new(
        empty_provider(),
        HumanSettings::default(),
        Arc::new(Vec::new()),
    );
    let err = operator
        .validate_params(&json!({
            "prompt": "Pick one",
            "choices": ["a", "b"],
            "timeout_seconds": 5
        }))
        .expect_err("missing default_choice should fail");
    assert_eq!(err.code, "WFG-HUMAN-002");
    Ok(())
}

#[tokio::test]
async fn human_decision_logs_choice() -> Result<()> {
    let workspace = TempDir::new()?;
    let execution_id = Uuid::new_v4().to_string();
    let decision_result = DecisionResult {
        choice: "b".to_string(),
        timestamp: Utc::now(),
        timeout_applied: false,
        default_used: false,
        response_text: Some("2".to_string()),
    };
    let mock = Arc::new(MockAiloopInterviewer::new());
    mock.push_decision(decision_result.clone());
    let operator = HumanDecisionOperator::new(
        provider_from_mock(mock),
        HumanSettings::default(),
        Arc::new(Vec::new()),
    );
    let mut ctx = build_execution_context(&workspace, execution_id.clone());
    ctx.task_id = "decision".to_string();
    let output = operator
        .execute(
            json!({
                "prompt": "Pick one",
                "choices": ["a", "b"],
            }),
            ctx,
        )
        .await?;
    assert_eq!(output["choice"], json!("b"));

    let audit_path = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(&execution_id)
        .join("audit.jsonl");
    let contents = fs::read_to_string(audit_path)?;
    let entry_line = contents.lines().next().expect("audit entry missing");
    let entry: Value = serde_json::from_str(entry_line)?;
    assert_eq!(entry["execution_id"], json!(execution_id));
    assert_eq!(entry["task_id"], json!("decision"));
    assert_eq!(entry["choice"], json!("b"));
    assert_eq!(entry["default_used"], json!(false));
    assert_eq!(entry["timeout_applied"], json!(false));
    Ok(())
}
