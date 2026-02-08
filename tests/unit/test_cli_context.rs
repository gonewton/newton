use newton::cli::{args::{ContextArgs, ContextCommand}, commands};
use tempfile::TempDir;

#[tokio::test]
async fn context_add_show_clear_cycle() {
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().to_path_buf();

    let add_args = ContextArgs {
        workspace_path: workspace.clone(),
        command: ContextCommand::Add {
            message: "First entry".to_string(),
            title: Some("Testing".to_string()),
        },
    };

    commands::context(add_args).await.unwrap();

    let context_file = workspace.join(".newton/state/context.md");
    let contents = std::fs::read_to_string(&context_file).unwrap();
    assert!(contents.contains("First entry"));

    let show_args = ContextArgs {
        workspace_path: workspace.clone(),
        command: ContextCommand::Show,
    };
    commands::context(show_args).await.unwrap();

    let clear_args = ContextArgs {
        workspace_path: workspace.clone(),
        command: ContextCommand::Clear,
    };
    commands::context(clear_args).await.unwrap();

    let cleared = std::fs::read_to_string(&context_file).unwrap();
    assert!(cleared.contains("# Newton Loop Context"));
}
