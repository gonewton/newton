#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::integrations::ailoop::tool_client::{ClientError, ToolClient};
use crate::integrations::ailoop::AiloopContext;
use crate::workflow::operators::gh_authorization::{
    AiloopApprover, ApprovalOutcome, AuthorizationRequest,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_AUTH_TIMEOUT: Duration = Duration::from_secs(300);

/// AiloopApprover backed by the in-tree ailoop HTTP transport (`ToolClient`).
///
/// Note: spec §5.2 references an external `ailoop-sdk` crate; that crate is not
/// yet published, so this implementation reuses the existing
/// `crate::integrations::ailoop::tool_client::ToolClient` HTTP wrapper. The
/// trait surface is identical; swapping in `ailoop-sdk` later is mechanical.
pub struct AiloopSdkApprover {
    client: ToolClient,
    default_channel: String,
}

impl AiloopSdkApprover {
    pub fn from_context(ctx: &AiloopContext) -> Result<Self, AppError> {
        Ok(Self {
            client: ToolClient::new(Arc::new(ctx.clone())),
            default_channel: ctx.channel().to_string(),
        })
    }

    pub fn default_channel(&self) -> &str {
        &self.default_channel
    }
}

#[async_trait]
impl AiloopApprover for AiloopSdkApprover {
    async fn authorize(&self, request: AuthorizationRequest) -> Result<ApprovalOutcome, AppError> {
        let timeout = request.timeout.unwrap_or(DEFAULT_AUTH_TIMEOUT);
        let action = request.operation.clone();
        let details = request.prompt.clone();

        match self
            .client
            .request_authorization(action, details, timeout)
            .await
        {
            Ok(resp) => {
                if resp.timed_out {
                    Ok(ApprovalOutcome::Timeout)
                } else if resp.authorized {
                    Ok(ApprovalOutcome::Approved)
                } else {
                    Ok(ApprovalOutcome::Denied {
                        reason: resp.reason,
                    })
                }
            }
            Err(e) => Ok(ApprovalOutcome::Unavailable {
                cause: format!("{}", classify(e)),
            }),
        }
    }
}

fn classify(err: ClientError) -> ClientError {
    err
}
