use crate::{
    ProjectionBinding, ProjectionError, ProjectionPort, ProjectionReceipt, ProjectionUpdate,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Narrow command seam, implemented by the existing Newton GhRunner integration.
#[async_trait]
pub trait GhCommand: Send + Sync {
    /// Run a typed `gh` argument vector. Implementations must not invoke a shell.
    async fn run(&self, args: &[String]) -> Result<(), ProjectionError>;
}

/// GitHub Project status assignment to an existing durably bound item.
///
/// This adapter never creates resources, searches issue bodies, lists work, or
/// reads board status. Retrying an ambiguous result assigns the same option to
/// the same item; it cannot duplicate an issue or create internal work.
pub struct GithubProjectProjection {
    command: Arc<dyn GhCommand>,
}

impl GithubProjectProjection {
    /// Compose an existing authorized gh command boundary or a deterministic fake.
    pub fn with_command(command: Arc<dyn GhCommand>) -> Self {
        Self { command }
    }

    /// Reuse Newton's current GhRunner, including guarded subprocess cleanup.
    ///
    /// The embedding application must authorize writes before binding/delivery;
    /// enabling this transport does not grant permission by itself.
    #[cfg(feature = "existing-gh")]
    pub fn with_existing_gh(workspace: impl Into<std::path::PathBuf>) -> Self {
        Self::with_command(Arc::new(ExistingGhCommand {
            runner: Arc::new(newton_core::workflow::operators::gh::default_runner()),
            workspace: workspace.into(),
        }))
    }
}

#[async_trait]
impl ProjectionPort for GithubProjectProjection {
    async fn reflect(
        &self,
        binding: &ProjectionBinding,
        update: &ProjectionUpdate,
    ) -> Result<ProjectionReceipt, ProjectionError> {
        let ProjectionBinding::GithubProjectItem {
            project_id,
            item_id,
            field_id,
            status_options,
        } = binding
        else {
            return Err(ProjectionError::Invalid(
                "GitHub projection requires an existing Project item binding".into(),
            ));
        };
        let option = status_options.get(&update.derived_status).ok_or_else(|| {
            ProjectionError::Invalid(format!(
                "no option configured for internal status {}",
                update.derived_status
            ))
        })?;
        if [project_id, item_id, field_id, option]
            .iter()
            .any(|value| value.trim().is_empty())
        {
            return Err(ProjectionError::Invalid(
                "Project, item, field, and option IDs must be nonempty".into(),
            ));
        }
        self.command
            .run(
                &[
                    "project",
                    "item-edit",
                    "--project-id",
                    project_id,
                    "--id",
                    item_id,
                    "--field-id",
                    field_id,
                    "--single-select-option-id",
                    option,
                ]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
            )
            .await?;
        Ok(ProjectionReceipt {
            external_ref: Some(item_id.clone()),
        })
    }
}

#[cfg(feature = "existing-gh")]
struct ExistingGhCommand {
    runner: Arc<dyn newton_core::workflow::operators::gh::GhRunner>,
    workspace: std::path::PathBuf,
}

#[cfg(feature = "existing-gh")]
#[async_trait]
impl GhCommand for ExistingGhCommand {
    async fn run(&self, args: &[String]) -> Result<(), ProjectionError> {
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.runner.run(&args.iter().map(String::as_str).collect::<Vec<_>>(), &self.workspace),
        ).await
            .map_err(|_| ProjectionError::Delivery("gh status assignment timed out; the same assignment may be retried".into()))?
            .map(|_| ())
            // Do not persist arbitrary gh stderr; it can contain sensitive details.
            .map_err(|_| ProjectionError::Delivery("gh status assignment failed; check authorized gh access and retry projection delivery".into()))
    }
}
