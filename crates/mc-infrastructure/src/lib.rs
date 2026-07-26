#![doc = "Infrastructure adapters that implement MC application ports."]

mod artifact_store;
mod command_tools;
mod context_snapshot;
mod editing;
mod event_store;
mod model_provider;
mod repository;
mod repository_tools;
mod sandbox;
mod worktree;

pub use artifact_store::{PgObjectArtifactStore, build_s3_store};
pub use command_tools::SandboxCommandTools;
pub use context_snapshot::PgContextSnapshotStore;
pub use editing::LocalRepositoryEditor;
pub use event_store::PgEventStore;
pub use mc_application::DOMAIN_CONTRACT_VERSION;
pub use model_provider::{OpenAiResponsesProvider, PgModelInvocationService};
pub use repository::GitRepositoryStore;
pub use repository_tools::LocalRepositoryReader;
pub use sandbox::{DockerSandbox, SandboxLimits};
pub use worktree::GitWorktreeManager;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Applies every pending canonical database migration.
pub async fn migrate(database_url: &str) -> anyhow::Result<()> {
    tracing::info!(
        operation = "database_migrate",
        "applying database migrations"
    );
    let pool = sqlx::PgPool::connect(database_url).await?;
    MIGRATOR.run(&pool).await?;
    tracing::info!(
        operation = "database_migrate",
        "database migrations applied"
    );
    Ok(())
}
