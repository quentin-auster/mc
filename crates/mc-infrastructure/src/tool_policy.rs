use std::path::{Component, Path};

use mc_application::{
    PolicyDenialReason, RolePolicy, ToolAction, ToolPolicyConfig, ToolPolicyDecision,
    ToolPolicyEngine, ToolPolicyError, ToolPolicyRequest, ToolRole,
};
use sqlx::PgPool;
use tracing::{info, instrument, warn};

#[derive(Clone, Debug)]
pub struct PgToolPolicyEngine {
    pool: PgPool,
    config: ToolPolicyConfig,
}

impl PgToolPolicyEngine {
    #[must_use]
    pub fn new(pool: PgPool, config: ToolPolicyConfig) -> Self {
        Self { pool, config }
    }
}

impl ToolPolicyEngine for PgToolPolicyEngine {
    #[instrument(skip(self, request), fields(decision_id = %request.id, run_id = %request.run_id, ?request.role))]
    async fn authorize(
        &self,
        request: ToolPolicyRequest,
    ) -> Result<ToolPolicyDecision, ToolPolicyError> {
        let mut transaction = self.pool.begin().await.map_err(storage)?;
        let role = role_name(request.role);
        let lock_key = format!("{}:{role}", request.run_id);
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut *transaction)
            .await
            .map_err(storage)?;
        let used: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM tool_policy_decisions WHERE run_id = $1 AND role = $2 AND allowed",
        )
        .bind(request.run_id.as_uuid())
        .bind(role)
        .fetch_one(&mut *transaction)
        .await
        .map_err(storage)?;
        let policy = self.config.roles.get(&request.role);
        let max_calls = policy.map_or(0, |policy| policy.max_calls);
        let denial_reason = if policy.is_none() {
            Some(PolicyDenialReason::RoleDenied)
        } else if used as u64 >= max_calls {
            Some(PolicyDenialReason::BudgetExceeded)
        } else {
            evaluate(policy, &request.action)
        };
        let allowed = denial_reason.is_none();
        let consumed = used as u64 + u64::from(allowed);
        let remaining_calls = max_calls.saturating_sub(consumed);
        let decision = ToolPolicyDecision {
            id: request.id,
            run_id: request.run_id,
            role: request.role,
            action: request.action,
            allowed,
            denial_reason,
            remaining_calls,
            policy_version: self.config.version,
            decided_at: request.requested_at,
        };
        sqlx::query(
            "INSERT INTO tool_policy_decisions (id, run_id, role, action, allowed, denial_reason, remaining_calls, policy_version, decided_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, to_timestamp($9::double precision / 1000))",
        )
        .bind(decision.id.as_uuid())
        .bind(decision.run_id.as_uuid())
        .bind(role_name(decision.role))
        .bind(serde_json::to_value(&decision.action).map_err(storage)?)
        .bind(decision.allowed)
        .bind(decision.denial_reason.map(denial_name))
        .bind(decision.remaining_calls as i64)
        .bind(i64::from(decision.policy_version.get()))
        .bind(decision.decided_at.unix_milliseconds())
        .execute(&mut *transaction)
        .await
        .map_err(storage)?;
        transaction.commit().await.map_err(storage)?;
        if allowed {
            info!(remaining_calls, "tool action authorized");
        } else {
            warn!(?denial_reason, remaining_calls, "tool action denied");
        }
        Ok(decision)
    }
}

fn evaluate(policy: Option<&RolePolicy>, action: &ToolAction) -> Option<PolicyDenialReason> {
    let Some(policy) = policy else {
        return Some(PolicyDenialReason::RoleDenied);
    };
    match action {
        ToolAction::Read { path } => {
            if policy.allow_read && allowed_path(policy, path) {
                None
            } else {
                Some(PolicyDenialReason::PathDenied)
            }
        }
        ToolAction::Edit { path } => {
            if policy.allow_edit && allowed_path(policy, path) {
                None
            } else {
                Some(PolicyDenialReason::PathDenied)
            }
        }
        ToolAction::Command { category, argv } => policy
            .command_prefixes
            .get(category)
            .is_some_and(|prefixes| {
                prefixes.iter().any(|prefix| {
                    !prefix.is_empty()
                        && argv.len() >= prefix.len()
                        && argv[..prefix.len()] == prefix[..]
                })
            })
            .then_some(())
            .map_or(Some(PolicyDenialReason::CommandDenied), |_| None),
        ToolAction::Network { host } => policy
            .network_hosts
            .contains(host)
            .then_some(())
            .map_or(Some(PolicyDenialReason::NetworkDenied), |_| None),
        ToolAction::Secret { name } => policy
            .secret_names
            .contains(name)
            .then_some(())
            .map_or(Some(PolicyDenialReason::SecretDenied), |_| None),
    }
}

fn allowed_path(policy: &RolePolicy, path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        && policy
            .path_prefixes
            .iter()
            .any(|prefix| path.starts_with(prefix))
}

const fn role_name(role: ToolRole) -> &'static str {
    match role {
        ToolRole::Explorer => "explorer",
        ToolRole::Editor => "editor",
        ToolRole::Executor => "executor",
        ToolRole::Reviewer => "reviewer",
        ToolRole::Administrator => "administrator",
    }
}

const fn denial_name(reason: PolicyDenialReason) -> &'static str {
    match reason {
        PolicyDenialReason::RoleDenied => "role_denied",
        PolicyDenialReason::PathDenied => "path_denied",
        PolicyDenialReason::CommandDenied => "command_denied",
        PolicyDenialReason::NetworkDenied => "network_denied",
        PolicyDenialReason::SecretDenied => "secret_denied",
        PolicyDenialReason::BudgetExceeded => "budget_exceeded",
    }
}

fn storage(error: impl std::error::Error + Send + Sync + 'static) -> ToolPolicyError {
    ToolPolicyError::Storage(Box::new(error))
}
