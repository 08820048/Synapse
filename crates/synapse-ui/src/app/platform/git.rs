use std::{
    fmt, fs,
    path::{Component, Path, PathBuf},
    process::{Command, Output},
};

const RECENT_COMMIT_LIMIT: usize = 20;
const DIFF_PREVIEW_LIMIT: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitFileChange {
    pub path: PathBuf,
    pub status: GitFileStatus,
    pub staged: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitCommit {
    pub id: String,
    pub author: String,
    pub date: String,
    pub summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitRepositoryStatus {
    pub branch: String,
    pub upstream: Option<String>,
    pub changed_files: usize,
    pub ahead: usize,
    pub behind: usize,
    pub conflicted: bool,
    pub detached: bool,
    pub changes: Vec<GitFileChange>,
    pub recent_commits: Vec<GitCommit>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitSyncResult {
    pub committed: bool,
    pub status: GitRepositoryStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum GitError {
    GitUnavailable,
    NotRepository,
    VaultIsNotRepositoryRoot(PathBuf),
    DetachedHead,
    NoUpstream,
    ConflictsPresent,
    DirtyWorkingTree,
    EmptyCommitMessage,
    NoChanges,
    InvalidPath,
    CommandFailed(&'static str),
    RebaseFailed { aborted: bool },
}

impl fmt::Display for GitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GitUnavailable => write!(formatter, "Git is not installed or is unavailable"),
            Self::NotRepository => write!(formatter, "The current Vault is not a Git repository"),
            Self::VaultIsNotRepositoryRoot(root) => write!(
                formatter,
                "The Vault must be the repository root (repository: {})",
                root.display()
            ),
            Self::DetachedHead => write!(formatter, "The repository is in detached HEAD state"),
            Self::NoUpstream => write!(formatter, "The current branch has no upstream branch"),
            Self::ConflictsPresent => {
                write!(
                    formatter,
                    "Resolve the existing Git conflicts before continuing"
                )
            }
            Self::DirtyWorkingTree => {
                write!(
                    formatter,
                    "Commit local changes before pulling remote changes"
                )
            }
            Self::EmptyCommitMessage => write!(formatter, "The commit message cannot be empty"),
            Self::NoChanges => write!(formatter, "There are no local changes to commit"),
            Self::InvalidPath => write!(formatter, "The selected path is outside the Vault"),
            Self::CommandFailed(operation) => write!(formatter, "Git failed to {operation}"),
            Self::RebaseFailed { aborted: true } => write!(
                formatter,
                "Remote changes conflict with local notes; the rebase was aborted"
            ),
            Self::RebaseFailed { aborted: false } => write!(
                formatter,
                "Remote changes conflict with local notes and Git could not abort the rebase"
            ),
        }
    }
}

pub(crate) fn inspect_repository(root: &Path) -> Result<GitRepositoryStatus, GitError> {
    validate_repository_root(root)?;
    let output = checked_git_output(
        root,
        &["status", "--porcelain=v2", "--branch", "-z"],
        "read repository status",
    )?;
    let mut status = parse_status(&output.stdout)?;
    status.recent_commits = read_recent_commits(root)?;
    Ok(status)
}

pub(crate) fn commit_repository(
    root: &Path,
    message: &str,
) -> Result<GitRepositoryStatus, GitError> {
    let initial = inspect_repository(root)?;
    ensure_writable(&initial)?;
    stage_and_commit(root, &initial, message)?;
    inspect_repository(root)
}

pub(crate) fn pull_repository(root: &Path) -> Result<GitRepositoryStatus, GitError> {
    let initial = inspect_repository(root)?;
    ensure_remote_ready(&initial)?;
    if initial.changed_files > 0 {
        return Err(GitError::DirtyWorkingTree);
    }
    fetch_and_rebase(root)?;
    inspect_repository(root)
}

pub(crate) fn push_repository(root: &Path) -> Result<GitRepositoryStatus, GitError> {
    let initial = inspect_repository(root)?;
    ensure_remote_ready(&initial)?;
    checked_git_output(root, &["push"], "push local changes")?;
    inspect_repository(root)
}

pub(crate) fn sync_repository(
    root: &Path,
    message: Option<&str>,
) -> Result<GitSyncResult, GitError> {
    let initial = inspect_repository(root)?;
    ensure_remote_ready(&initial)?;
    let committed = if initial.changed_files > 0 {
        stage_and_commit(root, &initial, message.unwrap_or_default())?;
        true
    } else {
        false
    };
    fetch_and_rebase(root)?;
    checked_git_output(root, &["push"], "push local changes")?;
    Ok(GitSyncResult {
        committed,
        status: inspect_repository(root)?,
    })
}

pub(crate) fn read_file_diff(root: &Path, path: &Path) -> Result<String, GitError> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return Err(GitError::InvalidPath);
    }
    validate_repository_root(root)?;
    let output = git_output_with_path(
        root,
        &["diff", "--no-color", "--no-ext-diff", "HEAD", "--"],
        path,
    )?;
    if output.status.success() && !output.stdout.is_empty() {
        return Ok(diff_preview(&output.stdout));
    }
    if !output.status.success()
        && git_output(root, &["rev-parse", "--verify", "HEAD"])?
            .status
            .success()
    {
        return Err(GitError::CommandFailed("read the file diff"));
    }

    let file = root.join(path);
    if !file.is_file() {
        return Ok(String::new());
    }
    let bytes = fs::read(file).map_err(|_| GitError::CommandFailed("read the changed file"))?;
    if bytes.contains(&0) {
        return Ok("Binary file".to_owned());
    }
    let content = String::from_utf8_lossy(&bytes);
    let preview = format!(
        "--- /dev/null\n+++ b/{}\n{}",
        path.display(),
        content
            .lines()
            .map(|line| format!("+{line}\n"))
            .collect::<String>()
    );
    Ok(truncate_preview(preview))
}

