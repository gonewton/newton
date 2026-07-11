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
    /// `Some(reason)` when a stdout/stderr capture write to the artifact
    /// file was dropped (I/O failure) or skipped (`OUTPUT_CAPTURE_LIMIT_BYTES`
    /// exceeded) at some point across the run's iterations — surfaced on the
    /// task result so a truncated artifact isn't silently mistaken for the
    /// whole output. See spec 074 S15.
    pub(super) stdout_capture_warning: Option<String>,
    pub(super) stderr_capture_warning: Option<String>,
}

/// Write `text` (plus a trailing newline) to `file`, honoring
/// `OUTPUT_CAPTURE_LIMIT_BYTES` and surfacing any I/O failure as a
/// truncation reason instead of dropping it silently.
///
/// Shared by the stdout and stderr capture call sites inside
/// `execute_sdk_engine` (previously duplicated three times: once for
/// stdout, once for stderr `RawLine`, once for stderr `JsonLine`). Extracted
/// as a standalone, deterministic function — given a `bytes_so_far` and
/// `text`, no subprocess or SDK event stream involved — so its byte-cap and
/// write-error branches (spec 074 S15) are directly unit-testable without
/// needing a real `aikit_sdk::AgentEvent` stream or a real agent binary. See
/// this module's `#[cfg(test)] mod tests`.
///
/// Returns the updated byte count (unchanged when the cap was already hit)
/// and the truncation reason to carry forward (first cause wins across
/// repeated calls within the same iteration — pass the previous call's
/// return value back in as `existing_warning`).
fn write_capture_chunk(
    file: &mut std::fs::File,
    path: &Path,
    bytes_so_far: usize,
    text: &str,
    existing_warning: Option<String>,
    stream_label: &str,
) -> (usize, Option<String>) {
    use std::io::Write;

    if bytes_so_far + text.len() < OUTPUT_CAPTURE_LIMIT_BYTES {
        let mut warning = existing_warning;
        if let Err(err) = file
            .write_all(text.as_bytes())
            .and_then(|()| file.write_all(b"\n"))
        {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "AgentOperator (SDK): failed to write {stream_label} capture artifact"
            );
            if warning.is_none() {
                warning = Some(format!("write error: {err}"));
            }
        }
        (bytes_so_far + text.len() + 1, warning)
    } else {
        let warning = existing_warning.or_else(|| {
            Some(format!(
                "output exceeded {OUTPUT_CAPTURE_LIMIT_BYTES} byte capture limit"
            ))
        });
        (bytes_so_far, warning)
    }
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
    // Truncation causes (I/O failure or hitting `OUTPUT_CAPTURE_LIMIT_BYTES`)
    // across all loop iterations; stdout/stderr artifacts are opened in
    // append mode each iteration, so a truncation anywhere in the run is
    // relevant to the whole result. See spec 074 S15.
    let mut stdout_capture_warning: Option<String> = None;
    let mut stderr_capture_warning: Option<String> = None;
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
        // Per-iteration truncation cause; first cause encountered within
        // this iteration wins (mirrors the command-engine path). Merged into
        // the run-level `stdout_capture_warning`/`stderr_capture_warning`
        // below once this iteration's events are fully processed. See spec
        // 074 S15.
        let mut iter_stdout_capture_warning: Option<String> = None;
        let mut iter_stderr_capture_warning: Option<String> = None;

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
                if matches!(event.stream, aikit_sdk::AgentEventStream::Stdout) {
                    let (new_bytes, warning) = write_capture_chunk(
                        &mut stdout_file,
                        stdout_path,
                        stdout_bytes,
                        &text,
                        iter_stdout_capture_warning.take(),
                        "stdout",
                    );
                    stdout_bytes = new_bytes;
                    iter_stdout_capture_warning = warning;
                }

                if signal_found.is_none() {
                    if let Some((sig_name, sig_data)) = match_signals(&text, compiled_signals) {
                        signal_found = Some(sig_name);
                        signal_data_found = sig_data;
                    }
                }
            }
        }

        if let Some(reason) = &iter_stdout_capture_warning {
            super::artifacts::append_capture_truncation_marker(stdout_path, reason);
        }
        if iter_stdout_capture_warning.is_some() {
            stdout_capture_warning = iter_stdout_capture_warning;
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
                        let (new_bytes, warning) = write_capture_chunk(
                            &mut stderr_file,
                            stderr_path,
                            stderr_bytes,
                            s,
                            iter_stderr_capture_warning.take(),
                            "stderr",
                        );
                        stderr_bytes = new_bytes;
                        iter_stderr_capture_warning = warning;
                    }
                    aikit_sdk::AgentEventPayload::JsonLine(v) => {
                        let text = v.to_string();
                        let (new_bytes, warning) = write_capture_chunk(
                            &mut stderr_file,
                            stderr_path,
                            stderr_bytes,
                            &text,
                            iter_stderr_capture_warning.take(),
                            "stderr",
                        );
                        stderr_bytes = new_bytes;
                        iter_stderr_capture_warning = warning;
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
            if let Some(reason) = &iter_stderr_capture_warning {
                super::artifacts::append_capture_truncation_marker(stderr_path, reason);
            }
            if iter_stderr_capture_warning.is_some() {
                stderr_capture_warning = iter_stderr_capture_warning;
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
        stdout_capture_warning,
        stderr_capture_warning,
    })
}

