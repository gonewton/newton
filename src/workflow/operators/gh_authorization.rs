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

const MAX_TIMEOUT_SECONDS: f64 = 86_400.0;
const DEFAULT_TIMEOUT_SECONDS: u64 = 300;

pub fn parse_authorization_params(
    map: &Map<String, Value>,
) -> Result<AuthorizationParams, AppError> {
    let require = match map.get("require_authorization") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "require_authorization must be a boolean",
            ));
        }
    };

    let prompt = match map.get("authorization_prompt") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            if s.is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "authorization_prompt must be a non-empty string",
                ));
            }
            Some(s.clone())
        }
        Some(_) => {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "authorization_prompt must be a string",
            ));
        }
    };

    let channel = match map.get("authorization_channel") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            if s.is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "authorization_channel must be a non-empty string",
                ));
            }
            Some(s.clone())
        }
        Some(_) => {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "authorization_channel must be a string",
            ));
        }
    };

    let timeout = match map.get("authorization_timeout_seconds") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let n = v.as_f64().ok_or_else(|| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    "authorization_timeout_seconds must be a number",
                )
                .with_code("WFG-GH-AUTH-005")
            })?;
            if !n.is_finite() || n <= 0.0 || n > MAX_TIMEOUT_SECONDS {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "authorization_timeout_seconds must be > 0 and <= {MAX_TIMEOUT_SECONDS}, got: {n}"
                    ),
                )
                .with_code("WFG-GH-AUTH-005"));
            }
            Some(Duration::from_millis((n * 1000.0) as u64))
        }
    };

    let on_unavailable = match map.get("on_authorization_unavailable") {
        None | Some(Value::Null) => OnUnavailable::Fail,
        Some(Value::String(s)) => match s.as_str() {
            "fail" => OnUnavailable::Fail,
            "skip" => OnUnavailable::Skip,
            other => {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("on_authorization_unavailable must be 'fail' or 'skip'; got: {other}"),
                )
                .with_code("WFG-GH-AUTH-004"));
            }
        },
        Some(_) => {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "on_authorization_unavailable must be a string",
            )
            .with_code("WFG-GH-AUTH-004"));
        }
    };

    Ok(AuthorizationParams {
        require,
        prompt,
        channel,
        timeout,
        on_unavailable,
    })
}

pub fn default_timeout() -> Duration {
    Duration::from_secs(DEFAULT_TIMEOUT_SECONDS)
}

/// Compute a stable, short hash of a serializable payload for request id derivation.
pub fn short_hash(payload: &Value) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let canonical = payload.to_string();
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn parse_defaults() {
        let p = parse_authorization_params(&obj(json!({}))).unwrap();
        assert!(!p.require);
        assert!(p.prompt.is_none());
        assert!(p.channel.is_none());
        assert!(p.timeout.is_none());
        assert_eq!(p.on_unavailable, OnUnavailable::Fail);
    }

    #[test]
    fn parse_full_valid() {
        let p = parse_authorization_params(&obj(json!({
            "require_authorization": true,
            "authorization_prompt": "approve me",
            "authorization_channel": "release-bot",
            "authorization_timeout_seconds": 120,
            "on_authorization_unavailable": "skip"
        })))
        .unwrap();
        assert!(p.require);
        assert_eq!(p.prompt.as_deref(), Some("approve me"));
        assert_eq!(p.channel.as_deref(), Some("release-bot"));
        assert_eq!(p.timeout, Some(Duration::from_secs(120)));
        assert_eq!(p.on_unavailable, OnUnavailable::Skip);
    }

    #[test]
    fn parse_rejects_non_bool_require() {
        let err =
            parse_authorization_params(&obj(json!({"require_authorization": "yes"}))).unwrap_err();
        assert_eq!(err.category, ErrorCategory::ValidationError);
    }

    #[test]
    fn parse_rejects_unknown_on_unavailable() {
        let err = parse_authorization_params(&obj(json!({"on_authorization_unavailable": "halt"})))
            .unwrap_err();
        assert_eq!(err.code, "WFG-GH-AUTH-004");
    }

    #[test]
    fn parse_rejects_zero_timeout() {
        let err = parse_authorization_params(&obj(json!({"authorization_timeout_seconds": 0})))
            .unwrap_err();
        assert_eq!(err.code, "WFG-GH-AUTH-005");
    }

    #[test]
    fn parse_rejects_negative_timeout() {
        let err = parse_authorization_params(&obj(json!({"authorization_timeout_seconds": -5})))
            .unwrap_err();
        assert_eq!(err.code, "WFG-GH-AUTH-005");
    }

    #[test]
    fn parse_rejects_huge_timeout() {
        let err =
            parse_authorization_params(&obj(json!({"authorization_timeout_seconds": 86_401})))
                .unwrap_err();
        assert_eq!(err.code, "WFG-GH-AUTH-005");
    }

    #[test]
    fn parse_rejects_empty_prompt() {
        let err =
            parse_authorization_params(&obj(json!({"authorization_prompt": ""}))).unwrap_err();
        assert_eq!(err.category, ErrorCategory::ValidationError);
    }

    #[test]
    fn parse_rejects_empty_channel() {
        let err =
            parse_authorization_params(&obj(json!({"authorization_channel": ""}))).unwrap_err();
        assert_eq!(err.category, ErrorCategory::ValidationError);
    }
}
