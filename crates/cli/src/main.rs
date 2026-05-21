use std::path::PathBuf;

use newton_cli::cli::context::NewtonContext;
use newton_cli::cli::framework_setup::build_app;
use newton_cli::cli::log_invocation::{kind_for_command, peek_command};
use newton_cli::cli::mcp;
use newton_cli::Result;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let raw_args: Vec<String> = std::env::args().collect();
    let (log_dir, app_args) = extract_log_dir(&raw_args);
    let (app_args, is_legacy_run) = intercept_legacy_run(app_args);
    if is_legacy_run {
        eprintln!(
            "[newton] DEPRECATED: `newton run` is deprecated; use `newton workflow run` instead"
        );
    }
    let log_inv = build_log_invocation(&app_args);
    let _log_guard = newton_core::logging::init(&log_inv, log_dir.as_deref())?;

    let ctx = NewtonContext::new();

    if mcp::is_mcp_serve(&app_args) || mcp::is_mcp_subcommand(&app_args) {
        let code = mcp::run(app_args, ctx).await;
        std::process::exit(code);
    }

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

/// Detect the legacy `newton run <args>` invocation and rewrite it to
/// `newton workflow run <args>`. Returns the (possibly rewritten) argv and a
/// boolean indicating whether a rewrite occurred so the caller can emit a
/// deprecation notice.
fn intercept_legacy_run(argv: Vec<String>) -> (Vec<String>, bool) {
    if argv.get(1).map(|s| s.as_str()) == Some("run") {
        let mut new_argv = vec![argv[0].clone(), "workflow".to_string()];
        new_argv.extend_from_slice(&argv[1..]);
        (new_argv, true)
    } else {
        (argv, false)
    }
}

fn build_log_invocation(argv: &[String]) -> newton_core::logging::LogInvocation {
    use newton_core::logging::LogInvocation;
    let kind = peek_command(argv).map(kind_for_command).unwrap_or_else(|| {
        use newton_core::logging::LogInvocationKind;
        LogInvocationKind::Run
    });
    let workspace: Option<PathBuf> = argv.windows(2).find_map(|w| {
        if w[0] == "--workspace" {
            Some(PathBuf::from(&w[1]))
        } else {
            None
        }
    });
    LogInvocation::new(kind, workspace)
}
