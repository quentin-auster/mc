use std::path::{Path, PathBuf};

use bytes::Bytes;
use mc_application::{WorktreeError, WorktreeManager, WorktreeOutcome, WorktreeState};
use mc_domain::{RepositoryId, RunId, SnapshotId};
use sqlx::{PgPool, Row};
use tokio::process::Command;
use tracing::{info, instrument};

#[derive(Clone, Debug)]
pub struct GitWorktreeManager {
    pool: PgPool,
    mirror_root: PathBuf,
    worktree_root: PathBuf,
}

impl GitWorktreeManager {
    #[must_use]
    pub fn new(
        pool: PgPool,
        mirror_root: impl Into<PathBuf>,
        worktree_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            pool,
            mirror_root: mirror_root.into(),
            worktree_root: worktree_root.into(),
        }
    }

    fn worktree_path(&self, run_id: RunId) -> PathBuf {
        self.worktree_root.join(run_id.to_string())
    }
}

impl WorktreeManager for GitWorktreeManager {
    #[instrument(skip(self), fields(%run_id, %snapshot_id))]
    async fn create(
        &self,
        run_id: RunId,
        snapshot_id: SnapshotId,
    ) -> Result<WorktreeState, WorktreeError> {
        let row = sqlx::query(
            "SELECT snapshot.repository_id, snapshot.source_revision FROM repository_snapshots AS snapshot JOIN runs AS run ON run.snapshot_id = snapshot.id WHERE run.id = $1 AND snapshot.id = $2",
        )
        .bind(run_id.as_uuid())
        .bind(snapshot_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(storage)?
        .ok_or(WorktreeError::NotFound { run_id })?;
        let repository_id = RepositoryId::from_uuid(row.try_get("repository_id").map_err(storage)?);
        let revision: String = row
            .try_get::<Option<String>, _>("source_revision")
            .map_err(storage)?
            .ok_or_else(|| storage(std::io::Error::other("snapshot has no source revision")))?;
        let mirror = self.mirror_root.join(format!("{repository_id}.git"));
        let path = self.worktree_path(run_id);
        tokio::fs::create_dir_all(&self.worktree_root)
            .await
            .map_err(storage)?;
        git(
            &mirror,
            [
                "worktree",
                "add",
                "--detach",
                path.to_str().ok_or_else(non_utf8)?,
                &revision,
            ],
        )
        .await?;
        sqlx::query(
            "INSERT INTO run_worktrees (run_id, snapshot_id, path, status) VALUES ($1, $2, $3, 'active')",
        )
        .bind(run_id.as_uuid())
        .bind(snapshot_id.as_uuid())
        .bind(path.to_string_lossy().as_ref())
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        info!(path = %path.display(), "run worktree created");
        Ok(WorktreeState {
            run_id,
            snapshot_id,
            path,
            dirty_files: Vec::new(),
            diff: Bytes::new(),
            retained: false,
        })
    }

    #[instrument(skip(self), fields(%run_id))]
    async fn inspect(&self, run_id: RunId) -> Result<WorktreeState, WorktreeError> {
        let row = sqlx::query(
            "SELECT snapshot_id, path, status FROM run_worktrees WHERE run_id = $1 AND status <> 'cleaned'",
        )
        .bind(run_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(storage)?
        .ok_or(WorktreeError::NotFound { run_id })?;
        let snapshot_id = SnapshotId::from_uuid(row.try_get("snapshot_id").map_err(storage)?);
        let path = PathBuf::from(row.try_get::<String, _>("path").map_err(storage)?);
        let status = git_bytes(&path, ["status", "--porcelain=v1", "-z"]).await?;
        let dirty_files = parse_status(&status);
        let diff = git_bytes(&path, ["diff", "--binary", "HEAD"]).await?;
        sqlx::query(
            "UPDATE run_worktrees SET dirty_files = $2, diff = $3, updated_at = now() WHERE run_id = $1",
        )
        .bind(run_id.as_uuid())
        .bind(&dirty_files)
        .bind(&diff)
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(WorktreeState {
            run_id,
            snapshot_id,
            path,
            dirty_files,
            diff: Bytes::from(diff),
            retained: row.try_get::<String, _>("status").map_err(storage)? == "retained",
        })
    }

    #[instrument(skip(self), fields(%run_id, ?outcome, retain_failed))]
    async fn complete(
        &self,
        run_id: RunId,
        outcome: WorktreeOutcome,
        retain_failed: bool,
    ) -> Result<Option<WorktreeState>, WorktreeError> {
        let state = self.inspect(run_id).await?;
        if outcome == WorktreeOutcome::Failed && retain_failed {
            sqlx::query(
                "UPDATE run_worktrees SET status = 'retained', updated_at = now() WHERE run_id = $1",
            )
            .bind(run_id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(storage)?;
            info!(path = %state.path.display(), "failed run worktree retained");
            return Ok(Some(WorktreeState {
                retained: true,
                ..state
            }));
        }
        let repository_id: sqlx::types::Uuid =
            sqlx::query_scalar("SELECT repository_id FROM repository_snapshots WHERE id = $1")
                .bind(state.snapshot_id.as_uuid())
                .fetch_one(&self.pool)
                .await
                .map_err(storage)?;
        let mirror = self
            .mirror_root
            .join(format!("{}.git", RepositoryId::from_uuid(repository_id)));
        git(
            &mirror,
            [
                "worktree",
                "remove",
                "--force",
                state.path.to_str().ok_or_else(non_utf8)?,
            ],
        )
        .await?;
        sqlx::query(
            "UPDATE run_worktrees SET status = 'cleaned', updated_at = now() WHERE run_id = $1",
        )
        .bind(run_id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        info!("run worktree cleaned");
        Ok(None)
    }
}

fn parse_status(status: &[u8]) -> Vec<String> {
    status
        .split(|byte| *byte == 0)
        .filter(|entry| entry.len() >= 4)
        .map(|entry| String::from_utf8_lossy(&entry[3..]).into_owned())
        .collect()
}

async fn git<const N: usize>(repository: &Path, args: [&str; N]) -> Result<(), WorktreeError> {
    git_bytes(repository, args).await.map(|_| ())
}

async fn git_bytes<const N: usize>(
    repository: &Path,
    args: [&str; N],
) -> Result<Vec<u8>, WorktreeError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .await
        .map_err(storage)?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(storage(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        )))
    }
}

fn non_utf8() -> WorktreeError {
    storage(std::io::Error::other("managed worktree path is not UTF-8"))
}

fn storage(error: impl std::error::Error + Send + Sync + 'static) -> WorktreeError {
    WorktreeError::Storage(Box::new(error))
}
