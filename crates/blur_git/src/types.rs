//! Data types for `blur_git`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Information about the repository's current `HEAD`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadInfo {
    /// Short branch name if on a branch, `None` if detached.
    pub branch: Option<String>,
    /// Full hex OID of the HEAD commit, or `None` for an unborn branch.
    pub oid: Option<String>,
    /// Whether `HEAD` is detached.
    pub detached: bool,
    /// Upstream tracking branch name, if configured (e.g. `origin/main`).
    pub upstream: Option<String>,
    /// Number of commits ahead of upstream.
    pub ahead: usize,
    /// Number of commits behind upstream.
    pub behind: usize,
}

/// Kind of change for a file in the index or working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Typechange,
}

/// Status of a single file in the repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStatus {
    pub path: PathBuf,
    pub index: Option<ChangeKind>,
    pub worktree: Option<ChangeKind>,
    pub conflicted: bool,
}

/// Options for querying commit history with [`crate::GitRepo::log`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogOptions {
    /// Maximum number of commits to return (`0` means default limit of 200).
    pub limit: usize,
    /// Walk all branches/refs instead of only `HEAD`.
    pub all_branches: bool,
    /// Restrict history to commits touching this path.
    pub path: Option<PathBuf>,
    /// Case-insensitive substring filter on author name or email.
    pub author: Option<String>,
    /// Case-insensitive substring filter on commit message (summary or body).
    pub message: Option<String>,
}

/// Kind of git reference label attached to a commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RefKind {
    LocalBranch,
    RemoteBranch,
    Tag,
    Head,
}

/// A branch, tag, or HEAD decoration on a commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
}

/// Metadata for a single commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    pub id: String,
    pub short_id: String,
    pub summary: String,
    pub body: String,
    pub author_name: String,
    pub author_email: String,
    /// Commit timestamp in Unix seconds.
    pub time: i64,
    pub parents: Vec<String>,
    pub refs: Vec<RefLabel>,
    pub insertions: Option<usize>,
    pub deletions: Option<usize>,
}

/// Per-file diff statistics for a commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStat {
    pub path: PathBuf,
    pub insertions: usize,
    pub deletions: usize,
}

/// Kind of line inside a diff hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
}

/// A single line inside a [`Hunk`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
    pub content: String,
}

/// A contiguous hunk in a [`FileDiff`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

/// Unified diff for a single file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub status: ChangeKind,
    pub hunks: Vec<Hunk>,
    pub binary: bool,
    pub insertions: usize,
    pub deletions: usize,
}

/// Kind of gutter change marker for a line in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LineChangeKind {
    Added,
    Modified,
    Deleted,
}

/// Gutter diff indicator for a 0-based buffer line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineChange {
    /// 0-based line number in the current buffer.
    pub line: usize,
    pub kind: LineChangeKind,
}

/// Information about a local or remote branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub is_head: bool,
    pub is_remote: bool,
    pub upstream: Option<String>,
    pub last_commit_summary: Option<String>,
    pub last_commit_time: Option<i64>,
}

/// A single entry in the git stash list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
    pub oid: String,
}

/// Blame attribution for a single line of a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameLine {
    /// 1-based line number in the file.
    pub line: usize,
    pub commit_short: String,
    pub author: String,
    pub time: i64,
    pub summary: String,
}

/// Configured git remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
}

/// Information about a git worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub name: String,
    pub path: PathBuf,
    pub branch: Option<String>,
}
