#![doc = "Infrastructure adapters that implement MC application ports."]

mod artifact_store;
mod context_snapshot;
mod event_store;
mod model_provider;
mod repository;

pub use artifact_store::{PgObjectArtifactStore, build_s3_store};
pub use context_snapshot::PgContextSnapshotStore;
pub use event_store::PgEventStore;
pub use mc_application::DOMAIN_CONTRACT_VERSION;
pub use model_provider::{OpenAiResponsesProvider, PgModelInvocationService};
pub use repository::GitRepositoryStore;

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
