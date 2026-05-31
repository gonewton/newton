use std::path::PathBuf;
use tempfile::TempDir;

pub fn temp_dir() -> TempDir {
    tempfile::tempdir().expect("Failed to create temp dir")
}

pub fn fixture_path(relative: &str) -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir).join("fixtures").join(relative)
}

pub fn load_fixture(relative: &str) -> String {
    std::fs::read_to_string(fixture_path(relative)).expect("Failed to read fixture file")
}
