use std::{
    ops::Range,
    path::{Component, Path, PathBuf},
};

use bytes::Bytes;
use mc_application::{
    GitBlameLine, GitCommitSummary, RepositoryReadError, RepositoryReader, TextMatch,
};
use tokio::process::Command;
use tracing::{info, instrument};
use walkdir::WalkDir;

#[derive(Clone, Debug)]
pub struct LocalRepositoryReader {
    worktree_root: PathBuf,
}

impl LocalRepositoryReader {
    #[must_use]
    pub fn new(worktree_root: impl Into<PathBuf>) -> Self {
        Self {
            worktree_root: worktree_root.into(),
        }
    }

    async fn worktree(&self, requested: &Path) -> Result<PathBuf, RepositoryReadError> {
        let root = tokio::fs::canonicalize(&self.worktree_root)
            .await
            .map_err(storage)?;
        let worktree = tokio::fs::canonicalize(requested).await.map_err(storage)?;
        if worktree.starts_with(&root) && worktree != root {
            Ok(worktree)
        } else {
            Err(RepositoryReadError::InvalidPath)
        }
    }

    async fn path(&self, worktree: &Path, relative: &Path) -> Result<PathBuf, RepositoryReadError> {
        validate_relative(relative)?;
        let resolved = tokio::fs::canonicalize(worktree.join(relative))
            .await
            .map_err(storage)?;
        if resolved.starts_with(worktree) {
            Ok(resolved)
        } else {
            Err(RepositoryReadError::InvalidPath)
        }
    }
}

impl RepositoryReader for LocalRepositoryReader {
    #[instrument(skip(self), fields(worktree = %worktree.display(), path = %path.display()))]
    async fn list_files(
        &self,
        worktree: PathBuf,
        path: PathBuf,
    ) -> Result<Vec<PathBuf>, RepositoryReadError> {
        let worktree = self.worktree(&worktree).await?;
        let start = self.path(&worktree, &path).await?;
        let mut files = Vec::new();
        for entry in WalkDir::new(start).follow_links(false) {
            let entry = entry.map_err(storage)?;
            if entry.file_type().is_file() {
                files.push(
                    entry
                        .path()
                        .strip_prefix(&worktree)
                        .map(Path::to_path_buf)
                        .map_err(storage)?,
                );
            }
        }
        files.sort();
        info!(file_count = files.len(), "repository files listed");
        Ok(files)
    }

    #[instrument(skip(self), fields(worktree = %worktree.display(), path = %path.display(), start = range.start, end = range.end))]
    async fn read_lines(
        &self,
        worktree: PathBuf,
        path: PathBuf,
        range: Range<u64>,
    ) -> Result<String, RepositoryReadError> {
        validate_range(&range)?;
        let worktree = self.worktree(&worktree).await?;
        let path = self.path(&worktree, &path).await?;
        let contents = tokio::fs::read_to_string(path).await.map_err(storage)?;
        Ok(contents
            .lines()
            .enumerate()
            .filter(|(index, _)| {
                let line = *index as u64 + 1;
                line >= range.start && line < range.end
            })
            .map(|(_, line)| line)
            .collect::<Vec<_>>()
            .join("\n"))
    }

    #[instrument(skip(self, needle), fields(worktree = %worktree.display(), path = %path.display()))]
    async fn search_exact(
        &self,
        worktree: PathBuf,
        path: PathBuf,
        needle: String,
    ) -> Result<Vec<TextMatch>, RepositoryReadError> {
        let worktree = self.worktree(&worktree).await?;
        let start = self.path(&worktree, &path).await?;
        let mut matches = Vec::new();
        for entry in WalkDir::new(start).follow_links(false) {
            let entry = entry.map_err(storage)?;
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(contents) = tokio::fs::read_to_string(entry.path()).await else {
                continue;
            };
            for (index, line) in contents.lines().enumerate() {
                if line.contains(&needle) {
                    matches.push(TextMatch {
                        path: entry
                            .path()
                            .strip_prefix(&worktree)
                            .map(Path::to_path_buf)
                            .map_err(storage)?,
                        line: index as u64 + 1,
                        text: line.to_owned(),
                    });
                }
            }
        }
        info!(
            match_count = matches.len(),
            "repository exact text searched"
        );
        Ok(matches)
    }

    async fn diff(&self, worktree: PathBuf) -> Result<Bytes, RepositoryReadError> {
        let worktree = self.worktree(&worktree).await?;
        git_bytes(&worktree, &["diff", "--binary", "HEAD"])
            .await
            .map(Bytes::from)
    }

