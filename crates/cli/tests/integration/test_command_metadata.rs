//! Asserts that every command registered by `build_app` carries the metadata
//! the spec §4.1 requires (summary, syntax, allowed category).

use newton_cli::cli::categories;
use newton_cli::cli::framework_setup::{
    enumerate_effective_app_tree_commands, enumerate_tree_commands, REGISTERED_COMMAND_IDS,
};

#[test]
fn registered_ids_match_expected_set() {
    let tree_paths: Vec<String> = enumerate_tree_commands()
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    for id in REGISTERED_COMMAND_IDS {
        assert!(
            tree_paths.contains(&id.to_string()),
            "expected `{id}` in tree registry"
        );
    }
    // enumerate_tree_commands reports Newton-owned commands only.
    let extras: Vec<&String> = tree_paths
        .iter()
        .filter(|p| !REGISTERED_COMMAND_IDS.contains(&p.as_str()))
        .collect();
    assert!(extras.is_empty(), "unexpected extras: {extras:?}");
}

#[test]
fn category_bindings_match_spec_4_1() {
    // Spec §4.1 binds each command to an exact category; future renames or
    // accidental category drift should fail this test loudly.
    let expected: &[(&str, &str)] = &[
        ("workflow", categories::WORKFLOW),
        ("data/get", categories::WORKFLOW),
        ("data/post", categories::WORKFLOW),
        ("data/put", categories::WORKFLOW),
        ("data/patch", categories::WORKFLOW),
        ("data/delete", categories::WORKFLOW),
        ("serve", categories::OPS),
        ("optimize", categories::OPS),
        ("init", categories::WORKSPACE),
        ("doctor", categories::OPERATIONAL),
        ("config", categories::OPERATIONAL),
        // "completion" removed — now provided by cli-framework built-in, not in newton's registry
    ];
    let cmds = enumerate_tree_commands();
    for (name, want) in expected {
        let cmd = cmds
            .iter()
            .find(|(p, _)| p == *name)
            .map(|(_, c)| c)
            .unwrap_or_else(|| panic!("expected `{name}` in tree registry"));
        assert_eq!(
            cmd.category(),
            Some(*want),
            "command `{name}` should have category `{want}`, got {:?}",
            cmd.category()
        );
    }
}

#[test]
fn effective_app_registry_includes_framework_builtins() {
    let paths: Vec<String> = enumerate_effective_app_tree_commands()
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    for builtin in ["completion", "spec", "chat"] {
        assert!(
            paths.contains(&builtin.to_string()),
            "expected `{builtin}` in effective app tree registry"
        );
    }
}

#[test]
fn metadata_is_populated_for_every_command() {
    for (path, cmd) in enumerate_tree_commands() {
        assert!(!cmd.summary().is_empty(), "{} summary empty", path);
        assert!(cmd.syntax().is_some(), "{} syntax missing", path);
        let cat = cmd
            .category()
            .unwrap_or_else(|| panic!("{} category missing", path));
        assert!(
            categories::is_allowed(cat),
            "{} category `{}` not in allowed set",
            path,
            cat
        );
    }
}
