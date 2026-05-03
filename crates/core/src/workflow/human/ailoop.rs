#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::human::{ApprovalDefault, ApprovalResult, DecisionResult, Interviewer};
use ailoop_core::models::{MessageContent, ResponseType};
use async_trait::async_trait;
use chrono::Utc;
use std::time::Duration;

const ACTION_HEADLINE_LIMIT: usize = 80;

pub struct AiloopInterviewer {
    ws_url: String,
    channel: String,
    fail_fast: bool,
    default_timeout: Duration,
}

impl AiloopInterviewer {
    pub fn new(
        ws_url: String,
        channel: String,
        fail_fast: bool,
        default_timeout: Duration,
    ) -> Self {
        Self {
            ws_url,
            channel,
            fail_fast,
            default_timeout,
        }
    }

    fn resolve_timeout(&self, timeout: Option<Duration>) -> Duration {
        timeout.unwrap_or(self.default_timeout)
    }
}

fn truncate_action(prompt: &str) -> String {
    prompt.chars().take(ACTION_HEADLINE_LIMIT).collect()
}

fn to_timeout_secs(d: Duration) -> u32 {
    d.as_secs().min(u32::MAX as u64) as u32
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

        let result = ailoop_core::client::authorize(
            &self.ws_url,
            &self.channel,
            &action,
            to_timeout_secs(effective_timeout),
        )
        .await;

        match result {
            Ok(Some(msg)) => match msg.content {
                MessageContent::Response {
                    response_type,
                    answer,
                } => match response_type {
                    ResponseType::AuthorizationApproved => Ok(ApprovalResult {
                        approved: true,
                        reason: answer.unwrap_or_default(),
                        timestamp: Utc::now(),
                        timeout_applied: false,
                        default_used: false,
                    }),
                    ResponseType::AuthorizationDenied => Ok(ApprovalResult {
                        approved: false,
                        reason: answer.unwrap_or_default(),
                        timestamp: Utc::now(),
                        timeout_applied: false,
                        default_used: false,
                    }),
                    ResponseType::Timeout => handle_approval_timeout(default_on_timeout),
                    ResponseType::Cancelled | ResponseType::Text => handle_approval_unavailable(
                        "ailoop returned unexpected response type",
                        default_on_timeout,
                        self.fail_fast,
                    ),
                },
                _ => handle_approval_unavailable(
                    "ailoop returned unexpected message content",
                    default_on_timeout,
                    self.fail_fast,
                ),
            },
            // No response received within the timeout window
            Ok(None) => handle_approval_timeout(default_on_timeout),
            Err(e) => {
                handle_approval_transport_error(&e.to_string(), default_on_timeout, self.fail_fast)
            }
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

        let result = ailoop_core::client::ask(
            &self.ws_url,
            &self.channel,
            &question,
            to_timeout_secs(effective_timeout),
            Some(choices.to_vec()),
        )
        .await;

        match result {
            Ok(Some(msg)) => match msg.content {
                MessageContent::Response {
                    response_type,
                    answer,
                } => match response_type {
                    ResponseType::Text => {
                        let answer = match answer {
                            Some(a) => a,
                            None => {
                                return Err(AppError::new(
                                    ErrorCategory::ValidationError,
                                    "ailoop returned no answer and timed_out=false",
                                )
                                .with_code("WFG-HUMAN-104"))
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
                    ResponseType::Timeout => handle_choice_timeout(default_choice),
                    ResponseType::Cancelled
                    | ResponseType::AuthorizationApproved
                    | ResponseType::AuthorizationDenied => handle_choice_unavailable(
                        "ailoop returned unexpected response type",
                        default_choice,
                        self.fail_fast,
                    ),
                },
                _ => handle_choice_unavailable(
                    "ailoop returned unexpected message content",
                    default_choice,
                    self.fail_fast,
                ),
            },
            Ok(None) => handle_choice_timeout(default_choice),
            Err(e) => handle_choice_transport_error(&e.to_string(), default_choice, self.fail_fast),
        }
    }
}

fn handle_approval_timeout(
    default_on_timeout: Option<ApprovalDefault>,
) -> Result<ApprovalResult, AppError> {
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
        "ailoop approval request timed out and no default_on_timeout configured",
    )
    .with_code("WFG-HUMAN-105"))
}

fn handle_approval_unavailable(
    cause: &str,
    default_on_timeout: Option<ApprovalDefault>,
    fail_fast: bool,
) -> Result<ApprovalResult, AppError> {
    if fail_fast {
        return Err(AppError::new(
            ErrorCategory::IoError,
            format!("ailoop authorize transport failure: {cause}"),
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
        format!("ailoop authorize transport failure (no default_on_timeout): {cause}"),
    )
    .with_code("WFG-HUMAN-105"))
}

fn handle_approval_transport_error(
    err: &str,
    default_on_timeout: Option<ApprovalDefault>,
    fail_fast: bool,
) -> Result<ApprovalResult, AppError> {
    handle_approval_unavailable(err, default_on_timeout, fail_fast)
}

fn handle_choice_timeout(default_choice: Option<&str>) -> Result<DecisionResult, AppError> {
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
        "ailoop ask request timed out and no default_choice configured",
    )
    .with_code("WFG-HUMAN-103"))
}

