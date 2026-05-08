//! Tests for the feature-gated `ask` command (issue #231 Stage D).

#![cfg(feature = "ask")]

use newton_cli::ask::{CommandMatcher, CommandSummary, SubstringMatcher};

fn fixture() -> Vec<CommandSummary> {
    vec![
        CommandSummary {
            name: "checkpoints".into(),
            summary: "Manage and inspect workflow execution checkpoints".into(),
            syntax: "list | clean".into(),
            category: "maintenance".into(),
        },
        CommandSummary {
            name: "run".into(),
            summary: "Execute a workflow graph".into(),
            syntax: "[WORKFLOW] [INPUT_FILE] [OPTIONS]".into(),
            category: "workflow".into(),
        },
        CommandSummary {
            name: "doctor".into(),
            summary: "Run local environment diagnostic probes".into(),
            syntax: "[OPTIONS]".into(),
            category: "operational".into(),
        },
    ]
}

#[test]
fn substring_matcher_ranks_checkpoints_first_for_list_query() {
    let ranked = SubstringMatcher.rank("list checkpoints", &fixture());
    assert_eq!(ranked.first().map(|r| r.name.as_str()), Some("checkpoints"));
}

#[test]
fn substring_matcher_returns_zero_for_unrelated_query() {
    let ranked = SubstringMatcher.rank("xyzzy", &fixture());
    assert!(ranked.iter().all(|r| r.score == 0.0));
}

#[test]
fn empty_query_returns_cli_ask_001() {
    let err = newton_cli::ask::run("   ", &fixture()).expect_err("empty query must fail");
    assert!(format!("{err}").contains("CLI-ASK-001"));
}

#[test]
fn no_matches_returns_cli_ask_002() {
    let err = newton_cli::ask::run("xyzzy", &fixture())
        .expect_err("no-match query must surface CLI-ASK-002");
    assert!(
        format!("{err}").contains("CLI-ASK-002"),
        "expected CLI-ASK-002 in: {err}"
    );
}
