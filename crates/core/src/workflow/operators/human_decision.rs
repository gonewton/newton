#![allow(clippy::result_large_err)] // Operator param parsing returns AppError for consistent diagnostics.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::human::{
    audit, AuditEntry, DecisionContent, DecisionOption, DecisionRecommendation, Interviewer,
    InterviewerProvider,
};
use crate::workflow::operator::{ExecutionContext, Operator};
use crate::workflow::schema::HumanSettings;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct HumanDecisionOutput {
    pub choice: String,
}

struct ParsedOption {
    id: String,
    label: String,
    detail_markdown: Option<String>,
}

struct ParsedRecommendation {
    option_id: String,
    rationale_markdown: Option<String>,
}

enum DecisionParams {
    Structured {
        decision_id: Option<String>,
        summary: String,
        context_markdown: Option<String>,
        options: Vec<ParsedOption>,
        recommendation: Option<ParsedRecommendation>,
        timeout_seconds: Option<u64>,
        default_choice: Option<String>,
    },
    Legacy {
        prompt: String,
        choices: Vec<String>,
        timeout_seconds: Option<u64>,
        default_choice: Option<String>,
    },
}

impl DecisionParams {
    fn parse(value: &Value) -> Result<Self, AppError> {
        let has_options = value.get("options").is_some();
        let has_prompt = value.get("prompt").is_some();

        match (has_options, has_prompt) {
            (true, false) => Self::parse_structured(value),
            (false, true) => Self::parse_legacy(value),
            (true, true) => Err(AppError::new(
                ErrorCategory::ValidationError,
                "HumanDecisionOperator params must have either 'options' (structured) \
                 or 'prompt' (legacy), not both",
            )),
            (false, false) => Err(AppError::new(
                ErrorCategory::ValidationError,
                "HumanDecisionOperator params must have either 'options' (structured) \
                 or 'prompt' (legacy)",
            )),
        }
    }

    fn parse_structured(value: &Value) -> Result<Self, AppError> {
        let summary = value
            .get("summary")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    "HumanDecisionOperator structured params require a non-empty 'summary'",
                )
            })?
            .to_string();

        let decision_id = value
            .get("decision_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);

        let context_markdown = value
            .get("context_markdown")
            .and_then(Value::as_str)
            .map(String::from);

        let options_raw = value
            .get("options")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    "HumanDecisionOperator structured params require an 'options' array",
                )
            })?;

        let mut options: Vec<ParsedOption> = Vec::new();
        for opt_val in options_raw {
            let id = opt_val
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    AppError::new(
                        ErrorCategory::ValidationError,
                        "each option must have a non-empty 'id'",
                    )
                })?
                .to_string();
            let label = opt_val
                .get("label")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    AppError::new(
                        ErrorCategory::ValidationError,
                        "each option must have a non-empty 'label'",
                    )
                })?
                .to_string();
            let detail_markdown = opt_val
                .get("detail_markdown")
                .and_then(Value::as_str)
                .map(String::from);
            options.push(ParsedOption {
                id,
                label,
                detail_markdown,
            });
        }

        let recommendation = if let Some(rec_val) = value.get("recommendation") {
            let option_id = rec_val
                .get("option_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    AppError::new(
                        ErrorCategory::ValidationError,
                        "recommendation must have a non-empty 'option_id'",
                    )
                })?
                .to_string();
            let rationale_markdown = rec_val
                .get("rationale_markdown")
                .and_then(Value::as_str)
                .map(String::from);
            Some(ParsedRecommendation {
                option_id,
                rationale_markdown,
            })
        } else {
            None
        };

        let timeout_seconds = value.get("timeout_seconds").and_then(Value::as_u64);

        let default_choice = value
            .get("default_choice")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);

        Ok(DecisionParams::Structured {
            decision_id,
            summary,
            context_markdown,
            options,
            recommendation,
            timeout_seconds,
            default_choice,
        })
    }

    fn parse_legacy(value: &Value) -> Result<Self, AppError> {
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

        Ok(DecisionParams::Legacy {
            prompt,
            choices,
            timeout_seconds,
            default_choice,
        })
    }

    fn validate_structured(
        options: &[ParsedOption],
        recommendation: &Option<ParsedRecommendation>,
        timeout_seconds: Option<u64>,
        default_choice: &Option<String>,
    ) -> Result<(), AppError> {
        if options.len() < 2 {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "HumanDecisionOperator requires at least 2 options",
            )
            .with_code("WFG-HUMAN-201"));
        }

        let mut seen_ids = std::collections::HashSet::new();
        for opt in options {
            if !seen_ids.insert(&opt.id) {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "duplicate option id '{}' in HumanDecisionOperator options",
                        opt.id
                    ),
                )
                .with_code("WFG-HUMAN-202"));
            }
        }

        if let Some(rec) = recommendation {
            if !options.iter().any(|o| o.id == rec.option_id) {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "recommendation.option_id '{}' does not match any option id",
                        rec.option_id
                    ),
                )
                .with_code("WFG-HUMAN-203"));
            }
        }

        if let Some(dc) = default_choice {
            if !options.iter().any(|o| &o.id == dc) {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("default_choice '{dc}' does not match any option id"),
                )
                .with_code("WFG-HUMAN-204"));
            }
        }

        if timeout_seconds.is_some() && default_choice.is_none() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "default_choice is required when timeout_seconds is set",
            )
            .with_code("WFG-HUMAN-002"));
        }

        Ok(())
    }
}

