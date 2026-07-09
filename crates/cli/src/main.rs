use std::path::PathBuf;

use newton_cli::cli::context::NewtonContext;
use newton_cli::cli::exit::CliExit;
use newton_cli::cli::framework_setup::build_app;
use newton_cli::cli::log_invocation::{kind_for_command, peek_command};
use newton_cli::cli::mcp;
use newton_cli::Result;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let raw_args: Vec<String> = std::env::args().collect();
    let (log_dir, app_args) = extract_log_dir(&raw_args);
    let log_inv = build_log_invocation(&app_args);
    let _log_guard = newton_core::logging::init(&log_inv, log_dir.as_deref())?;

    let ctx = NewtonContext::new();

    if mcp::is_mcp_subcommand(&app_args) {
        let code = mcp::run(app_args, ctx).await;
        std::process::exit(code);
    }

    let mut app = build_app(ctx)?;

    // Extend the exit seam above: a handler that needs to terminate a direct
    // CLI invocation with a specific exit code returns `Err(CliExit{..}.into())`
    // instead of calling `std::process::exit` itself (spec 074, PR-1 / B3).
    // That keeps the same `Err` safe to flow through `newton serve --with-mcp`
    // (cli-framework turns it into an MCP error frame; the server keeps
    // running) while still reproducing the historical CLI exit behavior here,
    // the only place allowed to call `std::process::exit` outside `mcp::run`.
    match app.run_with_args(app_args).await {
        Ok(()) => Ok(()),
        Err(e) => match e.downcast::<CliExit>() {
            Ok(exit) => {
                eprintln!("{}", exit.message);
                std::process::exit(exit.code);
            }
            Err(e) => Err(e),
        },
    }
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
