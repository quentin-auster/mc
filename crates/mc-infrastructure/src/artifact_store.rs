use std::{ops::Range, sync::Arc};

use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use mc_application::{ArtifactBody, ArtifactStore, ArtifactStoreError, NewArtifact};
use mc_domain::{Artifact, ArtifactId, ArtifactKind, Sha256Digest};
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, aws::AmazonS3Builder, path::Path};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tracing::{info, instrument};

const UPLOAD_PART_SIZE: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub struct PgObjectArtifactStore {
    pool: PgPool,
    objects: Arc<dyn ObjectStore>,
}

impl std::fmt::Debug for PgObjectArtifactStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PgObjectArtifactStore")
            .finish_non_exhaustive()
    }
}

impl PgObjectArtifactStore {
    #[must_use]
    pub fn new(pool: PgPool, objects: Arc<dyn ObjectStore>) -> Self {
        Self { pool, objects }
    }

    async fn location(&self, artifact_id: ArtifactId) -> Result<(Path, u64), ArtifactStoreError> {
        let row = sqlx::query("SELECT object_key, byte_length FROM artifacts WHERE id = $1")
            .bind(artifact_id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(storage)?
            .ok_or(ArtifactStoreError::NotFound { artifact_id })?;
        let length: i64 = row.try_get("byte_length").map_err(storage)?;
        Ok((
            Path::from(row.try_get::<String, _>("object_key").map_err(storage)?),
            length as u64,
        ))
    }
}

impl ArtifactStore for PgObjectArtifactStore {
    #[instrument(skip(self, body), fields(artifact_id = %artifact.id, run_id = %artifact.run_id))]
    async fn put(
        &self,
        artifact: NewArtifact,
        mut body: ArtifactBody,
    ) -> Result<Artifact, ArtifactStoreError> {
        let temporary = tempfile::tempfile().map_err(storage)?;
        let mut file = tokio::fs::File::from_std(temporary);
        let mut hasher = Sha256::new();
        let mut byte_length = 0_u64;
        while let Some(chunk) = body.next().await {
            let chunk = chunk?;
            hasher.update(&chunk);
            file.write_all(&chunk).await.map_err(storage)?;
            byte_length += chunk.len() as u64;
        }
        file.flush().await.map_err(storage)?;
        file.seek(std::io::SeekFrom::Start(0))
            .await
            .map_err(storage)?;

        let digest_text = format!("{:x}", hasher.finalize());
        let digest = Sha256Digest::new(&digest_text).map_err(storage)?;
        let object_key = format!("sha256/{}/{}", &digest_text[..2], digest_text);
        let location = Path::from(object_key.clone());
        let deduplicated = match self.objects.head(&location).await {
            Ok(_) => true,
            Err(object_store::Error::NotFound { .. }) => {
                upload_file(self.objects.as_ref(), &location, &mut file).await?;
                false
            }
            Err(error) => return Err(storage(error)),
        };

        sqlx::query(
            "INSERT INTO artifacts (id, run_id, kind, digest, media_type, byte_length, object_key, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, to_timestamp($8::double precision / 1000))",
        )
        .bind(artifact.id.as_uuid())
        .bind(artifact.run_id.as_uuid())
        .bind(artifact_kind(artifact.kind))
        .bind(digest.as_str())
        .bind(&artifact.media_type)
        .bind(byte_length as i64)
        .bind(&object_key)
        .bind(artifact.created_at.unix_milliseconds())
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        info!(byte_length, %object_key, deduplicated, "artifact stored");
        Ok(Artifact {
            id: artifact.id,
            run_id: artifact.run_id,
            kind: artifact.kind,
            digest,
            media_type: artifact.media_type,
            byte_length,
            object_key,
            created_at: artifact.created_at,
        })
    }

    #[instrument(skip(self), fields(%artifact_id))]
    async fn read(&self, artifact_id: ArtifactId) -> Result<ArtifactBody, ArtifactStoreError> {
        let (location, _) = self.location(artifact_id).await?;
        let stream = self
            .objects
            .get(&location)
            .await
            .map_err(storage)?
            .into_stream()
            .map_err(storage);
        info!(%location, "artifact stream opened");
        Ok(Box::pin(stream))
    }

    #[instrument(skip(self), fields(%artifact_id, start = range.start, end = range.end))]
    async fn read_range(
        &self,
        artifact_id: ArtifactId,
        range: Range<u64>,
    ) -> Result<Bytes, ArtifactStoreError> {
        let (location, length) = self.location(artifact_id).await?;
        if range.start >= range.end || range.end > length {
            return Err(ArtifactStoreError::InvalidRange {
                start: range.start,
                end: range.end,
                length,
            });
        }
        let bytes = self
            .objects
            .get_range(&location, range)
            .await
            .map_err(storage)?;
        info!(byte_length = bytes.len(), %location, "artifact range read");
        Ok(bytes)
    }
}

pub fn build_s3_store(
    endpoint: &str,
    bucket: &str,
    access_key: &str,
    secret_key: &str,
) -> Result<Arc<dyn ObjectStore>, object_store::Error> {
    AmazonS3Builder::new()
        .with_bucket_name(bucket)
        .with_region("us-east-1")
        .with_endpoint(endpoint)
        .with_access_key_id(access_key)
        .with_secret_access_key(secret_key)
        .with_allow_http(endpoint.starts_with("http://"))
        .with_virtual_hosted_style_request(false)
        .build()
        .map(|store| Arc::new(store) as Arc<dyn ObjectStore>)
}

async fn upload_file(
    objects: &dyn ObjectStore,
    location: &Path,
    file: &mut tokio::fs::File,
) -> Result<(), ArtifactStoreError> {
    let mut buffer = vec![0_u8; UPLOAD_PART_SIZE];
    let first_read = file.read(&mut buffer).await.map_err(storage)?;
    if first_read == 0 {
        objects
            .put(location, PutPayload::from_bytes(Bytes::new()))
            .await
            .map_err(storage)?;
        return Ok(());
    }

    let mut upload = objects.put_multipart(location).await.map_err(storage)?;
    upload
        .put_part(PutPayload::from_bytes(Bytes::copy_from_slice(
            &buffer[..first_read],
        )))
        .await
        .map_err(storage)?;
    loop {
        let read = file.read(&mut buffer).await.map_err(storage)?;
        if read == 0 {
            break;
        }
        upload
            .put_part(PutPayload::from_bytes(Bytes::copy_from_slice(
                &buffer[..read],
            )))
            .await
            .map_err(storage)?;
    }
    upload.complete().await.map_err(storage)?;
    Ok(())
}

const fn artifact_kind(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::ModelPayload => "model_payload",
        ArtifactKind::ToolArguments => "tool_arguments",
        ArtifactKind::ToolResult => "tool_result",
        ArtifactKind::Patch => "patch",
        ArtifactKind::Log => "log",
        ArtifactKind::RepositoryBundle => "repository_bundle",
        ArtifactKind::Other => "other",
    }
}

fn storage(error: impl std::error::Error + Send + Sync + 'static) -> ArtifactStoreError {
    ArtifactStoreError::Storage(Box::new(error))
}
