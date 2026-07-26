use std::{path::Path, process::Command};

use mc_application::{RepositoryReadError, RepositoryReader};
use mc_infrastructure::LocalRepositoryReader;

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

#[tokio::test]
async fn reads_repository_state_and_rejects_path_escape() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("worktrees");
    let worktree = root.join("run");
    std::fs::create_dir_all(worktree.join("src")).unwrap();
    git(&worktree, &["init", "-b", "main"]);
    git(&worktree, &["config", "user.name", "MC Test"]);
    git(&worktree, &["config", "user.email", "mc@example.invalid"]);
    std::fs::write(worktree.join("src/lib.rs"), "alpha\nneedle\nomega\n").unwrap();
    std::fs::write(worktree.join("README.md"), "first\n").unwrap();
    git(&worktree, &["add", "."]);
    git(&worktree, &["commit", "-m", "initial"]);
    std::fs::write(worktree.join("README.md"), "second\n").unwrap();
    git(&worktree, &["add", "README.md"]);
    git(&worktree, &["commit", "-m", "second"]);
    std::fs::write(
        worktree.join("src/lib.rs"),
        "alpha\nneedle changed\nomega\n",
    )
    .unwrap();

    let reader = LocalRepositoryReader::new(&root);
    let files = reader
        .list_files(worktree.clone(), "src".into())
        .await
        .unwrap();
    assert_eq!(files, vec![std::path::PathBuf::from("src/lib.rs")]);
    assert_eq!(
        reader
            .read_lines(worktree.clone(), "src/lib.rs".into(), 2..4)
            .await
            .unwrap(),
        "needle changed\nomega"
    );
    let matches = reader
        .search_exact(worktree.clone(), "src".into(), "needle".to_owned())
        .await
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].line, 2);
    assert!(
        String::from_utf8_lossy(&reader.diff(worktree.clone()).await.unwrap())
            .contains("+needle changed")
    );
    let history = reader
        .history(worktree.clone(), Some("README.md".into()), 10)
        .await
        .unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].subject, "second");
    let blame = reader
        .blame(worktree.clone(), "README.md".into(), 1..2)
        .await
        .unwrap();
    assert_eq!(blame.len(), 1);
    assert_eq!(blame[0].text, "second");

    let escaped = reader
        .read_lines(worktree.clone(), "../outside".into(), 1..2)
        .await;
    assert!(matches!(escaped, Err(RepositoryReadError::InvalidPath)));

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(temporary.path(), worktree.join("escape")).unwrap();
        let symlink_escape = reader.list_files(worktree, "escape".into()).await;
        assert!(matches!(
            symlink_escape,
            Err(RepositoryReadError::InvalidPath)
        ));
    }
}
