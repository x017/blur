//! `GitRepo` implementation backed by `git2` and `crate::cli`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use git2::{
    ApplyLocation, BranchType, Delta, Diff, DiffOptions, ObjectType, Oid, Patch, Repository,
    ResetType, Signature, Sort, StashFlags, Status, StatusOptions, Tree,
};
use tracing::debug;

use crate::cli;
use crate::types::{
    BlameLine, BranchInfo, ChangeKind, CommitInfo, DiffLine, DiffLineKind, FileDiff, FileStat,
    FileStatus, HeadInfo, Hunk, LineChange, LineChangeKind, LogOptions, RefKind, RefLabel,
    RemoteInfo, StashEntry, WorktreeInfo,
};

/// Handle to a Git repository on disk.
///
/// Opens a `git2::Repository` per operation so the handle is `Send + Sync + Clone`
/// and never holds stale index/reference caches across external `git` commands.
#[derive(Debug, Clone)]
pub struct GitRepo {
    root: PathBuf,
}

impl GitRepo {
    /// Discover the Git repository containing `path`.
    pub fn discover(path: &Path) -> Result<Self> {
        let repo = Repository::discover(path)
            .with_context(|| format!("no git repository found from {}", path.display()))?;
        let root = repo
            .workdir()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| repo.path().to_path_buf());
        Ok(Self { root })
    }

    /// Root directory of the repository working tree (or `.git` dir for bare repos).
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn open(&self) -> Result<Repository> {
        Repository::open(&self.root)
            .with_context(|| format!("failed to open git repo at {}", self.root.display()))
    }

    fn rel_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            if let Ok(stripped) = path.strip_prefix(&self.root) {
                return stripped.to_path_buf();
            }
            if let (Ok(canon_root), Ok(canon_path)) =
                (self.root.canonicalize(), path.canonicalize())
            {
                if let Ok(stripped) = canon_path.strip_prefix(&canon_root) {
                    return stripped.to_path_buf();
                }
            }
        }
        path.to_path_buf()
    }

    /// Current `HEAD` branch, OID, detached state, upstream, and ahead/behind counts.
    pub fn head(&self) -> HeadInfo {
        let Ok(repo) = self.open() else {
            return HeadInfo::default();
        };

        let detached = repo.head_detached().unwrap_or(false);
        let info = match repo.head() {
            Ok(head_ref) => {
                let oid = head_ref.target().map(|o| o.to_string());
                let branch = if detached {
                    None
                } else {
                    head_ref.shorthand().map(ToOwned::to_owned)
                };

                let mut upstream = None;
                let mut ahead = 0usize;
                let mut behind = 0usize;

                if let Some(ref branch_name) = branch {
                    if let Ok(local_branch) = repo.find_branch(branch_name, BranchType::Local) {
                        if let Ok(up) = local_branch.upstream() {
                            upstream = up.name().ok().flatten().map(ToOwned::to_owned);
                            if let (Some(local_oid), Some(up_oid)) =
                                (head_ref.target(), up.get().target())
                            {
                                if let Ok((a, b)) = repo.graph_ahead_behind(local_oid, up_oid) {
                                    ahead = a;
                                    behind = b;
                                }
                            }
                        }
                    }
                }

                HeadInfo {
                    branch,
                    oid,
                    detached,
                    upstream,
                    ahead,
                    behind,
                }
            }
            Err(_) => {
                // Unborn HEAD: parse symbolic ref if available.
                let branch = repo
                    .find_reference("HEAD")
                    .ok()
                    .and_then(|r| r.symbolic_target().map(ToOwned::to_owned))
                    .map(|target| {
                        target
                            .strip_prefix("refs/heads/")
                            .unwrap_or(&target)
                            .to_string()
                    });
                HeadInfo {
                    branch,
                    oid: None,
                    detached: false,
                    upstream: None,
                    ahead: 0,
                    behind: 0,
                }
            }
        };
        info
    }

    /// Working tree and index status for all changed, untracked, and conflicted files.
    pub fn status(&self) -> Result<Vec<FileStatus>> {
        let repo = self.open()?;
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .renames_head_to_index(true)
            .renames_index_to_workdir(true);

        let statuses = repo.statuses(Some(&mut opts))?;
        let mut out = Vec::with_capacity(statuses.len());

        for entry in statuses.iter() {
            let st = entry.status();
            let path_str = entry
                .head_to_index()
                .and_then(|d| d.new_file().path())
                .or_else(|| entry.index_to_workdir().and_then(|d| d.new_file().path()))
                .or_else(|| entry.path().map(Path::new));
            let Some(path) = path_str else {
                continue;
            };

            let index = if st.contains(Status::INDEX_NEW) {
                Some(ChangeKind::Added)
            } else if st.contains(Status::INDEX_MODIFIED) {
                Some(ChangeKind::Modified)
            } else if st.contains(Status::INDEX_DELETED) {
                Some(ChangeKind::Deleted)
            } else if st.contains(Status::INDEX_RENAMED) {
                Some(ChangeKind::Renamed)
            } else if st.contains(Status::INDEX_TYPECHANGE) {
                Some(ChangeKind::Typechange)
            } else {
                None
            };

            let worktree = if st.contains(Status::WT_NEW) {
                Some(ChangeKind::Untracked)
            } else if st.contains(Status::WT_MODIFIED) {
                Some(ChangeKind::Modified)
            } else if st.contains(Status::WT_DELETED) {
                Some(ChangeKind::Deleted)
            } else if st.contains(Status::WT_RENAMED) {
                Some(ChangeKind::Renamed)
            } else if st.contains(Status::WT_TYPECHANGE) {
                Some(ChangeKind::Typechange)
            } else {
                None
            };

            let conflicted = st.contains(Status::CONFLICTED);

            if index.is_some() || worktree.is_some() || conflicted {
                out.push(FileStatus {
                    path: path.to_path_buf(),
                    index,
                    worktree,
                    conflicted,
                });
            }
        }

        Ok(out)
    }

    /// Walk commit history according to `opts`, attaching ref decorations.
    pub fn log(&self, opts: LogOptions) -> Result<Vec<CommitInfo>> {
        let repo = self.open()?;
        let limit = if opts.limit == 0 { 200 } else { opts.limit };

        let ref_map = collect_ref_map(&repo);

        let mut revwalk = repo.revwalk()?;
        revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;

        if opts.all_branches {
            if let Ok(refs) = repo.references() {
                for r in refs.flatten() {
                    if let Some(oid) = r.target() {
                        let _ = revwalk.push(oid);
                    } else if let Ok(peeled) = r.peel_to_commit() {
                        let _ = revwalk.push(peeled.id());
                    }
                }
            }
            let _ = revwalk.push_head();
        } else if revwalk.push_head().is_err() {
            // Unborn HEAD -> empty history.
            return Ok(Vec::new());
        }

        let filter_path = opts.path.as_deref().map(|p| self.rel_path(p));
        let author_filter = opts.author.as_deref().map(str::to_lowercase);
        let msg_filter = opts.message.as_deref().map(str::to_lowercase);

        let mut commits = Vec::new();
        for oid_res in revwalk {
            if commits.len() >= limit {
                break;
            }
            let oid = oid_res?;
            let commit = repo.find_commit(oid)?;

            if let Some(ref fpath) = filter_path {
                if !commit_touches_path(&repo, &commit, fpath) {
                    continue;
                }
            }

            let author = commit.author();
            let author_name = author.name().unwrap_or("").to_string();
            let author_email = author.email().unwrap_or("").to_string();

            if let Some(ref needle) = author_filter {
                if !author_name.to_lowercase().contains(needle)
                    && !author_email.to_lowercase().contains(needle)
                {
                    continue;
                }
            }

            let full_msg = commit.message().unwrap_or("");
            let summary = commit.summary().unwrap_or("").to_string();
            let body = full_msg
                .split_once('\n')
                .map(|(_, rest)| rest.trim().to_string())
                .unwrap_or_default();

            if let Some(ref needle) = msg_filter {
                if !full_msg.to_lowercase().contains(needle) {
                    continue;
                }
            }

            let id = oid.to_string();
            let short_id = id.chars().take(7).collect();
            let parents = commit.parent_ids().map(|p| p.to_string()).collect();
            let refs = ref_map.get(&oid).cloned().unwrap_or_default();

            commits.push(CommitInfo {
                id,
                short_id,
                summary,
                body,
                author_name,
                author_email,
                time: commit.time().seconds(),
                parents,
                refs,
                insertions: None,
                deletions: None,
            });
        }

        Ok(commits)
    }

    /// Compute `(insertions, deletions, per_file_stats)` for a commit.
    pub fn commit_stats(&self, id: &str) -> Result<(usize, usize, Vec<FileStat>)> {
        let repo = self.open()?;
        let commit = resolve_commit(&repo, id)?;
        let tree = commit.tree()?;
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

        let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
        let mut total_ins = 0usize;
        let mut total_del = 0usize;
        let mut stats = Vec::new();

        for i in 0..diff.deltas().len() {
            let Some(delta) = diff.get_delta(i) else {
                continue;
            };
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(Path::to_path_buf)
                .unwrap_or_default();

            let mut ins = 0usize;
            let mut del = 0usize;
            if let Ok(Some(patch)) = Patch::from_diff(&diff, i) {
                let (_, additions, deletions) = patch.line_stats()?;
                ins = additions;
                del = deletions;
            }
            total_ins += ins;
            total_del += del;
            stats.push(FileStat {
                path,
                insertions: ins,
                deletions: del,
            });
        }

        Ok((total_ins, total_del, stats))
    }

    /// Unified diff for a single file: `staged == false` diffs worktree vs index,
    /// `staged == true` diffs index vs `HEAD`.
    pub fn diff_file(&self, path: &Path, staged: bool) -> Result<FileDiff> {
        let repo = self.open()?;
        let rel = self.rel_path(path);

        let mut opts = DiffOptions::new();
        opts.pathspec(&rel)
            .include_untracked(true)
            .show_untracked_content(true)
            .recurse_untracked_dirs(true);

        let diff = if staged {
            let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
            repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))?
        } else {
            repo.diff_index_to_workdir(None, Some(&mut opts))?
        };

        let mut files = extract_file_diffs(&diff)?;
        if let Some(fd) = files.pop() {
            Ok(fd)
        } else {
            Ok(FileDiff {
                path: rel,
                old_path: None,
                status: ChangeKind::Modified,
                hunks: Vec::new(),
                binary: false,
                insertions: 0,
                deletions: 0,
            })
        }
    }

    /// Unified diffs for all files changed in commit `id` relative to its first parent.
    pub fn diff_commit(&self, id: &str) -> Result<Vec<FileDiff>> {
        let repo = self.open()?;
        let commit = resolve_commit(&repo, id)?;
        let tree = commit.tree()?;
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
        let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
        extract_file_diffs(&diff)
    }

    /// Unified diffs between two commits/revisions `a` and `b`.
    pub fn diff_commits(&self, a: &str, b: &str) -> Result<Vec<FileDiff>> {
        let repo = self.open()?;
        let tree_a = resolve_commit(&repo, a)?.tree()?;
        let tree_b = resolve_commit(&repo, b)?.tree()?;
        let diff = repo.diff_tree_to_tree(Some(&tree_a), Some(&tree_b), None)?;
        extract_file_diffs(&diff)
    }

    /// Compute gutter line-change markers for `current_text` compared against the index
    /// (falling back to `HEAD` if not in the index). Line numbers are 0-based.
    pub fn line_changes(&self, path: &Path, current_text: &str) -> Vec<LineChange> {
        let Ok(repo) = self.open() else {
            return Vec::new();
        };
        let rel = self.rel_path(path);

        let base_bytes: Option<Vec<u8>> = read_index_blob(&repo, &rel)
            .or_else(|| read_head_blob(&repo, &rel));

        let Some(base) = base_bytes else {
            // New/untracked file: every line is Added.
            let line_count = current_text.lines().count();
            return (0..line_count)
                .map(|line| LineChange {
                    line,
                    kind: LineChangeKind::Added,
                })
                .collect();
        };

        let mut opts = DiffOptions::new();
        opts.context_lines(0);

        let Ok(patch) = Patch::from_buffers(&base, None, current_text.as_bytes(), None, Some(&mut opts))
        else {
            return Vec::new();
        };

        let mut changes = Vec::new();
        for h_idx in 0..patch.num_hunks() {
            let Ok((hunk, _)) = patch.hunk(h_idx) else {
                continue;
            };
            let old_lines = hunk.old_lines() as usize;
            let new_lines = hunk.new_lines() as usize;
            let new_start_0 = (hunk.new_start() as usize).saturating_sub(1);

            if old_lines == 0 && new_lines > 0 {
                for offset in 0..new_lines {
                    changes.push(LineChange {
                        line: new_start_0 + offset,
                        kind: LineChangeKind::Added,
                    });
                }
            } else if old_lines > 0 && new_lines == 0 {
                let marker_line = (hunk.new_start() as usize).saturating_sub(1);
                changes.push(LineChange {
                    line: marker_line,
                    kind: LineChangeKind::Deleted,
                });
            } else if old_lines > 0 && new_lines > 0 {
                let modified_count = old_lines.min(new_lines);
                for offset in 0..modified_count {
                    changes.push(LineChange {
                        line: new_start_0 + offset,
                        kind: LineChangeKind::Modified,
                    });
                }
                for offset in modified_count..new_lines {
                    changes.push(LineChange {
                        line: new_start_0 + offset,
                        kind: LineChangeKind::Added,
                    });
                }
            }
        }

        changes
    }

    /// Stage a single file path (additions, modifications, or deletions).
    pub fn stage_path(&self, path: &Path) -> Result<()> {
        let repo = self.open()?;
        let rel = self.rel_path(path);
        let mut index = repo.index()?;
        let abs = self.root.join(&rel);
        if abs.exists() {
            index.add_path(&rel)?;
        } else {
            index.remove_path(&rel)?;
        }
        index.write()?;
        Ok(())
    }

    /// Unstage a single file path back to `HEAD` (or remove from index if unborn).
    pub fn unstage_path(&self, path: &Path) -> Result<()> {
        let repo = self.open()?;
        let rel = self.rel_path(path);
        match repo.head().and_then(|h| h.peel(ObjectType::Commit)) {
            Ok(head_obj) => {
                repo.reset_default(Some(&head_obj), [&rel])?;
            }
            Err(_) => {
                let mut index = repo.index()?;
                let _ = index.remove_path(&rel);
                index.write()?;
            }
        }
        Ok(())
    }

    /// Stage all working tree changes (new, modified, deleted).
    pub fn stage_all(&self) -> Result<()> {
        let repo = self.open()?;
        let mut index = repo.index()?;
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
        index.update_all(["*"].iter(), None)?;
        index.write()?;
        Ok(())
    }

    /// Unstage all staged changes back to `HEAD`.
    pub fn unstage_all(&self) -> Result<()> {
        let repo = self.open()?;
        match repo.head().and_then(|h| h.peel(ObjectType::Commit)) {
            Ok(head_obj) => {
                repo.reset(&head_obj, ResetType::Mixed, None)?;
            }
            Err(_) => {
                let mut index = repo.index()?;
                index.clear()?;
                index.write()?;
            }
        }
        Ok(())
    }

    /// Discard working tree changes for `path` (restores from index, or deletes untracked file).
    pub fn discard_path(&self, path: &Path) -> Result<()> {
        let repo = self.open()?;
        let rel = self.rel_path(path);
        let index = repo.index()?;
        if index.get_path(&rel, 0).is_some() {
            let mut cb = git2::build::CheckoutBuilder::new();
            cb.path(&rel).force();
            repo.checkout_index(None, Some(&mut cb))?;
        } else {
            let abs = self.root.join(&rel);
            if abs.is_dir() {
                std::fs::remove_dir_all(&abs)?;
            } else if abs.exists() {
                std::fs::remove_file(&abs)?;
            }
        }
        Ok(())
    }

    /// Stage a single hunk from the working tree into the index.
    pub fn stage_hunk(&self, path: &Path, hunk_index: usize) -> Result<()> {
        let diff = self.diff_file(path, false)?;
        let hunk = diff
            .hunks
            .get(hunk_index)
            .ok_or_else(|| anyhow!("hunk index {hunk_index} out of range"))?;
        let all_lines: Vec<usize> = (0..hunk.lines.len()).collect();
        self.apply_partial_patch(&diff.path, hunk, &all_lines, false, ApplyLocation::Index)
    }

    /// Unstage a single hunk from the index back to `HEAD`.
    pub fn unstage_hunk(&self, path: &Path, hunk_index: usize) -> Result<()> {
        let diff = self.diff_file(path, true)?;
        let hunk = diff
            .hunks
            .get(hunk_index)
            .ok_or_else(|| anyhow!("hunk index {hunk_index} out of range"))?;
        let all_lines: Vec<usize> = (0..hunk.lines.len()).collect();
        self.apply_partial_patch(&diff.path, hunk, &all_lines, true, ApplyLocation::Index)
    }

    /// Discard a single hunk from the working tree.
    pub fn discard_hunk(&self, path: &Path, hunk_index: usize) -> Result<()> {
        let diff = self.diff_file(path, false)?;
        let hunk = diff
            .hunks
            .get(hunk_index)
            .ok_or_else(|| anyhow!("hunk index {hunk_index} out of range"))?;
        let all_lines: Vec<usize> = (0..hunk.lines.len()).collect();
        self.apply_partial_patch(&diff.path, hunk, &all_lines, true, ApplyLocation::WorkDir)
    }

    /// Stage selected 0-based `line_indices` within `hunk_index` into the index.
    pub fn stage_lines(
        &self,
        path: &Path,
        hunk_index: usize,
        line_indices: &[usize],
    ) -> Result<()> {
        let diff = self.diff_file(path, false)?;
        let hunk = diff
            .hunks
            .get(hunk_index)
            .ok_or_else(|| anyhow!("hunk index {hunk_index} out of range"))?;
        self.apply_partial_patch(&diff.path, hunk, line_indices, false, ApplyLocation::Index)
    }

    /// Unstage selected 0-based `line_indices` within `hunk_index` from the index.
    pub fn unstage_lines(
        &self,
        path: &Path,
        hunk_index: usize,
        line_indices: &[usize],
    ) -> Result<()> {
        let diff = self.diff_file(path, true)?;
        let hunk = diff
            .hunks
            .get(hunk_index)
            .ok_or_else(|| anyhow!("hunk index {hunk_index} out of range"))?;
        self.apply_partial_patch(&diff.path, hunk, line_indices, true, ApplyLocation::Index)
    }

    /// Discard selected 0-based `line_indices` within `hunk_index` from the working tree.
    pub fn discard_lines(
        &self,
        path: &Path,
        hunk_index: usize,
        line_indices: &[usize],
    ) -> Result<()> {
        let diff = self.diff_file(path, false)?;
        let hunk = diff
            .hunks
            .get(hunk_index)
            .ok_or_else(|| anyhow!("hunk index {hunk_index} out of range"))?;
        self.apply_partial_patch(&diff.path, hunk, line_indices, true, ApplyLocation::WorkDir)
    }

    fn apply_partial_patch(
        &self,
        rel_path: &Path,
        hunk: &Hunk,
        line_indices: &[usize],
        reverse: bool,
        location: ApplyLocation,
    ) -> Result<()> {
        let selected: HashSet<usize> = line_indices.iter().copied().collect();
        let patch_str = build_partial_patch(rel_path, hunk, &selected, reverse)?;
        if patch_str.is_empty() {
            return Ok(());
        }
        let repo = self.open()?;
        let diff = Diff::from_buffer(patch_str.as_bytes())
            .context("failed to parse generated partial patch")?;
        repo.apply(&diff, location, None)
            .context("failed to apply partial patch")?;
        Ok(())
    }

    /// Create a commit with `message` (optionally amending `HEAD`).
    ///
    /// Uses the `git` CLI when `commit.gpgsign` is enabled or commit hooks are present;
    /// otherwise commits directly via `git2`. Returns the new commit's hex OID.
    pub fn commit(&self, message: &str, amend: bool) -> Result<String> {
        let mut repo = self.open()?;
        if should_use_cli_commit(&repo) {
            debug!("using git CLI for commit (hooks or gpgsign configured)");
            let mut args = vec!["commit", "-F", "-"];
            if amend {
                args.push("--amend");
            }
            cli::run_with_stdin(&self.root, &args, message.as_bytes())?;
            let repo_after = self.open()?;
            let oid = repo_after
                .head()?
                .target()
                .ok_or_else(|| anyhow!("HEAD has no target after commit"))?;
            return Ok(oid.to_string());
        }

        // Collect MERGE_HEAD parents before borrowing immutable objects from `repo`.
        let mut merge_oids = Vec::new();
        if !amend {
            let _ = repo.mergehead_foreach(|oid| {
                merge_oids.push(*oid);
                true
            });
        }

        let sig = repo.signature().context(
            "failed to read git user.name / user.email signature from config",
        )?;
        let mut index = repo.index()?;
        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;

        if amend {
            let head_commit = repo
                .head()
                .context("cannot amend without HEAD")?
                .peel_to_commit()?;
            let oid = head_commit.amend(
                Some("HEAD"),
                Some(&sig),
                Some(&sig),
                None,
                Some(message),
                Some(&tree),
            )?;
            return Ok(oid.to_string());
        }

        let mut parent_commits = Vec::new();
        if let Ok(head_ref) = repo.head() {
            if let Ok(parent) = head_ref.peel_to_commit() {
                parent_commits.push(parent);
            }
        }
        for m_oid in merge_oids {
            if let Ok(mc) = repo.find_commit(m_oid) {
                if !parent_commits.iter().any(|p| p.id() == mc.id()) {
                    parent_commits.push(mc);
                }
            }
        }

        let parent_refs: Vec<&git2::Commit<'_>> = parent_commits.iter().collect();
        let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)?;
        let _ = repo.cleanup_state();
        Ok(oid.to_string())
    }

    /// Read the configured `commit.template` file, if any.
    pub fn commit_template(&self) -> Option<String> {
        let repo = self.open().ok()?;
        let config = repo.config().ok()?;
        let template_path = config.get_path("commit.template").ok()?;
        let resolved = if template_path.is_absolute() {
            template_path
        } else {
            self.root.join(template_path)
        };
        std::fs::read_to_string(resolved).ok()
    }

    /// List all local and remote branches.
    pub fn branches(&self) -> Result<Vec<BranchInfo>> {
        let repo = self.open()?;
        let mut out = Vec::new();

        for item in repo.branches(None)? {
            let (branch, btype) = item?;
            let Some(name) = branch.name()?.map(ToOwned::to_owned) else {
                continue;
            };
            let is_remote = btype == BranchType::Remote;
            let is_head = !is_remote && branch.is_head();
            let upstream = if !is_remote {
                branch
                    .upstream()
                    .ok()
                    .and_then(|u| u.name().ok().flatten().map(ToOwned::to_owned))
            } else {
                None
            };

            let commit = branch.get().peel_to_commit().ok();
            let last_commit_summary = commit
                .as_ref()
                .and_then(|c| c.summary().map(ToOwned::to_owned));
            let last_commit_time = commit.as_ref().map(|c| c.time().seconds());

            out.push(BranchInfo {
                name,
                is_head,
                is_remote,
                upstream,
                last_commit_summary,
                last_commit_time,
            });
        }

        Ok(out)
    }

    /// Create a local branch `name` at `from` (or `HEAD`), optionally checking it out.
    pub fn create_branch(&self, name: &str, from: Option<&str>, checkout: bool) -> Result<()> {
        let repo = self.open()?;
        let target = match from {
            Some(spec) => resolve_commit(&repo, spec)?,
            None => repo.head()?.peel_to_commit()?,
        };
        repo.branch(name, &target, false)?;
        drop(target);
        drop(repo);
        if checkout {
            self.checkout(name)?;
        }
        Ok(())
    }

    /// Checkout a local branch, remote branch, tag, or commit OID.
    pub fn checkout(&self, name_or_oid: &str) -> Result<()> {
        let repo = self.open()?;
        let local_ref = format!("refs/heads/{name_or_oid}");
        if let Ok(reference) = repo.find_reference(&local_ref) {
            let obj = reference.peel(ObjectType::Commit)?;
            repo.checkout_tree(&obj, None)?;
            repo.set_head(&local_ref)?;
            return Ok(());
        }

        let obj = repo.revparse_single(name_or_oid)?;
        let commit = obj.peel_to_commit()?;
        repo.checkout_tree(commit.as_object(), None)?;
        repo.set_head_detached(commit.id())?;
        Ok(())
    }

    /// Delete a local branch by name.
    pub fn delete_branch(&self, name: &str) -> Result<()> {
        let repo = self.open()?;
        let mut branch = repo.find_branch(name, BranchType::Local)?;
        branch.delete()?;
        Ok(())
    }

    /// List all stash entries.
    pub fn stashes(&self) -> Vec<StashEntry> {
        let Ok(mut repo) = self.open() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let _ = repo.stash_foreach(|index, message, oid| {
            out.push(StashEntry {
                index,
                message: message.to_string(),
                oid: oid.to_string(),
            });
            true
        });
        out
    }

    /// Save working tree changes onto the stash stack.
    pub fn stash_save(&self, message: Option<&str>, include_untracked: bool) -> Result<()> {
        let mut repo = self.open()?;
        let sig = default_signature(&repo)?;
        let flags = if include_untracked {
            StashFlags::INCLUDE_UNTRACKED
        } else {
            StashFlags::DEFAULT
        };
        repo.stash_save(&sig, message.unwrap_or("WIP"), Some(flags))?;
        Ok(())
    }

    /// Apply stash at `index` without removing it from the stash stack.
    pub fn stash_apply(&self, index: usize) -> Result<()> {
        let mut repo = self.open()?;
        repo.stash_apply(index, None)?;
        Ok(())
    }

    /// Apply and remove stash at `index`.
    pub fn stash_pop(&self, index: usize) -> Result<()> {
        let mut repo = self.open()?;
        repo.stash_pop(index, None)?;
        Ok(())
    }

    /// Drop stash at `index`.
    pub fn stash_drop(&self, index: usize) -> Result<()> {
        let mut repo = self.open()?;
        repo.stash_drop(index)?;
        Ok(())
    }

    /// Per-line blame information for `path` (1-based line numbers).
    pub fn blame(&self, path: &Path) -> Result<Vec<BlameLine>> {
        let repo = self.open()?;
        let rel = self.rel_path(path);
        let blame = repo.blame_file(&rel, None)?;

        let mut summary_cache: HashMap<Oid, String> = HashMap::new();
        let mut lines = Vec::new();

        for hunk in blame.iter() {
            let oid = hunk.final_commit_id();
            let oid_str = oid.to_string();
            let commit_short: String = oid_str.chars().take(7).collect();
            let sig = hunk.final_signature();
            let author = sig.name().unwrap_or("").to_string();
            let time = sig.when().seconds();

            let summary = summary_cache
                .entry(oid)
                .or_insert_with(|| {
                    repo.find_commit(oid)
                        .ok()
                        .and_then(|c| c.summary().map(ToOwned::to_owned))
                        .unwrap_or_default()
                })
                .clone();

            let start = hunk.final_start_line();
            let count = hunk.lines_in_hunk();
            for offset in 0..count {
                lines.push(BlameLine {
                    line: start + offset,
                    commit_short: commit_short.clone(),
                    author: author.clone(),
                    time,
                    summary: summary.clone(),
                });
            }
        }

        Ok(lines)
    }

    /// Commit history touching `path`, up to `limit` commits.
    pub fn file_history(&self, path: &Path, limit: usize) -> Vec<CommitInfo> {
        self.log(LogOptions {
            limit,
            all_branches: false,
            path: Some(path.to_path_buf()),
            author: None,
            message: None,
        })
        .unwrap_or_default()
    }

    /// List configured remotes.
    pub fn remotes(&self) -> Vec<RemoteInfo> {
        let Ok(repo) = self.open() else {
            return Vec::new();
        };
        let Ok(names) = repo.remotes() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for name in names.iter().flatten() {
            if let Ok(remote) = repo.find_remote(name) {
                out.push(RemoteInfo {
                    name: name.to_string(),
                    url: remote.url().unwrap_or("").to_string(),
                });
            }
        }
        out
    }

    /// List the main working tree and any linked worktrees.
    pub fn worktrees(&self) -> Vec<WorktreeInfo> {
        let Ok(repo) = self.open() else {
            return Vec::new();
        };
        let mut out = Vec::new();

        // Main worktree
        let main_branch = repo
            .head()
            .ok()
            .filter(|h| h.is_branch())
            .and_then(|h| h.shorthand().map(ToOwned::to_owned));
        out.push(WorktreeInfo {
            name: "main".to_string(),
            path: self.root.clone(),
            branch: main_branch,
        });

        if let Ok(wt_names) = repo.worktrees() {
            for name in wt_names.iter().flatten() {
                if let Ok(wt) = repo.find_worktree(name) {
                    let wt_path = wt.path().to_path_buf();
                    let wt_branch = Repository::open(&wt_path)
                        .ok()
                        .and_then(|r| {
                            r.head()
                                .ok()
                                .filter(|h| h.is_branch())
                                .and_then(|h| h.shorthand().map(ToOwned::to_owned))
                        });
                    out.push(WorktreeInfo {
                        name: name.to_string(),
                        path: wt_path,
                        branch: wt_branch,
                    });
                }
            }
        }

        out
    }

    /// Return all currently conflicted file paths in the index.
    pub fn conflicts(&self) -> Vec<PathBuf> {
        let Ok(repo) = self.open() else {
            return Vec::new();
        };
        let Ok(index) = repo.index() else {
            return Vec::new();
        };
        let Ok(conflicts) = index.conflicts() else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for conflict in conflicts.flatten() {
            let path_bytes = conflict
                .our
                .as_ref()
                .or(conflict.their.as_ref())
                .or(conflict.ancestor.as_ref())
                .map(|e| &e.path);
            if let Some(bytes) = path_bytes {
                let p = PathBuf::from(String::from_utf8_lossy(bytes).into_owned());
                if seen.insert(p.clone()) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// Commits per calendar day (UTC) over the last `days` days.
    pub fn heatmap(&self, days: u32) -> Vec<(NaiveDate, usize)> {
        let Ok(repo) = self.open() else {
            return Vec::new();
        };
        let Ok(mut revwalk) = repo.revwalk() else {
            return Vec::new();
        };
        let _ = revwalk.set_sorting(Sort::TIME);
        if revwalk.push_head().is_err() {
            return Vec::new();
        }

        let cutoff = Utc::now() - Duration::days(i64::from(days));
        let cutoff_secs = cutoff.timestamp();
        let mut counts: BTreeMap<NaiveDate, usize> = BTreeMap::new();

        for oid_res in revwalk {
            let Ok(oid) = oid_res else {
                break;
            };
            let Ok(commit) = repo.find_commit(oid) else {
                continue;
            };
            let ts = commit.time().seconds();
            if ts < cutoff_secs {
                continue;
            }
            if let Some(dt) = DateTime::from_timestamp(ts, 0) {
                *counts.entry(dt.date_naive()).or_insert(0) += 1;
            }
        }

        counts.into_iter().collect()
    }

    /// Count how many commits touched each file across up to `limit_commits` recent commits,
    /// sorted descending by touch count.
    pub fn file_churn(&self, limit_commits: usize) -> Vec<(PathBuf, usize)> {
        let Ok(repo) = self.open() else {
            return Vec::new();
        };
        let Ok(mut revwalk) = repo.revwalk() else {
            return Vec::new();
        };
        let _ = revwalk.set_sorting(Sort::TIME);
        if revwalk.push_head().is_err() {
            return Vec::new();
        }

        let mut counts: HashMap<PathBuf, usize> = HashMap::new();
        for oid_res in revwalk.take(limit_commits) {
            let Ok(oid) = oid_res else {
                break;
            };
            let Ok(commit) = repo.find_commit(oid) else {
                continue;
            };
            let Ok(tree) = commit.tree() else {
                continue;
            };
            let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
            if let Ok(diff) = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None) {
                let mut touched = HashSet::new();
                for delta in diff.deltas() {
                    if let Some(p) = delta.new_file().path().or_else(|| delta.old_file().path()) {
                        touched.insert(p.to_path_buf());
                    }
                }
                for p in touched {
                    *counts.entry(p).or_insert(0) += 1;
                }
            }
        }

        let mut out: Vec<(PathBuf, usize)> = counts.into_iter().collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }
}

fn resolve_commit<'a>(repo: &'a Repository, spec: &str) -> Result<git2::Commit<'a>> {
    let obj = repo
        .revparse_single(spec)
        .with_context(|| format!("failed to resolve revision `{spec}`"))?;
    obj.peel_to_commit()
        .with_context(|| format!("revision `{spec}` does not point to a commit"))
}