fn validate_repository_root(root: &Path) -> Result<(), GitError> {
    let vault_root =
        fs::canonicalize(root).map_err(|_| GitError::CommandFailed("read the Vault directory"))?;
    let repository = git_output(root, &["rev-parse", "--show-toplevel"])?;
    if !repository.status.success() {
        return Err(GitError::NotRepository);
    }
    let repository_root = String::from_utf8_lossy(&repository.stdout)
        .trim()
        .to_owned();
    let repository_root = fs::canonicalize(repository_root)
        .map_err(|_| GitError::CommandFailed("read the repository root"))?;
    if repository_root != vault_root {
        return Err(GitError::VaultIsNotRepositoryRoot(repository_root));
    }
    Ok(())
}

fn ensure_writable(status: &GitRepositoryStatus) -> Result<(), GitError> {
    if status.detached {
        return Err(GitError::DetachedHead);
    }
    if status.conflicted {
        return Err(GitError::ConflictsPresent);
    }
    Ok(())
}

fn ensure_remote_ready(status: &GitRepositoryStatus) -> Result<(), GitError> {
    ensure_writable(status)?;
    if status.upstream.is_none() {
        return Err(GitError::NoUpstream);
    }
    Ok(())
}

fn stage_and_commit(
    root: &Path,
    status: &GitRepositoryStatus,
    message: &str,
) -> Result<(), GitError> {
    ensure_writable(status)?;
    if status.changed_files == 0 {
        return Err(GitError::NoChanges);
    }
    let message = message.trim();
    if message.is_empty() {
        return Err(GitError::EmptyCommitMessage);
    }
    checked_git_output(root, &["add", "--all", "--", "."], "stage local changes")?;
    let staged = git_output(root, &["diff", "--cached", "--quiet"])?;
    match staged.status.code() {
        Some(1) => {}
        Some(0) => return Err(GitError::NoChanges),
        _ => return Err(GitError::CommandFailed("inspect staged changes")),
    }
    checked_git_output(root, &["commit", "-m", message], "commit local changes")?;
    Ok(())
}

fn fetch_and_rebase(root: &Path) -> Result<(), GitError> {
    checked_git_output(root, &["fetch", "--prune"], "fetch remote changes")?;
    let rebase = git_output(root, &["rebase", "@{upstream}"])?;
    if rebase.status.success() {
        return Ok(());
    }
    let aborted =
        git_output(root, &["rebase", "--abort"]).is_ok_and(|output| output.status.success());
    Err(GitError::RebaseFailed { aborted })
}

