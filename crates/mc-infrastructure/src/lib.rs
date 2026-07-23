#![doc = "Infrastructure adapters that implement MC application ports."]

pub use mc_application::DOMAIN_CONTRACT_VERSION;

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
