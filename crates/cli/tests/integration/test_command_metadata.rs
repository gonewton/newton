//! Asserts that every command registered by `build_app` carries the metadata
//! the spec §4.1 requires (summary, syntax, allowed category).

use newton_cli::cli::categories;
use newton_cli::cli::framework_setup::{enumerate_commands, REGISTERED_COMMAND_IDS};

#[test]
fn registered_ids_match_expected_set() {
    let ids: Vec<&str> = enumerate_commands().iter().map(|c| c.id).collect();
    for id in REGISTERED_COMMAND_IDS {
        assert!(ids.contains(id), "expected `{id}` in registry");
    }
    // When `ask` feature is enabled, enumerate_commands adds the extra `ask`
    // command on top of REGISTERED_COMMAND_IDS — that's expected.
    let extras: Vec<&&str> = ids
        .iter()
        .filter(|id| !REGISTERED_COMMAND_IDS.contains(id))
        .collect();
    if cfg!(feature = "ask") {
        assert_eq!(extras, vec![&"ask"]);
    } else {
        assert!(extras.is_empty(), "unexpected extras: {extras:?}");
    }
}

#[test]
fn metadata_is_populated_for_every_command() {
    for cmd in enumerate_commands() {
        assert!(!cmd.summary.is_empty(), "{} summary empty", cmd.id);
        assert!(cmd.syntax.is_some(), "{} syntax missing", cmd.id);
        let cat = cmd
            .category
            .unwrap_or_else(|| panic!("{} category missing", cmd.id));
        assert!(
            categories::is_allowed(cat),
            "{} category `{}` not in allowed set",
            cmd.id,
            cat
        );
    }
}
