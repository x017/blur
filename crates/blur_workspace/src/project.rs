//! Project root management and directory listing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ignore::WalkBuilder;

/// Represents an open project consisting of one or more workspace root directories.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Project {
    pub roots: Vec<PathBuf>,
}

impl Project {
    /// Creates a new project with the given root directories.
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }

    /// Adds a root directory if it is not already present.
    pub fn add_root(&mut self, root: PathBuf) {
        if !self.roots.contains(&root) {
            self.roots.push(root);
        }
    }

    /// Removes a root directory, returning `true` if it was present.
    pub fn remove_root(&mut self, root: &Path) -> bool {
        let before = self.roots.len();
        self.roots.retain(|r| r != root);
        self.roots.len() != before
    }

    /// Returns the primary (first) root directory, if any.
    pub fn primary_root(&self) -> Option<&Path> {
        self.roots.first().map(PathBuf::as_path)
    }
}

/// Options controlling [`list_dir`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListOptions {
    pub show_hidden: bool,
    pub respect_gitignore: bool,
}

impl Default for ListOptions {
    fn default() -> Self {
        Self {
            show_hidden: false,
            respect_gitignore: true,
        }
    }
}

/// A single entry returned by [`list_dir`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
}

/// Lists the immediate children of `path`, sorted with directories first and then
/// case-insensitively by name.
pub fn list_dir(path: &Path, opts: &ListOptions) -> Result<Vec<DirEntry>> {
    if !path.exists() {
        anyhow::bail!("directory does not exist: {}", path.display());
    }
    if !path.is_dir() {
        anyhow::bail!("path is not a directory: {}", path.display());
    }

    let mut builder = WalkBuilder::new(path);
    builder
        .max_depth(Some(1))
        .hidden(!opts.show_hidden)
        .git_ignore(opts.respect_gitignore)
        .git_global(opts.respect_gitignore)
        .git_exclude(opts.respect_gitignore)
        .ignore(opts.respect_gitignore)
        .parents(opts.respect_gitignore)
        .require_git(false);

    let mut entries = Vec::new();
    for result in builder.build() {
        let entry = result.with_context(|| format!("failed reading directory {}", path.display()))?;
        if entry.depth() == 0 {
            continue;
        }
        let entry_path = entry.path().to_path_buf();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !opts.show_hidden && name == ".git" {
            continue;
        }
        let file_type = entry.file_type();
        let is_symlink = file_type.is_some_and(|ft| ft.is_symlink());
        let is_dir = if is_symlink {
            std::fs::metadata(&entry_path).is_ok_and(|m| m.is_dir())
        } else {
            file_type.is_some_and(|ft| ft.is_dir())
        };
        let size = if is_dir {
            0
        } else {
            entry.metadata().map(|m| m.len()).unwrap_or(0)
        };

        entries.push(DirEntry {
            name,
            path: entry_path,
            is_dir,
            is_symlink,
            size,
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(entries)
}