fn default_signature(repo: &Repository) -> Result<Signature<'static>> {
    if let Ok(sig) = repo.signature() {
        let name = sig.name().unwrap_or("Blur").to_string();
        let email = sig.email().unwrap_or("blur@local").to_string();
        return Ok(Signature::now(&name, &email)?);
    }
    Ok(Signature::now("Blur", "blur@local")?)
}

fn should_use_cli_commit(repo: &Repository) -> bool {
    if let Ok(cfg) = repo.config() {
        if cfg.get_bool("commit.gpgsign").unwrap_or(false) {
            return true;
        }
        if let Ok(hooks_path) = cfg.get_path("core.hooksPath") {
            let dir = if hooks_path.is_absolute() {
                hooks_path
            } else {
                repo.workdir()
                    .unwrap_or_else(|| repo.path())
                    .join(hooks_path)
            };
            if has_commit_hooks(&dir) {
                return true;
            }
        }
    }
    has_commit_hooks(&repo.path().join("hooks"))
}

fn has_commit_hooks(hooks_dir: &Path) -> bool {
    for name in ["pre-commit", "prepare-commit-msg", "commit-msg"] {
        let p = hooks_dir.join(name);
        if p.is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(&p) {
                    if meta.permissions().mode() & 0o111 != 0 {
                        return true;
                    }
                }
            }
            #[cfg(not(unix))]
            {
                return true;
            }
        }
    }
    false
}

