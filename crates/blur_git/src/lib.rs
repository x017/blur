//! GPUI-free Git service crate for Blur.
//!
//! Combines `git2` for fast local repository operations (status, log, diffs,
//! hunk/line partial staging, branches, stashes, blame, analytics) with a
//! non-interactive `git` CLI module for network and hook-aware operations.

pub mod cli;
pub mod conflict;
pub mod graph;
mod repo;
mod types;

pub use repo::GitRepo;
pub use types::{
    BlameLine, BranchInfo, ChangeKind, CommitInfo, DiffLine, DiffLineKind, FileDiff, FileStat,
    FileStatus, HeadInfo, Hunk, LineChange, LineChangeKind, LogOptions, RefKind, RefLabel,
    RemoteInfo, StashEntry, WorktreeInfo,
};