    async fn history(
        &self,
        worktree: PathBuf,
        path: Option<PathBuf>,
        limit: u32,
    ) -> Result<Vec<GitCommitSummary>, RepositoryReadError> {
        let worktree = self.worktree(&worktree).await?;
        let mut arguments = vec![
            "log".to_owned(),
            format!("-n{}", limit.max(1)),
            "--format=%H%x1f%an%x1f%at%x1f%s".to_owned(),
        ];
        if let Some(path) = path {
            validate_relative(&path)?;
            arguments.push("--".to_owned());
            arguments.push(path.to_string_lossy().into_owned());
        }
        let output = git_owned(&worktree, &arguments).await?;
        output
            .lines()
            .map(|line| {
                let mut fields = line.splitn(4, '\u{1f}');
                Ok(GitCommitSummary {
                    commit: required(&mut fields)?,
                    author: required(&mut fields)?,
                    timestamp: required(&mut fields)?.parse().map_err(storage)?,
                    subject: required(&mut fields)?,
                })
            })
            .collect()
    }

    async fn blame(
        &self,
        worktree: PathBuf,
        path: PathBuf,
        range: Range<u64>,
    ) -> Result<Vec<GitBlameLine>, RepositoryReadError> {
        validate_range(&range)?;
        validate_relative(&path)?;
        let worktree = self.worktree(&worktree).await?;
        self.path(&worktree, &path).await?;
        let range_argument = format!("-L{},{}", range.start, range.end - 1);
        let output = git_owned(
            &worktree,
            &[
                "blame".to_owned(),
                "--line-porcelain".to_owned(),
                range_argument,
                "--".to_owned(),
                path.to_string_lossy().into_owned(),
            ],
        )
        .await?;
        parse_blame(&output)
    }
}

fn validate_relative(path: &Path) -> Result<(), RepositoryReadError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        Err(RepositoryReadError::InvalidPath)
    } else {
        Ok(())
    }
}

fn validate_range(range: &Range<u64>) -> Result<(), RepositoryReadError> {
    if range.start == 0 || range.start >= range.end {
        Err(RepositoryReadError::InvalidRange {
            start: range.start,
            end: range.end,
        })
    } else {
        Ok(())
    }
}

fn parse_blame(output: &str) -> Result<Vec<GitBlameLine>, RepositoryReadError> {
    let mut result = Vec::new();
    let mut commit = String::new();
    let mut line = 0_u64;
    let mut author = String::new();
    for value in output.lines() {
        if let Some(text) = value.strip_prefix('\t') {
            result.push(GitBlameLine {
                line,
                commit: commit.clone(),
                author: author.clone(),
                text: text.to_owned(),
            });
        } else if let Some(value) = value.strip_prefix("author ") {
            author = value.to_owned();
        } else {
            let fields: Vec<_> = value.split_whitespace().collect();
            let object_id = fields
                .first()
                .map(|value| value.trim_start_matches('^'))
                .unwrap_or_default();
            if fields.len() >= 3
                && object_id.len() >= 7
                && object_id.bytes().all(|byte| byte.is_ascii_hexdigit())
                && fields[1].bytes().all(|byte| byte.is_ascii_digit())
                && fields[2].bytes().all(|byte| byte.is_ascii_digit())
            {
                commit = object_id.to_owned();
                line = fields[2].parse().map_err(storage)?;
            }
        }
    }
    Ok(result)
}

async fn git_bytes(repository: &Path, args: &[&str]) -> Result<Vec<u8>, RepositoryReadError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .await
        .map_err(storage)?;
    output_result(output)
}

async fn git_owned(repository: &Path, args: &[String]) -> Result<String, RepositoryReadError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .await
        .map_err(storage)?;
    String::from_utf8(output_result(output)?).map_err(storage)
}

fn output_result(output: std::process::Output) -> Result<Vec<u8>, RepositoryReadError> {
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(storage(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        )))
    }
}

fn required<'a>(fields: &mut impl Iterator<Item = &'a str>) -> Result<String, RepositoryReadError> {
    fields
        .next()
        .map(str::to_owned)
        .ok_or_else(|| storage(std::io::Error::other("malformed Git output")))
}

fn storage(error: impl std::error::Error + Send + Sync + 'static) -> RepositoryReadError {
    RepositoryReadError::Storage(Box::new(error))
}
