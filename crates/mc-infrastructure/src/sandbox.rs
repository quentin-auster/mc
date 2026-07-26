use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use futures_util::stream;
use mc_application::{
    ArtifactStore, NewArtifact, Sandbox, SandboxError, SandboxRequest, SandboxResult,
};
use mc_domain::{ArtifactId, ArtifactKind, Timestamp};
use tokio::process::Command;
use tracing::{info, instrument, warn};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SandboxLimits {
    pub cpus: u16,
    pub memory_megabytes: u64,
    pub processes: u32,
    pub timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct DockerSandbox<A> {
    artifacts: A,
    worktree_root: PathBuf,
    image: String,
    limits: SandboxLimits,
    allowed_environment: BTreeSet<String>,
}

impl<A> DockerSandbox<A> {
    #[must_use]
    pub fn new(
        artifacts: A,
        worktree_root: impl Into<PathBuf>,
        image: impl Into<String>,
        limits: SandboxLimits,
        allowed_environment: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            artifacts,
            worktree_root: worktree_root.into(),
            image: image.into(),
            limits,
            allowed_environment: allowed_environment.into_iter().collect(),
        }
    }
}

impl<A> Sandbox for DockerSandbox<A>
where
    A: ArtifactStore,
{
    #[instrument(skip(self, request), fields(run_id = %request.run_id, image = %self.image, argv0 = request.argv.first().map(String::as_str)))]
    async fn execute(&self, request: SandboxRequest) -> Result<SandboxResult, SandboxError> {
        if request.argv.is_empty() {
            return Err(SandboxError::EmptyCommand);
        }
        for name in request.environment.keys() {
            if !self.allowed_environment.contains(name) {
                return Err(SandboxError::EnvironmentDenied { name: name.clone() });
            }
        }
        let root = tokio::fs::canonicalize(&self.worktree_root)
            .await
            .map_err(storage)?;
        let worktree = tokio::fs::canonicalize(&request.worktree)
            .await
            .map_err(storage)?;
        if !worktree.starts_with(&root) || worktree == root {
            return Err(SandboxError::InvalidWorktree);
        }

        let mut command = Command::new("docker");
        command
            .kill_on_drop(true)
            .args(["run", "--rm", "--network", "none"])
            .args(["--cap-drop", "ALL"])
            .args(["--security-opt", "no-new-privileges"])
            .args(["--pids-limit", &self.limits.processes.to_string()])
            .args(["--cpus", &self.limits.cpus.to_string()])
            .args(["--memory", &format!("{}m", self.limits.memory_megabytes)])
            .args(["--workdir", "/workspace"])
            .arg("--volume")
            .arg(format!("{}:/workspace:rw", worktree.display()));
        for (name, value) in &request.environment {
            command.arg("--env").arg(format!("{name}={value}"));
        }
        command.arg(&self.image).args(&request.argv);

        let started = Instant::now();
        let outcome = tokio::time::timeout(self.limits.timeout, command.output()).await;
        let duration = started.elapsed();
        let (exit_code, timed_out, stdout, stderr) = match outcome {
            Ok(result) => {
                let output = result.map_err(storage)?;
                (
                    output.status.code(),
                    false,
                    Bytes::from(output.stdout),
                    Bytes::from(output.stderr),
                )
            }
            Err(_) => (
                None,
                true,
                Bytes::new(),
                Bytes::from_static(b"sandbox timed out"),
            ),
        };
        let created_at = now();
        let stdout_artifact_id = self
            .store_output(request.run_id, stdout, "text/plain", created_at)
            .await?;
        let stderr_artifact_id = self
            .store_output(request.run_id, stderr, "text/plain", created_at)
            .await?;
        if timed_out {
            warn!(
                duration_milliseconds = duration.as_millis(),
                "sandbox timed out"
            );
        } else {
            info!(
                exit_code,
                duration_milliseconds = duration.as_millis(),
                "sandbox completed"
            );
        }
        Ok(SandboxResult {
            exit_code,
            timed_out,
            duration,
            stdout_artifact_id,
            stderr_artifact_id,
        })
    }
}

impl<A> DockerSandbox<A>
where
    A: ArtifactStore,
{
    async fn store_output(
        &self,
        run_id: mc_domain::RunId,
        bytes: Bytes,
        media_type: &str,
        created_at: Timestamp,
    ) -> Result<ArtifactId, SandboxError> {
        let id = ArtifactId::new();
        self.artifacts
            .put(
                NewArtifact {
                    id,
                    run_id,
                    kind: ArtifactKind::Log,
                    media_type: media_type.to_owned(),
                    created_at,
                },
                Box::pin(stream::once(async move { Ok(bytes) })),
            )
            .await
            .map_err(storage)?;
        Ok(id)
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

fn storage(error: impl std::error::Error + Send + Sync + 'static) -> SandboxError {
    SandboxError::Storage(Box::new(error))
}
