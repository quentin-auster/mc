use std::{path::Path, process::Command};

use mc_application::{
    CreateRepositorySnapshot, RegisterRepository, RepositoryStore, WorktreeManager, WorktreeOutcome,
};
use mc_domain::{RepositoryId, RunId, SnapshotId, TaskId, Timestamp};
use mc_infrastructure::{GitRepositoryStore, GitWorktreeManager};
use sqlx::PgPool;

const CREATED_AT: Timestamp = Timestamp::from_unix_milliseconds(1_750_000_000_000);

fn git(repository: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires PostgreSQL and Git; CI runs ignored worktree tests explicitly"]
async fn tracks_retains_and_cleans_run_worktrees(pool: PgPool) {
    let temporary = tempfile::tempdir().unwrap();
    let origin = temporary.path().join("origin");
    std::fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "-b", "main"]);
    git(&origin, &["config", "user.name", "MC Test"]);
    git(&origin, &["config", "user.email", "mc@example.invalid"]);
    std::fs::write(origin.join("tracked.txt"), "before\n").unwrap();
    git(&origin, &["add", "tracked.txt"]);
    git(&origin, &["commit", "-m", "initial"]);

    let mirrors = temporary.path().join("mirrors");
    let repository_store = GitRepositoryStore::new(pool.clone(), &mirrors);
    let repository_id = RepositoryId::new();
    repository_store
        .register(RegisterRepository {
            id: repository_id,
            locator: origin.to_string_lossy().into_owned(),
        })
        .await
        .unwrap();
    let snapshot_id = SnapshotId::new();
    repository_store
        .snapshot(CreateRepositorySnapshot {
            id: snapshot_id,
            repository_id,
            branch: "main".to_owned(),
            created_at: CREATED_AT,
        })
        .await
        .unwrap();

    let task_id = TaskId::new();
    let run_id = RunId::new();
    sqlx::query("INSERT INTO tasks (id, repository_id) VALUES ($1, $2)")
        .bind(task_id.as_uuid())
        .bind(repository_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO task_contracts (task_id, version, objective) VALUES ($1, 1, 'test')")
        .bind(task_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO runs (id, repository_id, task_id, task_contract_version, snapshot_id, status) VALUES ($1, $2, $3, 1, $4, 'running')")
        .bind(run_id.as_uuid())
        .bind(repository_id.as_uuid())
        .bind(task_id.as_uuid())
        .bind(snapshot_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let manager =
        GitWorktreeManager::new(pool.clone(), &mirrors, temporary.path().join("worktrees"));
    let created = manager.create(run_id, snapshot_id).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(created.path.join("tracked.txt")).unwrap(),
        "before\n"
    );
    std::fs::write(created.path.join("tracked.txt"), "after\n").unwrap();
    std::fs::write(created.path.join("untracked.txt"), "new\n").unwrap();

    let dirty = manager.inspect(run_id).await.unwrap();
    assert!(dirty.dirty_files.contains(&"tracked.txt".to_owned()));
    assert!(dirty.dirty_files.contains(&"untracked.txt".to_owned()));
    assert!(String::from_utf8_lossy(&dirty.diff).contains("+after"));
    let persisted_diff: Vec<u8> =
        sqlx::query_scalar("SELECT diff FROM run_worktrees WHERE run_id = $1")
            .bind(run_id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(persisted_diff, dirty.diff);

    let retained = manager
        .complete(run_id, WorktreeOutcome::Failed, true)
        .await
        .unwrap()
        .unwrap();
    assert!(retained.retained);
    assert!(retained.path.exists());

    assert!(
        manager
            .complete(run_id, WorktreeOutcome::Succeeded, false)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!retained.path.exists());
    let status: String = sqlx::query_scalar("SELECT status FROM run_worktrees WHERE run_id = $1")
        .bind(run_id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "cleaned");
}
