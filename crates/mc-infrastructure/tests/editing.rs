use std::{path::Path, process::Command};

use bytes::Bytes;
use futures_util::StreamExt;
use mc_application::{ArtifactStore, EditError, RepositoryEditor};
use mc_domain::RunId;
use mc_infrastructure::{
    LocalRepositoryEditor, PgEventStore, PgObjectArtifactStore, build_s3_store,
};
use sqlx::PgPool;
use uuid::Uuid;

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn git(repository: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
}

async fn seed_run(pool: &PgPool) -> RunId {
    let repository_id = Uuid::new_v4();
    let snapshot_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories (id, locator) VALUES ($1, 'editing-test')")
        .bind(repository_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO repository_snapshots (id, repository_id, tree_digest) VALUES ($1, $2, $3)",
    )
    .bind(snapshot_id)
    .bind(repository_id)
    .bind(DIGEST)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO tasks (id, repository_id) VALUES ($1, $2)")
        .bind(task_id)
        .bind(repository_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO task_contracts (task_id, version, objective) VALUES ($1, 1, 'test')")
        .bind(task_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO runs (id, repository_id, task_id, task_contract_version, snapshot_id, status) VALUES ($1, $2, $3, 1, $4, 'running')")
        .bind(run_id)
        .bind(repository_id)
        .bind(task_id)
        .bind(snapshot_id)
        .execute(pool)
        .await
        .unwrap();
    RunId::from_uuid(run_id)
}

async fn artifact(store: &PgObjectArtifactStore, id: mc_domain::ArtifactId) -> Vec<u8> {
    store
        .read(id)
        .await
        .unwrap()
        .map(|chunk| chunk.unwrap())
        .collect::<Vec<_>>()
        .await
        .concat()
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires Git, PostgreSQL, and MinIO; CI runs ignored editing tests explicitly"]
async fn edits_capture_artifacts_events_and_reject_escape(pool: PgPool) {
    let run_id = seed_run(&pool).await;
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("worktrees");
    let worktree = root.join(run_id.to_string());
    std::fs::create_dir_all(&worktree).unwrap();
    git(&worktree, &["init", "-b", "main"]);
    git(&worktree, &["config", "user.name", "MC Test"]);
    git(&worktree, &["config", "user.email", "mc@example.invalid"]);
    std::fs::write(worktree.join("tracked.txt"), "one\ntwo\n").unwrap();
    git(&worktree, &["add", "tracked.txt"]);
    git(&worktree, &["commit", "-m", "initial"]);

    let objects = build_s3_store(
        "http://localhost:59000",
        "mc-artifacts",
        "mcdev",
        "mc_dev_only_password",
    )
    .unwrap();
    let artifacts = PgObjectArtifactStore::new(pool.clone(), objects);
    let editor =
        LocalRepositoryEditor::new(&root, artifacts.clone(), PgEventStore::new(pool.clone()));

    let replaced = editor
        .replace_lines(
            run_id,
            worktree.clone(),
            "tracked.txt".into(),
            1..2,
            Bytes::from_static(b"changed\n"),
        )
        .await
        .unwrap();
    assert_eq!(
        artifact(&artifacts, replaced.before_artifact_id).await,
        b"one\ntwo\n"
    );
    assert_eq!(
        artifact(&artifacts, replaced.after_artifact_id).await,
        b"changed\ntwo\n"
    );
    editor
        .revert(run_id, worktree.clone(), "tracked.txt".into())
        .await
        .unwrap();
    let created = editor
        .create(
            run_id,
            worktree.clone(),
            "new.txt".into(),
            Bytes::from_static(b"new\n"),
        )
        .await
        .unwrap();
    assert!(
        artifact(&artifacts, created.before_artifact_id)
            .await
            .is_empty()
    );
    editor
        .delete(run_id, worktree.clone(), "new.txt".into())
        .await
        .unwrap();
    editor
        .apply_patch(
            run_id,
            worktree.clone(),
            "tracked.txt".into(),
            Bytes::from_static(
                b"--- a/tracked.txt\n+++ b/tracked.txt\n@@ -1,2 +1,2 @@\n-one\n+patched\n two\n",
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(worktree.join("tracked.txt")).unwrap(),
        "patched\ntwo\n"
    );

    let event_count: i64 = sqlx::query_scalar("SELECT count(*) FROM run_events WHERE run_id = $1")
        .bind(run_id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(event_count, 5);
    let escaped = editor
        .create(run_id, worktree.clone(), "../escape".into(), Bytes::new())
        .await;
    assert!(matches!(escaped, Err(EditError::InvalidPath)));

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(temporary.path(), worktree.join("link")).unwrap();
        let symlink_escape = editor.delete(run_id, worktree, "link".into()).await;
        assert!(matches!(symlink_escape, Err(EditError::InvalidPath)));
    }
}