fn collect_ref_map(repo: &Repository) -> HashMap<Oid, Vec<RefLabel>> {
    let mut map: HashMap<Oid, Vec<RefLabel>> = HashMap::new();

    if let Ok(head) = repo.head() {
        if let Some(oid) = head.target() {
            let name = if head.is_branch() {
                format!("HEAD -> {}", head.shorthand().unwrap_or("HEAD"))
            } else {
                "HEAD".to_string()
            };
            map.entry(oid).or_default().push(RefLabel {
                name,
                kind: RefKind::Head,
            });
        }
    }

    if let Ok(refs) = repo.references() {
        for r in refs.flatten() {
            let Some(full_name) = r.name() else {
                continue;
            };
            let oid = r
                .target()
                .or_else(|| r.peel_to_commit().ok().map(|c| c.id()));
            let Some(oid) = oid else {
                continue;
            };

            let (kind, short) = if let Some(s) = full_name.strip_prefix("refs/heads/") {
                (RefKind::LocalBranch, s.to_string())
            } else if let Some(s) = full_name.strip_prefix("refs/remotes/") {
                if s.ends_with("/HEAD") {
                    continue;
                }
                (RefKind::RemoteBranch, s.to_string())
            } else if let Some(s) = full_name.strip_prefix("refs/tags/") {
                (RefKind::Tag, s.to_string())
            } else {
                continue;
            };

            map.entry(oid).or_default().push(RefLabel { name: short, kind });
        }
    }

    map
}