pub struct HumanDecisionOperator {
    provider: InterviewerProvider,
    cached: Mutex<Option<Arc<dyn Interviewer>>>,
    audit_path: PathBuf,
    default_timeout_seconds: u64,
    redact_keys: Arc<Vec<String>>,
}

impl HumanDecisionOperator {
    pub fn new(
        provider: InterviewerProvider,
        human_settings: HumanSettings,
        redact_keys: Arc<Vec<String>>,
    ) -> Self {
        Self {
            provider,
            cached: Mutex::new(None),
            audit_path: human_settings.audit_path,
            default_timeout_seconds: human_settings.default_timeout_seconds,
            redact_keys,
        }
    }

    fn interviewer(&self) -> Result<Arc<dyn Interviewer>, AppError> {
        let mut guard = self.cached.lock().unwrap();
        if let Some(existing) = guard.as_ref() {
            return Ok(existing.clone());
        }
        let resolved = (self.provider)()?;
        *guard = Some(resolved.clone());
        Ok(resolved)
    }
}

#[async_trait]
impl Operator for HumanDecisionOperator {
    fn name(&self) -> &'static str {
        "HumanDecisionOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let parsed = DecisionParams::parse(params)?;
        match &parsed {
            DecisionParams::Structured {
                options,
                recommendation,
                timeout_seconds,
                default_choice,
                ..
            } => {
                DecisionParams::validate_structured(
                    options,
                    recommendation,
                    *timeout_seconds,
                    default_choice,
                )?;
            }
            DecisionParams::Legacy {
                timeout_seconds,
                default_choice,
                ..
            } => {
                if timeout_seconds.is_some() && default_choice.is_none() {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        "default_choice is required when timeout_seconds is set",
                    )
                    .with_code("WFG-HUMAN-002"));
                }
            }
        }
        Ok(())
    }

    fn params_schema(&self) -> schemars::Schema {
        // permissive — structured vs legacy discriminated by presence of `options` or `prompt`
        serde_json::from_value::<schemars::Schema>(serde_json::json!({"type": "object"}))
            .unwrap_or_default()
    }

    fn output_schema(&self) -> schemars::Schema {
        schemars::schema_for!(HumanDecisionOutput)
    }

    async fn execute(&self, params: Value, ctx: ExecutionContext) -> Result<Value, AppError> {
        let parsed = DecisionParams::parse(&params)?;

        match parsed {
            DecisionParams::Structured {
                decision_id,
                summary,
                context_markdown,
                options,
                recommendation,
                timeout_seconds,
                default_choice,
            } => {
                DecisionParams::validate_structured(
                    &options,
                    &recommendation,
                    timeout_seconds,
                    &default_choice,
                )?;

                let effective_decision_id = decision_id.unwrap_or_else(|| ctx.task_id.clone());

                let timeout_duration = timeout_seconds.map(Duration::from_secs).or_else(|| {
                    if default_choice.is_some() && self.default_timeout_seconds > 0 {
                        Some(Duration::from_secs(self.default_timeout_seconds))
                    } else {
                        None
                    }
                });

                let content = DecisionContent {
                    decision_id: effective_decision_id.clone(),
                    summary: summary.clone(),
                    context_markdown,
                    options: options
                        .iter()
                        .map(|o| DecisionOption {
                            id: o.id.clone(),
                            label: o.label.clone(),
                            detail_markdown: o.detail_markdown.clone(),
                        })
                        .collect(),
                    recommendation: recommendation.as_ref().map(|r| DecisionRecommendation {
                        option_id: r.option_id.clone(),
                        rationale_markdown: r.rationale_markdown.clone(),
                    }),
                };

                let interviewer = self.interviewer()?;
                let result = interviewer
                    .ask_decision(content, timeout_duration, default_choice.as_deref())
                    .await?;

                let label = options
                    .iter()
                    .find(|o| o.id == result.choice)
                    .map(|o| o.label.clone())
                    .unwrap_or_else(|| result.choice.clone());

                let option_ids: Vec<String> = options.iter().map(|o| o.id.clone()).collect();
                let mut entry = AuditEntry {
                    timestamp: result.timestamp.to_rfc3339(),
                    execution_id: ctx.execution_id.clone(),
                    task_id: ctx.task_id.clone(),
                    interviewer_type: interviewer.interviewer_type().to_string(),
                    prompt: summary,
                    choices: Some(option_ids),
                    approved: None,
                    choice: Some(result.choice.clone()),
                    responder: None,
                    response_text: result.response_text.clone(),
                    timeout_applied: result.timeout_applied,
                    default_used: result.default_used,
                    decision_id: Some(effective_decision_id),
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
                    "timeout_applied": result.timeout_applied,
                    "default_used": result.default_used,
                    "label": label,
                }))
            }

            DecisionParams::Legacy {
                prompt,
                choices,
                timeout_seconds,
                default_choice,
            } => {
                if timeout_seconds.is_some() && default_choice.is_none() {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        "default_choice is required when timeout_seconds is set",
                    )
                    .with_code("WFG-HUMAN-002"));
                }

                let timeout_duration = timeout_seconds.map(Duration::from_secs).or_else(|| {
                    if default_choice.is_some() && self.default_timeout_seconds > 0 {
                        Some(Duration::from_secs(self.default_timeout_seconds))
                    } else {
                        None
                    }
                });

                // Compile legacy params into DecisionContent so ailoop renders a structured card.
                let content = DecisionContent {
                    decision_id: ctx.task_id.clone(),
                    summary: prompt.clone(),
                    context_markdown: None,
                    options: choices
                        .iter()
                        .map(|c| DecisionOption {
                            id: c.clone(),
                            label: c.clone(),
                            detail_markdown: None,
                        })
                        .collect(),
                    recommendation: None,
                };

                let interviewer = self.interviewer()?;
                let result = interviewer
                    .ask_decision(content, timeout_duration, default_choice.as_deref())
                    .await?;

                let label = result.choice.clone();

                let mut entry = AuditEntry {
                    timestamp: result.timestamp.to_rfc3339(),
                    execution_id: ctx.execution_id.clone(),
                    task_id: ctx.task_id.clone(),
                    interviewer_type: interviewer.interviewer_type().to_string(),
                    prompt: prompt.clone(),
                    choices: Some(choices.clone()),
                    approved: None,
                    choice: Some(result.choice.clone()),
                    responder: None,
                    response_text: result.response_text.clone(),
                    timeout_applied: result.timeout_applied,
                    default_used: result.default_used,
                    decision_id: None,
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
                    "timeout_applied": result.timeout_applied,
                    "default_used": result.default_used,
                    "label": label,
                }))
            }
        }
    }
}
