use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::workflow::state::{AppErrorSummary, TaskRunRecord};

pub(super) const FAILURE_DIAGNOSIS_STREAM_CAP_BYTES: usize = 16 * 1024;

pub(super) enum FailureDiagnosisInput<'a> {
    Outcome(&'a TaskOutcome),
    Record {
        task_id: &'a str,
        record: &'a TaskRunRecord,
    },
}

#[derive(Clone)]
pub struct TaskOutcome {
    pub task_id: String,
    pub record: TaskRunRecord,
    pub context_patch: Option<Value>,
    pub failed: bool,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub error_summary: Option<AppErrorSummary>,
    pub resolved_params: Value,
}

pub(super) fn tail_truncate_utf8(s: &str, cap: usize) -> (&str, usize, bool) {
    let len = s.len();
    if len <= cap {
        return (s, len, false);
    }
    let mut start = len - cap;
    while start < len && !s.is_char_boundary(start) {
        start += 1;
    }
    (&s[start..], len, true)
}

pub(super) fn eprint_task_failure_diagnosis(
    input: FailureDiagnosisInput<'_>,
    verbose_streams_already_printed: bool,
) {
    let mut buf: Vec<u8> = Vec::new();
    let _ = write_task_failure_diagnosis(&mut buf, input, verbose_streams_already_printed);
    use std::io::Write as _;
    let _ = std::io::stderr().write_all(&buf);
}

pub(super) fn write_task_failure_diagnosis<W: std::io::Write>(
    w: &mut W,
    input: FailureDiagnosisInput<'_>,
    verbose_streams_already_printed: bool,
) -> std::io::Result<()> {
    let (task_id, record, error_summary): (&str, &TaskRunRecord, Option<&AppErrorSummary>) =
        match input {
            FailureDiagnosisInput::Outcome(outcome) => (
                outcome.task_id.as_str(),
                &outcome.record,
                outcome.error_summary.as_ref(),
            ),
            FailureDiagnosisInput::Record { task_id, record } => (task_id, record, None),
        };

    writeln!(w, "--- task failed: {task_id} ---")?;

    let (code_str, message_str): (String, String) = if let Some(summary) = error_summary {
        (summary.code.clone(), summary.message.clone())
    } else {
        let code = record
            .error_code
            .clone()
            .unwrap_or_else(|| "<unavailable>".to_string());
        let message = record
            .output
            .as_object()
            .and_then(|m| m.get("error"))
            .and_then(|e| e.as_object())
            .and_then(|m| m.get("message"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "<unavailable>".to_string());
        (code, message)
    };
    writeln!(w, "code={code_str}")?;
    writeln!(w, "message={message_str}")?;

    let output_map = match &record.output {
        Value::Object(m) => Some(m),
        _ => None,
    };

    if let Some(output_map) = output_map {
        if let Some(exit_code_val) = output_map.get("exit_code") {
            if let Some(n) = exit_code_val.as_i64() {
                writeln!(w, "exit_code={n}")?;
            } else if let Some(n) = exit_code_val.as_u64() {
                writeln!(w, "exit_code={n}")?;
            } else if let Some(n) = exit_code_val.as_f64() {
                writeln!(w, "exit_code={n}")?;
            }
        }

        if !verbose_streams_already_printed {
            for stream_key in &["stderr", "stdout"] {
                if let Some(Value::String(s)) = output_map.get(*stream_key) {
                    let trimmed = s.trim_end();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let (slice, orig_len, truncated) =
                        tail_truncate_utf8(s.as_str(), FAILURE_DIAGNOSIS_STREAM_CAP_BYTES);
                    if truncated {
                        writeln!(
                            w,
                            "--- {stream_key} ({orig_len} bytes, truncated to {cap} bytes) ---",
                            cap = FAILURE_DIAGNOSIS_STREAM_CAP_BYTES,
                        )?;
                    } else {
                        writeln!(w, "--- {stream_key} ({orig_len} bytes) ---")?;
                    }
                    if slice.as_bytes().last().copied() == Some(b'\n') {
                        write!(w, "{slice}")?;
                    } else {
                        writeln!(w, "{slice}")?;
                    }
                }
            }
        }

        if let Some(Value::String(p)) = output_map.get("stderr_artifact") {
            writeln!(w, "stderr artifact: {p}")?;
        }
        if let Some(Value::String(p)) = output_map.get("stdout_artifact") {
            writeln!(w, "stdout artifact: {p}")?;
        }
    }
    Ok(())
}

/// `--verbose` per-task completion hook (spec 074, P5b): print the task's
/// captured stdout/stderr to the terminal right after it completes, headed
/// by a line identifying the task and its outcome so multi-task output stays
/// readable. If the inline capture hit `OUTPUT_CAPTURE_LIMIT_BYTES` (the cap
/// `CommandOperator`/`AgentOperator` apply when populating `stdout`/`stderr`)
/// a truncation marker is printed alongside the artifact pointer so the
/// human knows to go to the artifact file for the rest.
pub(super) fn print_task_verbose_output(outcome: &TaskOutcome) {
    use crate::workflow::operators::OUTPUT_CAPTURE_LIMIT_BYTES;

    println!(
        "--- task {} completed: {} ---",
        outcome.task_id,
        outcome.record.status.as_str()
    );

    let output = &outcome.record.output;

    if let Value::Object(output_map) = output {
        if let Some(Value::String(stdout)) = output_map.get("stdout") {
            if !stdout.trim().is_empty() {
                print!("{stdout}");
                // Byte length landing on the cap is the signal that
                // `limit_bytes`/`OUTPUT_CAPTURE_LIMIT_BYTES` cut the stream
                // off at capture time rather than the process simply
                // producing exactly that much output.
                if stdout.len() >= OUTPUT_CAPTURE_LIMIT_BYTES {
                    println!(
                        "[stdout truncated at capture: hit {OUTPUT_CAPTURE_LIMIT_BYTES} byte cap]"
                    );
                }
            }
        }
        if let Some(Value::String(stderr)) = output_map.get("stderr") {
            if !stderr.trim().is_empty() {
                eprint!("{stderr}");
                if stderr.len() >= OUTPUT_CAPTURE_LIMIT_BYTES {
                    eprintln!(
                        "[stderr truncated at capture: hit {OUTPUT_CAPTURE_LIMIT_BYTES} byte cap]"
                    );
                }
            }
        }
        if let Some(Value::String(artifact_path)) = output_map.get("stdout_artifact") {
            println!("stdout artifact: {artifact_path}");
        }
        if let Some(Value::String(artifact_path)) = output_map.get("stderr_artifact") {
            eprintln!("stderr artifact: {artifact_path}");
        }
    }
}
