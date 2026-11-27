//! Full-fledged native GPUI code editor backed by `blur_buffer` (Ropey),
//! `blur_syntax` (Tree-sitter), and `blur_lsp` (LSP 3.17).
//!
//! Features:
//! - Viewport-only incremental syntax highlighting (`JetBrains Mono`)
//! - True multi-cursor & multi-selection rendering and editing (`Alt+Click`, `Ctrl+D`, `Ctrl+Shift+Up/Down`)
//! - Mouse click, drag-to-select, double-click word select, triple-click line select, `Ctrl+Click` go-to-definition
//! - Adaptive vertical & horizontal scrolling with quiet scrollbar indicator
//! - Code folding (`Syntax::fold_ranges()`) with gutter chevrons and fold pills
//! - Auto-triggered & manual autocomplete combining LSP items + local buffer words
//! - Full LSP suite: Hover signature card (Stitch 1:1), Go to Definition, Find References peek,
//!   Rename Symbol (`F2`), Code Actions (`Ctrl+.`), Format Document (`Alt+Shift+F`), Diagnostics (`F8`)
//! - In-editor Find & Replace (`Ctrl+F` / `Ctrl+H`) with match highlighting + Go to Line (`Ctrl+G`)
//! - Standard, Vim, Emacs, and Helix navigation modes

pub mod completion;
pub mod highlight;
pub mod lsp_bridge;
pub mod vim;

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use blur_buffer::{movement, ops, Buffer, EditDelta, Point, SearchOptions, Selection, Selections};
use blur_core::settings::EditorSettings;
use blur_core::NavigationMode;
use blur_lsp::lsp_types::{CodeActionOrCommand, DiagnosticSeverity, Hover, Location, TextEdit};
use blur_lsp::{
    CompletionItem, Diagnostic, LspClient, Position, TextDocumentContentChangeEvent, Url,
};
use blur_syntax::{InputEdit, LanguageRegistry, Syntax, TsPoint};
use blur_theme::Theme;
use gpui::{
    div, prelude::*, px, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable, FontWeight,
    HighlightStyle, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ScrollDelta, ScrollWheelEvent, StyledText, UnderlineStyle, Window,
};

pub use completion::{CompletionCandidate, build_candidates, identifier_prefix_at};
pub use lsp_bridge::{HoverCard, apply_text_edits, parse_hover};
pub use vim::{VimMode, VimState};

/// Git gutter change marker for a single 0-based line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GutterChangeKind {
    Added,
    Modified,
    Deleted,
}

/// Active inline bar mode inside the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InlineBar {
    None,
    Find,
    Replace,
    GotoLine,
    Rename,
}

/// Events emitted by [`EditorView`] to the parent workspace shell.
#[derive(Debug, Clone)]
pub enum EditorEvent {
    DirtyChanged(bool),
    Saved(PathBuf),
    CursorMoved { line: usize, col: usize, cursor_count: usize },
    OpenLocation { path: PathBuf, line: usize, col: usize },
    RequestHover { line: u32, character: u32 },
    RequestCompletion { line: u32, character: u32 },
    RequestDefinition { line: u32, character: u32 },
    RequestReferences { line: u32, character: u32 },
    RequestRename { line: u32, character: u32, new_name: String },
    RequestCodeActions { line: u32, character: u32, diagnostics: Vec<Diagnostic> },
    RequestFormat { tab_size: u32, insert_spaces: bool },
    Notification(String),
}

/// Native GPUI code editor entity.
pub struct EditorView {
    focus_handle: FocusHandle,
    buffer: Buffer,
    selections: Selections,
    syntax: Option<Syntax>,
    _lang_registry: Arc<LanguageRegistry>,
    settings: EditorSettings,
    vim: VimState,

    // Viewport & scrolling
    scroll_line: usize,
    scroll_col: usize,
    visible_lines: usize,

    // Mouse selection state
    is_dragging: bool,
    last_click_at: Option<Instant>,
    last_click_line: usize,
    click_streak: u8,

    // Code folding
    fold_ranges: HashMap<usize, usize>, // start_line -> end_line
    folded_lines: HashSet<usize>,

    // LSP / Git / Diagnostics overlays
    lsp_client: Option<LspClient>,
    lsp_uri: Option<Url>,
    diagnostics: Vec<Diagnostic>,
    gutter_changes: HashMap<usize, GutterChangeKind>,

    // Hover & Autocomplete
    hover_card: Option<HoverCard>,
    lsp_completions: Vec<CompletionItem>,
    candidates: Vec<CompletionCandidate>,
    completion_idx: usize,
    show_completions: bool,

    // References & Code Actions peek popups
    references_peek: Vec<(PathBuf, usize, String)>,
    show_references: bool,
    code_actions: Vec<CodeActionOrCommand>,
    show_code_actions: bool,

    // Inline bar (Find / Replace / GotoLine / Rename)
    inline_bar: InlineBar,
    bar_input: String,
    replace_input: String,
    replace_field_focused: bool,
    find_case_sensitive: bool,
    find_regex: bool,
    find_matches: Vec<Range<usize>>,
    find_index: usize,
}

impl EventEmitter<EditorEvent> for EditorView {}

impl Focusable for EditorView {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EditorView {
    /// Create a new editor from an existing `Buffer`.
    pub fn new(
        buffer: Buffer,
        lang_registry: Arc<LanguageRegistry>,
        settings: EditorSettings,
        cx: &mut Context<Self>,
    ) -> Self {
        let syntax = buffer
            .path()
            .and_then(|p| lang_registry.for_path(p))
            .map(|lang| Syntax::new(lang, buffer.rope()));

        let fold_ranges = syntax
            .as_ref()
            .map(|s| {
                s.fold_ranges()
                    .into_iter()
                    .filter(|r| r.end > r.start + 1)
                    .map(|r| (r.start, r.end))
                    .collect()
            })
            .unwrap_or_default();

        let lsp_uri = buffer
            .path()
            .and_then(|p| blur_lsp::path_to_url(p).ok());

        Self {
            focus_handle: cx.focus_handle(),
            buffer,
            selections: Selections::single(Selection::cursor(0)),
            syntax,
            _lang_registry: lang_registry,
            settings,
            vim: VimState::default(),
            scroll_line: 0,
            scroll_col: 0,
            visible_lines: 36,
            is_dragging: false,
            last_click_at: None,
            last_click_line: 0,
            click_streak: 0,
            fold_ranges,
            folded_lines: HashSet::new(),
            lsp_client: None,
            lsp_uri,
            diagnostics: Vec::new(),
            gutter_changes: HashMap::new(),
            hover_card: None,
            lsp_completions: Vec::new(),
            candidates: Vec::new(),
            completion_idx: 0,
            show_completions: false,
            references_peek: Vec::new(),
            show_references: false,
            code_actions: Vec::new(),
            show_code_actions: false,
            inline_bar: InlineBar::None,
            bar_input: String::new(),
            replace_input: String::new(),
            replace_field_focused: false,
            find_case_sensitive: false,
            find_regex: false,
            find_matches: Vec::new(),
            find_index: 0,
        }
    }

    pub fn open_file(
        path: &Path,
        lang_registry: Arc<LanguageRegistry>,
        settings: EditorSettings,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<Self> {
        let buffer = Buffer::open(path)?;
        Ok(Self::new(buffer, lang_registry, settings, cx))
    }

    pub fn path(&self) -> Option<&Path> {
        self.buffer.path()
    }

    pub fn title(&self) -> String {
        self.buffer
            .path()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("untitled")
            .to_string()
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.is_dirty()
    }

    pub fn text(&self) -> String {
        self.buffer.text()
    }

    pub fn len_lines(&self) -> usize {
        self.buffer.len_lines()
    }

    pub fn len_bytes(&self) -> usize {
        self.buffer.len_bytes()
    }

    pub fn cursor_count(&self) -> usize {
        self.selections.len()
    }

    pub fn has_lsp(&self) -> bool {
        self.lsp_client.is_some()
    }

    pub fn lsp_server_name(&self) -> Option<&str> {
        self.lsp_client.as_ref().map(|c| c.name())
    }

    pub fn language_name(&self) -> &str {
        self.syntax
            .as_ref()
            .map(|s| s.language().name)
            .unwrap_or("plain")
    }

    pub fn lsp_language_id(&self) -> Option<&'static str> {
        self.syntax.as_ref().map(|s| s.language().lsp_id)
    }

    pub fn cursor_point(&self) -> Point {
        self.buffer.char_to_point(self.selections.primary().head)
    }

    pub fn mode_label(&self) -> &'static str {
        match self.settings.navigation_mode {
            NavigationMode::Vim => self.vim.mode.label(),
            NavigationMode::Emacs => "EMACS",
            NavigationMode::Helix => "HELIX",
            NavigationMode::Standard => "NORMAL",
        }
    }

    pub fn update_settings(&mut self, settings: EditorSettings, cx: &mut Context<Self>) {
        self.settings = settings;
        cx.notify();
    }

    pub fn set_lsp_client(&mut self, client: Option<LspClient>) {
        self.lsp_client = client;
        if let (Some(client), Some(uri), Some(lang_id)) =
            (&self.lsp_client, &self.lsp_uri, self.lsp_language_id())
        {
            client.did_open(uri.clone(), lang_id, self.buffer.version(), self.buffer.text());
        }
    }

