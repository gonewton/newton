#![allow(clippy::result_large_err)] // Operator param parsing returns AppError for consistent diagnostics.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::human::{audit, AuditEntry, Interviewer};
use crate::core::workflow_graph::operator::{ExecutionContext, Operator};
use crate::core::workflow_graph::schema::HumanSettings;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct DecisionParams {
    prompt: String,
    choices: Vec<String>,
    timeout_seconds: Option<u64>,
    default_choice: Option<String>,
}

impl DecisionParams {
    fn parse(value: &Value) -> Result<Self, AppError> {
        let prompt = value
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    "HumanDecisionOperator requires a non-empty prompt",
                )
            })?
            .to_string();

        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .map(|array| {
                array
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if choices.len() < 2 {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "HumanDecisionOperator requires at least two choices",
            ));
        }

        let timeout_seconds = value.get("timeout_seconds").and_then(Value::as_u64);

        let default_choice = value
            .get("default_choice")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        if let Some(default) = &default_choice {
            if !choices.contains(default) {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "default_choice must be one of the provided choices",
                ));
            }
        }

        Ok(Self {
            prompt,
            choices,
            timeout_seconds,
            default_choice,
        })
    }
}

pub struct HumanDecisionOperator {
    interviewer: Arc<dyn Interviewer>,
    audit_path: PathBuf,
    default_timeout_seconds: u64,
    redact_keys: Arc<Vec<String>>,
}

impl HumanDecisionOperator {
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
impl Operator for HumanDecisionOperator {
    fn name(&self) -> &'static str {
        "HumanDecisionOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let parsed = DecisionParams::parse(params)?;
        if parsed.timeout_seconds.is_some() && parsed.default_choice.is_none() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "default_choice is required when timeout_seconds is set",
            )
            .with_code("WFG-HUMAN-002"));
        }
        Ok(())
    }

    async fn execute(&self, params: Value, ctx: ExecutionContext) -> Result<Value, AppError> {
        let parsed = DecisionParams::parse(&params)?;
        let timeout_duration = parsed.timeout_seconds.map(Duration::from_secs).or_else(|| {
            if parsed.default_choice.is_some() && self.default_timeout_seconds > 0 {
                Some(Duration::from_secs(self.default_timeout_seconds))
            } else {
                None
            }
        });
        let default_choice_ref = parsed.default_choice.as_deref();
        let result = self
            .interviewer
            .ask_choice(
                &parsed.prompt,
                &parsed.choices,
                timeout_duration,
                default_choice_ref,
            )
            .await?;
        let mut entry = AuditEntry {
            timestamp: result.timestamp.to_rfc3339(),
            execution_id: ctx.execution_id.clone(),
            task_id: ctx.task_id.clone(),
            interviewer_type: self.interviewer.interviewer_type().to_string(),
            prompt: parsed.prompt.clone(),
            choices: Some(parsed.choices.clone()),
            approved: None,
            choice: Some(result.choice.clone()),
            responder: None,
            response_text: result.response_text.clone(),
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
            "choice": result.choice,
            "timestamp": result.timestamp.to_rfc3339(),
        }))
    }
}
