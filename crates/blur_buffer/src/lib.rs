//! `blur_buffer` — GPUI-free Ropey text buffer, multi-cursor selections, undo/redo,
//! incremental edit deltas (tree-sitter + LSP UTF-16), movement, and editing operations.

pub mod movement;
pub mod ops;

mod history;
mod selection;

use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};

pub use ropey::{Rope, RopeSlice};
pub use selection::{Selection, Selections};

use history::{EditBatch, History, RecordedEdit};

/// A 0-based `(line, col)` position where `col` is in Unicode scalar values (`char`s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct Point {
    pub line: usize,
    pub col: usize,
}

/// An LSP UTF-16 position (`line` and UTF-16 code-unit `character` offset, both 0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

/// Line ending style detected on open and preserved on save.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum LineEnding {
    #[default]
    Lf,
    Crlf,
}

/// Options controlling [`Buffer::find_all`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

/// Everything tree-sitter and LSP incremental sync need for one applied edit.
///
/// Deltas returned from [`Buffer::edit`], [`Buffer::undo`], and [`Buffer::redo`] are in
/// **sequential application order**: each delta's coordinates are valid against the document
/// state right after the previous delta was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditDelta {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    /// `(row, byte_col)` in the document before this delta.
    pub start_point: (usize, usize),
    /// `(row, byte_col)` of the end of the replaced range before this delta.
    pub old_end_point: (usize, usize),
    /// `(row, byte_col)` of the end of the inserted text after this delta.
    pub new_end_point: (usize, usize),
    /// UTF-16 range `(start, end)` in the document BEFORE this delta.
    pub lsp_range: (LspPosition, LspPosition),
    pub new_text: String,
}

/// Result of [`Buffer::undo`] or [`Buffer::redo`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoResult {
    pub deltas: Vec<EditDelta>,
    pub selections: Selections,
}

/// A UTF-8 text buffer backed by [`Rope`], with file metadata, CRLF preservation,
/// monotonically increasing version counter, and grouped undo/redo history.
#[derive(Debug, Clone)]
pub struct Buffer {
    rope: Rope,
    path: Option<PathBuf>,
    version: i32,
    line_ending: LineEnding,
    history: History,
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Buffer {
    /// Creates an empty buffer with LF line endings.
    pub fn new() -> Self {
        Self {
            rope: Rope::new(),
            path: None,
            version: 0,
            line_ending: LineEnding::Lf,
            history: History::new(),
        }
    }

    /// Creates a buffer from `text`, detecting CRLF vs LF and normalizing line endings to `\n`.
    pub fn from_text(text: &str) -> Self {
        let line_ending = if text.contains("\r\n") {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        };
        let normalized = normalize_line_endings(text);
        Self {
            rope: Rope::from_str(&normalized),
            path: None,
            version: 0,
            line_ending,
            history: History::new(),
        }
    }

    /// Opens a UTF-8 file from disk, detecting CRLF/LF and normalizing to `\n` internally.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path_ref = path.as_ref();
        let raw = std::fs::read_to_string(path_ref)
            .with_context(|| format!("failed to read buffer from {}", path_ref.display()))?;
        let mut buf = Self::from_text(&raw);
        buf.path = Some(path_ref.to_path_buf());
        Ok(buf)
    }