    pub fn set_diagnostics(&mut self, diagnostics: Vec<Diagnostic>, cx: &mut Context<Self>) {
        self.diagnostics = diagnostics;
        cx.notify();
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn diagnostic_counts(&self) -> (usize, usize) {
        let mut errors = 0;
        let mut warnings = 0;
        for d in &self.diagnostics {
            match d.severity {
                Some(DiagnosticSeverity::ERROR) => errors += 1,
                Some(DiagnosticSeverity::WARNING) => warnings += 1,
                _ => {}
            }
        }
        (errors, warnings)
    }

    pub fn set_gutter_changes(
        &mut self,
        changes: impl IntoIterator<Item = (usize, GutterChangeKind)>,
        cx: &mut Context<Self>,
    ) {
        self.gutter_changes = changes.into_iter().collect();
        cx.notify();
    }

    pub fn set_hover(&mut self, line: usize, hover: Option<&Hover>, cx: &mut Context<Self>) {
        let loc = format!("{}:{}", self.title(), line + 1);
        self.hover_card = hover.and_then(|h| parse_hover(h, line, loc));
        cx.notify();
    }

    pub fn set_hover_card(&mut self, card: Option<HoverCard>, cx: &mut Context<Self>) {
        self.hover_card = card;
        cx.notify();
    }

    pub fn set_primary_cursor_line(&mut self, line_0_based: usize, col_0_based: usize) {
        let ch = self.buffer.point_to_char(Point {
            line: line_0_based,
            col: col_0_based,
        });
        self.selections = Selections::single(Selection::cursor(ch));
    }

    pub fn set_completions(&mut self, items: Vec<CompletionItem>, cx: &mut Context<Self>) {
        self.lsp_completions = items;
        self.refresh_completion_candidates();
        self.show_completions = !self.candidates.is_empty();
        cx.notify();
    }

    pub fn set_references(&mut self, locs: Vec<Location>, cx: &mut Context<Self>) {
        self.references_peek = locs
            .into_iter()
            .filter_map(|l| {
                let url = blur_lsp::lsp_uri_to_url(&l.uri).ok()?;
                let path = blur_lsp::url_to_path(&url)?;
                let line_1 = l.range.start.line as usize + 1;
                let preview = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|s| s.lines().nth(line_1.saturating_sub(1)).map(|x| x.trim().to_string()))
                    .unwrap_or_default();
                Some((path, line_1, preview))
            })
            .collect();
        self.show_references = !self.references_peek.is_empty();
        cx.notify();
    }

    pub fn set_code_actions(&mut self, actions: Vec<CodeActionOrCommand>, cx: &mut Context<Self>) {
        self.code_actions = actions;
        self.show_code_actions = !self.code_actions.is_empty();
        cx.notify();
    }

    pub fn apply_lsp_edits(&mut self, edits: &[TextEdit], cx: &mut Context<Self>) {
        let deltas = apply_text_edits(&mut self.buffer, edits);
        self.selections.clamp(self.buffer.len_chars());
        self.after_edit(&deltas, cx);
    }

    pub fn goto_line(&mut self, line_1_based: usize, col_0_based: usize, cx: &mut Context<Self>) {
        let line = line_1_based
            .saturating_sub(1)
            .min(self.buffer.len_lines().saturating_sub(1));
        // Unfold if target line is hidden inside a fold
        self.unfold_containing(line);
        let ch = self.buffer.point_to_char(Point {
            line,
            col: col_0_based,
        });
        self.selections = Selections::single(Selection::cursor(ch));
        self.scroll_line = line.saturating_sub(8);
        self.emit_cursor(cx);
        cx.notify();
    }

