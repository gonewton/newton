use std::path::PathBuf;

use newton_cli::cli::context::NewtonContext;
use newton_cli::cli::framework_setup::build_app;
use newton_cli::Result;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let raw_args: Vec<String> = std::env::args().collect();
    let (log_dir, app_args) = extract_log_dir(&raw_args);
    let log_inv = infer_log_invocation(&app_args);
    let _log_guard = newton_core::logging::init(&log_inv, log_dir.as_deref())?;

    let ctx = NewtonContext::new();
    let mut app = build_app(ctx)?;
    app.run_with_args(app_args).await
}

/// Strip `--log-dir <value>` / `--log-dir=<value>` from argv, preserving argv[0].
fn extract_log_dir(argv: &[String]) -> (Option<PathBuf>, Vec<String>) {
    let mut log_dir: Option<PathBuf> = None;
    let mut filtered: Vec<String> = Vec::with_capacity(argv.len());
    let mut i = 0;
    while i < argv.len() {
        if argv[i] == "--log-dir" && i + 1 < argv.len() {
            log_dir = Some(PathBuf::from(&argv[i + 1]));
            i += 2;
        } else if let Some(val) = argv[i].strip_prefix("--log-dir=") {
            log_dir = Some(PathBuf::from(val));
            i += 1;
        } else {
            filtered.push(argv[i].clone());
            i += 1;
        }
    }
    (log_dir, filtered)
}

/// Infer a `LogInvocation` from argv without full parsing — used to initialise
/// logging before the framework dispatches.  Falls back gracefully when argv
/// is too short or the command name is unrecognised.
fn infer_log_invocation(argv: &[String]) -> newton_core::logging::LogInvocation {
    use newton_core::logging::{LogInvocation, LogInvocationKind};

    // argv[0] is the binary name; argv[1] is the subcommand (may be prefixed by --log-dir
    // already stripped, but skip any remaining flags just in case).
    let command = argv.iter().skip(1).find(|a| !a.starts_with('-'));

    // Best-effort workspace extraction from --workspace flag.
    let workspace: Option<PathBuf> = argv.windows(2).find_map(|w| {
        if w[0] == "--workspace" {
            Some(PathBuf::from(&w[1]))
        } else {
            None
        }
    });

    let kind = match command.map(String::as_str) {
        Some("run") => LogInvocationKind::Run,
        Some("init") => LogInvocationKind::Init,
        Some("batch") => LogInvocationKind::Batch,
        Some("validate") => LogInvocationKind::Validate,
        Some("dot") => LogInvocationKind::Dot,
        Some("lint") => LogInvocationKind::Lint,
        Some("explain") => LogInvocationKind::Explain,
        Some("resume") => LogInvocationKind::Resume,
        Some("checkpoints") => LogInvocationKind::Checkpoints,
        Some("artifacts") => LogInvocationKind::Artifacts,
        Some("webhook") => LogInvocationKind::Webhook,
        Some("log") => LogInvocationKind::Log,
        Some("monitor") => LogInvocationKind::Monitor,
        Some("serve") => LogInvocationKind::Serve,
        _ => LogInvocationKind::Run,
    };

    LogInvocation::new(kind, workspace)
}