    /// Saves the buffer to its associated path using its original line ending.
    pub fn save(&mut self) -> Result<()> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| anyhow!("buffer has no associated file path"))?;
        self.write_to_path(&path)?;
        self.history.mark_saved();
        Ok(())
    }

    /// Updates the buffer's path and saves to `path` using its original line ending.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let p = path.as_ref().to_path_buf();
        self.write_to_path(&p)?;
        self.path = Some(p);
        self.history.mark_saved();
        Ok(())
    }

    fn write_to_path(&self, path: &Path) -> Result<()> {
        let content = match self.line_ending {
            LineEnding::Lf => self.text(),
            LineEnding::Crlf => self.text().replace('\n', "\r\n"),
        };
        std::fs::write(path, content)
            .with_context(|| format!("failed to write buffer to {}", path.display()))
    }

    /// Returns a reference to the underlying [`Rope`].
    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    /// Returns the entire buffer text as a `String` (with `\n` line endings).
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Returns the file path associated with this buffer, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Sets the file path associated with this buffer.
    pub fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    /// Returns the buffer's detected/configured line ending.
    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    /// Sets the line ending used when saving.
    pub fn set_line_ending(&mut self, line_ending: LineEnding) {
        self.line_ending = line_ending;
    }

    /// Returns `true` if the buffer has unsaved modifications.
    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }

    /// Returns the monotonically increasing document version (for LSP sync).
    pub fn version(&self) -> i32 {
        self.version
    }

    /// Returns the total number of Unicode scalar values (`char`s).
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Returns the total number of UTF-8 bytes.
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// Returns the number of lines in the buffer (always `>= 1`).
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// Returns the text of `line` without any trailing `\n`.
    pub fn line_text(&self, line: usize) -> String {
        if line >= self.rope.len_lines() {
            return String::new();
        }
        let s = self.rope.line(line).to_string();
        s.strip_suffix('\n').unwrap_or(&s).to_string()
    }

    /// Returns the character length of `line` without any trailing `\n`.
    pub fn line_len_chars(&self, line: usize) -> usize {
        if line >= self.rope.len_lines() {
            return 0;
        }
        let slice = self.rope.line(line);
        let len = slice.len_chars();
        if len > 0 && slice.char(len - 1) == '\n' {
            len - 1
        } else {
            len
        }
    }

    /// Records the selection state before the next edit transaction so `undo()` can restore it.
    pub fn set_selections_for_undo(&mut self, before: &Selections) {
        self.history.set_selections_before(before);
    }

    /// Records the selection state after an edit so `redo()` restores exact cursor positions.
    pub fn set_selections_after_edit(&mut self, after: &Selections) {
        self.history.set_selections_after(after);
    }

    /// Breaks the current undo coalescing group so the next edit starts a fresh undo transaction.
    pub fn break_undo_group(&mut self) {
        self.history.break_group();
    }

    /// Applies a set of non-overlapping edits whose char ranges are in ORIGINAL document coordinates.
    ///
    /// Edits may be passed in any order. Returned [`EditDelta`]s are in sequential application
    /// order so they can be fed directly to tree-sitter (`tree.edit`) and LSP (`didChange`).
    pub fn edit(&mut self, edits: Vec<(Range<usize>, String)>) -> Vec<EditDelta> {
        if edits.is_empty() {
            return Vec::new();
        }

        // Sort descending by start so applying each edit leaves earlier ranges untouched.
        let mut sorted: Vec<(Range<usize>, String)> = edits
            .into_iter()
            .map(|(r, t)| (r, normalize_line_endings(&t)))
            .collect();
        sorted.sort_by(|a, b| b.0.start.cmp(&a.0.start).then(b.0.end.cmp(&a.0.end)));

        let mut deltas = Vec::with_capacity(sorted.len());
        let mut steps = Vec::with_capacity(sorted.len());
        let mut last_end = 0usize;

        for (range, new_text) in sorted {
            let max_c = self.rope.len_chars();
            let start = range.start.min(max_c);
            let end = range.end.min(max_c).max(start);
            let clamped = start..end;

            let old_text = self.rope.slice(clamped.clone()).to_string();
            let delta = self.apply_single_raw(clamped.clone(), &new_text);
            last_end = start + new_text.chars().count();
            deltas.push(delta);
            steps.push(RecordedEdit {
                range: clamped,
                old_text,
                new_text,
            });
        }

        self.version += 1;
        let default_after = Selections::single(Selection::cursor(last_end.min(self.len_chars())));
        self.history.record_batch(EditBatch { steps }, default_after);
        deltas
    }

    /// Undoes the most recent undo group, returning its deltas and restored selections.
    pub fn undo(&mut self) -> Option<UndoResult> {
        let group = self.history.pop_undo()?;
        let mut deltas = Vec::new();

        // Replay batches in reverse order, and steps within each batch in reverse order.
        for batch in group.batches.iter().rev() {
            for step in batch.steps.iter().rev() {
                let new_len = step.new_text.chars().count();
                let current_range = step.range.start..(step.range.start + new_len);
                let delta = self.apply_single_raw(current_range, &step.old_text);
                deltas.push(delta);
            }
        }

        self.version += 1;
        let selections = group.selections_before.clone();
        self.history.push_redo(group);
        Some(UndoResult { deltas, selections })
    }

    /// Redoes the most recently undone group, returning its deltas and restored selections.
    pub fn redo(&mut self) -> Option<UndoResult> {
        let group = self.history.pop_redo()?;
        let mut deltas = Vec::new();

        for batch in &group.batches {
            for step in &batch.steps {
                let delta = self.apply_single_raw(step.range.clone(), &step.new_text);
                deltas.push(delta);
            }
        }

        self.version += 1;
        let selections = group.selections_after.clone();
        self.history.push_undo(group);
        Some(UndoResult { deltas, selections })
    }

    /// Applies a single edit directly to `self.rope` and computes its exact [`EditDelta`].
    fn apply_single_raw(&mut self, range: Range<usize>, new_text: &str) -> EditDelta {
        let start_byte = self.rope.char_to_byte(range.start);
        let old_end_byte = self.rope.char_to_byte(range.end);
        let start_point = self.byte_to_ts_point(start_byte);
        let old_end_point = self.byte_to_ts_point(old_end_byte);
        let lsp_start = self.char_to_lsp(range.start);
        let lsp_end = self.char_to_lsp(range.end);

        if range.start < range.end {
            self.rope.remove(range.start..range.end);
        }
        if !new_text.is_empty() {
            self.rope.insert(range.start, new_text);
        }

        let new_end_byte = start_byte + new_text.len();
        let new_end_point = self.byte_to_ts_point(new_end_byte);

        EditDelta {
            start_byte,
            old_end_byte,
            new_end_byte,
            start_point,
            old_end_point,
            new_end_point,
            lsp_range: (lsp_start, lsp_end),
            new_text: new_text.to_string(),
        }
    }

    fn byte_to_ts_point(&self, byte_idx: usize) -> (usize, usize) {
        let clamped = byte_idx.min(self.rope.len_bytes());
        let row = self.rope.byte_to_line(clamped);
        let line_start_byte = self.rope.line_to_byte(row);
        (row, clamped - line_start_byte)
    }

    // -------------------------------------------------------------------------
    // Coordinate conversion
    // -------------------------------------------------------------------------

    /// Converts a character index to a 0-based `(line, col)` [`Point`].
    pub fn char_to_point(&self, char_idx: usize) -> Point {
        let idx = char_idx.min(self.rope.len_chars());
        let line = self.rope.char_to_line(idx);
        let line_start = self.rope.line_to_char(line);
        Point {
            line,
            col: idx - line_start,
        }
    }

    /// Converts a [`Point`] to a character index, clamping line and column to valid bounds.
    pub fn point_to_char(&self, p: Point) -> usize {
        let max_line = self.rope.len_lines().saturating_sub(1);
        let line = p.line.min(max_line);
        let col = p.col.min(self.line_len_chars(line));
        self.rope.line_to_char(line) + col
    }

    /// Converts a character index to a 0-based UTF-16 [`LspPosition`].
    pub fn char_to_lsp(&self, char_idx: usize) -> LspPosition {
        let idx = char_idx.min(self.rope.len_chars());
        let line = self.rope.char_to_line(idx);
        let line_start_char = self.rope.line_to_char(line);
        let utf16_col =
            self.rope.char_to_utf16_cu(idx) - self.rope.char_to_utf16_cu(line_start_char);
        LspPosition {
            line: line as u32,
            character: utf16_col as u32,
        }
    }

    /// Converts a 0-based UTF-16 [`LspPosition`] to a character index, clamping to valid bounds.
    pub fn lsp_to_char(&self, p: LspPosition) -> usize {
        let max_line = self.rope.len_lines().saturating_sub(1);
        let line = (p.line as usize).min(max_line);
        let line_start_char = self.rope.line_to_char(line);
        let line_end_char = line_start_char + self.line_len_chars(line);
        let line_start_u16 = self.rope.char_to_utf16_cu(line_start_char);
        let line_end_u16 = self.rope.char_to_utf16_cu(line_end_char);
        let target_u16 = (line_start_u16 + p.character as usize).min(line_end_u16);
        self.rope.utf16_cu_to_char(target_u16)
    }

    /// Converts a character index to a UTF-8 byte offset (clamped).
    pub fn char_to_byte(&self, c: usize) -> usize {
        self.rope.char_to_byte(c.min(self.rope.len_chars()))
    }

    /// Converts a UTF-8 byte offset to a character index (clamped).
    pub fn byte_to_char(&self, b: usize) -> usize {
        self.rope.byte_to_char(b.min(self.rope.len_bytes()))
    }

    // -------------------------------------------------------------------------
    // Search & text queries
    // -------------------------------------------------------------------------

    /// Finds all non-overlapping occurrences of `query` in the buffer, returning character ranges.
    pub fn find_all(&self, query: &str, opts: SearchOptions) -> Vec<Range<usize>> {
        if query.is_empty() {
            return Vec::new();
        }
        let base = if opts.regex {
            query.to_string()
        } else {
            regex::escape(query)
        };
        let pattern = if opts.whole_word {
            format!(r"\b(?:{base})\b")
        } else {
            base
        };

        let Ok(re) = RegexBuilder::new(&pattern)
            .case_insensitive(!opts.case_sensitive)
            .build()
        else {
            return Vec::new();
        };

        let text = self.text();
        re.find_iter(&text)
            .map(|m| self.byte_to_char(m.start())..self.byte_to_char(m.end()))
            .collect()
    }

    /// Finds the character index of the matching bracket (`()`, `[]`, `{}`) for the bracket
    /// at `char_idx` (or `char_idx - 1` if `char_idx` is not on a bracket).
    pub fn matching_bracket(&self, char_idx: usize) -> Option<usize> {
        let len = self.rope.len_chars();
        if len == 0 {
            return None;
        }

        let (idx, ch) = if char_idx < len && is_bracket(self.rope.char(char_idx)) {
            (char_idx, self.rope.char(char_idx))
        } else if char_idx > 0 && char_idx <= len && is_bracket(self.rope.char(char_idx - 1)) {
            (char_idx - 1, self.rope.char(char_idx - 1))
        } else {
            return None;
        };

        match ch {
            '(' | '[' | '{' => {
                let target = match ch {
                    '(' => ')',
                    '[' => ']',
                    '{' => '}',
                    _ => unreachable!(),
                };
                let mut depth = 0usize;
                for i in idx..len {
                    let c = self.rope.char(i);
                    if c == ch {
                        depth += 1;
                    } else if c == target {
                        depth -= 1;
                        if depth == 0 {
                            return Some(i);
                        }
                    }
                }
                None
            }
            ')' | ']' | '}' => {
                let target = match ch {
                    ')' => '(',
                    ']' => '[',
                    '}' => '{',
                    _ => unreachable!(),
                };
                let mut depth = 0usize;
                for i in (0..=idx).rev() {
                    let c = self.rope.char(i);
                    if c == ch {
                        depth += 1;
                    } else if c == target {
                        depth -= 1;
                        if depth == 0 {
                            return Some(i);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Returns the character range of the identifier/word at `char_idx` (or immediately before it).
    pub fn word_range_at(&self, char_idx: usize) -> Option<Range<usize>> {
        let len = self.rope.len_chars();
        if len == 0 {
            return None;
        }

        let seed = if char_idx < len && is_word_char(self.rope.char(char_idx)) {
            char_idx
        } else if char_idx > 0 && char_idx <= len && is_word_char(self.rope.char(char_idx - 1)) {
            char_idx - 1
        } else {
            return None;
        };

        let mut start = seed;
        while start > 0 && is_word_char(self.rope.char(start - 1)) {
            start -= 1;
        }
        let mut end = seed + 1;
        while end < len && is_word_char(self.rope.char(end)) {
            end += 1;
        }
        Some(start..end)
    }
}

fn normalize_line_endings(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_string();
    }
    s.replace("\r\n", "\n").replace('\r', "\n")
}

fn is_bracket(c: char) -> bool {
    matches!(c, '(' | ')' | '[' | ']' | '{' | '}')
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_edit_and_deltas() {
        let mut buf = Buffer::from_text("hello world\nsecond line");
        assert_eq!(buf.version(), 0);
        assert!(!buf.is_dirty());

        // Replace "world" (6..11) with "rust" and "second" (12..18) with "2nd".
        let deltas = buf.edit(vec![
            (6..11, "rust".into()),
            (12..18, "2nd".into()),
        ]);
        assert_eq!(buf.text(), "hello rust\n2nd line");
        assert_eq!(buf.version(), 1);
        assert!(buf.is_dirty());
        assert_eq!(deltas.len(), 2);

        // Deltas are in descending (application) order: first (12..18), then (6..11).
        assert_eq!(deltas[0].start_point, (1, 0));
        assert_eq!(deltas[0].old_end_point, (1, 6));
        assert_eq!(deltas[0].new_end_point, (1, 3));
        assert_eq!(
            deltas[0].lsp_range,
            (LspPosition { line: 1, character: 0 }, LspPosition { line: 1, character: 6 })
        );

        assert_eq!(deltas[1].start_point, (0, 6));
        assert_eq!(deltas[1].old_end_point, (0, 11));
        assert_eq!(deltas[1].new_end_point, (0, 10));
    }

    #[test]
    fn undo_redo_and_grouping() {
        let mut buf = Buffer::from_text("abc");
        let mut sels = Selections::single(Selection::cursor(3));

        // Two rapid edits coalesce into one undo group.
        ops::insert(&mut buf, &mut sels, "d");
        ops::insert(&mut buf, &mut sels, "e");
        assert_eq!(buf.text(), "abcde");

        // Break group, then insert "f".
        buf.break_undo_group();
        ops::insert(&mut buf, &mut sels, "f");
        assert_eq!(buf.text(), "abcdef");

        // First undo reverts "f".
        let u1 = buf.undo().unwrap();
        assert_eq!(buf.text(), "abcde");
        assert_eq!(u1.selections.primary().head, 5);

        // Second undo reverts "de" together.
        let u2 = buf.undo().unwrap();
        assert_eq!(buf.text(), "abc");
        assert_eq!(u2.selections.primary().head, 3);
        assert!(!buf.is_dirty());

        // Redo restores "de".
        let r1 = buf.redo().unwrap();
        assert_eq!(buf.text(), "abcde");
        assert_eq!(r1.selections.primary().head, 5);
        assert!(buf.is_dirty());
    }

    #[test]
    fn utf16_lsp_conversion() {
        // '😀' is 1 char, 4 UTF-8 bytes, 2 UTF-16 code units.
        let buf = Buffer::from_text("a😀b\nc");
        assert_eq!(buf.char_to_lsp(0), LspPosition { line: 0, character: 0 });
        assert_eq!(buf.char_to_lsp(1), LspPosition { line: 0, character: 1 });
        assert_eq!(buf.char_to_lsp(2), LspPosition { line: 0, character: 3 });
        assert_eq!(buf.char_to_lsp(3), LspPosition { line: 0, character: 4 });
        assert_eq!(buf.char_to_lsp(4), LspPosition { line: 1, character: 0 });

        assert_eq!(buf.lsp_to_char(LspPosition { line: 0, character: 3 }), 2);
        assert_eq!(buf.lsp_to_char(LspPosition { line: 1, character: 1 }), 5);
    }

    #[test]
    fn crlf_preservation_on_save() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("blur_buffer_crlf_{}.txt", std::process::id()));
        std::fs::write(&path, "line1\r\nline2\r\n").unwrap();

        let mut buf = Buffer::open(&path).unwrap();
        assert_eq!(buf.line_ending(), LineEnding::Crlf);
        assert_eq!(buf.text(), "line1\nline2\n");

        buf.edit(vec![(5..5, "!".into())]);
        buf.save().unwrap();
        assert!(!buf.is_dirty());

        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(saved, "line1!\r\nline2\r\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn movement_and_graphemes() {
        // Family emoji with ZWJ or combining mark: "e\u{0301}" is 1 grapheme, 2 chars.
        let buf = Buffer::from_text("ae\u{0301}b\n    hello world");
        let s0 = Selection::cursor(1);
        let s1 = movement::right(&buf, s0, false);
        assert_eq!(s1.head, 3); // skipped "e\u{0301}" as one grapheme
        let s2 = movement::left(&buf, s1, false);
        assert_eq!(s2.head, 1);

        // Smart home on line 1 ("    hello world")
        let l1_end = movement::line_end(&buf, Selection::cursor(buf.rope().line_to_char(1) + 6), false);
        let h1 = movement::line_start(&buf, l1_end, false);
        assert_eq!(buf.char_to_point(h1.head), Point { line: 1, col: 4 });
        let h2 = movement::line_start(&buf, h1, false);
        assert_eq!(buf.char_to_point(h2.head), Point { line: 1, col: 0 });
    }

    #[test]
    fn multi_cursor_ops_and_paste() {
        let mut buf = Buffer::from_text("foo\nbar\nbaz");
        let mut sels = Selections::single(Selection::cursor(0));
        ops::add_cursor_below(&buf, &mut sels);
        ops::add_cursor_below(&buf, &mut sels);
        assert_eq!(sels.len(), 3);

        // Paste 3 lines -> distributes 1 line per cursor.
        ops::paste(&mut buf, &mut sels, "1_\n2_\n3_");
        assert_eq!(buf.text(), "1_foo\n2_bar\n3_baz");

        // Toggle line comment across all 3 lines.
        ops::toggle_line_comment(&mut buf, &mut sels, "//");
        assert_eq!(buf.text(), "// 1_foo\n// 2_bar\n// 3_baz");
        ops::toggle_line_comment(&mut buf, &mut sels, "//");
        assert_eq!(buf.text(), "1_foo\n2_bar\n3_baz");
    }

    #[test]
    fn autopair_and_newline_indent() {
        let mut buf = Buffer::from_text("fn main() ");
        let mut sels = Selections::single(Selection::cursor(10));
        ops::insert_with_autopair(&mut buf, &mut sels, "{");
        assert_eq!(buf.text(), "fn main() {}");
        assert_eq!(sels.primary().head, 11);

        ops::newline(&mut buf, &mut sels);
        assert_eq!(buf.text(), "fn main() {\n    }");
        assert_eq!(sels.primary().head, 16);
    }

    #[test]
    fn search_bracket_and_word() {
        let buf = Buffer::from_text("fn foo(a: [i32; 2]) { foo_bar }");
        let plain = buf.find_all(
            "foo",
            SearchOptions {
                case_sensitive: true,
                whole_word: true,
                regex: false,
            },
        );
        assert_eq!(plain, vec![3..6]);

        let re = buf.find_all(
            r"foo\w*",
            SearchOptions {
                case_sensitive: true,
                whole_word: false,
                regex: true,
            },
        );
        assert_eq!(re, vec![3..6, 22..29]);

        assert_eq!(buf.matching_bracket(6), Some(18));
        assert_eq!(buf.matching_bracket(18), Some(6));
        assert_eq!(buf.word_range_at(24), Some(22..29));
    }

    #[test]
    fn line_ops_and_select_next_occurrence() {
        let mut buf = Buffer::from_text("alpha\nbeta\ngamma");
        let mut sels = Selections::single(Selection::cursor(6)); // on "beta"

        ops::move_lines_up(&mut buf, &mut sels);
        assert_eq!(buf.text(), "beta\nalpha\ngamma");
        ops::move_lines_down(&mut buf, &mut sels);
        assert_eq!(buf.text(), "alpha\nbeta\ngamma");

        ops::duplicate_lines(&mut buf, &mut sels);
        assert_eq!(buf.text(), "alpha\nbeta\nbeta\ngamma");

        ops::delete_lines(&mut buf, &mut sels);
        assert_eq!(buf.text(), "alpha\nbeta\ngamma");

        // select_next_occurrence wraps and selects all occurrences
        let mut buf2 = Buffer::from_text("foo bar foo baz foo");
        let mut s2 = Selections::single(Selection::cursor(1));
        ops::select_next_occurrence(&buf2, &mut s2);
        assert_eq!(s2.len(), 1);
        assert_eq!(s2.primary().range(), 0..3);
        ops::select_next_occurrence(&buf2, &mut s2);
        assert_eq!(s2.len(), 2);
        ops::select_next_occurrence(&buf2, &mut s2);
        assert_eq!(s2.len(), 3);
        ops::insert(&mut buf2, &mut s2, "qux");
        assert_eq!(buf2.text(), "qux bar qux baz qux");
    }
}
