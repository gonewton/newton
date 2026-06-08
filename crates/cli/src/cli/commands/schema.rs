use newton_core::core::error::AppError;
use newton_core::core::types::ErrorCategory;
use newton_core::workflow::operator::OperatorRegistry;
use newton_core::workflow::schema::WorkflowSettings;
use newton_core::workflow::{operators, schema_export};
use std::fs;
use std::path::PathBuf;
use std::result::Result as StdResult;

pub struct SchemaExportArgs {
    pub out: Option<PathBuf>,
    pub pretty: bool,
    pub workspace: Option<PathBuf>,
}

pub fn schema_export_cmd(args: SchemaExportArgs) -> StdResult<(), AppError> {
    let workspace = args
        .workspace
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Build operator registry with default settings
    let settings = WorkflowSettings::default();
    let mut builder = OperatorRegistry::builder();
    operators::register_builtins(&mut builder, workspace, settings);
    let registry = builder.build();

    let schema = schema_export::composed_workflow_schema(&registry);

    let output = if args.pretty {
        serde_json::to_string_pretty(&schema).map_err(|e| {
            AppError::new(
                ErrorCategory::SerializationError,
                format!("serialize schema: {e}"),
            )
        })?
    } else {
        serde_json::to_string(&schema).map_err(|e| {
            AppError::new(
                ErrorCategory::SerializationError,
                format!("serialize schema: {e}"),
            )
        })?
    };

    match args.out {
        Some(path) => {
            fs::write(&path, output).map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to write schema to {}: {e}", path.display()),
                )
            })?;
            println!("Schema written to {}", path.display());
        }
        None => println!("{output}"),
    }

    Ok(())
}
