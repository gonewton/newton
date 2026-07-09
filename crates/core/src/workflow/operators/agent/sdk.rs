use super::command::OUTPUT_CAPTURE_LIMIT_BYTES;
use super::config::AgentOperatorConfig;
use super::quota::{quota_signal_to_error, sdk_io_error};
use super::signals::match_signals;
use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operators::engine::{extract_text_from_sdk_event, AikitEngineManager};
use indexmap::IndexMap;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

/// Result of SDK engine execution (signal, signal_data, exit_code, iteration, events_artifact_rel_path, token_usage).
pub(super) struct SdkExecResult {
    pub(super) signal: Option<String>,
    pub(super) signal_data: HashMap<String, String>,
    /// The SDK engine path never kills a subprocess directly (aikit-sdk owns
    /// that internally), so this is always `Some(0)` — kept as `Option<i32>`
    /// only to share a type with the command-engine path's `exit_code`,
    /// which genuinely can be `None` on a signal-triggered kill.
    pub(super) exit_code: Option<i32>,
    pub(super) iteration: u32,
    pub(super) events_artifact_path: Option<String>,
    /// Aggregated token usage from the SDK run.
    pub(super) token_usage: Option<serde_json::Value>,
}

/// Execute an AI engine via aikit-sdk, handling loop mode and signal matching.
/// Writes a NDJSON events artifact using SDK AgentEvent JSON serialization.
///
/// `deny(non_exhaustive_omitted_patterns)` ensures that any future SDK
/// `AgentEventPayload` variant becomes a compile error inside this function so
/// Newton must explicitly classify it (no silent fall-through).
#[allow(clippy::too_many_arguments)]
#[allow(unknown_lints)]
#[deny(non_exhaustive_omitted_patterns)]
pub(super) async fn execute_sdk_engine(
    manager: &AikitEngineManager,
    engine_name: &str,
    prompt: &str,
    model: Option<&str>,
    config: &AgentOperatorConfig,
    compiled_signals: &IndexMap<String, Regex>,
    stdout_path: &Path,
    stderr_path: &Path,
    events_ndjson_path: &Path,
    workspace_root: &Path,
    timeout: Duration,
) -> Result<SdkExecResult, AppError> {
    use std::io::Write;

    let max_iters = if config.loop_mode {
        config.max_iterations.unwrap_or(u32::MAX)
    } else {
        1
    };

    let mut iteration: u32 = 0;
    let mut last_signal: Option<String> = None;
    let mut last_signal_data: HashMap<String, String> = HashMap::new();
    let last_exit_code: Option<i32> = Some(0);
    let start = Instant::now();
    let mut fallback_token_usage: Option<serde_json::Value> = None;
    let mut primary_token_usage: Option<serde_json::Value> = None;
    let events_artifact_rel = events_ndjson_path.strip_prefix(workspace_root).map_or_else(
        |_| events_ndjson_path.to_string_lossy().to_string(),
        |p| p.to_string_lossy().to_string(),
    );
    let stderr_rel = stderr_path.strip_prefix(workspace_root).map_or_else(
        |_| stderr_path.to_string_lossy().to_string(),
        |p| p.to_string_lossy().to_string(),
    );

    let mut events_ndjson_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(events_ndjson_path)
        .map_err(|e| sdk_io_error(format!("failed to open events NDJSON artifact: {e}")))?;

    loop {
        iteration += 1;
        if iteration > max_iters {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("agent exceeded max_iterations ({max_iters}) in loop mode"),
            )
            .with_code("WFG-AGENT-003"));
        }

        if start.elapsed() >= timeout {
            return Err(AppError::new(
                ErrorCategory::TimeoutError,
                "agent operator timeout exceeded during SDK execution",
            )
            .with_code("WFG-AGENT-005"));
        }

        let remaining = timeout.saturating_sub(start.elapsed());
        // execute_engine_events returns `(events, inner_result)` so we always get the
        // already-collected events even when the SDK returns an error (e.g. QuotaExceeded).
        // This ensures the events artifact is populated before we return an error.
        let (events, iter_inner_result) = match tokio::time::timeout(
            remaining,
            manager.execute_engine_events(engine_name, prompt, model, Some(remaining)),
        )
        .await
        {
            Err(_) => {
                return Err(AppError::new(
                    ErrorCategory::TimeoutError,
                    "agent operator timeout exceeded during SDK execution",
                )
                .with_code("WFG-AGENT-005"));
            }
            Ok(Err(e)) => return Err(e), // fatal: spawn panic / is_runnable failure
            Ok(Ok(pair)) => pair,
        };

        let mut stdout_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(stdout_path)
            .map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to open stdout artifact: {e}"),
                )
            })?;

        let mut stdout_bytes: usize = 0;
        let mut signal_found: Option<String> = None;
        let mut signal_data_found: HashMap<String, String> = HashMap::new();

        for event in &events {
            let event_json = serde_json::to_string(event).map_err(|e| {
                sdk_io_error(format!("failed to serialize event to NDJSON artifact: {e}"))
            })?;
            events_ndjson_file
                .write_all(event_json.as_bytes())
                .and_then(|_| events_ndjson_file.write_all(b"\n"))
                .map_err(|e| {
                    sdk_io_error(format!("failed to write event to NDJSON artifact: {e}"))
                })?;

            match &event.payload {
                aikit_sdk::AgentEventPayload::TokenUsageLine { usage, .. } => {
                    fallback_token_usage = serde_json::to_value(usage).ok();
                    continue;
                }
                aikit_sdk::AgentEventPayload::RawBytes(_) => {
                    continue;
                }
                aikit_sdk::AgentEventPayload::QuotaExceeded { .. } => {
                    continue;
                }
                aikit_sdk::AgentEventPayload::RawLine(_)
                | aikit_sdk::AgentEventPayload::JsonLine(_) => {}
                aikit_sdk::AgentEventPayload::StreamMessage(msg)
                    if msg.phase == aikit_sdk::MessagePhase::Final
                        && msg.role == aikit_sdk::MessageRole::Assistant => {}
                aikit_sdk::AgentEventPayload::StreamMessage(_) => continue,
                aikit_sdk::AgentEventPayload::RawTransportLine { .. } => continue,
                aikit_sdk::AgentEventPayload::AikitTextDelta { .. } => continue,
                aikit_sdk::AgentEventPayload::AikitTextFinal { .. } => continue,
                aikit_sdk::AgentEventPayload::AikitToolUse { .. } => continue,
                aikit_sdk::AgentEventPayload::AikitToolResult { .. } => continue,
                aikit_sdk::AgentEventPayload::AikitSubagentSpawn { .. } => continue,
                aikit_sdk::AgentEventPayload::AikitSubagentResult { .. } => continue,
                aikit_sdk::AgentEventPayload::AikitContextCompressed { .. } => continue,
                aikit_sdk::AgentEventPayload::AikitStepFinish { .. } => continue,
                // Required by #[non_exhaustive] across crate boundary; the
                // `non_exhaustive_omitted_patterns` lint on the enclosing
                // function turns any new SDK variant into a compile error.
                _ => continue,
            }

            if let Some(text) = extract_text_from_sdk_event(event) {
                if matches!(event.stream, aikit_sdk::AgentEventStream::Stdout)
                    && stdout_bytes + text.len() < OUTPUT_CAPTURE_LIMIT_BYTES
                {
                    let _ = stdout_file.write_all(text.as_bytes());
                    let _ = stdout_file.write_all(b"\n");
                    stdout_bytes += text.len() + 1;
                }

                if signal_found.is_none() {
                    if let Some((sig_name, sig_data)) = match_signals(&text, compiled_signals) {
                        signal_found = Some(sig_name);
                        signal_data_found = sig_data;
                    }
                }
            }
        }

        let stderr_events: Vec<&aikit_sdk::AgentEvent> = events
            .iter()
            .filter(|e| matches!(e.stream, aikit_sdk::AgentEventStream::Stderr))
            .collect();
        if !stderr_events.is_empty() {
            let mut stderr_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(stderr_path)
                .map_err(|e| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!("failed to open stderr artifact: {e}"),
                    )
                })?;
            let mut stderr_bytes: usize = 0;
            for event in &stderr_events {
                match &event.payload {
                    aikit_sdk::AgentEventPayload::RawLine(s) => {
                        if stderr_bytes + s.len() < OUTPUT_CAPTURE_LIMIT_BYTES {
                            let _ = stderr_file.write_all(s.as_bytes());
                            let _ = stderr_file.write_all(b"\n");
                            stderr_bytes += s.len() + 1;
                        }
                    }
                    aikit_sdk::AgentEventPayload::JsonLine(v) => {
                        let text = v.to_string();
                        if stderr_bytes + text.len() < OUTPUT_CAPTURE_LIMIT_BYTES {
                            let _ = stderr_file.write_all(text.as_bytes());
                            let _ = stderr_file.write_all(b"\n");
                            stderr_bytes += text.len() + 1;
                        }
                    }
                    aikit_sdk::AgentEventPayload::RawBytes(_) => {}
                    aikit_sdk::AgentEventPayload::StreamMessage(_) => {}
                    aikit_sdk::AgentEventPayload::TokenUsageLine { .. } => {}
                    aikit_sdk::AgentEventPayload::QuotaExceeded { .. } => {}
                    aikit_sdk::AgentEventPayload::RawTransportLine { .. } => {}
                    aikit_sdk::AgentEventPayload::AikitTextDelta { .. } => {}
                    aikit_sdk::AgentEventPayload::AikitTextFinal { .. } => {}
                    aikit_sdk::AgentEventPayload::AikitToolUse { .. } => {}
                    aikit_sdk::AgentEventPayload::AikitToolResult { .. } => {}
                    aikit_sdk::AgentEventPayload::AikitSubagentSpawn { .. } => {}
                    aikit_sdk::AgentEventPayload::AikitSubagentResult { .. } => {}
                    aikit_sdk::AgentEventPayload::AikitContextCompressed { .. } => {}
                    aikit_sdk::AgentEventPayload::AikitStepFinish { .. } => {}
                    // Required by #[non_exhaustive] across crate boundary; the
                    // `non_exhaustive_omitted_patterns` lint on the enclosing
                    // function turns any new SDK variant into a compile error.
                    _ => {}
                }
            }
        }

        // All events have been flushed to the artifact files. Now resolve the inner SDK
        // result. Any WFG-AGENT-008 (RunError::QuotaExceeded) error is handled here so
        // the artifact paths point at non-empty files containing the quota evidence.
        let iter_run_result = match iter_inner_result {
            Ok(run_result) => run_result,
            Err(mut err) if err.code == "WFG-AGENT-008" => {
                err.add_context("events_artifact", &events_artifact_rel);
                if stderr_path.exists() {
                    err.add_context("stderr_artifact", &stderr_rel);
                }
                return Err(err);
            }
            Err(e) => return Err(e),
        };

        // Two distinct quota paths:
        //  1. RunError::QuotaExceeded → mapped to WFG-AGENT-008 in iter_inner_result (handled
        //     above, after events are flushed).
        //  2. RunResult.quota_exceeded → SDK returned Ok(RunResult) but the result carries a
        //     quota signal; handled here with the same artifact-context enrichment.
        if let Some(ref info) = iter_run_result.quota_exceeded {
            return Err(quota_signal_to_error(
                info,
                &events_artifact_rel,
                stderr_path,
                &stderr_rel,
            ));
        }

        if let Some(ref usage) = iter_run_result.token_usage {
            primary_token_usage = serde_json::to_value(usage).ok();
        }

        if let Some(sig) = signal_found {
            last_signal = Some(sig);
            last_signal_data = signal_data_found;
            break;
        }

        if !config.loop_mode {
            break;
        }
    }

    let events_artifact_path = events_artifact_rel;
    let token_usage = primary_token_usage.or(fallback_token_usage);

    Ok(SdkExecResult {
        signal: last_signal,
        signal_data: last_signal_data,
        exit_code: last_exit_code,
        iteration,
        events_artifact_path: Some(events_artifact_path),
        token_usage,
    })
}
