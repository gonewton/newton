use newton::VERSION;

fn is_semver_like(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    parts[0].parse::<u32>().is_ok() && parts[1].parse::<u32>().is_ok()
}

#[test]
fn version_constant_is_available() {
    assert!(!VERSION.is_empty());
    assert!(
        is_semver_like(VERSION),
        "VERSION should be semver-like, got {}",
        VERSION
    );
}

#[test]
fn version_format_validation() {
    let version = VERSION;
    assert!(version.contains('.'), "Version should contain dots");
    assert!(
        !version.starts_with('v'),
        "Version should not start with 'v'"
    );
    assert!(
        is_semver_like(version),
        "Version should be semver-like, got {}",
        version
    );
}

#[test]
fn version_matches_cargo_toml() {
    let cargo_version = env!("CARGO_PKG_VERSION");
    assert_eq!(VERSION, cargo_version);
    assert!(
        is_semver_like(cargo_version),
        "cargo version should be semver-like"
    );
}