#[cfg(test)]
mod tests {
    //! `execute_sdk_engine` consumes `aikit_sdk::AgentEvent` values from a
    //! real SDK-driven agent stream, and `aikit_sdk` offers no seam to
    //! inject a synthetic event stream from this crate, nor (on non-Windows)
    //! any way to redirect an agent binary's name to a local fake — so
    //! driving the byte-cap/write-error branches through the full
    //! `execute_sdk_engine` entry point would require a real installed
    //! agent binary with real credentials. Instead, `write_capture_chunk`
    //! (spec 074 S15) was extracted as the standalone, deterministic core of
    //! those branches — no event stream or subprocess involved — so it can
    //! be exercised directly here with a real temp file. Both call sites in
    //! `execute_sdk_engine` (stdout and stderr) delegate to this same
    //! function, so these tests cover their shared behavior.

    use super::*;
    use tempfile::TempDir;

    fn open_append(path: &Path) -> std::fs::File {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap()
    }

    #[test]
    fn write_capture_chunk_writes_text_and_advances_byte_count_on_success() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stdout.log");
        let mut file = open_append(&path);

        let (new_bytes, warning) =
            write_capture_chunk(&mut file, &path, 0, "hello world", None, "stdout");

        assert_eq!(new_bytes, "hello world".len() + 1);
        assert!(warning.is_none());
        drop(file);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "hello world\n");
    }

    #[test]
    fn write_capture_chunk_returns_cap_exceeded_warning_without_writing() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stdout.log");
        let mut file = open_append(&path);

        // Seed `bytes_so_far` right at the cap so even a short chunk trips
        // it — no need to actually write a megabyte of data to exercise
        // this arithmetic.
        let (new_bytes, warning) = write_capture_chunk(
            &mut file,
            &path,
            OUTPUT_CAPTURE_LIMIT_BYTES,
            "one more line",
            None,
            "stdout",
        );

        assert_eq!(
            new_bytes, OUTPUT_CAPTURE_LIMIT_BYTES,
            "byte count must not advance once the cap is hit"
        );
        let warning = warning.expect("cap-exceeded must produce a warning");
        assert!(
            warning.contains("byte capture limit"),
            "unexpected warning: {warning}"
        );
        drop(file);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(
            contents.is_empty(),
            "text must not be written once the cap is exceeded"
        );
    }

    #[test]
    fn write_capture_chunk_preserves_first_warning_across_repeated_calls() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stderr.log");
        let mut file = open_append(&path);

        let (bytes1, warning1) = write_capture_chunk(
            &mut file,
            &path,
            OUTPUT_CAPTURE_LIMIT_BYTES,
            "first",
            None,
            "stderr",
        );
        let (_bytes2, warning2) = write_capture_chunk(
            &mut file,
            &path,
            bytes1,
            "second",
            warning1.clone(),
            "stderr",
        );

        assert_eq!(
            warning1, warning2,
            "the first truncation cause must win across repeated calls"
        );
    }

    #[test]
    fn write_capture_chunk_returns_write_error_reason_when_write_fails() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stdout.log");
        std::fs::write(&path, b"").unwrap();
        // Open read-only: `write_all` on this handle fails with EBADF, a
        // reliable and portable way to exercise the write-error branch
        // without any fragile OS-level permission trick.
        let mut file = std::fs::File::open(&path).unwrap();

        let (new_bytes, warning) =
            write_capture_chunk(&mut file, &path, 0, "unwritable", None, "stdout");

        // The byte count still advances even though the write failed —
        // matches the pre-extraction behavior in `execute_sdk_engine`
        // (the counter tracks "attempted", not "succeeded", writes so a
        // string of failures doesn't wedge the cap check into never
        // firing).
        assert_eq!(new_bytes, "unwritable".len() + 1);
        let warning = warning.expect("write failure must produce a warning");
        assert!(
            warning.starts_with("write error:"),
            "unexpected warning: {warning}"
        );
    }
}