fn parse_status(output: &[u8]) -> Result<GitRepositoryStatus, GitError> {
    let mut branch = None;
    let mut upstream = None;
    let mut ahead = 0;
    let mut behind = 0;
    let mut changes = Vec::new();
    let mut records = output.split(|byte| *byte == 0).peekable();

    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        let line = String::from_utf8_lossy(record);
        if let Some(value) = line.strip_prefix("# branch.head ") {
            branch = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("# branch.upstream ") {
            upstream = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("# branch.ab ") {
            let mut values = value.split_whitespace();
            ahead = values
                .next()
                .and_then(|value| value.strip_prefix('+'))
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            behind = values
                .next()
                .and_then(|value| value.strip_prefix('-'))
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
        } else if let Some(change) = parse_change(&line) {
            if line.starts_with("2 ") {
                let _ = records.next();
            }
            changes.push(change);
        }
    }

    let branch = branch.ok_or(GitError::CommandFailed("read the current branch"))?;
    Ok(GitRepositoryStatus {
        detached: branch == "(detached)",
        branch,
        upstream,
        changed_files: changes.len(),
        ahead,
        behind,
        conflicted: changes
            .iter()
            .any(|change| change.status == GitFileStatus::Conflicted),
        changes,
        recent_commits: Vec::new(),
    })
}

fn parse_change(line: &str) -> Option<GitFileChange> {
    if let Some(path) = line.strip_prefix("? ") {
        return Some(GitFileChange {
            path: PathBuf::from(path),
            status: GitFileStatus::Untracked,
            staged: false,
        });
    }

    let (kind, field_count) = if line.starts_with("1 ") {
        ('1', 9)
    } else if line.starts_with("2 ") {
        ('2', 10)
    } else if line.starts_with("u ") {
        ('u', 11)
    } else {
        return None;
    };
    let fields = line.splitn(field_count, ' ').collect::<Vec<_>>();
    let xy = fields.get(1)?;
    let path = fields.last()?;
    let mut states = xy.chars();
    let index = states.next().unwrap_or('.');
    let worktree = states.next().unwrap_or('.');
    let status = if kind == 'u' {
        GitFileStatus::Conflicted
    } else if kind == '2' || index == 'R' || worktree == 'R' {
        GitFileStatus::Renamed
    } else if index == 'D' || worktree == 'D' {
        GitFileStatus::Deleted
    } else if index == 'A' {
        GitFileStatus::Added
    } else {
        GitFileStatus::Modified
    };
    Some(GitFileChange {
        path: PathBuf::from(path),
        status,
        staged: !matches!(index, '.' | '?'),
    })
}

fn read_recent_commits(root: &Path) -> Result<Vec<GitCommit>, GitError> {
    let head = git_output(root, &["rev-parse", "--verify", "HEAD"])?;
    if !head.status.success() {
        return Ok(Vec::new());
    }
    let limit = RECENT_COMMIT_LIMIT.to_string();
    let output = checked_git_output(
        root,
        &[
            "log",
            "-n",
            &limit,
            "--date=short",
            "--format=%h%x1f%an%x1f%ad%x1f%s",
        ],
        "read commit history",
    )?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(4, '');
            Some(GitCommit {
                id: fields.next()?.to_owned(),
                author: fields.next()?.to_owned(),
                date: fields.next()?.to_owned(),
                summary: fields.next()?.to_owned(),
            })
        })
        .collect())
}

fn diff_preview(bytes: &[u8]) -> String {
    truncate_preview(String::from_utf8_lossy(bytes).into_owned())
}

fn truncate_preview(mut preview: String) -> String {
    if preview.len() <= DIFF_PREVIEW_LIMIT {
        return preview;
    }
    let mut boundary = DIFF_PREVIEW_LIMIT;
    while !preview.is_char_boundary(boundary) {
        boundary -= 1;
    }
    preview.truncate(boundary);
    preview.push_str("\n… diff truncated …\n");
    preview
}

fn checked_git_output(
    root: &Path,
    arguments: &[&str],
    operation: &'static str,
) -> Result<Output, GitError> {
    let output = git_output(root, arguments)?;
    output
        .status
        .success()
        .then_some(output)
        .ok_or(GitError::CommandFailed(operation))
}

fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("core.quotepath=false")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C");
    command
}

fn git_output(root: &Path, arguments: &[&str]) -> Result<Output, GitError> {
    git_command(root)
        .args(arguments)
        .output()
        .map_err(git_io_error)
}

fn git_output_with_path(root: &Path, arguments: &[&str], path: &Path) -> Result<Output, GitError> {
    git_command(root)
        .args(arguments)
        .arg(path)
        .output()
        .map_err(git_io_error)
}

