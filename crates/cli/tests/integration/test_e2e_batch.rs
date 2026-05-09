#[path = "../support/mod.rs"]
mod support;

use std::fs;
use support::newton;

fn setup_minimal_batch_workspace(root: &std::path::Path, project_id: &str) {
    let configs_dir = root.join(".newton/configs");
    fs::create_dir_all(&configs_dir).unwrap();

    let conf = format!(
        "project_root={}\nrunner=.newton/scripts/executor.sh\nworkflow_file=.newton/workflows/wf.yaml\n",
        root.display()
    );
    fs::write(configs_dir.join(format!("{project_id}.conf")), conf).unwrap();

    let plan_dirs = [
        format!(".newton/plan/{project_id}/todo"),
        format!(".newton/plan/{project_id}/completed"),
        format!(".newton/plan/{project_id}/failed"),
    ];
    for d in &plan_dirs {
        fs::create_dir_all(root.join(d)).unwrap();
    }
}

#[test]
fn integ_batch_once_no_plans() {
    let dir = tempfile::tempdir().unwrap();
    setup_minimal_batch_workspace(dir.path(), "testproj");

    newton()
        .args([
            "batch",
            "testproj",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--once",
        ])
        .assert()
        .success();
}
