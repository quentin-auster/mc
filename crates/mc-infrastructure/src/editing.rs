use std::{
    ops::Range,
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use futures_util::stream;
use mc_application::{
    ArtifactStore, EditError, EditOperation, EditResult, EventStore, NewArtifact, NewRunEvent,
    RepositoryEditor,
};
use mc_domain::{ArtifactId, ArtifactKind, EventId, EventPayload, RunId, Timestamp, Version};
use tokio::{io::AsyncWriteExt, process::Command};
use tracing::{info, instrument};

#[derive(Clone, Debug)]
pub struct LocalRepositoryEditor<A, E> {
    worktree_root: PathBuf,
    artifacts: A,
    events: E,
}

impl<A, E> LocalRepositoryEditor<A, E> {
    #[must_use]
    pub fn new(worktree_root: impl Into<PathBuf>, artifacts: A, events: E) -> Self {
        Self {
            worktree_root: worktree_root.into(),
            artifacts,
            events,
        }
    }
}

impl<A, E> RepositoryEditor for LocalRepositoryEditor<A, E>
where
    A: ArtifactStore,
    E: EventStore,
{
    async fn apply_patch(
        &self,
        run_id: RunId,
        worktree: PathBuf,
        path: PathBuf,
        patch: Bytes,
    ) -> Result<EditResult, EditError> {
        let (worktree, target) = self.paths(&worktree, &path, true).await?;
        validate_single_file_patch(&path, &patch)?;
        let before = tokio::fs::read(&target).await.map_err(storage)?;
        git_apply(&worktree, &patch, true).await?;
        git_apply(&worktree, &patch, false).await?;
        let after = tokio::fs::read(&target).await.map_err(storage)?;
        self.record(
            run_id,
            path,
            EditOperation::ApplyPatch,
            Bytes::from(before),
            Bytes::from(after),
        )
        .await
    }

    async fn replace_lines(
        &self,
        run_id: RunId,
        worktree: PathBuf,
        path: PathBuf,
        range: Range<u64>,
        replacement: Bytes,
    ) -> Result<EditResult, EditError> {
        if range.start == 0 || range.start >= range.end {
            return Err(EditError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }
        let (_, target) = self.paths(&worktree, &path, true).await?;
        let before = tokio::fs::read(&target).await.map_err(storage)?;
        let text = std::str::from_utf8(&before).map_err(storage)?;
        let mut lines = text.split_inclusive('\n').collect::<Vec<_>>();
        let start = (range.start - 1) as usize;
        let end = (range.end - 1) as usize;
        if start >= lines.len() || end > lines.len() {
            return Err(EditError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }
        let replacement = std::str::from_utf8(&replacement).map_err(storage)?;
        lines.splice(start..end, [replacement]);
        let after = lines.concat().into_bytes();
        tokio::fs::write(&target, &after).await.map_err(storage)?;
        self.record(
            run_id,
            path,
            EditOperation::ReplaceRange,
            Bytes::from(before),
            Bytes::from(after),
        )
        .await
    }

    async fn create(
        &self,
        run_id: RunId,
        worktree: PathBuf,
        path: PathBuf,
        contents: Bytes,
    ) -> Result<EditResult, EditError> {
        let (worktree, target) = self.paths(&worktree, &path, false).await?;
        if tokio::fs::try_exists(&target).await.map_err(storage)? {
            return Err(EditError::AlreadyExists);
        }
        let parent = target.parent().ok_or(EditError::InvalidPath)?;
        let canonical_parent = tokio::fs::canonicalize(parent).await.map_err(storage)?;
        if !canonical_parent.starts_with(&worktree) {
            return Err(EditError::InvalidPath);
        }
        tokio::fs::write(&target, &contents)
            .await
            .map_err(storage)?;
        self.record(run_id, path, EditOperation::Create, Bytes::new(), contents)
            .await
    }

    async fn delete(
        &self,
        run_id: RunId,
        worktree: PathBuf,
        path: PathBuf,
    ) -> Result<EditResult, EditError> {
        let (_, target) = self.paths(&worktree, &path, true).await?;
        let before = tokio::fs::read(&target).await.map_err(storage)?;
        tokio::fs::remove_file(&target).await.map_err(storage)?;
        self.record(
            run_id,
            path,
            EditOperation::Delete,
            Bytes::from(before),
            Bytes::new(),
        )
        .await
    }

    async fn revert(
        &self,
        run_id: RunId,
        worktree: PathBuf,
        path: PathBuf,
    ) -> Result<EditResult, EditError> {
        let (worktree, target) = self.paths(&worktree, &path, true).await?;
        let before = tokio::fs::read(&target).await.map_err(storage)?;
        git(
            &worktree,
            &["checkout", "--", path.to_string_lossy().as_ref()],
        )
        .await?;
        let after = tokio::fs::read(&target).await.map_err(storage)?;
        self.record(
            run_id,
            path,
            EditOperation::Revert,
            Bytes::from(before),
            Bytes::from(after),
        )
        .await
    }
}

impl<A, E> LocalRepositoryEditor<A, E>
where
    A: ArtifactStore,
    E: EventStore,
{
    async fn paths(
        &self,
        requested_worktree: &Path,
        relative: &Path,
        must_exist: bool,
    ) -> Result<(PathBuf, PathBuf), EditError> {
        validate_relative(relative)?;
        let root = tokio::fs::canonicalize(&self.worktree_root)
            .await
            .map_err(storage)?;
        let worktree = tokio::fs::canonicalize(requested_worktree)
            .await
            .map_err(storage)?;
        if !worktree.starts_with(&root) || worktree == root {
            return Err(EditError::InvalidPath);
        }
        let target = worktree.join(relative);
        if must_exist {
            let canonical = tokio::fs::canonicalize(&target).await.map_err(storage)?;
            if !canonical.starts_with(&worktree) {
                return Err(EditError::InvalidPath);
            }
            Ok((worktree, canonical))
        } else {
            Ok((worktree, target))
        }
    }

    #[instrument(skip(self, before, after), fields(%run_id, path = %path.display(), ?operation))]
    async fn record(
        &self,
        run_id: RunId,
        path: PathBuf,
        operation: EditOperation,
        before: Bytes,
        after: Bytes,
    ) -> Result<EditResult, EditError> {
        let timestamp = now();
        let before_artifact_id = self.store(run_id, before, timestamp).await?;
        let after_artifact_id = self.store(run_id, after, timestamp).await?;
        let event_id = EventId::new();
        let payload = EventPayload::new(
            "repository_edited",
            Version::new(1).expect("event version is non-zero"),
            serde_json::json!({
                "operation": operation_name(operation),
                "path": path.to_string_lossy(),
                "before_artifact_id": before_artifact_id,
                "after_artifact_id": after_artifact_id
            }),
        )
        .map_err(storage)?;
        self.events
            .append(NewRunEvent {
                id: event_id,
                run_id,
                occurred_at: timestamp,
                causation_id: None,
                correlation_id: None,
                payload,
            })
            .await
            .map_err(storage)?;
        info!("repository edit recorded");
        Ok(EditResult {
            operation,
            path,
            before_artifact_id,
            after_artifact_id,
            event_id,
        })
    }

    async fn store(
        &self,
        run_id: RunId,
        contents: Bytes,
        created_at: Timestamp,
    ) -> Result<ArtifactId, EditError> {
        let id = ArtifactId::new();
        self.artifacts
            .put(
                NewArtifact {
                    id,
                    run_id,
                    kind: ArtifactKind::Patch,
                    media_type: "application/octet-stream".to_owned(),
                    created_at,
                },
                Box::pin(stream::once(async move { Ok(contents) })),
            )
            .await
            .map_err(storage)?;
        Ok(id)
    }
}

fn validate_relative(path: &Path) -> Result<(), EditError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        Err(EditError::InvalidPath)
    } else {
        Ok(())
    }
}

fn validate_single_file_patch(path: &Path, patch: &[u8]) -> Result<(), EditError> {
    let patch = std::str::from_utf8(patch).map_err(storage)?;
    let expected = path.to_string_lossy();
    let mut headers = 0;
    for line in patch
        .lines()
        .filter(|line| line.starts_with("--- ") || line.starts_with("+++ "))
    {
        headers += 1;
        let candidate = line[4..].split_whitespace().next().unwrap_or_default();
        let candidate = candidate
            .strip_prefix("a/")
            .or_else(|| candidate.strip_prefix("b/"))
            .unwrap_or(candidate);
        if candidate != expected && candidate != "/dev/null" {
            return Err(EditError::InvalidPath);
        }
    }
    if headers == 2 {
        Ok(())
    } else {
        Err(EditError::InvalidPath)
    }
}

async fn git_apply(worktree: &Path, patch: &[u8], check: bool) -> Result<(), EditError> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(worktree)
        .args(["apply", "--whitespace=nowarn"]);
    if check {
        command.arg("--check");
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(storage)?;
    child
        .stdin
        .take()
        .ok_or_else(|| storage(std::io::Error::other("Git stdin unavailable")))?
        .write_all(patch)
        .await
        .map_err(storage)?;
    let output = child.wait_with_output().await.map_err(storage)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(storage(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        )))
    }
}

async fn git(worktree: &Path, args: &[&str]) -> Result<(), EditError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(args)
        .output()
        .await
        .map_err(storage)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(storage(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        )))
    }
}

const fn operation_name(operation: EditOperation) -> &'static str {
    match operation {
        EditOperation::ApplyPatch => "apply_patch",
        EditOperation::ReplaceRange => "replace_range",
        EditOperation::Create => "create",
        EditOperation::Delete => "delete",
        EditOperation::Revert => "revert",
    }
}

fn now() -> Timestamp {
    Timestamp::from_unix_milliseconds(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
    )
}

fn storage(error: impl std::error::Error + Send + Sync + 'static) -> EditError {
    EditError::Storage(Box::new(error))
}
