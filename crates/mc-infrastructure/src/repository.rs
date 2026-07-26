use std::path::{Path, PathBuf};

use mc_application::{
    CreateRepositorySnapshot, RegisterRepository, RepositoryStore, RepositoryStoreError,
};
use mc_domain::{Repository, RepositoryId, RepositorySnapshot, Sha256Digest};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::process::Command;
use tracing::{info, instrument};

#[derive(Clone, Debug)]
pub struct GitRepositoryStore {
    pool: PgPool,
    mirror_root: PathBuf,
}

impl GitRepositoryStore {
    #[must_use]
    pub fn new(pool: PgPool, mirror_root: impl Into<PathBuf>) -> Self {
        Self {
            pool,
            mirror_root: mirror_root.into(),
        }
    }

    fn mirror_path(&self, repository_id: RepositoryId) -> PathBuf {
        self.mirror_root.join(format!("{repository_id}.git"))
    }
}

impl RepositoryStore for GitRepositoryStore {
    #[instrument(skip(self, repository), fields(repository_id = %repository.id))]
    async fn register(
        &self,
        repository: RegisterRepository,
    ) -> Result<Repository, RepositoryStoreError> {
        let record = Repository::new(repository.id, repository.locator.clone()).map_err(storage)?;
        tokio::fs::create_dir_all(&self.mirror_root)
            .await
            .map_err(storage)?;
        let path = self.mirror_path(repository.id);
        if path.exists() {
            git(&path, ["remote", "set-url", "origin", &repository.locator]).await?;
            git(&path, ["fetch", "--prune", "origin"]).await?;
        } else {
            command(
                Command::new("git")
                    .arg("clone")
                    .arg("--mirror")
                    .arg("--")
                    .arg(&repository.locator)
                    .arg(&path),
            )
            .await?;
        }
        sqlx::query(
            "INSERT INTO repositories (id, locator) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET locator = EXCLUDED.locator",
        )
        .bind(repository.id.as_uuid())
        .bind(&repository.locator)
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        info!(mirror = %path.display(), "repository registered");
        Ok(record)
    }

    #[instrument(skip(self, snapshot), fields(repository_id = %snapshot.repository_id, snapshot_id = %snapshot.id, branch = %snapshot.branch))]
    async fn snapshot(
        &self,
        snapshot: CreateRepositorySnapshot,
    ) -> Result<RepositorySnapshot, RepositoryStoreError> {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM repositories WHERE id = $1)")
                .bind(snapshot.repository_id.as_uuid())
                .fetch_one(&self.pool)
                .await
                .map_err(storage)?;
        if !exists {
            return Err(RepositoryStoreError::NotFound {
                repository_id: snapshot.repository_id,
            });
        }
        validate_branch(&snapshot.branch).await?;
        let path = self.mirror_path(snapshot.repository_id);
        git(&path, ["fetch", "--prune", "origin"]).await?;
        let reference = format!("refs/heads/{}", snapshot.branch);
        let commit = git(
            &path,
            ["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
        )
        .await?;
        let manifest = git_bytes(&path, ["ls-tree", "-r", "-z", &commit]).await?;
        let digest =
            Sha256Digest::new(format!("{:x}", Sha256::digest(manifest))).map_err(storage)?;
        sqlx::query(
            "INSERT INTO repository_snapshots (id, repository_id, source_revision, tree_digest, created_at) VALUES ($1, $2, $3, $4, to_timestamp($5::double precision / 1000))",
        )
        .bind(snapshot.id.as_uuid())
        .bind(snapshot.repository_id.as_uuid())
        .bind(&commit)
        .bind(digest.as_str())
        .bind(snapshot.created_at.unix_milliseconds())
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        info!(source_revision = %commit, tree_digest = %digest.as_str(), "repository snapshot created");
        Ok(RepositorySnapshot {
            id: snapshot.id,
            repository_id: snapshot.repository_id,
            parent_id: None,
            source_revision: Some(commit),
            tree_digest: digest,
            created_at: snapshot.created_at,
        })
    }
}

async fn validate_branch(branch: &str) -> Result<(), RepositoryStoreError> {
    if branch.trim().is_empty() {
        return Err(RepositoryStoreError::InvalidBranch {
            branch: branch.to_owned(),
        });
    }
    let output = Command::new("git")
        .args(["check-ref-format", "--branch", branch])
        .output()
        .await
        .map_err(storage)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(RepositoryStoreError::InvalidBranch {
            branch: branch.to_owned(),
        })
    }
}

async fn git<const N: usize>(
    repository: &Path,
    args: [&str; N],
) -> Result<String, RepositoryStoreError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repository).args(args);
    let output = command_output(&mut command).await?;
    String::from_utf8(output)
        .map(|value| value.trim().to_owned())
        .map_err(storage)
}

async fn git_bytes<const N: usize>(
    repository: &Path,
    args: [&str; N],
) -> Result<Vec<u8>, RepositoryStoreError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repository).args(args);
    command_output(&mut command).await
}

async fn command(command: &mut Command) -> Result<(), RepositoryStoreError> {
    command_output(command).await.map(|_| ())
}

async fn command_output(command: &mut Command) -> Result<Vec<u8>, RepositoryStoreError> {
    let output = command.output().await.map_err(storage)?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(storage(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        )))
    }
}

fn storage(error: impl std::error::Error + Send + Sync + 'static) -> RepositoryStoreError {
    RepositoryStoreError::Storage(Box::new(error))
}
