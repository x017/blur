//! Parallel project-wide text/regex search and in-file replacement.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use ignore::overrides::OverrideBuilder;
use ignore::{WalkBuilder, WalkState};
use parking_lot::Mutex;
use regex::{NoExpand, Regex};

/// Maximum file size scanned during project search (4 MiB).
const MAX_FILE_SIZE_BYTES: u64 = 4 * 1024 * 1024;
/// Number of initial bytes inspected for NUL to detect binary files.
const BINARY_SNIFF_BYTES: usize = 8192;
/// Maximum character length of a returned [`LineMatch::text`].
const MAX_LINE_CHARS: usize = 300;

/// Parameters for [`search`] and [`replace_in_file`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchQuery {
    pub pattern: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub max_results: usize,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
            include: Vec::new(),
            exclude: Vec::new(),
            max_results: 1000,
        }
    }
}

/// A single matching line within a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineMatch {
    /// 0-based line number.
    pub line: usize,
    /// Line text (trimmed to at most 300 characters, without trailing newline).
    pub text: String,
    /// Byte ranges of matches within `text`.
    pub ranges: Vec<Range<usize>>,
}

/// All matches found in a single file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMatches {
    pub path: PathBuf,
    pub matches: Vec<LineMatch>,
}

/// Aggregate result of a project search.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchResults {
    pub files: Vec<FileMatches>,
    pub total_matches: usize,
    pub truncated: bool,
    pub elapsed_ms: u64,
}

/// Compiles a [`SearchQuery`] into a [`Regex`].
fn build_regex(q: &SearchQuery) -> Result<Regex> {
    let mut pat = if q.regex {
        q.pattern.clone()
    } else {
        regex::escape(&q.pattern)
    };
    if q.whole_word {
        pat = format!(r"\b(?:{pat})\b");
    }
    if !q.case_sensitive {
        pat = format!("(?i){pat}");
    }
    Regex::new(&pat).with_context(|| format!("invalid search pattern: {}", q.pattern))
}

