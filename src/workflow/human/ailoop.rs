#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::integrations::ailoop::tool_client::{ChoiceQuestionRequest, ClientError, ToolClient};
use crate::workflow::human::{ApprovalDefault, ApprovalResult, DecisionResult, Interviewer};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;

const ACTION_HEADLINE_LIMIT: usize = 80;

pub struct AiloopInterviewer {
    client: Arc<ToolClient>,
    fail_fast: bool,
    default_timeout: Duration,
}

impl AiloopInterviewer {
    pub fn new(client: Arc<ToolClient>, fail_fast: bool, default_timeout: Duration) -> Self {
        Self {
            client,
            fail_fast,
            default_timeout,
        }
    }

    fn resolve_timeout(&self, timeout: Option<Duration>) -> Duration {
        timeout.unwrap_or(self.default_timeout)
    }
}

fn truncate_action(prompt: &str) -> String {
    let mut out = String::with_capacity(prompt.len().min(ACTION_HEADLINE_LIMIT));
    for (i, ch) in prompt.chars().enumerate() {
        if i >= ACTION_HEADLINE_LIMIT {
            break;
        }
        out.push(ch);
    }
    out
}

#[async_trait]
impl Interviewer for AiloopInterviewer {
    fn interviewer_type(&self) -> &'static str {
        "ailoop"
    }

    async fn ask_approval(
        &self,
        prompt: &str,
        timeout: Option<Duration>,
        default_on_timeout: Option<ApprovalDefault>,
    ) -> Result<ApprovalResult, AppError> {
        let effective_timeout = self.resolve_timeout(timeout);
        let action = truncate_action(prompt);
        let details = prompt.to_string();

        let response = self
            .client
            .request_authorization(action, details, effective_timeout)
            .await;

        match response {
            Ok(resp) => {
                if resp.timed_out {
                    if let Some(default) = default_on_timeout {
                        return Ok(ApprovalResult {
                            approved: matches!(default, ApprovalDefault::Approve),
                            reason: format!("default_on_timeout={}", default.as_str()),
                            timestamp: Utc::now(),
                            timeout_applied: true,
                            default_used: true,
                        });
                    }
                    return Err(AppError::new(
                        ErrorCategory::TimeoutError,
                        "ailoop approval request timed out and no default_on_timeout configured",
                    )
                    .with_code("WFG-HUMAN-105"));
                }
                Ok(ApprovalResult {
                    approved: resp.authorized,
                    reason: resp.reason.unwrap_or_default(),
                    timestamp: Utc::now(),
                    timeout_applied: false,
                    default_used: false,
                })
            }
            Err(err) => handle_approval_error(err, default_on_timeout, self.fail_fast),
        }
    }

    async fn ask_choice(
        &self,
        prompt: &str,
        choices: &[String],
        timeout: Option<Duration>,
        default_choice: Option<&str>,
    ) -> Result<DecisionResult, AppError> {
        let effective_timeout = self.resolve_timeout(timeout);

        let mut question = String::from(prompt);
        if !choices.is_empty() {
            question.push_str("\nChoices:");
            for (idx, choice) in choices.iter().enumerate() {
                question.push_str(&format!("\n  {}: {}", idx + 1, choice));
            }
        }

        let request = ChoiceQuestionRequest {
            question,
            choices: choices.to_vec(),
            default: default_choice.map(str::to_string),
            timeout_ms: effective_timeout.as_millis() as u64,
        };

        let response = self
            .client
            .ask_question_with_choices(request, effective_timeout)
            .await;

        match response {
            Ok(resp) => {
                if resp.timed_out {
                    if let Some(default) = default_choice {
                        return Ok(DecisionResult {
                            choice: default.to_string(),
                            timestamp: Utc::now(),
                            timeout_applied: true,
                            default_used: true,
                            response_text: None,
                        });
                    }
                    return Err(AppError::new(
                        ErrorCategory::TimeoutError,
                        "ailoop ask request timed out and no default_choice configured",
                    )
                    .with_code("WFG-HUMAN-103"));
                }
                let answer = match resp.answer {
                    Some(a) => a,
                    None => {
                        return Err(AppError::new(
                            ErrorCategory::ValidationError,
                            "ailoop returned no answer and timed_out=false",
                        )
                        .with_code("WFG-HUMAN-104"));
                    }
                };
                let trimmed = answer.trim();
                if let Some(matched) = choices.iter().find(|c| c.as_str() == trimmed) {
                    return Ok(DecisionResult {
                        choice: matched.clone(),
                        timestamp: Utc::now(),
                        timeout_applied: false,
                        default_used: false,
                        response_text: Some(answer),
                    });
                }
                if let Some(default) = default_choice {
                    if let Some(matched) = choices.iter().find(|c| c.as_str() == default) {
                        return Ok(DecisionResult {
                            choice: matched.clone(),
                            timestamp: Utc::now(),
                            timeout_applied: false,
                            default_used: true,
                            response_text: Some(answer),
                        });
                    }
                }
                Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("ailoop answer '{trimmed}' does not match any declared choice"),
                )
                .with_code("WFG-HUMAN-104"))
            }
            Err(err) => handle_choice_error(err, default_choice, self.fail_fast),
        }
    }
}

