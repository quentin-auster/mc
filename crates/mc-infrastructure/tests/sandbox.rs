use std::{collections::BTreeMap, time::Duration};

use futures_util::StreamExt;
use mc_application::{ArtifactStore, Sandbox, SandboxError, SandboxRequest};
use mc_domain::RunId;
use mc_infrastructure::{DockerSandbox, PgObjectArtifactStore, SandboxLimits, build_s3_store};
use sqlx::PgPool;
use uuid::Uuid;

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

async fn seed_run(pool: &PgPool) -> RunId {
    let repository_id = Uuid::new_v4();
    let snapshot_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories (id, locator) VALUES ($1, 'sandbox-test')")
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

async fn read_artifact(store: &PgObjectArtifactStore, id: mc_domain::ArtifactId) -> Vec<u8> {
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
#[ignore = "requires Docker, PostgreSQL, and MinIO; CI runs ignored sandbox tests explicitly"]
async fn enforces_limits_and_captures_outputs(pool: PgPool) {
    let run_id = seed_run(&pool).await;
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("worktrees");
    let worktree = root.join(run_id.to_string());
    std::fs::create_dir_all(&worktree).unwrap();
    let objects = build_s3_store(
        "http://localhost:59000",
        "mc-artifacts",
        "mcdev",
        "mc_dev_only_password",
    )
    .unwrap();
    let artifacts = PgObjectArtifactStore::new(pool, objects);
    let sandbox = DockerSandbox::new(
        artifacts.clone(),
        &root,
        "alpine:3.21",
        SandboxLimits {
            cpus: 1,
            memory_megabytes: 64,
            processes: 32,
            timeout: Duration::from_secs(5),
        },
        ["MC_ALLOWED".to_owned()],
    );
    let result = sandbox
        .execute(SandboxRequest {
            run_id,
            worktree: worktree.clone(),
            argv: vec![
                "sh".to_owned(),
                "-c".to_owned(),
                "printf '%s\\n' \"$MC_ALLOWED\"; cat /proc/net/dev".to_owned(),
            ],
            environment: BTreeMap::from([("MC_ALLOWED".to_owned(), "visible".to_owned())]),
        })
        .await
        .unwrap();
    assert_eq!(result.exit_code, Some(0));
    assert!(!result.timed_out);
    let stdout = read_artifact(&artifacts, result.stdout_artifact_id).await;
    let stdout = String::from_utf8(stdout).unwrap();
    assert!(stdout.starts_with("visible\n"));
    assert!(stdout.contains("lo:"));
    assert!(!stdout.contains("eth0:"));
    assert!(
        read_artifact(&artifacts, result.stderr_artifact_id)
            .await
            .is_empty()
    );

    let denied = sandbox
        .execute(SandboxRequest {
            run_id,
            worktree: worktree.clone(),
            argv: vec!["true".to_owned()],
            environment: BTreeMap::from([("SECRET".to_owned(), "hidden".to_owned())]),
        })
        .await;
    assert!(matches!(
        denied,
        Err(SandboxError::EnvironmentDenied { .. })
    ));
    let invalid = sandbox
        .execute(SandboxRequest {
            run_id,
            worktree: temporary.path().to_path_buf(),
            argv: vec!["true".to_owned()],
            environment: BTreeMap::new(),
        })
        .await;
    assert!(matches!(invalid, Err(SandboxError::InvalidWorktree)));

    let short_sandbox = DockerSandbox::new(
        artifacts,
        &root,
        "alpine:3.21",
        SandboxLimits {
            cpus: 1,
            memory_megabytes: 64,
            processes: 32,
            timeout: Duration::from_millis(100),
        },
        Vec::new(),
    );
    let timed_out = short_sandbox
        .execute(SandboxRequest {
            run_id,
            worktree,
            argv: vec!["sleep".to_owned(), "2".to_owned()],
            environment: BTreeMap::new(),
        })
        .await
        .unwrap();
    assert!(timed_out.timed_out);
    assert_eq!(timed_out.exit_code, None);
}