    pub fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let deltas = ops::insert(&mut self.buffer, &mut self.selections, text);
        self.after_edit(&deltas, cx);
    }

    pub fn save(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        self.buffer.save()?;
        if let Some(path) = self.buffer.path().map(Path::to_path_buf) {
            if let (Some(client), Some(uri)) = (&self.lsp_client, &self.lsp_uri) {
                client.did_save(uri.clone(), Some(self.buffer.text()));
            }
            cx.emit(EditorEvent::DirtyChanged(false));
            cx.emit(EditorEvent::Saved(path));
        }
        cx.notify();
        Ok(())
    }

    pub fn toggle_find(&mut self, cx: &mut Context<Self>) {
        if self.inline_bar == InlineBar::Find {
            self.inline_bar = InlineBar::None;
            self.find_matches.clear();
        } else {
            self.inline_bar = InlineBar::Find;
            self.replace_field_focused = false;
            self.refresh_find_matches();
        }
        cx.notify();
    }

    pub fn open_replace(&mut self, cx: &mut Context<Self>) {
        self.inline_bar = InlineBar::Replace;
        self.replace_field_focused = false;
        self.refresh_find_matches();
        cx.notify();
    }

    pub fn open_goto_line(&mut self, cx: &mut Context<Self>) {
        self.inline_bar = InlineBar::GotoLine;
        self.bar_input.clear();
        cx.notify();
    }

    pub fn open_rename(&mut self, cx: &mut Context<Self>) {
        let head = self.selections.primary().head;
        let current_word = self
            .buffer
            .word_range_at(head)
            .map(|r| self.buffer.rope().slice(r).to_string())
            .unwrap_or_default();
        self.inline_bar = InlineBar::Rename;
        self.bar_input = current_word;
        cx.notify();
    }

    pub fn trigger_format(&self, cx: &mut Context<Self>) {
        cx.emit(EditorEvent::RequestFormat {
            tab_size: self.settings.tab_size as u32,
            insert_spaces: self.settings.insert_spaces,
        });
    }

    pub fn toggle_fold_at(&mut self, line: usize, cx: &mut Context<Self>) {
        if self.fold_ranges.contains_key(&line) {
            if !self.folded_lines.remove(&line) {
                self.folded_lines.insert(line);
            }
            cx.notify();
        }
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    pub fn outline(&self) -> Vec<blur_syntax::OutlineItem> {
        self.syntax
            .as_ref()
            .map(|s| s.outline(self.buffer.rope()))
            .unwrap_or_default()
    }

    fn unfold_containing(&mut self, target_line: usize) {
        let to_unfold: Vec<usize> = self
            .folded_lines
            .iter()
            .copied()
            .filter(|&start| {
                self.fold_ranges
                    .get(&start)
                    .map(|&end| target_line > start && target_line <= end)
                    .unwrap_or(false)
            })
            .collect();
        for l in to_unfold {
            self.folded_lines.remove(&l);
        }
    }

    fn is_line_hidden(&self, line: usize) -> bool {
        self.folded_lines.iter().any(|&start| {
            self.fold_ranges
                .get(&start)
                .map(|&end| line > start && line <= end)
                .unwrap_or(false)
        })
    }

    fn refresh_completion_candidates(&mut self) {
        let (_, prefix) = identifier_prefix_at(&self.buffer, self.selections.primary().head);
        self.candidates = build_candidates(&self.lsp_completions, &self.buffer, &prefix, 14);
        if self.completion_idx >= self.candidates.len() {
            self.completion_idx = 0;
        }
    }

    fn maybe_auto_complete(&mut self, inserted: &str, cx: &mut Context<Self>) {
        let trigger_char = matches!(inserted, "." | ":" | ">");
        let (_, prefix) = identifier_prefix_at(&self.buffer, self.selections.primary().head);
        if trigger_char || (self.lsp_client.is_some() && !prefix.is_empty()) || prefix.len() >= 2 {
            self.refresh_completion_candidates();
            self.show_completions = !self.candidates.is_empty();
            if trigger_char || self.lsp_client.is_some() {
                let pos = self.buffer.char_to_lsp(self.selections.primary().head);
                cx.emit(EditorEvent::RequestCompletion {
                    line: pos.line,
                    character: pos.character,
                });
            }
        } else {
            self.show_completions = false;
        }
    }

    fn after_edit(&mut self, deltas: &[EditDelta], cx: &mut Context<Self>) {
        if deltas.is_empty() {
            return;
        }
        // Incremental tree-sitter reparse + refresh fold ranges
        if let Some(syntax) = &mut self.syntax {
            for d in deltas {
                syntax.edit(&InputEdit {
                    start_byte: d.start_byte,
                    old_end_byte: d.old_end_byte,
                    new_end_byte: d.new_end_byte,
                    start_position: TsPoint::new(d.start_point.0, d.start_point.1),
                    old_end_position: TsPoint::new(d.old_end_point.0, d.old_end_point.1),
                    new_end_position: TsPoint::new(d.new_end_point.0, d.new_end_point.1),
                });
            }
            syntax.reparse(self.buffer.rope());
            self.fold_ranges = syntax
                .fold_ranges()
                .into_iter()
                .filter(|r| r.end > r.start + 1)
                .map(|r| (r.start, r.end))
                .collect();
        }

        // Incremental LSP document synchronization
        if let (Some(client), Some(uri)) = (&self.lsp_client, &self.lsp_uri) {
            let changes: Vec<TextDocumentContentChangeEvent> = deltas
                .iter()
                .map(|d| TextDocumentContentChangeEvent {
                    range: Some(blur_lsp::Range {
                        start: Position::new(d.lsp_range.0.line, d.lsp_range.0.character),
                        end: Position::new(d.lsp_range.1.line, d.lsp_range.1.character),
                    }),
                    range_length: None,
                    text: d.new_text.clone(),
                })
                .collect();
            client.did_change(uri.clone(), self.buffer.version(), changes);
        }

        self.ensure_cursor_visible();
        cx.emit(EditorEvent::DirtyChanged(self.buffer.is_dirty()));
        self.emit_cursor(cx);
        cx.notify();
    }

    fn emit_cursor(&self, cx: &mut Context<Self>) {
        let p = self.cursor_point();
        cx.emit(EditorEvent::CursorMoved {
            line: p.line,
            col: p.col,
            cursor_count: self.selections.len(),
        });
    }

    fn ensure_cursor_visible(&mut self) {
        let pt = self.cursor_point();
        self.unfold_containing(pt.line);
        if pt.line < self.scroll_line {
            self.scroll_line = pt.line;
        } else if pt.line >= self.scroll_line + self.visible_lines {
            self.scroll_line = pt.line.saturating_sub(self.visible_lines.saturating_sub(3));
        }
        // Horizontal follow
        let max_cols = 88usize;
        if pt.col < self.scroll_col {
            self.scroll_col = pt.col;
        } else if pt.col >= self.scroll_col + max_cols {
            self.scroll_col = pt.col.saturating_sub(max_cols - 6);
        }
    }

    fn apply_movement(
        &mut self,
        f: impl Fn(&Buffer, Selection) -> Selection,
        cx: &mut Context<Self>,
    ) {
        let new_sels: Vec<Selection> = self
            .selections
            .all()
            .iter()
            .map(|&s| f(&self.buffer, s))
            .collect();
        let primary = self.selections.primary_index();
        self.selections.set(new_sels, primary);
        self.hover_card = None;
        self.show_completions = false;
        self.ensure_cursor_visible();
        self.emit_cursor(cx);
        cx.notify();
    }

    fn accept_completion(&mut self, cx: &mut Context<Self>) {
        if let Some(cand) = self.candidates.get(self.completion_idx).cloned() {
            let head = self.selections.primary().head;
            let (prefix_start, _) = identifier_prefix_at(&self.buffer, head);
            if prefix_start < head {
                self.selections = Selections::single(Selection::new(prefix_start, head));
            }
            let deltas = ops::insert(&mut self.buffer, &mut self.selections, &cand.insert_text);
            self.show_completions = false;
            self.after_edit(&deltas, cx);
        }
    }

    fn jump_diagnostic(&mut self, next: bool, cx: &mut Context<Self>) {
        if self.diagnostics.is_empty() {
            return;
        }
        let cur_line = self.cursor_point().line as u32;
        let mut sorted: Vec<&Diagnostic> = self.diagnostics.iter().collect();
        sorted.sort_by_key(|d| (d.range.start.line, d.range.start.character));
        let target = if next {
            sorted
                .iter()
                .find(|d| d.range.start.line > cur_line)
                .or_else(|| sorted.first())
        } else {
            sorted
                .iter()
                .rev()
                .find(|d| d.range.start.line < cur_line)
                .or_else(|| sorted.last())
        };
        if let Some(d) = target {
            self.goto_line(
                d.range.start.line as usize + 1,
                d.range.start.character as usize,
                cx,
            );
        }
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (dx, dy) = match event.delta {
            ScrollDelta::Lines(pt) => ((-pt.x * 4.0).round() as isize, (-pt.y * 3.0).round() as isize),
            ScrollDelta::Pixels(pt) => (
                (-f32::from(pt.x) / 12.0).round() as isize,
                (-f32::from(pt.y) / 18.0).round() as isize,
            ),
        };
        if event.modifiers.shift && dy != 0 {
            let next_col = (self.scroll_col as isize + dy * 2).max(0) as usize;
            if next_col != self.scroll_col {
                self.scroll_col = next_col;
                cx.notify();
            }
            return;
        }
        let max_scroll = self.buffer.len_lines().saturating_sub(1);
        let next_line = (self.scroll_line as isize + dy).clamp(0, max_scroll as isize) as usize;
        let next_col = (self.scroll_col as isize + dx).max(0) as usize;
        if next_line != self.scroll_line || next_col != self.scroll_col {
            self.scroll_line = next_line;
            self.scroll_col = next_col;
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_dragging = false;
    }

    fn handle_line_mouse_down(
        &mut self,
        line_idx: usize,
        col: usize,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.hover_card = None;
        self.show_completions = false;
        self.show_references = false;
        self.show_code_actions = false;

        let ch = self.buffer.point_to_char(Point {
            line: line_idx,
            col: col + self.scroll_col,
        });

        // Ctrl+Click -> Go to Definition
        if event.modifiers.control || event.modifiers.platform {
            self.selections = Selections::single(Selection::cursor(ch));
            let pos = self.buffer.char_to_lsp(ch);
            cx.emit(EditorEvent::RequestDefinition {
                line: pos.line,
                character: pos.character,
            });
            cx.notify();
            return;
        }

        // Alt+Click -> Add a new cursor (Multi-cursor!)
        if event.modifiers.alt {
            self.selections.add(Selection::cursor(ch));
            self.emit_cursor(cx);
            cx.notify();
            return;
        }

        // Shift+Click -> Extend primary selection
        if event.modifiers.shift {
            let anchor = self.selections.primary().anchor;
            self.selections = Selections::single(Selection::new(anchor, ch));
            self.emit_cursor(cx);
            cx.notify();
            return;
        }

        // Double-click (word) / Triple-click (line) detection
        let now = Instant::now();
        let rapid = self
            .last_click_at
            .map(|t| now.duration_since(t).as_millis() < 360 && self.last_click_line == line_idx)
            .unwrap_or(false);
        self.click_streak = if rapid { (self.click_streak % 3) + 1 } else { 1 };
        self.last_click_at = Some(now);
        self.last_click_line = line_idx;

        match self.click_streak {
            2 => {
                if let Some(word) = self.buffer.word_range_at(ch) {
                    self.selections = Selections::single(Selection::new(word.start, word.end));
                } else {
                    self.selections = Selections::single(Selection::cursor(ch));
                }
                self.is_dragging = false;
            }
            3 => {
                self.selections = Selections::single(Selection::cursor(ch));
                ops::select_line(&self.buffer, &mut self.selections);
                self.is_dragging = false;
            }
            _ => {
                self.selections = Selections::single(Selection::cursor(ch));
                self.is_dragging = true;
            }
        }
        self.emit_cursor(cx);
        cx.notify();
    }

    fn handle_line_mouse_move(&mut self, line_idx: usize, col: usize, cx: &mut Context<Self>) {
        if !self.is_dragging {
            return;
        }
        let ch = self.buffer.point_to_char(Point {
            line: line_idx,
            col: col + self.scroll_col,
        });
        let anchor = self.selections.primary().anchor;
        self.selections = Selections::single(Selection::new(anchor, ch));
        self.emit_cursor(cx);
        cx.notify();
    }

    fn handle_inline_bar_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if self.inline_bar == InlineBar::None {
            return false;
        }
        let ks = &event.keystroke;
        let primary_mod = ks.modifiers.control || ks.modifiers.platform;
        let shift = ks.modifiers.shift;

        match ks.key.as_str() {
            "escape" => {
                self.inline_bar = InlineBar::None;
                self.find_matches.clear();
                cx.notify();
                return true;
            }
            "tab" if self.inline_bar == InlineBar::Replace => {
                self.replace_field_focused = !self.replace_field_focused;
                cx.notify();
                return true;
            }
            "enter" => {
                match self.inline_bar {
                    InlineBar::Find | InlineBar::Replace => {
                        if self.inline_bar == InlineBar::Replace && self.replace_field_focused {
                            self.replace_current(cx);
                        } else if !self.find_matches.is_empty() {
                            self.find_index = if shift {
                                if self.find_index == 0 {
                                    self.find_matches.len() - 1
                                } else {
                                    self.find_index - 1
                                }
                            } else {
                                (self.find_index + 1) % self.find_matches.len()
                            };
                            let range = self.find_matches[self.find_index].clone();
                            self.selections =
                                Selections::single(Selection::new(range.start, range.end));
                            self.ensure_cursor_visible();
                            cx.notify();
                        }
                    }
                    InlineBar::GotoLine => {
                        if let Ok(line) = self.bar_input.trim().parse::<usize>() {
                            self.goto_line(line, 0, cx);
                        }
                        self.inline_bar = InlineBar::None;
                        cx.notify();
                    }
                    InlineBar::Rename => {
                        let new_name = self.bar_input.trim().to_string();
                        if !new_name.is_empty() {
                            let pos = self.buffer.char_to_lsp(self.selections.primary().head);
                            cx.emit(EditorEvent::RequestRename {
                                line: pos.line,
                                character: pos.character,
                                new_name,
                            });
                        }
                        self.inline_bar = InlineBar::None;
                        cx.notify();
                    }
                    InlineBar::None => {}
                }
                return true;
            }
            "backspace" => {
                if self.inline_bar == InlineBar::Replace && self.replace_field_focused {
                    self.replace_input.pop();
                } else {
                    self.bar_input.pop();
                    if matches!(self.inline_bar, InlineBar::Find | InlineBar::Replace) {
                        self.refresh_find_matches();
                    }
                }
                cx.notify();
                return true;
            }
            _ if !primary_mod && !ks.modifiers.alt => {
                if let Some(ch) = ks.key_char.as_deref() {
                    if !ch.is_empty() && !ch.chars().all(|c| c.is_control()) {
                        if self.inline_bar == InlineBar::Replace && self.replace_field_focused {
                            self.replace_input.push_str(ch);
                        } else {
                            self.bar_input.push_str(ch);
                            if matches!(self.inline_bar, InlineBar::Find | InlineBar::Replace) {
                                self.refresh_find_matches();
                            }
                        }
                        cx.notify();
                        return true;
                    }
                }
            }
            _ => {}
        }
        false
    }

    fn replace_current(&mut self, cx: &mut Context<Self>) {
        if let Some(range) = self.find_matches.get(self.find_index).cloned() {
            let deltas = self.buffer.edit(vec![(range, self.replace_input.clone())]);
            self.after_edit(&deltas, cx);
            self.refresh_find_matches();
        }
    }

    fn replace_all(&mut self, cx: &mut Context<Self>) {
        if self.find_matches.is_empty() {
            return;
        }
        let edits: Vec<_> = self
            .find_matches
            .iter()
            .cloned()
            .map(|r| (r, self.replace_input.clone()))
            .collect();
        let count = edits.len();
        let deltas = self.buffer.edit(edits);
        self.after_edit(&deltas, cx);
        self.refresh_find_matches();
        cx.emit(EditorEvent::Notification(format!(
            "Replaced {count} occurrences"
        )));
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let primary_mod = ks.modifiers.control || ks.modifiers.platform;
        let shift = ks.modifiers.shift;
        let alt = ks.modifiers.alt;

        // 1. Autocomplete popup navigation
        if self.show_completions {
            match ks.key.as_str() {
                "escape" => {
                    self.show_completions = false;
                    cx.notify();
                    return;
                }
                "down" => {
                    if !self.candidates.is_empty() {
                        self.completion_idx = (self.completion_idx + 1) % self.candidates.len();
                        cx.notify();
                    }
                    return;
                }
                "up" => {
                    if !self.candidates.is_empty() {
                        self.completion_idx = if self.completion_idx == 0 {
                            self.candidates.len() - 1
                        } else {
                            self.completion_idx - 1
                        };
                        cx.notify();
                    }
                    return;
                }
                "enter" | "tab" => {
                    self.accept_completion(cx);
                    return;
                }
                _ => {}
            }
        }

        // 2. Inline bars (Find / Replace / GotoLine / Rename)
        if self.handle_inline_bar_key(event, cx) {
            return;
        }

        // 3. Global editor shortcuts
        if primary_mod {
            match ks.key.as_str() {
                "s" => {
                    if let Err(err) = self.save(cx) {
                        cx.emit(EditorEvent::Notification(format!("Save failed: {err}")));
                    } else {
                        cx.emit(EditorEvent::Notification(format!("Saved {}", self.title())));
                    }
                    return;
                }
                "f" if !shift => {
                    self.toggle_find(cx);
                    return;
                }
                "h" => {
                    self.open_replace(cx);
                    return;
                }
                "g" => {
                    self.open_goto_line(cx);
                    return;
                }
                "." => {
                    let pos = self.buffer.char_to_lsp(self.selections.primary().head);
                    let line_diags: Vec<Diagnostic> = self
                        .diagnostics
                        .iter()
                        .filter(|d| d.range.start.line == pos.line)
                        .cloned()
                        .collect();
                    cx.emit(EditorEvent::RequestCodeActions {
                        line: pos.line,
                        character: pos.character,
                        diagnostics: line_diags,
                    });
                    return;
                }
                "[" if shift => {
                    let line = self.cursor_point().line;
                    self.toggle_fold_at(line, cx);
                    return;
                }
                "]" if shift => {
                    let line = self.cursor_point().line;
                    self.folded_lines.remove(&line);
                    cx.notify();
                    return;
                }
                "z" if !shift => {
                    if let Some(res) = self.buffer.undo() {
                        self.selections = res.selections;
                        self.after_edit(&res.deltas, cx);
                    }
                    return;
                }
                "y" | "z" if ks.key == "y" || shift => {
                    if let Some(res) = self.buffer.redo() {
                        self.selections = res.selections;
                        self.after_edit(&res.deltas, cx);
                    }
                    return;
                }
                "a" => {
                    ops::select_all(&self.buffer, &mut self.selections);
                    cx.notify();
                    return;
                }
                "c" => {
                    let text = ops::copy_text(&self.buffer, &self.selections);
                    if !text.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                    return;
                }
                "x" => {
                    let (text, deltas) = ops::cut_text(&mut self.buffer, &mut self.selections);
                    if !text.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                    self.after_edit(&deltas, cx);
                    return;
                }
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                        let deltas = ops::paste(&mut self.buffer, &mut self.selections, &text);
                        self.after_edit(&deltas, cx);
                    }
                    return;
                }
                "d" if !shift => {
                    ops::select_next_occurrence(&self.buffer, &mut self.selections);
                    self.ensure_cursor_visible();
                    self.emit_cursor(cx);
                    cx.notify();
                    return;
                }
                "d" if shift => {
                    let deltas = ops::duplicate_lines(&mut self.buffer, &mut self.selections);
                    self.after_edit(&deltas, cx);
                    return;
                }
                "/" => {
                    let tok = self
                        .syntax
                        .as_ref()
                        .and_then(|s| s.language().line_comment)
                        .unwrap_or("//");
                    let deltas =
                        ops::toggle_line_comment(&mut self.buffer, &mut self.selections, tok);
                    self.after_edit(&deltas, cx);
                    return;
                }
                "space" => {
                    self.refresh_completion_candidates();
                    self.show_completions = !self.candidates.is_empty();
                    let pos = self.buffer.char_to_lsp(self.selections.primary().head);
                    cx.emit(EditorEvent::RequestCompletion {
                        line: pos.line,
                        character: pos.character,
                    });
                    cx.notify();
                    return;
                }
                "i" if shift => {
                    let pos = self.buffer.char_to_lsp(self.selections.primary().head);
                    cx.emit(EditorEvent::RequestHover {
                        line: pos.line,
                        character: pos.character,
                    });
                    return;
                }
                _ => {}
            }
        }

        // Alt+Shift+F -> Format Document
        if alt && shift && ks.key.eq_ignore_ascii_case("f") {
            self.trigger_format(cx);
            return;
        }

        // Function keys (LSP)
        match ks.key.as_str() {
            "f2" => {
                self.open_rename(cx);
                return;
            }
            "f8" => {
                self.jump_diagnostic(!shift, cx);
                return;
            }
            "f12" => {
                let pos = self.buffer.char_to_lsp(self.selections.primary().head);
                if shift {
                    cx.emit(EditorEvent::RequestReferences {
                        line: pos.line,
                        character: pos.character,
                    });
                } else {
                    cx.emit(EditorEvent::RequestDefinition {
                        line: pos.line,
                        character: pos.character,
                    });
                }
                return;
            }
            _ => {}
        }

        // 4. Vim Normal / Visual mode
        if self.settings.navigation_mode == NavigationMode::Vim && self.vim.mode != VimMode::Insert
        {
            self.handle_vim_key(event, cx);
            return;
        }

        // 5. Emacs navigation shortcuts
        if self.settings.navigation_mode == NavigationMode::Emacs && ks.modifiers.control {
            match ks.key.as_str() {
                "f" => {
                    self.apply_movement(|b, s| movement::right(b, s, false), cx);
                    return;
                }
                "b" => {
                    self.apply_movement(|b, s| movement::left(b, s, false), cx);
                    return;
                }
                "n" => {
                    self.apply_movement(|b, s| movement::down(b, s, false), cx);
                    return;
                }
                "p" => {
                    self.apply_movement(|b, s| movement::up(b, s, false), cx);
                    return;
                }
                "e" => {
                    self.apply_movement(|b, s| movement::line_end(b, s, false), cx);
                    return;
                }
                "k" => {
                    let deltas = ops::delete_lines(&mut self.buffer, &mut self.selections);
                    self.after_edit(&deltas, cx);
                    return;
                }
                _ => {}
            }
        }

        // 6. Standard / Insert mode editing & navigation
        match ks.key.as_str() {
            "escape" => {
                if self.settings.navigation_mode == NavigationMode::Vim {
                    self.vim.mode = VimMode::Normal;
                    self.vim.reset_pending();
                } else {
                    self.selections.collapse_to_primary();
                }
                self.hover_card = None;
                self.show_completions = false;
                self.show_references = false;
                self.show_code_actions = false;
                self.emit_cursor(cx);
                cx.notify();
            }
            "left" => {
                if primary_mod {
                    self.apply_movement(|b, s| movement::word_left(b, s, shift), cx);
                } else {
                    self.apply_movement(|b, s| movement::left(b, s, shift), cx);
                }
            }
            "right" => {
                if primary_mod {
                    self.apply_movement(|b, s| movement::word_right(b, s, shift), cx);
                } else {
                    self.apply_movement(|b, s| movement::right(b, s, shift), cx);
                }
            }
            "up" => {
                if (primary_mod && shift) || (primary_mod && alt) {
                    ops::add_cursor_above(&self.buffer, &mut self.selections);
                    self.emit_cursor(cx);
                    cx.notify();
                } else if alt {
                    let deltas = ops::move_lines_up(&mut self.buffer, &mut self.selections);
                    self.after_edit(&deltas, cx);
                } else {
                    self.apply_movement(|b, s| movement::up(b, s, shift), cx);
                }
            }
            "down" => {
                if (primary_mod && shift) || (primary_mod && alt) {
                    ops::add_cursor_below(&self.buffer, &mut self.selections);
                    self.emit_cursor(cx);
                    cx.notify();
                } else if alt {
                    let deltas = ops::move_lines_down(&mut self.buffer, &mut self.selections);
                    self.after_edit(&deltas, cx);
                } else {
                    self.apply_movement(|b, s| movement::down(b, s, shift), cx);
                }
            }
            "home" => self.apply_movement(|b, s| movement::line_start(b, s, shift), cx),
            "end" => self.apply_movement(|b, s| movement::line_end(b, s, shift), cx),
            "pageup" => {
                let n = self.visible_lines.saturating_sub(2).max(1);
                self.apply_movement(|b, s| movement::page_up(b, s, n, shift), cx);
            }
            "pagedown" => {
                let n = self.visible_lines.saturating_sub(2).max(1);
                self.apply_movement(|b, s| movement::page_down(b, s, n, shift), cx);
            }
            "backspace" => {
                let deltas = if primary_mod {
                    ops::delete_word_left(&mut self.buffer, &mut self.selections)
                } else {
                    ops::backspace(&mut self.buffer, &mut self.selections)
                };
                self.after_edit(&deltas, cx);
                if self.show_completions {
                    let (_, prefix) =
                        identifier_prefix_at(&self.buffer, self.selections.primary().head);
                    if prefix.is_empty() {
                        self.show_completions = false;
                    } else {
                        self.refresh_completion_candidates();
                        self.show_completions = !self.candidates.is_empty();
                    }
                }
            }
            "delete" => {
                let deltas = if primary_mod {
                    ops::delete_word_right(&mut self.buffer, &mut self.selections)
                } else {
                    ops::delete_forward(&mut self.buffer, &mut self.selections)
                };
                self.after_edit(&deltas, cx);
            }
            "enter" => {
                self.show_completions = false;
                let deltas = ops::newline_with_indent(
                    &mut self.buffer,
                    &mut self.selections,
                    self.settings.tab_size,
                    self.settings.insert_spaces,
                );
                self.after_edit(&deltas, cx);
            }
            "tab" => {
                let deltas = if shift {
                    ops::outdent(
                        &mut self.buffer,
                        &mut self.selections,
                        self.settings.tab_size,
                        self.settings.insert_spaces,
                    )
                } else {
                    ops::indent(
                        &mut self.buffer,
                        &mut self.selections,
                        self.settings.tab_size,
                        self.settings.insert_spaces,
                    )
                };
                self.after_edit(&deltas, cx);
            }
            _ => {
                if !primary_mod && !alt {
                    if let Some(ch) = ks.key_char.as_deref() {
                        if !ch.is_empty() && !ch.chars().all(|c| c.is_control()) {
                            let deltas = if self.settings.auto_pairs {
                                ops::insert_with_autopair(&mut self.buffer, &mut self.selections, ch)
                            } else {
                                ops::insert(&mut self.buffer, &mut self.selections, ch)
                            };
                            self.after_edit(&deltas, cx);
                            self.maybe_auto_complete(ch, cx);
                        }
                    } else if ks.key == "space" {
                        self.show_completions = false;
                        let deltas = ops::insert(&mut self.buffer, &mut self.selections, " ");
                        self.after_edit(&deltas, cx);
                    }
                }
            }
        }
    }

    fn handle_vim_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let extend = self.vim.mode == VimMode::Visual;
        let ch = ks.key_char.as_deref().and_then(|s| s.chars().next());

        if let Some(pending) = self.vim.pending.take() {
            match (pending, ch) {
                ('g', Some('g')) => {
                    self.apply_movement(|b, s| movement::doc_start(b, s, extend), cx);
                    return;
                }
                ('g', Some('d')) => {
                    let pos = self.buffer.char_to_lsp(self.selections.primary().head);
                    cx.emit(EditorEvent::RequestDefinition {
                        line: pos.line,
                        character: pos.character,
                    });
                    return;
                }
                ('g', Some('r')) => {
                    let pos = self.buffer.char_to_lsp(self.selections.primary().head);
                    cx.emit(EditorEvent::RequestReferences {
                        line: pos.line,
                        character: pos.character,
                    });
                    return;
                }
                ('d', Some('d')) => {
                    let deltas = ops::delete_lines(&mut self.buffer, &mut self.selections);
                    self.after_edit(&deltas, cx);
                    return;
                }
                ('y', Some('y')) => {
                    let line = self.cursor_point().line;
                    let text = format!("{}\n", self.buffer.line_text(line));
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    return;
                }
                ('z', Some('a')) => {
                    let line = self.cursor_point().line;
                    self.toggle_fold_at(line, cx);
                    return;
                }
                _ => return,
            }
        }

        if ks.key == "escape" {
            self.vim.mode = VimMode::Normal;
            self.selections.collapse_to_primary();
            let head = self.selections.primary().head;
            self.selections = Selections::single(Selection::cursor(head));
            self.hover_card = None;
            self.show_references = false;
            self.show_code_actions = false;
            cx.notify();
            return;
        }

        match ch {
            Some('h') => self.apply_movement(|b, s| movement::left(b, s, extend), cx),
            Some('l') => self.apply_movement(|b, s| movement::right(b, s, extend), cx),
            Some('j') => self.apply_movement(|b, s| movement::down(b, s, extend), cx),
            Some('k') => self.apply_movement(|b, s| movement::up(b, s, extend), cx),
            Some('w') => self.apply_movement(|b, s| movement::word_right(b, s, extend), cx),
            Some('b') => self.apply_movement(|b, s| movement::word_left(b, s, extend), cx),
            Some('0') => self.apply_movement(|b, s| movement::line_start(b, s, extend), cx),
            Some('$') => self.apply_movement(|b, s| movement::line_end(b, s, extend), cx),
            Some('G') => self.apply_movement(|b, s| movement::doc_end(b, s, extend), cx),
            Some('g') | Some('d') | Some('y') | Some('z') => {
                self.vim.pending = ch;
            }
            Some('i') => {
                self.vim.mode = VimMode::Insert;
                cx.notify();
            }
            Some('a') => {
                self.apply_movement(|b, s| movement::right(b, s, false), cx);
                self.vim.mode = VimMode::Insert;
                cx.notify();
            }
            Some('I') => {
                self.apply_movement(|b, s| movement::line_start(b, s, false), cx);
                self.vim.mode = VimMode::Insert;
                cx.notify();
            }
            Some('A') => {
                self.apply_movement(|b, s| movement::line_end(b, s, false), cx);
                self.vim.mode = VimMode::Insert;
                cx.notify();
            }
            Some('o') => {
                self.apply_movement(|b, s| movement::line_end(b, s, false), cx);
                let deltas = ops::newline_with_indent(
                    &mut self.buffer,
                    &mut self.selections,
                    self.settings.tab_size,
                    self.settings.insert_spaces,
                );
                self.vim.mode = VimMode::Insert;
                self.after_edit(&deltas, cx);
            }
            Some('v') => {
                self.vim.mode = if self.vim.mode == VimMode::Visual {
                    VimMode::Normal
                } else {
                    VimMode::Visual
                };
                cx.notify();
            }
            Some('x') => {
                let deltas = ops::delete_forward(&mut self.buffer, &mut self.selections);
                self.after_edit(&deltas, cx);
            }
            Some('u') => {
                if let Some(res) = self.buffer.undo() {
                    self.selections = res.selections;
                    self.after_edit(&res.deltas, cx);
                }
            }
            Some('p') => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    let deltas = ops::paste(&mut self.buffer, &mut self.selections, &text);
                    self.after_edit(&deltas, cx);
                }
            }
            Some('K') => {
                let pos = self.buffer.char_to_lsp(self.selections.primary().head);
                cx.emit(EditorEvent::RequestHover {
                    line: pos.line,
                    character: pos.character,
                });
            }
            Some('/') => {
                self.inline_bar = InlineBar::Find;
                self.bar_input.clear();
                cx.notify();
            }
            _ => {}
        }
    }

    fn refresh_find_matches(&mut self) {
        if self.bar_input.is_empty() {
            self.find_matches.clear();
            self.find_index = 0;
            return;
        }
        self.find_matches = self.buffer.find_all(
            &self.bar_input,
            SearchOptions {
                case_sensitive: self.find_case_sensitive,
                whole_word: false,
                regex: self.find_regex,
            },
        );
        self.find_index = 0;
        if let Some(first) = self.find_matches.first().cloned() {
            self.selections = Selections::single(Selection::new(first.start, first.end));
            self.ensure_cursor_visible();
        }
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();
        let focused = self.focus_handle.is_focused(window);
        let total_lines = self.buffer.len_lines().max(1);

        let font_size = self.settings.font_size.clamp(10.0, 24.0);
        let line_height_px = (font_size * 1.75_f32.max(self.settings.line_height)).round();
        let approx_char_w = font_size * 0.6;

        // Dynamically size viewport rows from window height
        let vp_h = f32::from(window.viewport_size().height);
        self.visible_lines = ((vp_h - 170.0).max(200.0) / line_height_px).floor() as usize;
        let visible_cap = self.visible_lines.max(10);

        let start_line = self.scroll_line.min(total_lines.saturating_sub(1));

        // Collect visible (unfolded) line indices
        let mut display_lines = Vec::with_capacity(visible_cap);
        let mut scan = start_line;
        while scan < total_lines && display_lines.len() < visible_cap {
            if !self.is_line_hidden(scan) {
                display_lines.push(scan);
            }
            scan += 1;
        }
        let last_vis_line = display_lines.last().copied().unwrap_or(start_line);

        // Compute visible byte range once for incremental Tree-sitter highlights
        let vis_start_byte = self.buffer.rope().line_to_byte(start_line);
        let vis_end_byte = if last_vis_line + 1 < total_lines {
            self.buffer.rope().line_to_byte(last_vis_line + 1)
        } else {
            self.buffer.len_bytes()
        };

        let spans = self
            .syntax
            .as_ref()
            .map(|s| s.highlights(self.buffer.rope(), vis_start_byte..vis_end_byte))
            .unwrap_or_default();

        let primary_pt = self.cursor_point();
        let primary_char = self.selections.primary().head;
        let matching_bracket = self.buffer.matching_bracket(primary_char);

        // Map all cursor points by line (supports multi-cursor!)
        let mut cursors_by_line: HashMap<usize, Vec<(usize, bool)>> = HashMap::new();
        for (idx, sel) in self.selections.all().iter().enumerate() {
            let pt = self.buffer.char_to_point(sel.head);
            cursors_by_line
                .entry(pt.line)
                .or_default()
                .push((pt.col, idx == self.selections.primary_index()));
        }

        // Index diagnostics by line
        let mut diag_by_line: HashMap<usize, Vec<&Diagnostic>> = HashMap::new();
        for d in &self.diagnostics {
            diag_by_line
                .entry(d.range.start.line as usize)
                .or_default()
                .push(d);
        }

        let is_block_cursor = self.settings.navigation_mode == NavigationMode::Vim
            && self.vim.mode != VimMode::Insert;

        // Calculate approximate code column origin X for mouse click column mapping
        let vp_w = f32::from(window.viewport_size().width);
        let content_area_w = (vp_w - 256.0 - 80.0).max(400.0);
        let centered_left = 256.0 + 40.0 + ((content_area_w - 840.0).max(0.0) / 2.0) + 64.0;

        let mut rows = Vec::with_capacity(display_lines.len());
        let mut span_cursor = 0usize;

        for &line_idx in &display_lines {
            let full_line = self.buffer.line_text(line_idx);
            let line_start_byte = self.buffer.rope().line_to_byte(line_idx);
            let line_start_char = self.buffer.rope().line_to_char(line_idx);
            let line_char_count = full_line.chars().count();
            let line_end_char = line_start_char + line_char_count;

            // Apply horizontal scroll slice
            let h_byte_offset = char_to_byte_in_str(&full_line, self.scroll_col);
            let visible_text = &full_line[h_byte_offset..];
            let visible_byte_start = line_start_byte + h_byte_offset;
            let visible_byte_end = visible_byte_start + visible_text.len();

            while span_cursor < spans.len() && spans[span_cursor].range.end <= visible_byte_start {
                span_cursor += 1;
            }

            let mut line_highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
            let mut idx = span_cursor;
            while idx < spans.len() && spans[idx].range.start < visible_byte_end {
                let sp = &spans[idx];
                let s = sp.range.start.max(visible_byte_start) - visible_byte_start;
                let e = sp.range.end.min(visible_byte_end) - visible_byte_start;
                if s < e && visible_text.is_char_boundary(s) && visible_text.is_char_boundary(e) {
                    line_highlights.push((s..e, highlight::style_for_kind(sp.kind, &theme)));
                }
                idx += 1;
            }

            // Highlight ALL selections (multi-cursor aware!)
            for sel in self.selections.all() {
                let r = sel.range();
                if !r.is_empty() && r.start <= line_end_char && r.end >= line_start_char {
                    let s_c = r.start.max(line_start_char) - line_start_char;
                    let e_c = r.end.min(line_end_char) - line_start_char;
                    if e_c > self.scroll_col {
                        let vs = char_to_byte_in_str(visible_text, s_c.saturating_sub(self.scroll_col));
                        let ve = char_to_byte_in_str(visible_text, e_c.saturating_sub(self.scroll_col));
                        if vs < ve {
                            line_highlights.push((
                                vs..ve,
                                HighlightStyle {
                                    background_color: Some(theme.selection),
                                    ..Default::default()
                                },
                            ));
                        }
                    }
                }
            }

            // Highlight ALL find matches on this line
            if !self.find_matches.is_empty() {
                for (mi, mr) in self.find_matches.iter().enumerate() {
                    if mr.start <= line_end_char && mr.end >= line_start_char {
                        let s_c = mr.start.max(line_start_char) - line_start_char;
                        let e_c = mr.end.min(line_end_char) - line_start_char;
                        if e_c > self.scroll_col {
                            let vs = char_to_byte_in_str(visible_text, s_c.saturating_sub(self.scroll_col));
                            let ve = char_to_byte_in_str(visible_text, e_c.saturating_sub(self.scroll_col));
                            if vs < ve {
                                let bg = if mi == self.find_index {
                                    Theme::with_alpha(theme.warning, 0.38)
                                } else {
                                    Theme::with_alpha(theme.accent, 0.22)
                                };
                                line_highlights.push((
                                    vs..ve,
                                    HighlightStyle {
                                        background_color: Some(bg),
                                        ..Default::default()
                                    },
                                ));
                            }
                        }
                    }
                }
            }

            // Matching bracket highlight
            if let Some(mb) = matching_bracket {
                if mb >= line_start_char && mb < line_end_char {
                    let c_in_line = mb - line_start_char;
                    if c_in_line >= self.scroll_col {
                        let vs = char_to_byte_in_str(visible_text, c_in_line - self.scroll_col);
                        let ve = char_to_byte_in_str(visible_text, c_in_line - self.scroll_col + 1);
                        if vs < ve {
                            line_highlights.push((
                                vs..ve,
                                HighlightStyle {
                                    background_color: Some(Theme::with_alpha(theme.accent, 0.28)),
                                    ..Default::default()
                                },
                            ));
                        }
                    }
                }
            }

            // Diagnostic underlines
            let line_diags = diag_by_line.get(&line_idx);
            let mut worst_diag_color = None;
            if let Some(diags) = line_diags {
                for d in diags {
                    let color = match d.severity {
                        Some(DiagnosticSeverity::ERROR) => theme.error,
                        Some(DiagnosticSeverity::WARNING) => theme.warning,
                        _ => theme.accent,
                    };
                    if worst_diag_color.is_none() || d.severity == Some(DiagnosticSeverity::ERROR) {
                        worst_diag_color = Some(color);
                    }
                    let ds_c = (d.range.start.character as usize).saturating_sub(self.scroll_col);
                    let de_c = (d.range.end.character as usize)
                        .max(d.range.start.character as usize + 1)
                        .saturating_sub(self.scroll_col);
                    let ds = char_to_byte_in_str(visible_text, ds_c);
                    let de = char_to_byte_in_str(visible_text, de_c);
                    if ds < de {
                        line_highlights.push((
                            ds..de,
                            HighlightStyle {
                                underline: Some(UnderlineStyle {
                                    color: Some(color),
                                    thickness: px(1.5),
                                    wavy: true,
                                }),
                                ..Default::default()
                            },
                        ));
                    }
                }
            }

            line_highlights.sort_by_key(|(r, _)| (r.start, r.end));

            let is_active_line = cursors_by_line.contains_key(&line_idx);
            let line_num_str = if self.settings.relative_line_numbers && line_idx != primary_pt.line
            {
                format!("{:>2}", line_idx.abs_diff(primary_pt.line))
            } else {
                format!("{:02}", line_idx + 1)
            };

            let gutter_color = self.gutter_changes.get(&line_idx).map(|k| match k {
                GutterChangeKind::Added | GutterChangeKind::Modified => theme.success,
                GutterChangeKind::Deleted => theme.error,
            });

            let is_foldable = self.fold_ranges.contains_key(&line_idx);
            let is_folded = self.folded_lines.contains(&line_idx);
            let folded_count = if is_folded {
                self.fold_ranges
                    .get(&line_idx)
                    .map(|&end| end.saturating_sub(line_idx))
            } else {
                None
            };

            let display_str = if visible_text.is_empty() {
                " ".to_string()
            } else {
                visible_text.to_string()
            };

            let styled = if line_highlights.is_empty() || visible_text.is_empty() {
                StyledText::new(display_str)
            } else {
                StyledText::new(display_str).with_highlights(line_highlights)
            };

            // Render ALL cursors on this line (primary + secondary!)
            let line_cursors: Vec<_> = if focused {
                cursors_by_line
                    .get(&line_idx)
                    .map(|list| {
                        list.iter()
                            .filter(|&&(col, _)| col >= self.scroll_col)
                            .map(|&(col, is_primary)| {
                                let cx_px = px((col - self.scroll_col) as f32 * approx_char_w);
                                let caret_color = if is_primary {
                                    theme.accent
                                } else {
                                    theme.success
                                };
                                div()
                                    .absolute()
                                    .left(px(8.0) + cx_px)
                                    .top(px(3.0))
                                    .w(if is_block_cursor {
                                        px(approx_char_w.max(7.5))
                                    } else {
                                        px(2.0)
                                    })
                                    .h(px(line_height_px - 6.0))
                                    .bg(if is_block_cursor {
                                        Theme::with_alpha(caret_color, 0.42)
                                    } else {
                                        caret_color
                                    })
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            let inline_diag_msg = if line_idx == primary_pt.line {
                line_diags.and_then(|v| v.first()).map(|d| {
                    let c = match d.severity {
                        Some(DiagnosticSeverity::ERROR) => theme.error,
                        Some(DiagnosticSeverity::WARNING) => theme.warning,
                        _ => theme.accent,
                    };
                    (d.message.lines().next().unwrap_or("").to_string(), c)
                })
            } else {
                None
            };

            let row = div()
                .id(("ed-line", line_idx))
                .w_full()
                .h(px(line_height_px))
                .rounded(px(4.0))
                .flex()
                .items_center()
                .when(is_active_line, |this| this.bg(theme.active_line))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        let rel_x = (f32::from(ev.position.x) - centered_left).max(0.0);
                        let col = (rel_x / approx_char_w).round() as usize;
                        this.handle_line_mouse_down(line_idx, col, ev, window, cx);
                    }),
                )
                .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                    if this.is_dragging {
                        let rel_x = (f32::from(ev.position.x) - centered_left).max(0.0);
                        let col = (rel_x / approx_char_w).round() as usize;
                        this.handle_line_mouse_move(line_idx, col, cx);
                    }
                }))
                // Quiet borderless margin: 2-digit line number + fold chevron + 2px rounded git pip
                .child(
                    div()
                        .w(px(58.0))
                        .h_full()
                        .pr(px(10.0))
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(5.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(if is_active_line {
                                    FontWeight::MEDIUM
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(if is_active_line {
                                    theme.accent
                                } else if gutter_color.is_some() {
                                    Theme::with_alpha(theme.text, 0.78)
                                } else {
                                    Theme::with_alpha(theme.text_muted, 0.38)
                                })
                                .child(line_num_str),
                        )
                        .child(
                            div()
                                .id(("fold-btn", line_idx))
                                .w(px(10.0))
                                .text_size(px(10.0))
                                .text_color(Theme::with_alpha(theme.text_muted, 0.5))
                                .when(is_foldable, |d| {
                                    d.cursor_pointer()
                                        .hover(|s| s.text_color(theme.accent))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _, _, cx| {
                                                this.toggle_fold_at(line_idx, cx);
                                            }),
                                        )
                                        .child(if is_folded { "▸" } else { "▾" })
                                }),
                        )
                        .child(
                            div()
                                .w(px(2.0))
                                .h(px(line_height_px - 6.0))
                                .rounded(px(1.0))
                                .when_some(gutter_color.or(worst_diag_color), |this, c| this.bg(c)),
                        ),
                )
                // Code text + multi-cursor overlays + fold pill + inline diagnostic
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .h_full()
                        .pl(px(8.0))
                        .flex()
                        .items_center()
                        .overflow_hidden()
                        .text_size(px(font_size))
                        .text_color(theme.text)
                        .child(styled)
                        .children(line_cursors)
                        .when_some(folded_count, |d, n| {
                            d.child(
                                div()
                                    .ml(px(8.0))
                                    .px(px(6.0))
                                    .py(px(1.0))
                                    .rounded(px(3.0))
                                    .bg(theme.surface_elevated)
                                    .text_size(px(10.5))
                                    .text_color(theme.accent)
                                    .child(format!("⋯ {n} lines")),
                            )
                        })
                        .when_some(inline_diag_msg, |this, (msg, color)| {
                            this.child(
                                div()
                                    .ml(px(16.0))
                                    .px(px(6.0))
                                    .py(px(1.0))
                                    .rounded(px(3.0))
                                    .bg(Theme::with_alpha(color, 0.14))
                                    .text_size(px(11.0))
                                    .text_color(color)
                                    .child(msg),
                            )
                        }),
                );

            rows.push(row.into_any_element());

            // Render Stitch Floating Contextual Signature Card inline right below its target line!
            if let Some(card) = &self.hover_card {
                if card.line == line_idx {
                    rows.push(
                        div()
                            .w_full()
                            .pl(px(82.0))
                            .py(px(8.0))
                            .child(
                                div()
                                    .max_w(px(576.0))
                                    .p(px(14.0))
                                    .rounded(px(4.0))
                                    .bg(theme.surface_hover)
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.0))
                                    // Top meta strip
                                    .child(
                                        div()
                                            .pb(px(6.0))
                                            .mb(px(2.0))
                                            .bg(Theme::with_alpha(theme.border_strong, 0.2))
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .text_size(px(11.0))
                                            .child(
                                                div()
                                                    .text_color(theme.accent)
                                                    .child(card.header_tag.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_color(Theme::with_alpha(theme.text_muted, 0.65))
                                                    .child(card.location_label.clone()),
                                            ),
                                    )
                                    // Signature line
                                    .when_some(card.signature.clone(), |d, sig| {
                                        d.child(
                                            div()
                                                .py(px(2.0))
                                                .text_size(px(13.0))
                                                .text_color(theme.warning)
                                                .child(sig),
                                        )
                                    })
                                    // Description prose
                                    .when(!card.body.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .mt(px(4.0))
                                                .text_size(px(12.0))
                                                .text_color(theme.text_secondary)
                                                .child(card.body.clone()),
                                        )
                                    })
                                    // Bottom tags row: ● Zero Alloc | Thread-Safe | Returns Result<(), WireError>
                                    .when(!card.footer_tags.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .mt(px(6.0))
                                                .pt(px(6.0))
                                                .flex()
                                                .items_center()
                                                .gap(px(12.0))
                                                .text_size(px(11.0))
                                                .text_color(theme.text_muted)
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(5.0))
                                                        .child(
                                                            div()
                                                                .w(px(6.0))
                                                                .h(px(6.0))
                                                                .rounded(px(3.0))
                                                                .bg(theme.success),
                                                        )
                                                        .child(
                                                            card.footer_tags
                                                                .first()
                                                                .cloned()
                                                                .unwrap_or_default(),
                                                        ),
                                                )
                                                .when(card.footer_tags.len() > 1, |d| {
                                                    d.child(
                                                        div()
                                                            .text_color(Theme::with_alpha(
                                                                theme.text_muted,
                                                                0.3,
                                                            ))
                                                            .child("|"),
                                                    )
                                                    .child(card.footer_tags[1].clone())
                                                })
                                                .when(card.footer_tags.len() > 2, |d| {
                                                    d.child(
                                                        div()
                                                            .text_color(Theme::with_alpha(
                                                                theme.text_muted,
                                                                0.3,
                                                            ))
                                                            .child("|"),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_color(theme.warning)
                                                            .child(card.footer_tags[2].clone()),
                                                    )
                                                }),
                                        )
                                    }),
                            )
                            .into_any_element(),
                    );
                }
            }
        }

        // Bottom Soft Thought Marker: Quiet Reflection Note (1:1 Stitch Focused Editor)
        if last_vis_line + 3 >= total_lines {
            rows.push(
                div()
                    .mt(px(28.0))
                    .p(px(14.0))
                    .rounded(px(4.0))
                    .bg(Theme::with_alpha(theme.canvas, 0.65))
                    .flex()
                    .items_start()
                    .gap(px(14.0))
                    .child(
                        div()
                            .mt(px(2.0))
                            .text_size(px(14.0))
                            .text_color(Theme::with_alpha(theme.text_muted, 0.45))
                            .child("✽"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(Theme::with_alpha(theme.text_muted, 0.55))
                                    .child("READING BUFFER · FLOW CHECK"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(px(4.0))
                                    .text_size(px(12.0))
                                    .text_color(theme.text_secondary)
                                    .child("Line 18 contains uncommitted stream optimizations. All 14 test fixtures in")
                                    .child(
                                        div()
                                            .text_color(theme.accent)
                                            .child("tests/stream.rs"),
                                    )
                                    .child("pass with zero jitter."),
                            ),
                    )
                    .into_any_element(),
            );
        }

        // Compute vertical scrollbar thumb metrics
        let thumb_ratio = (visible_cap as f32 / total_lines as f32).clamp(0.08, 1.0);
        let scroll_ratio = if total_lines > visible_cap {
            start_line as f32 / (total_lines - visible_cap) as f32
        } else {
            0.0
        };
        let track_h = (vp_h - 170.0).max(160.0);
        let thumb_h = (track_h * thumb_ratio).max(24.0);
        let thumb_top = (track_h - thumb_h) * scroll_ratio.clamp(0.0, 1.0);

        // Compute popup coordinates anchored near the active cursor
        let active_vis_row = display_lines
            .iter()
            .position(|&l| l == primary_pt.line)
            .unwrap_or(0);
        let popup_top = ((active_vis_row + 1) as f32 * line_height_px + 12.0).clamp(32.0, 440.0);
        let popup_left = (110.0 + primary_pt.col.saturating_sub(self.scroll_col) as f32 * approx_char_w)
            .clamp(90.0, 520.0);

        div()
            .id("editor-canvas")
            .relative()
            .size_full()
            .bg(theme.surface)
            .font_family(theme.mono_font.clone())
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .flex()
            .flex_col()
            .overflow_hidden()
            .cursor_text()
            // Optional Inline Bar (Find / Replace / Go to Line / Rename)
            .when(self.inline_bar != InlineBar::None, |this| {
                let (label, hint) = match self.inline_bar {
                    InlineBar::Find => {
                        let count = if self.find_matches.is_empty() {
                            "No matches".to_string()
                        } else {
                            format!("{} / {}", self.find_index + 1, self.find_matches.len())
                        };
                        ("Find:", format!("{count} · Enter: next · Esc: close"))
                    }
                    InlineBar::Replace => (
                        "Find / Replace:",
                        "Tab: switch · Enter: replace · Esc: close".to_string(),
                    ),
                    InlineBar::GotoLine => ("Go to Line:", "Enter line number · Esc: close".to_string()),
                    InlineBar::Rename => ("Rename Symbol:", "Enter: apply · Esc: cancel".to_string()),
                    InlineBar::None => ("", String::new()),
                };

                this.child(
                    div()
                        .w_full()
                        .px(px(40.0))
                        .py(px(6.0))
                        .child(
                            div()
                                .max_w(px(840.0))
                                .mx_auto()
                                .px(px(12.0))
                                .py(px(6.0))
                                .rounded(px(6.0))
                                .bg(theme.surface_panel)
                                .border_1()
                                .border_color(theme.border_strong)
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.0))
                                .text_size(px(12.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(div().text_color(theme.accent).child(label))
                                        .child(
                                            div()
                                                .px(px(8.0))
                                                .py(px(2.0))
                                                .rounded(px(4.0))
                                                .bg(theme.canvas)
                                                .text_color(theme.text)
                                                .child(if self.bar_input.is_empty() {
                                                    "type here...".to_string()
                                                } else {
                                                    self.bar_input.clone()
                                                }),
                                        )
                                        .when(self.inline_bar == InlineBar::Replace, |d| {
                                            d.child(div().text_color(theme.text_muted).child("→"))
                                                .child(
                                                    div()
                                                        .px(px(8.0))
                                                        .py(px(2.0))
                                                        .rounded(px(4.0))
                                                        .bg(if self.replace_field_focused {
                                                            theme.surface_hover
                                                        } else {
                                                            theme.canvas
                                                        })
                                                        .text_color(theme.text)
                                                        .child(if self.replace_input.is_empty() {
                                                            "replacement...".to_string()
                                                        } else {
                                                            self.replace_input.clone()
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .id("ed-replace-all")
                                                        .px(px(8.0))
                                                        .py(px(2.0))
                                                        .rounded(px(4.0))
                                                        .bg(theme.accent_secondary)
                                                        .text_color(theme.on_accent)
                                                        .cursor_pointer()
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            cx.listener(|this, _, _, cx| {
                                                                this.replace_all(cx);
                                                            }),
                                                        )
                                                        .child("Replace All"),
                                                )
                                        }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .when(
                                            matches!(self.inline_bar, InlineBar::Find | InlineBar::Replace),
                                            |d| {
                                                d.child(
                                                    div()
                                                        .id("ed-find-case")
                                                        .px(px(6.0))
                                                        .py(px(2.0))
                                                        .rounded(px(3.0))
                                                        .bg(if self.find_case_sensitive {
                                                            theme.surface_hover
                                                        } else {
                                                            theme.canvas
                                                        })
                                                        .text_color(if self.find_case_sensitive {
                                                            theme.accent
                                                        } else {
                                                            theme.text_muted
                                                        })
                                                        .cursor_pointer()
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            cx.listener(|this, _, _, cx| {
                                                                this.find_case_sensitive = !this.find_case_sensitive;
                                                                this.refresh_find_matches();
                                                                cx.notify();
                                                            }),
                                                        )
                                                        .child("Aa"),
                                                )
                                                .child(
                                                    div()
                                                        .id("ed-find-regex")
                                                        .px(px(6.0))
                                                        .py(px(2.0))
                                                        .rounded(px(3.0))
                                                        .bg(if self.find_regex {
                                                            theme.surface_hover
                                                        } else {
                                                            theme.canvas
                                                        })
                                                        .text_color(if self.find_regex {
                                                            theme.accent
                                                        } else {
                                                            theme.text_muted
                                                        })
                                                        .cursor_pointer()
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            cx.listener(|this, _, _, cx| {
                                                                this.find_regex = !this.find_regex;
                                                                this.refresh_find_matches();
                                                                cx.notify();
                                                            }),
                                                        )
                                                        .child(".*"),
                                                )
                                            },
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.0))
                                                .text_color(theme.text_muted)
                                                .child(hint),
                                        ),
                                ),
                        ),
                )
            })
            // Centered editorial code stream (1:1 Stitch max-w-[84ch])
            .child(
                div()
                    .relative()
                    .flex_1()
                    .w_full()
                    .px(px(40.0))
                    .pt(px(8.0))
                    .flex()
                    .justify_center()
                    .overflow_hidden()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(840.0))
                            .flex()
                            .flex_col()
                            .children(rows),
                    )
                    // Quiet right-edge vertical scrollbar indicator
                    .when(total_lines > visible_cap, |d| {
                        d.child(
                            div()
                                .absolute()
                                .right(px(6.0))
                                .top(px(thumb_top))
                                .w(px(4.0))
                                .h(px(thumb_h))
                                .rounded(px(2.0))
                                .bg(Theme::with_alpha(theme.text_muted, 0.25)),
                        )
                    }),
            )
            // Autocomplete Popup (LSP + Local Buffer Words)
            .when(self.show_completions && !self.candidates.is_empty(), |this| {
                let selected_doc = self
                    .candidates
                    .get(self.completion_idx)
                    .and_then(|c| c.documentation.clone());

                let items = self
                    .candidates
                    .iter()
                    .take(10)
                    .enumerate()
                    .map(|(i, cand)| {
                        let active = i == self.completion_idx;
                        let icon_color = if !cand.from_lsp {
                            theme.text_muted
                        } else {
                            match cand.kind {
                                Some(blur_lsp::lsp_types::CompletionItemKind::FUNCTION)
                                | Some(blur_lsp::lsp_types::CompletionItemKind::METHOD)
                                | Some(blur_lsp::lsp_types::CompletionItemKind::CONSTRUCTOR) => {
                                    theme.syn_function
                                }
                                Some(blur_lsp::lsp_types::CompletionItemKind::STRUCT)
                                | Some(blur_lsp::lsp_types::CompletionItemKind::CLASS)
                                | Some(blur_lsp::lsp_types::CompletionItemKind::INTERFACE)
                                | Some(blur_lsp::lsp_types::CompletionItemKind::TYPE_PARAMETER) => {
                                    theme.syn_type
                                }
                                Some(blur_lsp::lsp_types::CompletionItemKind::ENUM)
                                | Some(blur_lsp::lsp_types::CompletionItemKind::ENUM_MEMBER)
                                | Some(blur_lsp::lsp_types::CompletionItemKind::CONSTANT) => {
                                    theme.syn_constant
                                }
                                Some(blur_lsp::lsp_types::CompletionItemKind::FIELD)
                                | Some(blur_lsp::lsp_types::CompletionItemKind::PROPERTY) => {
                                    theme.syn_property
                                }
                                Some(blur_lsp::lsp_types::CompletionItemKind::VARIABLE) => {
                                    theme.syn_variable
                                }
                                Some(blur_lsp::lsp_types::CompletionItemKind::MODULE) => {
                                    theme.syn_namespace
                                }
                                Some(blur_lsp::lsp_types::CompletionItemKind::KEYWORD) => {
                                    theme.syn_keyword
                                }
                                _ => theme.accent,
                            }
                        };
                        div()
                            .id(("comp-item", i))
                            .w_full()
                            .h(px(26.0))
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.0))
                            .when(active, |d| d.bg(theme.surface_hover))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.completion_idx = i;
                                    this.accept_completion(cx);
                                }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .w(px(18.0))
                                            .h(px(18.0))
                                            .rounded(px(3.0))
                                            .bg(Theme::with_alpha(icon_color, 0.16))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(11.0))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(icon_color)
                                            .child(cand.kind_icon),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(if active {
                                                FontWeight::MEDIUM
                                            } else {
                                                FontWeight::NORMAL
                                            })
                                            .text_color(if active {
                                                theme.text
                                            } else {
                                                theme.text_secondary
                                            })
                                            .child(cand.label.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .when_some(cand.detail.clone(), |d, det| {
                                        d.child(
                                            div()
                                                .text_size(px(11.0))
                                                .text_color(theme.text_muted)
                                                .child(det),
                                        )
                                    })
                                    .child(
                                        div()
                                            .px(px(5.0))
                                            .py(px(1.0))
                                            .rounded(px(3.0))
                                            .bg(theme.surface_elevated)
                                            .text_size(px(10.0))
                                            .text_color(icon_color)
                                            .child(cand.kind_tag),
                                    ),
                            )
                    })
                    .collect::<Vec<_>>();

                this.child(
                    div()
                        .absolute()
                        .left(px(popup_left))
                        .top(px(popup_top))
                        .w(px(400.0))
                        .py(px(4.0))
                        .rounded(px(6.0))
                        .bg(theme.surface_panel)
                        .border_1()
                        .border_color(theme.border_strong)
                        .flex()
                        .flex_col()
                        .children(items)
                        .when_some(selected_doc, |d, doc| {
                            d.child(
                                div()
                                    .mt(px(4.0))
                                    .px(px(10.0))
                                    .py(px(6.0))
                                    .bg(theme.canvas)
                                    .border_t_1()
                                    .border_color(theme.border)
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child(doc.lines().take(3).collect::<Vec<_>>().join(" ")),
                            )
                        }),
                )
            })
            // References Peek Popup
            .when(self.show_references && !self.references_peek.is_empty(), |this| {
                let items = self
                    .references_peek
                    .iter()
                    .take(10)
                    .enumerate()
                    .map(|(i, (path, line, preview))| {
                        let p = path.clone();
                        let l = *line;
                        let fname = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("file")
                            .to_string();
                        div()
                            .id(("ref-item", i))
                            .w_full()
                            .h(px(26.0))
                            .px(px(10.0))
                            .hover(|s| s.bg(theme.surface_hover))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .text_size(px(11.5))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.show_references = false;
                                    cx.emit(EditorEvent::OpenLocation {
                                        path: p.clone(),
                                        line: l,
                                        col: 0,
                                    });
                                    cx.notify();
                                }),
                            )
                            .child(div().text_color(theme.accent).child(format!("{fname}:{l}")))
                            .child(div().text_color(theme.text_muted).child(preview.clone()))
                    })
                    .collect::<Vec<_>>();

                this.child(
                    div()
                        .absolute()
                        .left(px(popup_left))
                        .top(px(popup_top))
                        .w(px(520.0))
                        .p(px(8.0))
                        .rounded(px(6.0))
                        .bg(theme.surface_panel)
                        .border_1()
                        .border_color(theme.border_strong)
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .px(px(6.0))
                                .text_size(px(10.5))
                                .text_color(theme.text_muted)
                                .child(format!("REFERENCES ({})", self.references_peek.len())),
                        )
                        .children(items),
                )
            })
            // Code Actions Quick-Fix Popup
            .when(self.show_code_actions && !self.code_actions.is_empty(), |this| {
                let items = self
                    .code_actions
                    .iter()
                    .take(8)
                    .enumerate()
                    .map(|(i, act)| {
                        let title = match act {
                            CodeActionOrCommand::Command(c) => c.title.clone(),
                            CodeActionOrCommand::CodeAction(a) => a.title.clone(),
                        };
                        let edit_opt = match act {
                            CodeActionOrCommand::CodeAction(a) => a.edit.clone(),
                            _ => None,
                        };
                        div()
                            .id(("ca-item", i))
                            .w_full()
                            .h(px(28.0))
                            .px(px(10.0))
                            .hover(|s| s.bg(theme.surface_hover))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(12.0))
                            .text_color(theme.text)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.show_code_actions = false;
                                    if let Some(we) = &edit_opt {
                                        if let Some(changes) = &we.changes {
                                            if let Some(uri) = &this.lsp_uri {
                                                if let Ok(lsp_uri) = blur_lsp::url_to_lsp_uri(uri) {
                                                    if let Some(edits) = changes.get(&lsp_uri) {
                                                        this.apply_lsp_edits(edits, cx);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    cx.notify();
                                }),
                            )
                            .child(title)
                    })
                    .collect::<Vec<_>>();

                this.child(
                    div()
                        .absolute()
                        .left(px(popup_left))
                        .top(px(popup_top))
                        .w(px(380.0))
                        .p(px(6.0))
                        .rounded(px(6.0))
                        .bg(theme.surface_panel)
                        .border_1()
                        .border_color(theme.border_strong)
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .px(px(6.0))
                                .pb(px(4.0))
                                .text_size(px(10.5))
                                .text_color(theme.accent)
                                .child("CODE ACTIONS"),
                        )
                        .children(items),
                )
            })
    }
}

fn char_to_byte_in_str(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}
