use crate::cli::args::{ContextArgs, ContextCommand};
use crate::core::{ConfigLoader, ContextManager};
use crate::Result;

/// Handles `newton context ...` subcommands for managing the shared context buffer.
pub async fn run(args: ContextArgs) -> Result<()> {
    let config = ConfigLoader::load_from_workspace(&args.workspace_path)?;
    let context_file = args.workspace_path.join(&config.context.file);

    match args.command {
        ContextCommand::Add { message, title } => {
            let entry_title = title.unwrap_or_else(|| "User feedback".to_string());
            ContextManager::add_context(&context_file, &entry_title, &message)?;
            println!("Added context entry with title '{}'.", entry_title);
        }
        ContextCommand::Show => {
            let contents = ContextManager::read_context(&context_file)?;
            if contents.trim().is_empty() {
                println!("Context is empty.");
            } else {
                println!("Current context:\n{}", contents);
            }
        }
        ContextCommand::Clear => {
            ContextManager::clear_context(&context_file)?;
            println!("Context cleared.");
        }
    }

    Ok(())
}
