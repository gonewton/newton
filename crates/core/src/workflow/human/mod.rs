use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::str::FromStr;
use std::time::Duration;

/// Default outcome applied when an approval times out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDefault {
    Approve,
    Reject,
}

impl ApprovalDefault {
    /// Return a lowercase string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalDefault::Approve => "approve",
            ApprovalDefault::Reject => "reject",
        }
    }
}

impl FromStr for ApprovalDefault {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_lowercase().as_str() {
            "approve" => Ok(ApprovalDefault::Approve),
            "reject" => Ok(ApprovalDefault::Reject),
            _ => Err("must be 'approve' or 'reject'"),
        }
    }
}

impl std::fmt::Display for ApprovalDefault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Result returned from an approval prompt.
#[derive(Debug, Clone)]
pub struct ApprovalResult {
    pub approved: bool,
    pub reason: String,
    pub timestamp: DateTime<Utc>,
    pub timeout_applied: bool,
    pub default_used: bool,
}

impl ApprovalResult {
    pub fn with_defaults(approved: bool, reason: String) -> Self {
        Self {
            approved,
            reason,
            timestamp: Utc::now(),
            timeout_applied: false,
            default_used: false,
        }
    }
}

/// Result returned from a multi-choice prompt.
#[derive(Debug, Clone)]
pub struct DecisionResult {
    pub choice: String,
    pub timestamp: DateTime<Utc>,
    pub timeout_applied: bool,
    pub default_used: bool,
    pub response_text: Option<String>,
}

/// Interface for blocking human input flows within workflow operators.
#[async_trait]
pub trait Interviewer: Send + Sync + 'static {
    /// Human-friendly identifier used in audit logs.
    fn interviewer_type(&self) -> &'static str;

    async fn ask_approval(
        &self,
        prompt: &str,
        timeout: Option<Duration>,
        default_on_timeout: Option<ApprovalDefault>,
    ) -> Result<ApprovalResult, crate::core::error::AppError>;

    async fn ask_choice(
        &self,
        prompt: &str,
        choices: &[String],
        timeout: Option<Duration>,
        default_choice: Option<&str>,
    ) -> Result<DecisionResult, crate::core::error::AppError>;
}

pub mod ailoop;
pub mod audit;
pub mod console;

pub use ailoop::AiloopInterviewer;
pub use audit::AuditEntry;
pub use console::ConsoleInterviewer;

use std::sync::Arc;

/// Selects the appropriate `Interviewer` backend based on environment override
/// (`NEWTON_HITL_TRANSPORT`) and the presence of an `AiloopContext`.
///
/// Precedence:
/// 1. `NEWTON_HITL_TRANSPORT=console` → console.
/// 2. `NEWTON_HITL_TRANSPORT=ailoop` → ailoop if a context is provided, else
///    log a warning and fall back to console.
/// 3. If an enabled `AiloopContext` is available → ailoop.
/// 4. Otherwise → console.
pub fn build_interviewer(
    ailoop: Option<&crate::integrations::ailoop::AiloopContext>,
    default_timeout: std::time::Duration,
) -> Arc<dyn Interviewer> {
    let override_env = std::env::var("NEWTON_HITL_TRANSPORT")
        .ok()
        .map(|v| v.to_lowercase());
    match override_env.as_deref() {
        Some("console") => Arc::new(ConsoleInterviewer::new()),
        Some("ailoop") => {
            if let Some(ctx) = ailoop {
                Arc::new(AiloopInterviewer::new(
                    ctx.ws_url().to_string(),
                    ctx.channel().to_string(),
                    ctx.config.fail_fast,
                    default_timeout,
                ))
            } else {
                tracing::warn!(
                    "NEWTON_HITL_TRANSPORT=ailoop requested but no AiloopContext available; falling back to console"
                );
                Arc::new(ConsoleInterviewer::new())
            }
        }
        _ => {
            if let Some(ctx) = ailoop {
                if ctx.is_enabled() {
                    return Arc::new(AiloopInterviewer::new(
                        ctx.ws_url().to_string(),
                        ctx.channel().to_string(),
                        ctx.config.fail_fast,
                        default_timeout,
                    ));
                }
            }
            Arc::new(ConsoleInterviewer::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::ailoop::config::AiloopConfig;
    use crate::integrations::ailoop::AiloopContext;
    use serial_test::serial;
    use std::path::PathBuf;
    use url::Url;

    fn make_ctx(enabled: bool) -> AiloopContext {
        let config = AiloopConfig {
            ws_url: Url::parse("ws://127.0.0.1:9999").unwrap(),
            channel: "test".to_string(),
            enabled,
            fail_fast: false,
        };
        AiloopContext::new(config, PathBuf::from("/tmp"), "test".to_string())
    }

    #[test]
    #[serial]
    fn test_build_interviewer_no_context_no_override() {
        std::env::remove_var("NEWTON_HITL_TRANSPORT");
        let i = build_interviewer(None, std::time::Duration::from_secs(60));
        assert_eq!(i.interviewer_type(), "console");
    }

    #[test]
    #[serial]
    fn test_build_interviewer_with_context_no_override() {
        std::env::remove_var("NEWTON_HITL_TRANSPORT");
        let ctx = make_ctx(true);
        let i = build_interviewer(Some(&ctx), std::time::Duration::from_secs(60));
        assert_eq!(i.interviewer_type(), "ailoop");
    }

    #[test]
    #[serial]
    fn test_build_interviewer_console_override_with_context() {
        std::env::set_var("NEWTON_HITL_TRANSPORT", "console");
        let ctx = make_ctx(true);
        let i = build_interviewer(Some(&ctx), std::time::Duration::from_secs(60));
        std::env::remove_var("NEWTON_HITL_TRANSPORT");
        assert_eq!(i.interviewer_type(), "console");
    }

    #[test]
    #[serial]
    fn test_build_interviewer_ailoop_override_no_context_warns_and_falls_back() {
        std::env::set_var("NEWTON_HITL_TRANSPORT", "ailoop");
        let i = build_interviewer(None, std::time::Duration::from_secs(60));
        std::env::remove_var("NEWTON_HITL_TRANSPORT");
        assert_eq!(i.interviewer_type(), "console");
    }
}