fn commit_touches_path(repo: &Repository, commit: &git2::Commit<'_>, path: &Path) -> bool {
    let Ok(tree) = commit.tree() else {
        return false;
    };
    let cur_id = tree.get_path(path).ok().map(|e| e.id());
    if commit.parent_count() == 0 {
        return cur_id.is_some();
    }
    for parent in commit.parents() {
        let parent_id = parent
            .tree()
            .ok()
            .and_then(|t: Tree<'_>| t.get_path(path).ok().map(|e| e.id()));
        if parent_id != cur_id {
            return true;
        }
    }
    let _ = repo;
    false
}

fn read_index_blob(repo: &Repository, rel: &Path) -> Option<Vec<u8>> {
    let index = repo.index().ok()?;
    let entry = index.get_path(rel, 0)?;
    let blob = repo.find_blob(entry.id).ok()?;
    Some(blob.content().to_vec())
}

fn read_head_blob(repo: &Repository, rel: &Path) -> Option<Vec<u8>> {
    let tree = repo.head().ok()?.peel_to_tree().ok()?;
    let entry = tree.get_path(rel).ok()?;
    let blob = repo.find_blob(entry.id()).ok()?;
    Some(blob.content().to_vec())
}

fn delta_to_change_kind(delta: Delta) -> ChangeKind {
    match delta {
        Delta::Added | Delta::Copied => ChangeKind::Added,
        Delta::Deleted => ChangeKind::Deleted,
        Delta::Renamed => ChangeKind::Renamed,
        Delta::Untracked => ChangeKind::Untracked,
        Delta::Typechange => ChangeKind::Typechange,
        _ => ChangeKind::Modified,
    }
}

fn extract_file_diffs(diff: &Diff<'_>) -> Result<Vec<FileDiff>> {
    let mut out = Vec::with_capacity(diff.deltas().len());

    for i in 0..diff.deltas().len() {
        let Some(delta) = diff.get_delta(i) else {
            continue;
        };
        let status = delta_to_change_kind(delta.status());
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let old_path = if status == ChangeKind::Renamed {
            delta.old_file().path().map(Path::to_path_buf)
        } else {
            None
        };
        let binary = delta.flags().is_binary();

        let mut hunks = Vec::new();
        let mut insertions = 0usize;
        let mut deletions = 0usize;

        if !binary {
            if let Some(patch) = Patch::from_diff(diff, i)? {
                for h_idx in 0..patch.num_hunks() {
                    let (hunk_hdr, num_lines) = patch.hunk(h_idx)?;
                    let header = String::from_utf8_lossy(hunk_hdr.header())
                        .trim_end_matches(['\r', '\n'])
                        .to_string();
                    let mut lines = Vec::with_capacity(num_lines);
                    for l_idx in 0..num_lines {
                        let line = patch.line_in_hunk(h_idx, l_idx)?;
                        let kind = match line.origin() {
                            '+' => {
                                insertions += 1;
                                DiffLineKind::Addition
                            }
                            '-' => {
                                deletions += 1;
                                DiffLineKind::Deletion
                            }
                            ' ' => DiffLineKind::Context,
                            // Skip `\ No newline at end of file` meta-lines in DiffLine list
                            _ => continue,
                        };
                        let content = String::from_utf8_lossy(line.content()).into_owned();
                        lines.push(DiffLine {
                            kind,
                            old_lineno: line.old_lineno(),
                            new_lineno: line.new_lineno(),
                            content,
                        });
                    }
                    hunks.push(Hunk {
                        header,
                        old_start: hunk_hdr.old_start(),
                        old_lines: hunk_hdr.old_lines(),
                        new_start: hunk_hdr.new_start(),
                        new_lines: hunk_hdr.new_lines(),
                        lines,
                    });
                }
            }
        }

        out.push(FileDiff {
            path,
            old_path,
            status,
            hunks,
            binary,
            insertions,
            deletions,
        });
    }

    Ok(out)
}

fn build_partial_patch(
    rel_path: &Path,
    hunk: &Hunk,
    selected: &HashSet<usize>,
    reverse: bool,
) -> Result<String> {
    let mut patch_lines: Vec<(char, &str)> = Vec::with_capacity(hunk.lines.len());
    let mut old_count: u32 = 0;
    let mut new_count: u32 = 0;
    let mut has_changes = false;

    for (idx, line) in hunk.lines.iter().enumerate() {
        let is_selected = selected.contains(&idx);
        if !reverse {
            match line.kind {
                DiffLineKind::Context => {
                    patch_lines.push((' ', &line.content));
                    old_count += 1;
                    new_count += 1;
                }
                DiffLineKind::Addition => {
                    if is_selected {
                        patch_lines.push(('+', &line.content));
                        new_count += 1;
                        has_changes = true;
                    }
                }
                DiffLineKind::Deletion => {
                    if is_selected {
                        patch_lines.push(('-', &line.content));
                        old_count += 1;
                        has_changes = true;
                    } else {
                        patch_lines.push((' ', &line.content));
                        old_count += 1;
                        new_count += 1;
                    }
                }
            }
        } else {
            // Reverse patch: transform `new` state back toward `old` state.
            match line.kind {
                DiffLineKind::Context => {
                    patch_lines.push((' ', &line.content));
                    old_count += 1;
                    new_count += 1;
                }
                DiffLineKind::Addition => {
                    if is_selected {
                        patch_lines.push(('-', &line.content));
                        old_count += 1;
                        has_changes = true;
                    } else {
                        patch_lines.push((' ', &line.content));
                        old_count += 1;
                        new_count += 1;
                    }
                }
                DiffLineKind::Deletion => {
                    if is_selected {
                        patch_lines.push(('+', &line.content));
                        new_count += 1;
                        has_changes = true;
                    }
                }
            }
        }
    }

    if !has_changes {
        return Ok(String::new());
    }

    let path_str = rel_path
        .to_str()
        .ok_or_else(|| anyhow!("non-UTF-8 path in patch"))?;
    let (old_start, new_start) = if reverse {
        (hunk.new_start, hunk.new_start)
    } else {
        (hunk.old_start, hunk.old_start)
    };

    let mut out = String::new();
    out.push_str(&format!("diff --git a/{path_str} b/{path_str}\n"));
    out.push_str(&format!("--- a/{path_str}\n"));
    out.push_str(&format!("+++ b/{path_str}\n"));
    out.push_str(&format!(
        "@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
    ));

    for (prefix, content) in patch_lines {
        out.push(prefix);
        out.push_str(content);
        if !content.ends_with('\n') {
            out.push_str("\n\\ No newline at end of file\n");
        }
    }

    if out.is_empty() {
        bail!("generated empty patch");
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conflict::{self, Resolution};
    use crate::graph;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempRepo {
        dir: PathBuf,
        repo: GitRepo,
    }

    impl TempRepo {
        fn new(label: &str) -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "blur_git_test_{label}_{}_{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();

            let r = Repository::init(&dir).unwrap();
            let mut cfg = r.config().unwrap();
            cfg.set_str("user.name", "Test User").unwrap();
            cfg.set_str("user.email", "test@example.com").unwrap();
            cfg.set_bool("commit.gpgsign", false).unwrap();

            let repo = GitRepo::discover(&dir).unwrap();
            Self { dir, repo }
        }

        fn write(&self, rel: &str, content: &str) {
            let p = self.dir.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, content).unwrap();
        }

        fn read(&self, rel: &str) -> String {
            std::fs::read_to_string(self.dir.join(rel)).unwrap()
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn status_stage_commit_and_log() {
        let t = TempRepo::new("basic");
        t.write("hello.txt", "line1\nline2\n");

        let st = t.repo.status().unwrap();
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].path, PathBuf::from("hello.txt"));
        assert_eq!(st[0].worktree, Some(ChangeKind::Untracked));
        assert_eq!(st[0].index, None);

        t.repo.stage_path(Path::new("hello.txt")).unwrap();
        let st = t.repo.status().unwrap();
        assert_eq!(st[0].index, Some(ChangeKind::Added));

        let oid1 = t.repo.commit("Initial commit", false).unwrap();
        assert_eq!(oid1.len(), 40);

        let head = t.repo.head();
        assert_eq!(head.oid.as_deref(), Some(oid1.as_str()));
        assert!(!head.detached);

        // Modify and check gutter line_changes
        let changes = t.repo.line_changes(Path::new("hello.txt"), "line1\nline2_mod\nline3\n");
        assert_eq!(
            changes,
            vec![
                LineChange { line: 1, kind: LineChangeKind::Modified },
                LineChange { line: 2, kind: LineChangeKind::Added },
            ]
        );

        // Amend commit
        t.write("hello.txt", "line1\nline2\nline3\n");
        t.repo.stage_all().unwrap();
        let oid2 = t.repo.commit("Initial commit (amended)", true).unwrap();
        assert_ne!(oid1, oid2);

        let log = t.repo.log(LogOptions::default()).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].summary, "Initial commit (amended)");
        assert!(!log[0].refs.is_empty());

        let (ins, del, stats) = t.repo.commit_stats(&oid2).unwrap();
        assert_eq!(ins, 3);
        assert_eq!(del, 0);
        assert_eq!(stats.len(), 1);
    }

    #[test]
    fn partial_hunk_and_line_staging() {
        let t = TempRepo::new("partial");
        t.write("a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
        t.repo.stage_all().unwrap();
        t.repo.commit("base", false).unwrap();

        // Edit line 1 and line 10 (two separate hunks)
        t.write("a.txt", "1a\n1b\n2\n3\n4\n5\n6\n7\n8\n9\n10a\n");
        let wt_diff = t.repo.diff_file(Path::new("a.txt"), false).unwrap();
        assert_eq!(wt_diff.hunks.len(), 2);

        // Stage only hunk 0
        t.repo.stage_hunk(Path::new("a.txt"), 0).unwrap();
        let staged_diff = t.repo.diff_file(Path::new("a.txt"), true).unwrap();
        assert_eq!(staged_diff.hunks.len(), 1);
        assert_eq!(staged_diff.insertions, 2);
        assert_eq!(staged_diff.deletions, 1);

        // Unstage hunk 0
        t.repo.unstage_hunk(Path::new("a.txt"), 0).unwrap();
        let staged_after = t.repo.diff_file(Path::new("a.txt"), true).unwrap();
        assert!(staged_after.hunks.is_empty());

        // Stage only the first added line ("1a\n") from hunk 0
        let wt_diff2 = t.repo.diff_file(Path::new("a.txt"), false).unwrap();
        let add_idx = wt_diff2.hunks[0]
            .lines
            .iter()
            .position(|l| l.kind == DiffLineKind::Addition && l.content == "1a\n")
            .unwrap();
        t.repo.stage_lines(Path::new("a.txt"), 0, &[add_idx]).unwrap();

        let staged_partial = t.repo.diff_file(Path::new("a.txt"), true).unwrap();
        assert_eq!(staged_partial.insertions, 1);
        assert_eq!(staged_partial.deletions, 0);

        // Discard hunk 1 in worktree
        let wt_diff3 = t.repo.diff_file(Path::new("a.txt"), false).unwrap();
        let last_hunk = wt_diff3.hunks.len() - 1;
        t.repo.discard_hunk(Path::new("a.txt"), last_hunk).unwrap();
        assert!(t.read("a.txt").ends_with("9\n10\n"));
    }

    #[test]
    fn branches_graph_layout_and_blame() {
        let t = TempRepo::new("graph");
        t.write("f.txt", "base\n");
        t.repo.stage_all().unwrap();
        t.repo.commit("c1 base", false).unwrap();
        let main_branch = t.repo.head().branch.unwrap();

        // Create feature branch and commit
        t.repo.create_branch("feature", None, true).unwrap();
        t.write("feat.txt", "feat\n");
        t.repo.stage_all().unwrap();
        t.repo.commit("c2 feature", false).unwrap();

        // Switch back to main and commit
        t.repo.checkout(&main_branch).unwrap();
        t.write("main.txt", "main\n");
        t.repo.stage_all().unwrap();
        t.repo.commit("c3 main", false).unwrap();

        // Merge feature into main via CLI
        cli::merge(&t.dir, "feature").unwrap();

        let commits = t
            .repo
            .log(LogOptions {
                limit: 50,
                all_branches: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(commits.len(), 4);
        assert_eq!(commits[0].parents.len(), 2);

        let rows = graph::layout(&commits);
        assert_eq!(rows.len(), 4);
        // Merge commit should branch out to at least 2 lanes
        assert!(rows[0].lane_count >= 2);
        assert!(rows[0].edges.len() >= 2);
        // Root commit has no outgoing parent edges
        assert!(rows.last().unwrap().edges.is_empty());

        // Branches list & delete
        let branches = t.repo.branches().unwrap();
        assert!(branches.iter().any(|b| b.name == "feature" && !b.is_head));
        t.repo.delete_branch("feature").unwrap();
        assert!(!t.repo.branches().unwrap().iter().any(|b| b.name == "feature"));

        // Blame & churn & heatmap
        let blame = t.repo.blame(Path::new("f.txt")).unwrap();
        assert_eq!(blame.len(), 1);
        assert_eq!(blame[0].summary, "c1 base");

        let churn = t.repo.file_churn(10);
        assert!(!churn.is_empty());

        let heatmap = t.repo.heatmap(7);
        assert!(!heatmap.is_empty());
    }

    #[test]
    fn stash_save_list_pop() {
        let t = TempRepo::new("stash");
        t.write("s.txt", "v1\n");
        t.repo.stage_all().unwrap();
        t.repo.commit("init", false).unwrap();

        t.write("s.txt", "v2 dirty\n");
        t.repo.stash_save(Some("my wip"), false).unwrap();
        assert_eq!(t.read("s.txt"), "v1\n");

        let stashes = t.repo.stashes();
        assert_eq!(stashes.len(), 1);
        assert!(stashes[0].message.contains("my wip"));

        t.repo.stash_pop(0).unwrap();
        assert_eq!(t.read("s.txt"), "v2 dirty\n");
        assert!(t.repo.stashes().is_empty());
    }

    #[test]
    fn conflict_parse_and_resolve_2way_and_diff3() {
        let two_way = "before\n<<<<<<< HEAD\nours line\n=======\ntheirs line\n>>>>>>> feat\nafter\n";
        let regions = conflict::parse(two_way);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start_line, 1);
        assert_eq!(regions[0].end_line, 5);
        assert_eq!(regions[0].ours, "ours line");
        assert_eq!(regions[0].theirs, "theirs line");
        assert_eq!(regions[0].base, None);
        assert_eq!(regions[0].ours_label, "HEAD");
        assert_eq!(regions[0].theirs_label, "feat");

        assert_eq!(
            conflict::resolve(two_way, 0, Resolution::Ours),
            "before\nours line\nafter\n"
        );
        assert_eq!(
            conflict::resolve(two_way, 0, Resolution::Theirs),
            "before\ntheirs line\nafter\n"
        );
        assert_eq!(
            conflict::resolve(two_way, 0, Resolution::Both),
            "before\nours line\ntheirs line\nafter\n"
        );

        let diff3 = "<<<<<<< HEAD\nours\n||||||| base\ncommon\n=======\ntheirs\n>>>>>>> branch\n";
        let d3 = conflict::parse(diff3);
        assert_eq!(d3.len(), 1);
        assert_eq!(d3[0].base.as_deref(), Some("common"));
        assert_eq!(conflict::resolve(diff3, 0, Resolution::Base), "common\n");
        assert_eq!(
            conflict::resolve(diff3, 0, Resolution::Custom("custom".into())),
            "custom\n"
        );
    }
}
