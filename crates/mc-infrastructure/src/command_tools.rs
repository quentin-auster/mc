use std::{collections::BTreeMap, path::PathBuf};

use mc_application::{
    CommandToolError, CommandToolKind, CommandToolRequest, CommandToolResult, CommandTools,
    Sandbox, SandboxRequest,
};
use mc_domain::RunId;
use tracing::{info, instrument};

#[derive(Clone, Debug)]
pub struct SandboxCommandTools<S> {
    sandbox: S,
    allowlists: BTreeMap<CommandToolKind, Vec<Vec<String>>>,
}

impl<S> SandboxCommandTools<S> {
    #[must_use]
    pub fn new(sandbox: S, allowlists: BTreeMap<CommandToolKind, Vec<Vec<String>>>) -> Self {
        Self {
            sandbox,
            allowlists,
        }
    }
}

impl<S> CommandTools for SandboxCommandTools<S>
where
    S: Sandbox,
{
    #[instrument(skip(self, request), fields(%run_id, ?request.kind, argv0 = request.argv.first().map(String::as_str)))]
    async fn execute(
        &self,
        run_id: RunId,
        worktree: PathBuf,
        request: CommandToolRequest,
    ) -> Result<CommandToolResult, CommandToolError> {
        let allowed = self.allowlists.get(&request.kind).is_some_and(|prefixes| {
            prefixes.iter().any(|prefix| {
                !prefix.is_empty()
                    && request.argv.len() >= prefix.len()
                    && request.argv[..prefix.len()] == prefix[..]
            })
        });
        if !allowed {
            return Err(CommandToolError::Denied { kind: request.kind });
        }
        let kind = request.kind;
        let command = request.argv.clone();
        let sandbox = self
            .sandbox
            .execute(SandboxRequest {
                run_id,
                worktree,
                argv: request.argv,
                environment: request.environment,
            })
            .await
            .map_err(storage)?;
        info!(
            exit_code = sandbox.exit_code,
            timed_out = sandbox.timed_out,
            "command tool completed"
        );
        Ok(CommandToolResult {
            kind,
            command,
            sandbox,
        })
    }
}

fn storage(error: impl std::error::Error + Send + Sync + 'static) -> CommandToolError {
    CommandToolError::Storage(Box::new(error))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use mc_application::{
        CommandToolError, CommandToolKind, CommandToolRequest, CommandTools, Sandbox, SandboxError,
        SandboxRequest, SandboxResult,
    };
    use mc_domain::{ArtifactId, RunId};

    use super::SandboxCommandTools;

    #[derive(Clone)]
    struct StubSandbox {
        calls: Arc<AtomicUsize>,
    }

    impl Sandbox for StubSandbox {
        async fn execute(&self, _request: SandboxRequest) -> Result<SandboxResult, SandboxError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(SandboxResult {
                exit_code: Some(0),
                timed_out: false,
                duration: Duration::from_millis(5),
                stdout_artifact_id: ArtifactId::new(),
                stderr_artifact_id: ArtifactId::new(),
            })
        }
    }

    #[tokio::test]
    async fn dispatches_only_allowlisted_category_prefixes() {
        let calls = Arc::new(AtomicUsize::new(0));
        let tools = SandboxCommandTools::new(
            StubSandbox {
                calls: calls.clone(),
            },
            BTreeMap::from([
                (
                    CommandToolKind::Test,
                    vec![vec!["cargo".to_owned(), "test".to_owned()]],
                ),
                (
                    CommandToolKind::Formatter,
                    vec![vec!["cargo".to_owned(), "fmt".to_owned()]],
                ),
            ]),
        );
        let result = tools
            .execute(
                RunId::new(),
                PathBuf::from("/managed/run"),
                CommandToolRequest {
                    kind: CommandToolKind::Test,
                    argv: vec![
                        "cargo".to_owned(),
                        "test".to_owned(),
                        "--workspace".to_owned(),
                    ],
                    environment: BTreeMap::new(),
                },
            )
            .await
            .unwrap();
        assert_eq!(result.kind, CommandToolKind::Test);
        assert_eq!(result.sandbox.exit_code, Some(0));

        let denied = tools
            .execute(
                RunId::new(),
                PathBuf::from("/managed/run"),
                CommandToolRequest {
                    kind: CommandToolKind::Build,
                    argv: vec!["cargo".to_owned(), "build".to_owned()],
                    environment: BTreeMap::new(),
                },
            )
            .await;
        assert!(matches!(denied, Err(CommandToolError::Denied { .. })));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
