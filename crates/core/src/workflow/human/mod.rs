#![allow(clippy::result_large_err)]

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

#[cfg(any(test, feature = "test-utils"))]
pub mod mock_ailoop;

pub use ailoop::AiloopInterviewer;
pub use audit::AuditEntry;
pub use console::ConsoleInterviewer;

#[cfg(any(test, feature = "test-utils"))]
pub use mock_ailoop::MockAiloopInterviewer;

use std::sync::Arc;

/// Type alias for a deferred interviewer constructor used by operators.
///
/// The closure is invoked at most once per operator instance (on the first
/// human prompt) and the resulting `Arc<dyn Interviewer>` is cached.
pub type InterviewerProvider =
    Arc<dyn Fn() -> Result<Arc<dyn Interviewer>, crate::core::error::AppError> + Send + Sync>;

/// Resolve an `Interviewer` by delegating exclusively to ailoop.
///
/// Returns `Ok(AiloopInterviewer)` when an enabled `AiloopContext` is provided.
/// Otherwise returns `Err(AppError)` with code `HIL-AILOOP-001`.
///
/// This function MUST NOT fall back to console or any other transport.
pub fn resolve_interviewer(
    ailoop: Option<&crate::integrations::ailoop::AiloopContext>,
    default_timeout: Duration,
) -> Result<Arc<dyn Interviewer>, crate::core::error::AppError> {
    match ailoop {
        Some(ctx) if ctx.is_enabled() => Ok(Arc::new(AiloopInterviewer::new(
            ctx.ws_url().to_string(),
            ctx.channel().to_string(),
            ctx.config.fail_fast,
            default_timeout,
        ))),
        _ => Err(missing_ailoop_error()),
    }
}

fn missing_ailoop_error() -> crate::core::error::AppError {
    crate::core::error::AppError::new(
        crate::core::types::ErrorCategory::ValidationError,
        "human-in-the-loop operator requires an enabled ailoop context; \
         configure ailoop (.newton/configs/monitor.conf and \
         NEWTON_AILOOP_INTEGRATION=1). See \
         docs/operators/human_decision.md#configuration",
    )
    .with_code("HIL-AILOOP-001")
}

/// Build an `InterviewerProvider` that re-evaluates ailoop availability when
/// first invoked. The provided context is captured by clone so the closure can
/// outlive the caller's borrow.
pub fn lazy_interviewer_provider(
    ailoop: Option<crate::integrations::ailoop::AiloopContext>,
    default_timeout: Duration,
) -> InterviewerProvider {
    Arc::new(move || resolve_interviewer(ailoop.as_ref(), default_timeout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::ailoop::config::AiloopConfig;
    use crate::integrations::ailoop::AiloopContext;
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
    fn resolve_interviewer_with_enabled_context_returns_ailoop() {
        let ctx = make_ctx(true);
        let i = resolve_interviewer(Some(&ctx), Duration::from_secs(60))
            .expect("enabled ctx should resolve");
        assert_eq!(i.interviewer_type(), "ailoop");
    }

    #[test]
    fn resolve_interviewer_with_no_context_errors() {
        let err = match resolve_interviewer(None, Duration::from_secs(60)) {
            Ok(_) => panic!("missing ctx must error"),
            Err(e) => e,
        };
        assert_eq!(err.code, "HIL-AILOOP-001");
        assert!(matches!(
            err.category,
            crate::core::types::ErrorCategory::ValidationError
        ));
    }

    #[test]
    fn resolve_interviewer_with_disabled_context_errors() {
        let ctx = make_ctx(false);
        let err = match resolve_interviewer(Some(&ctx), Duration::from_secs(60)) {
            Ok(_) => panic!("disabled ctx must error"),
            Err(e) => e,
        };
        assert_eq!(err.code, "HIL-AILOOP-001");
    }

    #[test]
    fn lazy_provider_does_not_construct_eagerly() {
        let provider = lazy_interviewer_provider(None, Duration::from_secs(60));
        // Provider is constructed but never invoked — no error yet.
        let _ = &provider; // keep alive
    }

    #[test]
    fn lazy_provider_resolves_on_invocation() {
        let ctx = make_ctx(true);
        let provider = lazy_interviewer_provider(Some(ctx), Duration::from_secs(60));
        let i = provider().unwrap_or_else(|_| panic!("enabled ctx should resolve via provider"));
        assert_eq!(i.interviewer_type(), "ailoop");
    }
}
