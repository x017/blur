//! Workspace file indexing and path-aware fuzzy file finder using `nucleo-matcher`.

use std::path::PathBuf;

use ignore::WalkBuilder;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// A file recorded in a [`FileIndex`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedFile {
    pub root_idx: usize,
    pub rel_path: PathBuf,
    pub abs_path: PathBuf,
}

/// A fuzzy-matched file returned by [`FileIndex::fuzzy`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzyMatch {
    pub abs_path: PathBuf,
    pub rel_path: PathBuf,
    pub score: u32,
    pub indices: Vec<usize>,
}

/// In-memory index of files across one or more workspace roots.
#[derive(Clone, Debug, Default)]
pub struct FileIndex {
    files: Vec<IndexedFile>,
}

impl FileIndex {
    /// Scans `roots` respecting `.gitignore` and skipping `.git` and hidden files.
    pub fn scan(roots: &[PathBuf]) -> Self {
        Self::scan_with_options(roots, false)
    }

    /// Scans `roots` with configurable hidden file inclusion (always skipping `.git`).
    pub fn scan_with_options(roots: &[PathBuf], show_hidden: bool) -> Self {
        let mut files = Vec::new();
        for (root_idx, root) in roots.iter().enumerate() {
            if !root.exists() {
                continue;
            }
            let mut builder = WalkBuilder::new(root);
            builder
                .hidden(!show_hidden)
                .git_ignore(true)
                .git_global(true)
                .git_exclude(true)
                .require_git(false)
                .filter_entry(|entry| entry.file_name() != ".git");

            for entry in builder.build().flatten() {
                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    continue;
                }
                let abs_path = entry.into_path();
                let rel_path = abs_path
                    .strip_prefix(root)
                    .unwrap_or(&abs_path)
                    .to_path_buf();
                files.push(IndexedFile {
                    root_idx,
                    rel_path,
                    abs_path,
                });
            }
        }
        Self { files }
    }

    /// Returns all indexed files.
    pub fn files(&self) -> &[IndexedFile] {
        &self.files
    }

    /// Performs path-aware fuzzy matching against indexed relative paths.
    ///
    /// Results are sorted by descending score, then shorter path, then lexicographical order,
    /// and capped at `limit`.
    pub fn fuzzy(&self, query: &str, limit: usize) -> Vec<FuzzyMatch> {
        if limit == 0 {
            return Vec::new();
        }
        let trimmed = query.trim();
        if trimmed.is_empty() {
            let mut all: Vec<FuzzyMatch> = self
                .files
                .iter()
                .map(|f| FuzzyMatch {
                    abs_path: f.abs_path.clone(),
                    rel_path: f.rel_path.clone(),
                    score: 0,
                    indices: Vec::new(),
                })
                .collect();
            all.sort_by(|a, b| {
                a.rel_path
                    .as_os_str()
                    .len()
                    .cmp(&b.rel_path.as_os_str().len())
                    .then_with(|| a.rel_path.cmp(&b.rel_path))
            });
            all.truncate(limit);
            return all;
        }

        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::parse(trimmed, CaseMatching::Smart, Normalization::Smart);
        let mut utf32_buf = Vec::new();
        let mut raw_indices = Vec::new();
        let mut matches = Vec::new();

        for file in &self.files {
            let rel_str = file.rel_path.to_string_lossy();
            utf32_buf.clear();
            raw_indices.clear();
            let haystack = Utf32Str::new(&rel_str, &mut utf32_buf);
            if let Some(score) = pattern.indices(haystack, &mut matcher, &mut raw_indices) {
                raw_indices.sort_unstable();
                raw_indices.dedup();
                matches.push(FuzzyMatch {
                    abs_path: file.abs_path.clone(),
                    rel_path: file.rel_path.clone(),
                    score,
                    indices: raw_indices.iter().map(|&i| i as usize).collect(),
                });
            }
        }

        matches.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| {
                    a.rel_path
                        .as_os_str()
                        .len()
                        .cmp(&b.rel_path.as_os_str().len())
                })
                .then_with(|| a.rel_path.cmp(&b.rel_path))
        });
        matches.truncate(limit);
        matches
    }
}
