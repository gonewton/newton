#![allow(clippy::result_large_err)] // Operator param parsing returns AppError for consistent diagnostics.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::human::{audit, ApprovalDefault, AuditEntry, Interviewer};
use crate::core::workflow_graph::operator::{ExecutionContext, Operator};
use crate::core::workflow_graph::schema::HumanSettings;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

struct ApprovalParams {
    prompt: String,
    timeout_seconds: Option<u64>,
    default_on_timeout: Option<ApprovalDefault>,
}

impl ApprovalParams {
    fn parse(value: &Value) -> Result<Self, AppError> {
        let prompt = value
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    "HumanApprovalOperator requires a non-empty prompt",
                )
            })?
            .to_string();

        let timeout_seconds = value.get("timeout_seconds").and_then(Value::as_u64);

        let default_on_timeout = value
            .get("default_on_timeout")
            .and_then(Value::as_str)
            .map(|v| {
                ApprovalDefault::from_str(v).map_err(|_| {
                    AppError::new(
                        ErrorCategory::ValidationError,
                        "default_on_timeout must be 'approve' or 'reject'",
                    )
                })
            })
            .transpose()?;

        Ok(Self {
            prompt,
            timeout_seconds,
            default_on_timeout,
        })
    }
}

pub struct HumanApprovalOperator {
    interviewer: Arc<dyn Interviewer>,
    audit_path: PathBuf,
    default_timeout_seconds: u64,
    redact_keys: Arc<Vec<String>>,
}

impl HumanApprovalOperator {
    pub fn new(
        interviewer: Arc<dyn Interviewer>,
        human_settings: HumanSettings,
        redact_keys: Arc<Vec<String>>,
    ) -> Self {
        Self {
            interviewer,
            audit_path: human_settings.audit_path,
            default_timeout_seconds: human_settings.default_timeout_seconds,
            redact_keys,
        }
    }
}

#[async_trait]
impl Operator for HumanApprovalOperator {
    fn name(&self) -> &'static str {
        "HumanApprovalOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let parsed = ApprovalParams::parse(params)?;
        if parsed.timeout_seconds.is_some() && parsed.default_on_timeout.is_none() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "default_on_timeout is required when timeout_seconds is set",
            )
            .with_code("WFG-HUMAN-001"));
        }
        Ok(())
    }

    async fn execute(&self, params: Value, ctx: ExecutionContext) -> Result<Value, AppError> {
        let parsed = ApprovalParams::parse(&params)?;
        let timeout_duration = parsed.timeout_seconds.map(Duration::from_secs).or_else(|| {
            if parsed.default_on_timeout.is_some() && self.default_timeout_seconds > 0 {
                Some(Duration::from_secs(self.default_timeout_seconds))
            } else {
                None
            }
        });
        let result = self
            .interviewer
            .ask_approval(&parsed.prompt, timeout_duration, parsed.default_on_timeout)
            .await?;
        let response_text = if result.default_used || result.reason.is_empty() {
            None
        } else {
            Some(result.reason.clone())
        };
        let mut entry = AuditEntry {
            timestamp: result.timestamp.to_rfc3339(),
            execution_id: ctx.execution_id.clone(),
            task_id: ctx.task_id.clone(),
            interviewer_type: self.interviewer.interviewer_type().to_string(),
            prompt: parsed.prompt.clone(),
            choices: None,
            approved: Some(result.approved),
            choice: None,
            responder: None,
            response_text,
            timeout_applied: result.timeout_applied,
            default_used: result.default_used,
        };
        audit::append_entry(
            &ctx.workspace_path,
            &self.audit_path,
            &ctx.execution_id,
            &mut entry,
            self.redact_keys.as_ref(),
        )?;
        Ok(json!({
            "approved": result.approved,
            "reason": result.reason,
            "timestamp": result.timestamp.to_rfc3339(),
        }))
    }
}