fn handle_choice_unavailable(
    cause: &str,
    default_choice: Option<&str>,
    fail_fast: bool,
) -> Result<DecisionResult, AppError> {
    if fail_fast {
        return Err(AppError::new(
            ErrorCategory::IoError,
            format!("ailoop ask transport failure: {cause}"),
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
        format!("ailoop ask transport failure (no default_choice): {cause}"),
    )
    .with_code("WFG-HUMAN-103"))
}

fn handle_choice_transport_error(
    err: &str,
    default_choice: Option<&str>,
    fail_fast: bool,
) -> Result<DecisionResult, AppError> {
    handle_choice_unavailable(err, default_choice, fail_fast)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailoop_core::models::{Message, MessageContent, ResponseType};
    use futures::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    /// Start a minimal WS server that responds once with `response_content`.
    /// Returns the ws:// URL and a JoinHandle.
    async fn start_ws_responder(
        response_content: MessageContent,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("ws://127.0.0.1:{port}");

        let handle = tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> =
                    tokio_tungstenite::accept_async(stream).await.unwrap();
                let (mut sender, mut receiver) = ws.split();

                if let Some(Ok(WsMessage::Text(text))) = receiver.next().await {
                    let msg: Message = serde_json::from_str(&text).unwrap();
                    let reply = Message::response(msg.channel.clone(), response_content, msg.id);
                    let reply_json = serde_json::to_string(&reply).unwrap();
                    let _ = sender.send(WsMessage::Text(reply_json)).await;
                }
            }
        });

        // Give the listener a moment to become ready
        tokio::time::sleep(Duration::from_millis(5)).await;
        (url, handle)
    }

    fn make_interviewer(ws_url: &str, fail_fast: bool) -> AiloopInterviewer {
        AiloopInterviewer::new(
            ws_url.to_string(),
            "test-channel".to_string(),
            fail_fast,
            Duration::from_secs(5),
        )
    }

    fn unreachable_interviewer(fail_fast: bool) -> AiloopInterviewer {
        AiloopInterviewer::new(
            "ws://127.0.0.1:1".to_string(),
            "test-channel".to_string(),
            fail_fast,
            Duration::from_millis(100),
        )
    }

    // ── ask_approval ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_ask_approval_approved() {
        let (url, _h) = start_ws_responder(MessageContent::Response {
            answer: None,
            response_type: ResponseType::AuthorizationApproved,
        })
        .await;
        let interviewer = make_interviewer(&url, true);
        let result = interviewer
            .ask_approval("Deploy?", Some(Duration::from_secs(2)), None)
            .await
            .unwrap();
        assert!(result.approved);
        assert!(!result.timeout_applied);
        assert!(!result.default_used);
    }

    #[tokio::test]
    async fn test_ask_approval_denied() {
        let (url, _h) = start_ws_responder(MessageContent::Response {
            answer: Some("not now".to_string()),
            response_type: ResponseType::AuthorizationDenied,
        })
        .await;
        let interviewer = make_interviewer(&url, true);
        let result = interviewer
            .ask_approval("Deploy?", Some(Duration::from_secs(2)), None)
            .await
            .unwrap();
        assert!(!result.approved);
        assert_eq!(result.reason, "not now");
    }

    #[tokio::test]
    async fn test_ask_approval_server_timeout_with_default_approve() {
        let (url, _h) = start_ws_responder(MessageContent::Response {
            answer: None,
            response_type: ResponseType::Timeout,
        })
        .await;
        let interviewer = make_interviewer(&url, false);
        let result = interviewer
            .ask_approval(
                "Deploy?",
                Some(Duration::from_secs(2)),
                Some(ApprovalDefault::Approve),
            )
            .await
            .unwrap();
        assert!(result.approved);
        assert!(result.timeout_applied);
        assert!(result.default_used);
    }

    #[tokio::test]
    async fn test_ask_approval_timeout_no_default_returns_105() {
        let (url, _h) = start_ws_responder(MessageContent::Response {
            answer: None,
            response_type: ResponseType::Timeout,
        })
        .await;
        let interviewer = make_interviewer(&url, true);
        let err = interviewer
            .ask_approval("Deploy?", Some(Duration::from_secs(2)), None)
            .await
            .unwrap_err();
        assert_eq!(err.code, "WFG-HUMAN-105");
    }

    #[tokio::test]
    async fn test_ask_approval_fail_fast_unreachable_returns_102() {
        let interviewer = unreachable_interviewer(true);
        let err = interviewer
            .ask_approval("Deploy?", Some(Duration::from_millis(100)), None)
            .await
            .unwrap_err();
        assert_eq!(err.code, "WFG-HUMAN-102");
    }

    #[tokio::test]
    async fn test_ask_approval_no_failfast_no_default_unreachable_returns_105() {
        let interviewer = unreachable_interviewer(false);
        let err = interviewer
            .ask_approval("Deploy?", Some(Duration::from_millis(100)), None)
            .await
            .unwrap_err();
        assert_eq!(err.code, "WFG-HUMAN-105");
    }

    #[tokio::test]
    async fn test_ask_approval_no_failfast_with_default_falls_back() {
        let interviewer = unreachable_interviewer(false);
        let result = interviewer
            .ask_approval(
                "Deploy?",
                Some(Duration::from_millis(100)),
                Some(ApprovalDefault::Reject),
            )
            .await
            .unwrap();
        assert!(!result.approved);
        assert!(result.default_used);
    }

    // ── ask_choice ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_ask_choice_success_match() {
        let (url, _h) = start_ws_responder(MessageContent::Response {
            answer: Some("fix".to_string()),
            response_type: ResponseType::Text,
        })
        .await;
        let interviewer = make_interviewer(&url, true);
        let choices = vec!["fix".to_string(), "skip".to_string()];
        let result = interviewer
            .ask_choice(
                "Which?",
                &choices,
                Some(Duration::from_secs(2)),
                Some("skip"),
            )
            .await
            .unwrap();
        assert_eq!(result.choice, "fix");
        assert!(!result.timeout_applied);
        assert!(!result.default_used);
    }

    #[tokio::test]
    async fn test_ask_choice_unmatched_answer_returns_104() {
        let (url, _h) = start_ws_responder(MessageContent::Response {
            answer: Some("banana".to_string()),
            response_type: ResponseType::Text,
        })
        .await;
        let interviewer = make_interviewer(&url, true);
        let choices = vec!["apple".to_string(), "cherry".to_string()];
        let err = interviewer
            .ask_choice("Pick?", &choices, Some(Duration::from_secs(2)), None)
            .await
            .unwrap_err();
        assert_eq!(err.code, "WFG-HUMAN-104");
    }

    #[tokio::test]
    async fn test_ask_choice_fail_fast_unreachable_returns_101() {
        let interviewer = unreachable_interviewer(true);
        let choices = vec!["a".to_string(), "b".to_string()];
        let err = interviewer
            .ask_choice("Pick?", &choices, Some(Duration::from_millis(100)), None)
            .await
            .unwrap_err();
        assert_eq!(err.code, "WFG-HUMAN-101");
    }

    #[tokio::test]
    async fn test_ask_choice_no_failfast_no_default_returns_103() {
        let interviewer = unreachable_interviewer(false);
        let choices = vec!["a".to_string(), "b".to_string()];
        let err = interviewer
            .ask_choice("Pick?", &choices, Some(Duration::from_millis(100)), None)
            .await
            .unwrap_err();
        assert_eq!(err.code, "WFG-HUMAN-103");
    }

    #[tokio::test]
    async fn test_ask_choice_no_failfast_with_default_falls_back() {
        let interviewer = unreachable_interviewer(false);
        let choices = vec!["a".to_string(), "b".to_string()];
        let result = interviewer
            .ask_choice(
                "Pick?",
                &choices,
                Some(Duration::from_millis(100)),
                Some("b"),
            )
            .await
            .unwrap();
        assert_eq!(result.choice, "b");
        assert!(result.default_used);
    }

    #[tokio::test]
    async fn test_ask_choice_server_timeout_with_default() {
        let (url, _h) = start_ws_responder(MessageContent::Response {
            answer: None,
            response_type: ResponseType::Timeout,
        })
        .await;
        let interviewer = make_interviewer(&url, false);
        let choices = vec!["a".to_string(), "b".to_string()];
        let result = interviewer
            .ask_choice("Pick?", &choices, Some(Duration::from_secs(2)), Some("a"))
            .await
            .unwrap();
        assert_eq!(result.choice, "a");
        assert!(result.timeout_applied);
        assert!(result.default_used);
    }

    // ── unit tests for helpers ────────────────────────────────────────────────

    #[test]
    fn test_truncate_action_short() {
        let s = "short prompt";
        assert_eq!(truncate_action(s), s);
    }

    #[test]
    fn test_truncate_action_long() {
        let long = "x".repeat(200);
        let result = truncate_action(&long);
        assert_eq!(result.len(), ACTION_HEADLINE_LIMIT);
    }

    #[test]
    fn test_to_timeout_secs_normal() {
        assert_eq!(to_timeout_secs(Duration::from_secs(60)), 60);
    }
}
