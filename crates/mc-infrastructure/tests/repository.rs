use std::{path::Path, process::Command};

use mc_application::{
    CreateRepositorySnapshot, RegisterRepository, RepositoryStore, RepositoryStoreError,
};
use mc_domain::{RepositoryId, SnapshotId, Timestamp};
use mc_infrastructure::GitRepositoryStore;
use sqlx::PgPool;

const CREATED_AT: Timestamp = Timestamp::from_unix_milliseconds(1_750_000_000_000);

fn git(repository: &Path, args: &[&str]) -> String {
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
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(repository: &Path, contents: &str, message: &str) -> String {
    std::fs::write(repository.join("tracked.txt"), contents).unwrap();
    git(repository, &["add", "tracked.txt"]);
    git(repository, &["commit", "-m", message]);
    git(repository, &["rev-parse", "HEAD"])
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires PostgreSQL and Git; CI runs ignored repository tests explicitly"]
async fn registers_fetches_and_snapshots_immutable_commits(pool: PgPool) {
    let temporary = tempfile::tempdir().unwrap();
    let origin = temporary.path().join("origin");
    std::fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "-b", "main"]);
    git(&origin, &["config", "user.name", "MC Test"]);
    git(&origin, &["config", "user.email", "mc@example.invalid"]);
    let first_commit = commit(&origin, "one\n", "first");

    let mirrors = temporary.path().join("mirrors");
    let store = GitRepositoryStore::new(pool.clone(), &mirrors);
    let repository_id = RepositoryId::new();
    let registered = store
        .register(RegisterRepository {
            id: repository_id,
            locator: origin.to_string_lossy().into_owned(),
        })
        .await
        .unwrap();
    assert_eq!(registered.id, repository_id);

    let first = store
        .snapshot(CreateRepositorySnapshot {
            id: SnapshotId::new(),
            repository_id,
            branch: "main".to_owned(),
            created_at: CREATED_AT,
        })
        .await
        .unwrap();
    assert_eq!(
        first.source_revision.as_deref(),
        Some(first_commit.as_str())
    );

    let second_commit = commit(&origin, "two\n", "second");
    store
        .register(RegisterRepository {
            id: repository_id,
            locator: origin.to_string_lossy().into_owned(),
        })
        .await
        .unwrap();
    let second = store
        .snapshot(CreateRepositorySnapshot {
            id: SnapshotId::new(),
            repository_id,
            branch: "main".to_owned(),
            created_at: CREATED_AT,
        })
        .await
        .unwrap();
    assert_eq!(
        second.source_revision.as_deref(),
        Some(second_commit.as_str())
    );
    assert_ne!(first.tree_digest, second.tree_digest);

    let invalid = store
        .snapshot(CreateRepositorySnapshot {
            id: SnapshotId::new(),
            repository_id,
            branch: "../escape".to_owned(),
            created_at: CREATED_AT,
        })
        .await;
    assert!(matches!(
        invalid,
        Err(RepositoryStoreError::InvalidBranch { .. })
    ));
    let missing = store
        .snapshot(CreateRepositorySnapshot {
            id: SnapshotId::new(),
            repository_id: RepositoryId::new(),
            branch: "main".to_owned(),
            created_at: CREATED_AT,
        })
        .await;
    assert!(matches!(
        missing,
        Err(RepositoryStoreError::NotFound { .. })
    ));
}