/// Searches `roots` in parallel using `ignore::WalkParallel`.
///
/// Skips binary files (NUL byte in the first 8 KB) and files larger than 4 MB.
/// Respects `.gitignore`, include/exclude globs, `cancel`, and `q.max_results`.
pub fn search(roots: &[PathBuf], q: &SearchQuery, cancel: &AtomicBool) -> Result<SearchResults> {
    let started = Instant::now();
    if q.pattern.is_empty() || roots.is_empty() || q.max_results == 0 {
        return Ok(SearchResults {
            files: Vec::new(),
            total_matches: 0,
            truncated: false,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
    }

    let re = build_regex(q)?;
    let max_results = q.max_results;
    let total_counter = AtomicUsize::new(0);
    let truncated = AtomicBool::new(false);
    let collected: Mutex<Vec<FileMatches>> = Mutex::new(Vec::new());

    for root in roots {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if !root.exists() {
            continue;
        }

        let mut override_builder = OverrideBuilder::new(root);
        for glob in &q.include {
            let trimmed = glob.trim();
            if !trimmed.is_empty() {
                override_builder
                    .add(trimmed)
                    .with_context(|| format!("invalid include glob: {trimmed}"))?;
            }
        }
        for glob in &q.exclude {
            let trimmed = glob.trim();
            if !trimmed.is_empty() {
                let negated = if trimmed.starts_with('!') {
                    trimmed.to_string()
                } else {
                    format!("!{trimmed}")
                };
                override_builder
                    .add(&negated)
                    .with_context(|| format!("invalid exclude glob: {trimmed}"))?;
            }
        }
        let overrides = override_builder.build()?;

        let mut builder = WalkBuilder::new(root);
        builder
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .require_git(false)
            .overrides(overrides)
            .filter_entry(|entry| entry.file_name() != ".git");

        builder.build_parallel().run(|| {
            let re = &re;
            let total_counter = &total_counter;
            let truncated = &truncated;
            let collected = &collected;

            Box::new(move |entry_res| {
                if cancel.load(Ordering::Relaxed) {
                    return WalkState::Quit;
                }
                if total_counter.load(Ordering::Relaxed) >= max_results {
                    truncated.store(true, Ordering::Relaxed);
                    return WalkState::Quit;
                }

                let entry = match entry_res {
                    Ok(e) => e,
                    Err(_) => return WalkState::Continue,
                };
                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    return WalkState::Continue;
                }
                let Ok(meta) = entry.metadata() else {
                    return WalkState::Continue;
                };
                if meta.len() > MAX_FILE_SIZE_BYTES {
                    return WalkState::Continue;
                }

                let path = entry.into_path();
                let Ok(bytes) = std::fs::read(&path) else {
                    return WalkState::Continue;
                };
                let sniff_len = bytes.len().min(BINARY_SNIFF_BYTES);
                if bytes[..sniff_len].contains(&0) {
                    return WalkState::Continue;
                }

                let content = String::from_utf8_lossy(&bytes);
                let mut file_matches = Vec::new();
                let mut file_match_count = 0usize;

                for (line_idx, raw_line) in content.lines().enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        return WalkState::Quit;
                    }
                    let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
                    let mut ranges = Vec::new();
                    for m in re.find_iter(line) {
                        ranges.push(m.start()..m.end());
                    }
                    if ranges.is_empty() {
                        continue;
                    }

                    let (trimmed_text, trimmed_ranges) = trim_line_and_ranges(line, ranges);
                    file_match_count += trimmed_ranges.len().max(1);
                    file_matches.push(LineMatch {
                        line: line_idx,
                        text: trimmed_text,
                        ranges: trimmed_ranges,
                    });

                    if total_counter.load(Ordering::Relaxed) + file_match_count >= max_results {
                        truncated.store(true, Ordering::Relaxed);
                        break;
                    }
                }

                if !file_matches.is_empty() {
                    let prev = total_counter.fetch_add(file_match_count, Ordering::Relaxed);
                    if prev + file_match_count >= max_results {
                        truncated.store(true, Ordering::Relaxed);
                    }
                    collected.lock().push(FileMatches {
                        path,
                        matches: file_matches,
                    });
                }

                if total_counter.load(Ordering::Relaxed) >= max_results {
                    truncated.store(true, Ordering::Relaxed);
                    WalkState::Quit
                } else {
                    WalkState::Continue
                }
            })
        });
    }

    let mut files = collected.into_inner();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let total_matches: usize = files
        .iter()
        .map(|f| f.matches.iter().map(|m| m.ranges.len().max(1)).sum::<usize>())
        .sum();

    Ok(SearchResults {
        files,
        total_matches,
        truncated: truncated.load(Ordering::Relaxed),
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// Trims a line to at most [`MAX_LINE_CHARS`] characters and clamps/drops match byte ranges
/// that fall outside the trimmed prefix.
fn trim_line_and_ranges(line: &str, ranges: Vec<Range<usize>>) -> (String, Vec<Range<usize>>) {
    let mut byte_limit = line.len();
    for (count, (byte_idx, _)) in line.char_indices().enumerate() {
        if count == MAX_LINE_CHARS {
            byte_limit = byte_idx;
            break;
        }
    }
    let text = line[..byte_limit].to_string();
    let adjusted = ranges
        .into_iter()
        .filter_map(|r| {
            if r.start >= byte_limit {
                None
            } else {
                Some(r.start..r.end.min(byte_limit))
            }
        })
        .collect();
    (text, adjusted)
}

/// Replaces all occurrences matching `q` in `path` with `replacement`.
///
/// Returns the number of replacements performed. When `q.regex` is `false`, `replacement`
/// is treated literally without capture-group expansion.
pub fn replace_in_file(path: &Path, q: &SearchQuery, replacement: &str) -> Result<usize> {
    if q.pattern.is_empty() {
        return Ok(0);
    }
    let re = build_regex(q)?;
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read file {}", path.display()))?;
    let count = re.find_iter(&content).count();
    if count == 0 {
        return Ok(0);
    }

    let replaced = if q.regex {
        re.replace_all(&content, replacement).into_owned()
    } else {
        re.replace_all(&content, NoExpand(replacement)).into_owned()
    };

    std::fs::write(path, replaced)
        .with_context(|| format!("failed to write file {}", path.display()))?;
    Ok(count)
}
