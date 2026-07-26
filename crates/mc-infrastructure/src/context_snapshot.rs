use bytes::Bytes;
use futures_util::{StreamExt, stream};
use mc_application::{
    ArtifactStore, ContextSnapshotError, ContextSnapshotStore, NewArtifact, NewContextSnapshot,
};
use mc_domain::{
    ArtifactId, ArtifactKind, ContextItemKind, ContextSnapshot, ContextSnapshotId, Sha256Digest,
};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use tracing::{info, instrument};

#[derive(Clone, Debug)]
pub struct PgContextSnapshotStore<A> {
    pool: PgPool,
    artifacts: A,
}

impl<A> PgContextSnapshotStore<A> {
    #[must_use]
    pub fn new(pool: PgPool, artifacts: A) -> Self {
        Self { pool, artifacts }
    }
}

impl<A> ContextSnapshotStore for PgContextSnapshotStore<A>
where
    A: ArtifactStore,
{
    #[instrument(skip(self, snapshot), fields(snapshot_id = %snapshot.id, run_id = %snapshot.run_id, policy_version = snapshot.policy_version.get(), item_count = snapshot.items.len()))]
    async fn persist(
        &self,
        snapshot: NewContextSnapshot,
    ) -> Result<ContextSnapshot, ContextSnapshotError> {
        for item in &snapshot.items {
            if item.inclusion_reason.trim().is_empty() {
                return Err(ContextSnapshotError::EmptyInclusionReason { item_id: item.id });
            }
        }

        let mut hasher = Sha256::new();
        let mut estimated_tokens = 0_u64;
        let mut stored_items = Vec::with_capacity(snapshot.items.len());
        for item in &snapshot.items {
            hasher.update(&item.rendered);
            estimated_tokens = estimated_tokens
                .checked_add(item.estimated_tokens)
                .ok_or_else(|| storage(std::io::Error::other("context token count overflow")))?;
            let artifact_id = ArtifactId::new();
            let artifact = self
                .artifacts
                .put(
                    NewArtifact {
                        id: artifact_id,
                        run_id: snapshot.run_id,
                        kind: ArtifactKind::ContextPayload,
                        media_type: "application/octet-stream".to_owned(),
                        created_at: snapshot.created_at,
                    },
                    Box::pin(stream::once({
                        let rendered = item.rendered.clone();
                        async move { Ok(rendered) }
                    })),
                )
                .await
                .map_err(storage)?;
            stored_items.push((item, artifact));
        }

        let rendered_artifact_id = ArtifactId::new();
        let rendered_artifact = self
            .artifacts
            .put(
                NewArtifact {
                    id: rendered_artifact_id,
                    run_id: snapshot.run_id,
                    kind: ArtifactKind::ContextPayload,
                    media_type: "application/octet-stream".to_owned(),
                    created_at: snapshot.created_at,
                },
                Box::pin(stream::iter(
                    snapshot
                        .items
                        .iter()
                        .map(|item| Ok(item.rendered.clone()))
                        .collect::<Vec<_>>(),
                )),
            )
            .await
            .map_err(storage)?;
        let rendered_digest =
            Sha256Digest::new(format!("{:x}", hasher.finalize())).map_err(storage)?;
        if rendered_artifact.digest != rendered_digest {
            return Err(storage(std::io::Error::other(
                "context snapshot artifact digest mismatch",
            )));
        }

        let mut transaction = self.pool.begin().await.map_err(storage)?;
        sqlx::query(
            "INSERT INTO context_snapshots (id, run_id, policy_version, rendered_digest, rendered_artifact_id, estimated_tokens, created_at) VALUES ($1, $2, $3, $4, $5, $6, to_timestamp($7::double precision / 1000))",
        )
        .bind(snapshot.id.as_uuid())
        .bind(snapshot.run_id.as_uuid())
        .bind(i64::from(snapshot.policy_version.get()))
        .bind(rendered_digest.as_str())
        .bind(rendered_artifact_id.as_uuid())
        .bind(estimated_tokens as i64)
        .bind(snapshot.created_at.unix_milliseconds())
        .execute(&mut *transaction)
        .await
        .map_err(storage)?;

        for (index, (item, artifact)) in stored_items.iter().enumerate() {
            sqlx::query(
                "INSERT INTO context_items (id, context_snapshot_id, run_id, sequence, kind, source, inclusion_reason, estimated_tokens, rendered_digest, rendered_artifact_id, compacted_from) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            )
            .bind(item.id.as_uuid())
            .bind(snapshot.id.as_uuid())
            .bind(snapshot.run_id.as_uuid())
            .bind((index + 1) as i64)
            .bind(context_item_kind(item.kind))
            .bind(serde_json::to_value(&item.source).map_err(storage)?)
            .bind(&item.inclusion_reason)
            .bind(item.estimated_tokens as i64)
            .bind(artifact.digest.as_str())
            .bind(artifact.id.as_uuid())
            .bind(
                item.compacted_from
                    .iter()
                    .map(|id| id.as_uuid())
                    .collect::<Vec<_>>(),
            )
            .execute(&mut *transaction)
            .await
            .map_err(storage)?;
        }
        transaction.commit().await.map_err(storage)?;

        info!(
            estimated_tokens,
            byte_length = rendered_artifact.byte_length,
            "context snapshot persisted"
        );
        Ok(ContextSnapshot {
            id: snapshot.id,
            run_id: snapshot.run_id,
            policy_version: snapshot.policy_version,
            rendered_digest,
            rendered_artifact_id,
            estimated_tokens,
            created_at: snapshot.created_at,
        })
    }

    #[instrument(skip(self), fields(%snapshot_id))]
    async fn reconstruct(
        &self,
        snapshot_id: ContextSnapshotId,
    ) -> Result<Bytes, ContextSnapshotError> {
        let artifact_id =
            sqlx::query("SELECT rendered_artifact_id FROM context_snapshots WHERE id = $1")
                .bind(snapshot_id.as_uuid())
                .fetch_optional(&self.pool)
                .await
                .map_err(storage)?
                .ok_or(ContextSnapshotError::NotFound { snapshot_id })?
                .try_get::<Option<sqlx::types::Uuid>, _>("rendered_artifact_id")
                .map_err(storage)?
                .ok_or(ContextSnapshotError::NotFound { snapshot_id })?;
        let mut body = self
            .artifacts
            .read(ArtifactId::from_uuid(artifact_id))
            .await
            .map_err(storage)?;
        let mut bytes = Vec::new();
        while let Some(chunk) = body.next().await {
            bytes.extend_from_slice(&chunk.map_err(storage)?);
        }
        info!(byte_length = bytes.len(), "context snapshot reconstructed");
        Ok(Bytes::from(bytes))
    }
}

const fn context_item_kind(kind: ContextItemKind) -> &'static str {
    match kind {
        ContextItemKind::Instruction => "instruction",
        ContextItemKind::TaskContract => "task_contract",
        ContextItemKind::Conversation => "conversation",
        ContextItemKind::RepositoryEvidence => "repository_evidence",
        ContextItemKind::ToolResult => "tool_result",
        ContextItemKind::MemoryClaim => "memory_claim",
        ContextItemKind::Summary => "summary",
    }
}

fn storage(error: impl std::error::Error + Send + Sync + 'static) -> ContextSnapshotError {
    ContextSnapshotError::Storage(Box::new(error))
}
