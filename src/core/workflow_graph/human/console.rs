use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::human::{
    ApprovalDefault, ApprovalResult, DecisionResult, Interviewer,
};
use async_trait::async_trait;
use chrono::Utc;
use std::io::{self, Write};
use std::time::Duration;
use tokio::task::spawn_blocking;
use tokio::time::timeout;

pub struct ConsoleInterviewer;

impl ConsoleInterviewer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ConsoleInterviewer {
    fn default() -> Self {
        Self::new()
    }
}

async fn read_line_blocking() -> Result<String, AppError> {
    spawn_blocking(|| {
        let mut buffer = String::new();
        io::stdin().read_line(&mut buffer).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to read stdin: {}", err),
            )
        })?;
        Ok(buffer)
    })
    .await
    .map_err(|err| {
        AppError::new(
            ErrorCategory::InternalError,
            format!("console input task cancelled: {}", err),
        )
    })?
}

async fn read_input_with_timeout(
    timeout_duration: Option<Duration>,
) -> Result<(Option<String>, bool), AppError> {
    if let Some(duration) = timeout_duration {
        match timeout(duration, read_line_blocking()).await {
            Ok(line) => Ok((Some(line?), false)),
            Err(_) => Ok((None, true)),
        }
    } else {
        let line = read_line_blocking().await?;
        Ok((Some(line), false))
    }
}

#[async_trait]
impl Interviewer for ConsoleInterviewer {
    fn interviewer_type(&self) -> &'static str {
        "console"
    }

    async fn ask_approval(
        &self,
        prompt: &str,
        timeout: Option<Duration>,
        default_on_timeout: Option<ApprovalDefault>,
    ) -> Result<ApprovalResult, AppError> {
        loop {
            print!("{} (approve/reject): ", prompt);
            io::stdout().flush().ok();
            let (line_opt, timed_out) = read_input_with_timeout(timeout).await?;
            if timed_out {
                let default = default_on_timeout.unwrap_or(ApprovalDefault::Reject);
                return Ok(ApprovalResult {
                    approved: matches!(default, ApprovalDefault::Approve),
                    reason: format!("default_on_timeout={}", default.as_str()),
                    timestamp: Utc::now(),
                    timeout_applied: true,
                    default_used: true,
                });
            }

            let line = line_opt.unwrap_or_default();
            let trimmed = line.trim();
            if trimmed.is_empty() {
                println!("Please respond with 'approve' or 'reject'.");
                continue;
            }

            let mut parts = trimmed.splitn(2, char::is_whitespace);
            let first = parts.next().unwrap_or("").to_lowercase();
            let reason = parts.next().unwrap_or("").trim().to_string();

            match first.as_str() {
                "approve" | "yes" | "y" => {
                    return Ok(ApprovalResult {
                        approved: true,
                        reason,
                        timestamp: Utc::now(),
                        timeout_applied: false,
                        default_used: false,
                    });
                }
                "reject" | "no" | "n" => {
                    return Ok(ApprovalResult {
                        approved: false,
                        reason,
                        timestamp: Utc::now(),
                        timeout_applied: false,
                        default_used: false,
                    });
                }
                _ => {
                    println!("Please respond with 'approve' or 'reject'.");
                    continue;
                }
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
        println!("{}", prompt);
        for (idx, choice) in choices.iter().enumerate() {
            println!("{:>2}: {}", idx + 1, choice);
        }
        print!("Enter choice: ");
        io::stdout().flush().ok();
        let (line_opt, timed_out) = read_input_with_timeout(timeout).await?;
        let line = line_opt.as_deref().unwrap_or("");
        let trimmed_input = line.trim().to_string();
        let idx = trimmed_input.parse::<usize>().ok();
        let choice = idx
            .and_then(|id| choices.get(id - 1))
            .or_else(|| default_choice.and_then(|d| choices.iter().find(|c| c == &&d.to_string())))
            .cloned()
            .unwrap_or_else(|| choices.first().cloned().unwrap_or_default());
        let mut decision = DecisionResult {
            choice,
            timestamp: Utc::now(),
            timeout_applied: timed_out,
            default_used: false,
            response_text: if timed_out {
                None
            } else {
                Some(trimmed_input.clone())
            },
        };
        if timed_out {
            if let Some(default) = default_choice {
                decision.choice = default.to_string();
            }
            decision.default_used = true;
        }
        Ok(decision)
    }
}