fn handle_choice_error(
    err: ClientError,
    default_choice: Option<&str>,
    fail_fast: bool,
) -> Result<DecisionResult, AppError> {
    if fail_fast {
        return Err(AppError::new(
            ErrorCategory::IoError,
            format!("ailoop ask transport failure: {err}"),
        )
        .with_code("WFG-HUMAN-101"));
    }
    if let Some(default) = default_choice {
        return Ok(DecisionResult {
            choice: default.to_string(),
            timestamp: Utc::now(),
            timeout_applied: true,
            default_used: true,
            response_text: None,
        });
    }
    Err(AppError::new(
        ErrorCategory::TimeoutError,
        format!("ailoop ask transport failure (no default_choice): {err}"),
    )
    .with_code("WFG-HUMAN-103"))
}

fn handle_approval_error(
    err: ClientError,
    default_on_timeout: Option<ApprovalDefault>,
    fail_fast: bool,
) -> Result<ApprovalResult, AppError> {
    if fail_fast {
        return Err(AppError::new(
            ErrorCategory::IoError,
            format!("ailoop authorize transport failure: {err}"),
        )
        .with_code("WFG-HUMAN-102"));
    }
    if let Some(default) = default_on_timeout {
        return Ok(ApprovalResult {
            approved: matches!(default, ApprovalDefault::Approve),
            reason: format!("default_on_timeout={}", default.as_str()),
            timestamp: Utc::now(),
            timeout_applied: true,
            default_used: true,
        });
    }
    Err(AppError::new(
        ErrorCategory::TimeoutError,
        format!("ailoop authorize transport failure (no default_on_timeout): {err}"),
    )
    .with_code("WFG-HUMAN-105"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::ailoop::config::AiloopConfig;
    use crate::integrations::ailoop::AiloopContext;
    use serde_json::json;
    use std::path::PathBuf;
    use url::Url;
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_client(server_uri: &str, channel: &str) -> Arc<ToolClient> {
        let config = AiloopConfig {
            http_url: Url::parse(server_uri).unwrap(),
            ws_url: Url::parse("ws://localhost:8080").unwrap(),
            channel: channel.to_string(),
            enabled: true,
            fail_fast: false,
        };
        let ctx = Arc::new(AiloopContext::new(
            config,
            PathBuf::from("/tmp"),
            "test".to_string(),
        ));
        Arc::new(ToolClient::new(ctx))
    }

    fn unreachable_client(channel: &str) -> Arc<ToolClient> {
        make_client("http://127.0.0.1:1", channel)
    }

    #[tokio::test]
    async fn test_ask_choice_success_match() {
        let server = MockServer::start().await;
        let channel = "ch-success";
        Mock::given(method("POST"))
            .and(path_regex(format!(r"^/+questions/{channel}$")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "answer": "fix",
                "timed_out": false
            })))
            .mount(&server)
            .await;
        let interviewer = AiloopInterviewer::new(
            make_client(&server.uri(), channel),
            true,
            Duration::from_secs(5),
        );
        let choices = vec!["fix".to_string(), "skip".to_string()];
        let result = interviewer
            .ask_choice(
                "prompt",
                &choices,
                Some(Duration::from_secs(1)),
                Some("skip"),
            )
            .await
            .expect("ok");
        assert_eq!(result.choice, "fix");
        assert!(!result.timeout_applied);
        assert!(!result.default_used);
    }

    #[tokio::test]
    async fn test_ask_choice_unmatched_answer_returns_104() {
        let server = MockServer::start().await;
        let channel = "ch-unmatched";
        Mock::given(method("POST"))
            .and(path_regex(format!(r"^/+questions/{channel}$")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "answer": "banana",
                "timed_out": false
            })))
            .mount(&server)
            .await;
        let interviewer = AiloopInterviewer::new(
            make_client(&server.uri(), channel),
            true,
            Duration::from_secs(5),
        );
        let choices = vec!["apple".to_string(), "cherry".to_string()];
        let result = interviewer
            .ask_choice("p", &choices, Some(Duration::from_secs(1)), None)
            .await;
        let err = result.expect_err("should error");
        assert_eq!(err.code, "WFG-HUMAN-104");
    }

    #[tokio::test]
    async fn test_ask_choice_fail_fast_unreachable_returns_101() {
        let interviewer = AiloopInterviewer::new(
            unreachable_client("ch-fail"),
            true,
            Duration::from_millis(100),
        );
        let choices = vec!["a".to_string(), "b".to_string()];
        let result = interviewer
            .ask_choice("p", &choices, Some(Duration::from_millis(100)), None)
            .await;
        let err = result.expect_err("should error");
        assert_eq!(err.code, "WFG-HUMAN-101");
    }

    #[tokio::test]
    async fn test_ask_choice_no_failfast_no_default_returns_103() {
        let interviewer = AiloopInterviewer::new(
            unreachable_client("ch-fail-2"),
            false,
            Duration::from_millis(100),
        );
        let choices = vec!["a".to_string(), "b".to_string()];
        let result = interviewer
            .ask_choice("p", &choices, Some(Duration::from_millis(100)), None)
            .await;
        let err = result.expect_err("should error");
        assert_eq!(err.code, "WFG-HUMAN-103");
    }

    #[tokio::test]
    async fn test_ask_choice_no_failfast_with_default_falls_back() {
        let interviewer = AiloopInterviewer::new(
            unreachable_client("ch-fail-3"),
            false,
            Duration::from_millis(100),
        );
        let choices = vec!["a".to_string(), "b".to_string()];
        let result = interviewer
            .ask_choice("p", &choices, Some(Duration::from_millis(100)), Some("b"))
            .await
            .expect("ok");
        assert_eq!(result.choice, "b");
        assert!(result.timeout_applied);
        assert!(result.default_used);
    }

    #[tokio::test]
    async fn test_ask_approval_success() {
        let server = MockServer::start().await;
        let channel = "auth-ok";
        Mock::given(method("POST"))
            .and(path_regex(format!(r"^/+authorization/{channel}$")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "authorized": true,
                "timed_out": false,
                "reason": null
            })))
            .mount(&server)
            .await;
        let interviewer = AiloopInterviewer::new(
            make_client(&server.uri(), channel),
            true,
            Duration::from_secs(5),
        );
        let result = interviewer
            .ask_approval("Approve?", Some(Duration::from_secs(1)), None)
            .await
            .expect("ok");
        assert!(result.approved);
        assert_eq!(result.reason, "");
        assert!(!result.timeout_applied);
        assert!(!result.default_used);
    }

    #[tokio::test]
    async fn test_ask_approval_timeout_no_default_returns_105() {
        let server = MockServer::start().await;
        let channel = "auth-timeout";
        Mock::given(method("POST"))
            .and(path_regex(format!(r"^/+authorization/{channel}$")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "authorized": false,
                "timed_out": true,
                "reason": null
            })))
            .mount(&server)
            .await;
        let interviewer = AiloopInterviewer::new(
            make_client(&server.uri(), channel),
            true,
            Duration::from_secs(5),
        );
        let result = interviewer
            .ask_approval("p", Some(Duration::from_secs(1)), None)
            .await;
        let err = result.expect_err("should err");
        assert_eq!(err.code, "WFG-HUMAN-105");
    }

    #[tokio::test]
    async fn test_ask_approval_fail_fast_unreachable_returns_102() {
        let interviewer = AiloopInterviewer::new(
            unreachable_client("auth-fail"),
            true,
            Duration::from_millis(100),
        );
        let result = interviewer
            .ask_approval("p", Some(Duration::from_millis(100)), None)
            .await;
        let err = result.expect_err("should err");
        assert_eq!(err.code, "WFG-HUMAN-102");
    }

    #[tokio::test]
    async fn test_ask_approval_timeout_with_default_applies() {
        let server = MockServer::start().await;
        let channel = "auth-default";
        Mock::given(method("POST"))
            .and(path_regex(format!(r"^/+authorization/{channel}$")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "authorized": false,
                "timed_out": true,
                "reason": null
            })))
            .mount(&server)
            .await;
        let interviewer = AiloopInterviewer::new(
            make_client(&server.uri(), channel),
            true,
            Duration::from_secs(5),
        );
        let result = interviewer
            .ask_approval(
                "p",
                Some(Duration::from_secs(1)),
                Some(ApprovalDefault::Approve),
            )
            .await
            .expect("ok");
        assert!(result.approved);
        assert!(result.timeout_applied);
        assert!(result.default_used);
    }
}