fn git_io_error(error: std::io::Error) -> GitError {
    if error.kind() == std::io::ErrorKind::NotFound {
        GitError::GitUnavailable
    } else {
        GitError::CommandFailed("start Git")
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, path::PathBuf, process::Command};

    use super::{
        GitFileChange, GitFileStatus, GitRepositoryStatus, commit_repository, inspect_repository,
        parse_status, pull_repository, push_repository, read_file_diff, sync_repository,
    };

    #[test]
    fn porcelain_status_preserves_branch_and_file_details() {
        let status = parse_status(
            b"# branch.oid abc\x00# branch.head main\x00# branch.upstream origin/main\x00# branch.ab +2 -3\x001 .M N... a b c d note.md\x00? image one.png\x00u UU N... a b c d e f g conflict.md\x00",
        )
        .unwrap();

        assert_eq!(
            status,
            GitRepositoryStatus {
                branch: "main".to_owned(),
                upstream: Some("origin/main".to_owned()),
                changed_files: 3,
                ahead: 2,
                behind: 3,
                conflicted: true,
                detached: false,
                changes: vec![
                    GitFileChange {
                        path: PathBuf::from("note.md"),
                        status: GitFileStatus::Modified,
                        staged: false,
                    },
                    GitFileChange {
                        path: PathBuf::from("image one.png"),
                        status: GitFileStatus::Untracked,
                        staged: false,
                    },
                    GitFileChange {
                        path: PathBuf::from("conflict.md"),
                        status: GitFileStatus::Conflicted,
                        staged: true,
                    },
                ],
                recent_commits: Vec::new(),
            }
        );
    }

    #[test]
    fn workspace_operations_commit_preview_and_push_notes() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let remote = directory.path().join("remote.git");
        let vault = directory.path().join("vault");
        run_git(
            directory.path(),
            &["init", "--bare", remote.to_str().unwrap()],
        );
        run_git(directory.path(), &["init", vault.to_str().unwrap()]);
        run_git(&vault, &["config", "user.name", "Synapse Test"]);
        run_git(&vault, &["config", "user.email", "synapse@example.invalid"]);
        run_git(&vault, &["config", "commit.gpgsign", "false"]);
        fs::write(vault.join("中文笔记.md"), "first\n").unwrap();
        assert!(
            read_file_diff(&vault, Path::new("中文笔记.md"))
                .unwrap()
                .contains("+first")
        );
        run_git(&vault, &["add", "中文笔记.md"]);
        run_git(&vault, &["commit", "-m", "Initial"]);
        run_git(
            &vault,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        run_git(&vault, &["push", "-u", "origin", "HEAD"]);
        fs::write(vault.join("中文笔记.md"), "updated\n").unwrap();

        let before = inspect_repository(&vault).unwrap();
        assert_eq!(before.changed_files, 1);
        assert!(
            read_file_diff(&vault, Path::new("中文笔记.md"))
                .unwrap()
                .contains("+++ b/中文笔记.md")
        );

        let after_commit = commit_repository(&vault, "Update note").unwrap();
        assert_eq!(after_commit.changed_files, 0);
        assert_eq!(after_commit.recent_commits[0].summary, "Update note");
        let after_push = push_repository(&vault).unwrap();
        assert_eq!((after_push.ahead, after_push.behind), (0, 0));

        fs::write(vault.join("note.md"), "synced\n").unwrap();
        let after_sync = sync_repository(&vault, Some("Sync note")).unwrap();
        assert!(after_sync.committed);
        assert_eq!(after_sync.status.recent_commits[0].summary, "Sync note");

        let peer = directory.path().join("peer");
        run_git(
            directory.path(),
            &["clone", remote.to_str().unwrap(), peer.to_str().unwrap()],
        );
        run_git(&peer, &["config", "user.name", "Synapse Peer"]);
        run_git(&peer, &["config", "user.email", "peer@example.invalid"]);
        run_git(&peer, &["config", "commit.gpgsign", "false"]);
        fs::write(peer.join("remote.md"), "remote\n").unwrap();
        run_git(&peer, &["add", "remote.md"]);
        run_git(&peer, &["commit", "-m", "Remote note"]);
        run_git(&peer, &["push"]);

        let after_pull = pull_repository(&vault).unwrap();
        assert_eq!((after_pull.ahead, after_pull.behind), (0, 0));
        assert_eq!(
            fs::read_to_string(vault.join("remote.md")).unwrap(),
            "remote\n"
        );
    }

    fn run_git(root: &Path, arguments: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
