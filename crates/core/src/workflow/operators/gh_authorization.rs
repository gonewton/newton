#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use async_trait::async_trait;
use serde_json::{Map, Value};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct AuthorizationRequest {
    pub request_id: String,
    pub prompt: String,
    pub channel: Option<String>,
    pub timeout: Option<Duration>,
    pub operation: String,
    pub task_id: Option<String>,
}

#[derive(Clone, Debug)]
pub enum ApprovalOutcome {
    Approved,
    Denied { reason: Option<String> },
    Timeout,
    Unavailable { cause: String },
}

#[async_trait]
pub trait AiloopApprover: Send + Sync + 'static {
    async fn authorize(&self, request: AuthorizationRequest) -> Result<ApprovalOutcome, AppError>;
}

pub struct NoopApprover;

#[async_trait]
impl AiloopApprover for NoopApprover {
    async fn authorize(&self, _: AuthorizationRequest) -> Result<ApprovalOutcome, AppError> {
        Ok(ApprovalOutcome::Unavailable {
            cause: "ailoop disabled".into(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnUnavailable {
    Fail,
    Skip,
}

#[derive(Clone, Debug)]
pub struct AuthorizationParams {
    pub require: bool,
    pub prompt: Option<String>,
    pub channel: Option<String>,
    pub timeout: Option<Duration>,
    pub on_unavailable: OnUnavailable,
}

impl Default for AuthorizationParams {
    fn default() -> Self {
        Self {
            require: false,
            prompt: None,
            channel: None,
            timeout: None,
            on_unavailable: OnUnavailable::Fail,
        }
    }
}

const MAX_AUTH_TIMEOUT_SECS: u64 = 86_400;

pub fn parse_authorization_params(
    map: &Map<String, Value>,
) -> Result<AuthorizationParams, AppError> {
    let mut out = AuthorizationParams::default();

    if let Some(v) = map.get("require_authorization") {
        match v.as_bool() {
            Some(b) => out.require = b,
            None => {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "require_authorization must be a boolean",
                ));
            }
        }
    }

    if let Some(v) = map.get("authorization_prompt") {
        let s = v.as_str().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ValidationError,
                "authorization_prompt must be a string",
            )
        })?;
        if s.is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "authorization_prompt must be non-empty",
            ));
        }
        out.prompt = Some(s.to_string());
    }

    if let Some(v) = map.get("authorization_channel") {
        let s = v.as_str().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ValidationError,
                "authorization_channel must be a string",
            )
        })?;
        if s.is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "authorization_channel must be non-empty",
            ));
        }
        out.channel = Some(s.to_string());
    }

    if let Some(v) = map.get("authorization_timeout_seconds") {
        let n = v.as_f64().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ValidationError,
                "authorization_timeout_seconds must be a number",
            )
            .with_code("WFG-GH-AUTH-005")
        })?;
        if !n.is_finite() || n <= 0.0 || n > MAX_AUTH_TIMEOUT_SECS as f64 {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!(
                    "authorization_timeout_seconds must be a finite number in (0, {MAX_AUTH_TIMEOUT_SECS}]"
                ),
            )
            .with_code("WFG-GH-AUTH-005"));
        }
        out.timeout = Some(Duration::from_millis((n * 1000.0) as u64));
    }

    if let Some(v) = map.get("on_authorization_unavailable") {
        let s = v.as_str().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ValidationError,
                "on_authorization_unavailable must be a string",
            )
            .with_code("WFG-GH-AUTH-004")
        })?;
        out.on_unavailable = match s {
            "fail" => OnUnavailable::Fail,
            "skip" => OnUnavailable::Skip,
            other => {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("on_authorization_unavailable must be 'fail' or 'skip'; got: {other}"),
                )
                .with_code("WFG-GH-AUTH-004"));
            }
        };
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map_of(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn parse_defaults() {
        let p = parse_authorization_params(&Map::new()).unwrap();
        assert!(!p.require);
        assert!(p.prompt.is_none());
        assert!(p.channel.is_none());
        assert!(p.timeout.is_none());
        assert_eq!(p.on_unavailable, OnUnavailable::Fail);
    }

    #[test]
    fn parse_full() {
        let m = map_of(json!({
            "require_authorization": true,
            "authorization_prompt": "Approve?",
            "authorization_channel": "release-bot",
            "authorization_timeout_seconds": 60,
            "on_authorization_unavailable": "skip"
        }));
        let p = parse_authorization_params(&m).unwrap();
        assert!(p.require);
        assert_eq!(p.prompt.as_deref(), Some("Approve?"));
        assert_eq!(p.channel.as_deref(), Some("release-bot"));
        assert_eq!(p.timeout, Some(Duration::from_secs(60)));
        assert_eq!(p.on_unavailable, OnUnavailable::Skip);
    }

    #[test]
    fn rejects_unknown_on_unavailable() {
        let m = map_of(json!({"on_authorization_unavailable": "halt"}));
        let err = parse_authorization_params(&m).unwrap_err();
        assert_eq!(err.code, "WFG-GH-AUTH-004");
    }

    #[test]
    fn rejects_zero_timeout() {
        let m = map_of(json!({"authorization_timeout_seconds": 0}));
        let err = parse_authorization_params(&m).unwrap_err();
        assert_eq!(err.code, "WFG-GH-AUTH-005");
    }

    #[test]
    fn rejects_negative_timeout() {
        let m = map_of(json!({"authorization_timeout_seconds": -5}));
        let err = parse_authorization_params(&m).unwrap_err();
        assert_eq!(err.code, "WFG-GH-AUTH-005");
    }

    #[test]
    fn rejects_too_large_timeout() {
        let m = map_of(json!({"authorization_timeout_seconds": 86_401}));
        let err = parse_authorization_params(&m).unwrap_err();
        assert_eq!(err.code, "WFG-GH-AUTH-005");
    }

    #[test]
    fn rejects_non_bool_require() {
        let m = map_of(json!({"require_authorization": "yes"}));
        assert!(parse_authorization_params(&m).is_err());
    }

    #[test]
    fn rejects_empty_prompt() {
        let m = map_of(json!({"authorization_prompt": ""}));
        assert!(parse_authorization_params(&m).is_err());
    }

    #[test]
    fn rejects_empty_channel() {
        let m = map_of(json!({"authorization_channel": ""}));
        assert!(parse_authorization_params(&m).is_err());
    }
}
