use std::collections::{BTreeMap, BTreeSet};

use mc_application::{
    CommandToolKind, PolicyDenialReason, RolePolicy, ToolAction, ToolPolicyConfig,
    ToolPolicyEngine, ToolPolicyRequest, ToolRole,
};
use mc_domain::{RunId, Timestamp, ToolPolicyDecisionId, Version};
use mc_infrastructure::PgToolPolicyEngine;
use sqlx::PgPool;
use uuid::Uuid;

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DECIDED_AT: Timestamp = Timestamp::from_unix_milliseconds(1_750_000_000_000);

async fn seed_run(pool: &PgPool) -> RunId {
    let repository_id = Uuid::new_v4();
    let snapshot_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories (id, locator) VALUES ($1, 'policy-test')")
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

fn request(run_id: RunId, role: ToolRole, action: ToolAction) -> ToolPolicyRequest {
    ToolPolicyRequest {
        id: ToolPolicyDecisionId::new(),
        run_id,
        role,
        action,
        requested_at: DECIDED_AT,
    }
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires PostgreSQL; CI runs ignored tool-policy tests explicitly"]
async fn enforces_roles_rules_and_durable_budgets(pool: PgPool) {
    let run_id = seed_run(&pool).await;
    let editor = RolePolicy {
        allow_read: true,
        allow_edit: true,
        path_prefixes: vec!["src".into()],
        command_prefixes: BTreeMap::from([(
            CommandToolKind::Test,
            vec![vec!["cargo".to_owned(), "test".to_owned()]],
        )]),
        network_hosts: BTreeSet::from(["github.com".to_owned()]),
        secret_names: BTreeSet::from(["CI_TOKEN".to_owned()]),
        max_calls: 2,
    };
    let engine = PgToolPolicyEngine::new(
        pool.clone(),
        ToolPolicyConfig {
            version: Version::new(4).unwrap(),
            roles: BTreeMap::from([(ToolRole::Editor, editor)]),
        },
    );

    let read = engine
        .authorize(request(
            run_id,
            ToolRole::Editor,
            ToolAction::Read {
                path: "src/lib.rs".into(),
            },
        ))
        .await
        .unwrap();
    assert!(read.allowed);
    assert_eq!(read.remaining_calls, 1);
    assert_eq!(read.policy_version.get(), 4);

    let denied_secret = engine
        .authorize(request(
            run_id,
            ToolRole::Editor,
            ToolAction::Secret {
                name: "DATABASE_URL".to_owned(),
            },
        ))
        .await
        .unwrap();
    assert_eq!(
        denied_secret.denial_reason,
        Some(PolicyDenialReason::SecretDenied)
    );
    assert_eq!(denied_secret.remaining_calls, 1);

    let command = engine
        .authorize(request(
            run_id,
            ToolRole::Editor,
            ToolAction::Command {
                category: CommandToolKind::Test,
                argv: vec![
                    "cargo".to_owned(),
                    "test".to_owned(),
                    "--workspace".to_owned(),
                ],
            },
        ))
        .await
        .unwrap();
    assert!(command.allowed);
    assert_eq!(command.remaining_calls, 0);

    let exhausted = engine
        .authorize(request(
            run_id,
            ToolRole::Editor,
            ToolAction::Network {
                host: "github.com".to_owned(),
            },
        ))
        .await
        .unwrap();
    assert_eq!(
        exhausted.denial_reason,
        Some(PolicyDenialReason::BudgetExceeded)
    );

    let missing_role = engine
        .authorize(request(
            run_id,
            ToolRole::Explorer,
            ToolAction::Read {
                path: "src/lib.rs".into(),
            },
        ))
        .await
        .unwrap();
    assert_eq!(
        missing_role.denial_reason,
        Some(PolicyDenialReason::RoleDenied)
    );

    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM tool_policy_decisions WHERE run_id = $1")
            .bind(run_id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 5);
    let persisted_secret: serde_json::Value = sqlx::query_scalar(
        "SELECT action FROM tool_policy_decisions WHERE denial_reason = 'secret_denied'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted_secret["name"], "DATABASE_URL");
    assert!(persisted_secret.get("value").is_none());
}
