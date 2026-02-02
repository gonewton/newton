use insta::assert_snapshot;
use newton::VERSION;

#[test]
fn version_constant_is_available() {
    assert_snapshot!("version_constant", VERSION);
}

#[test]
fn version_format_validation() {
    let version = VERSION;
    assert!(version.contains('.'), "Version should contain dots");
    assert!(
        !version.starts_with('v'),
        "Version should not start with 'v'"
    );
    assert_snapshot!("version_format", version);
}

#[test]
fn version_matches_cargo_toml() {
    let cargo_version = env!("CARGO_PKG_VERSION");
    assert_eq!(VERSION, cargo_version);
    assert_snapshot!(
        "version_consistency",
        format!("lib.rs: {}\ncargo.toml: {}", VERSION, cargo_version)
    );
}
