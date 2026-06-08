use newton_cli::cli::framework_setup::enumerate_effective_app_tree_commands;

#[test]
fn chat_tool_list_excludes_filtered_commands() {
    let commands = enumerate_effective_app_tree_commands();

    let chat_tool_names: Vec<String> = commands
        .iter()
        .filter(|(path, cmd)| {
            cmd.expose_chat && path != "completion" && path != "chat" && path != "spec"
        })
        .map(|(path, _)| format!("newton_{}", path.replace('/', "_")))
        .collect();

    for tool in &[
        "newton_serve",
        "newton_optimize",
        "newton_init",
        "newton_run",
    ] {
        assert!(
            !chat_tool_names.contains(&tool.to_string()),
            "{tool} must not appear in the chat tool list"
        );
    }

    for tool in &[
        "newton_workflow",
        "newton_config",
        "newton_doctor",
        "newton_data_get",
        "newton_data_post",
        "newton_data_put",
        "newton_data_patch",
        "newton_data_delete",
    ] {
        assert!(
            chat_tool_names.contains(&tool.to_string()),
            "{tool} must appear in the chat tool list"
        );
    }
}
