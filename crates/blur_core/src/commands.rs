//! Command metadata and registry with fuzzy search.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Metadata describing an executable Blur command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandMeta {
    pub id: String,
    pub title: String,
    pub category: String,
    pub description: Option<String>,
}

impl CommandMeta {
    /// Creates a new `CommandMeta` without a description.
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        category: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            category: category.into(),
            description: None,
        }
    }

    /// Attaches an optional description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Registry of all available commands.
#[derive(Clone, Debug, Default)]
pub struct CommandRegistry {
    commands: BTreeMap<String, CommandMeta>,
}

impl CommandRegistry {
    /// Creates an empty command registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a command registry populated with all canonical built-in Blur commands.
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        let builtins: &[(&str, &str, &str)] = &[
            // Workspace
            ("workspace.new_file", "New File", "Workspace"),
            ("workspace.command_palette", "Command Palette", "Workspace"),
            ("workspace.file_finder", "Go to File", "Workspace"),
            ("workspace.project_search", "Search in Project", "Workspace"),
            ("workspace.toggle_sidebar", "Toggle Sidebar", "Workspace"),
            ("workspace.toggle_panel", "Toggle Bottom Panel", "Workspace"),
            ("workspace.toggle_terminal", "Toggle Terminal", "Workspace"),
            ("workspace.new_terminal", "New Terminal", "Workspace"),
            ("workspace.open_folder", "Open Folder", "Workspace"),
            ("workspace.open_settings", "Open Settings", "Workspace"),
            ("workspace.theme_selector", "Select Theme", "Workspace"),
            ("workspace.zen_mode", "Toggle Zen Mode", "Workspace"),
            ("workspace.view_editor", "Show Editor View", "Workspace"),
            ("workspace.view_git", "Show Git View", "Workspace"),
            ("workspace.view_lsp", "Show LSP View", "Workspace"),
            ("workspace.view_utilities", "Show Utilities View", "Workspace"),
            ("workspace.view_settings", "Show Settings View", "Workspace"),
            ("workspace.split_right", "Split Editor Right", "Workspace"),
            ("workspace.split_down", "Split Editor Down", "Workspace"),
            ("workspace.close_group", "Close Editor Group", "Workspace"),
            ("workspace.focus_next_group", "Focus Next Editor Group", "Workspace"),
            ("workspace.quit", "Quit Blur", "Workspace"),
            // Editor
            ("editor.save", "Save File", "Editor"),
            ("editor.save_all", "Save All Files", "Editor"),
            ("editor.close_tab", "Close Tab", "Editor"),
            ("editor.next_tab", "Next Tab", "Editor"),
            ("editor.prev_tab", "Previous Tab", "Editor"),
            ("editor.undo", "Undo", "Editor"),
            ("editor.redo", "Redo", "Editor"),
            ("editor.cut", "Cut", "Editor"),
            ("editor.copy", "Copy", "Editor"),
            ("editor.paste", "Paste", "Editor"),
            ("editor.select_all", "Select All", "Editor"),
            ("editor.find", "Find in File", "Editor"),
            ("editor.replace", "Find and Replace", "Editor"),
            ("editor.find_next", "Find Next Match", "Editor"),
            ("editor.find_prev", "Find Previous Match", "Editor"),
            ("editor.goto_line", "Go to Line", "Editor"),
            ("editor.goto_symbol", "Go to Symbol", "Editor"),
            ("editor.goto_definition", "Go to Definition", "Editor"),
            ("editor.find_references", "Find References", "Editor"),
            ("editor.rename_symbol", "Rename Symbol", "Editor"),
            ("editor.code_actions", "Code Actions", "Editor"),
            ("editor.format", "Format Document", "Editor"),
            ("editor.hover", "Show Hover", "Editor"),
            ("editor.trigger_completion", "Trigger Completion", "Editor"),
            ("editor.toggle_comment", "Toggle Line Comment", "Editor"),
            ("editor.duplicate_line", "Duplicate Line", "Editor"),
            ("editor.move_line_up", "Move Line Up", "Editor"),
            ("editor.move_line_down", "Move Line Down", "Editor"),
            ("editor.delete_line", "Delete Line", "Editor"),
            ("editor.add_cursor_above", "Add Cursor Above", "Editor"),
            ("editor.add_cursor_below", "Add Cursor Below", "Editor"),
            ("editor.select_next_occurrence", "Select Next Occurrence", "Editor"),
            ("editor.fold", "Fold Region", "Editor"),
            ("editor.unfold", "Unfold Region", "Editor"),
            ("editor.indent", "Indent Line", "Editor"),
            ("editor.outdent", "Outdent Line", "Editor"),
            ("editor.markdown_preview", "Toggle Markdown Preview", "Editor"),
            // Git
            ("git.stage_hunk", "Stage Hunk", "Git"),
            ("git.commit", "Commit Changes", "Git"),
            ("git.fetch", "Fetch", "Git"),
            ("git.pull", "Pull", "Git"),
            ("git.push", "Push", "Git"),
            ("git.graph", "Show Git Graph", "Git"),
            ("git.stash", "Stash Changes", "Git"),
            ("git.blame", "Toggle Git Blame", "Git"),
            // Tasks
            ("task.run", "Run Task", "Tasks"),
            ("task.run_test", "Run Test", "Tasks"),
        ];

        for &(id, title, category) in builtins {
            reg.register(CommandMeta::new(id, title, category));
        }
        reg
    }

    /// Registers or replaces a command by its ID.
    pub fn register(&mut self, meta: CommandMeta) {
        self.commands.insert(meta.id.clone(), meta);
    }

    /// Removes a command by its ID.
    pub fn unregister(&mut self, id: &str) -> Option<CommandMeta> {
        self.commands.remove(id)
    }

    /// Looks up a command by its ID.
    pub fn get(&self, id: &str) -> Option<&CommandMeta> {
        self.commands.get(id)
    }

    /// Returns all registered commands sorted by category and title.
    pub fn all(&self) -> Vec<&CommandMeta> {
        let mut items: Vec<&CommandMeta> = self.commands.values().collect();
        items.sort_by(|a, b| a.category.cmp(&b.category).then_with(|| a.title.cmp(&b.title)));
        items
    }

    /// Fuzzy-searches commands by title, category, or ID.
    ///
    /// Returns `(CommandMeta, score, title_char_indices)` sorted by descending score, then
    /// shorter title, then title lexicographically.
    pub fn fuzzy_search(&self, query: &str) -> Vec<(CommandMeta, u32, Vec<usize>)> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return self
                .all()
                .into_iter()
                .map(|m| (m.clone(), 0, Vec::new()))
                .collect();
        }

        let mut results: Vec<(CommandMeta, u32, Vec<usize>)> = Vec::new();
        for meta in self.commands.values() {
            // Prefer direct title match (with highlight indices in `title`).
            if let Some((score, indices)) = fuzzy_score(trimmed, &meta.title) {
                results.push((meta.clone(), score + 10, indices));
                continue;
            }
            // Next try "<category> <title>" and map any indices that fall in `title`.
            let combined = format!("{} {}", meta.category, meta.title);
            if let Some((score, combined_indices)) = fuzzy_score(trimmed, &combined) {
                let offset = meta.category.chars().count() + 1;
                let title_indices: Vec<usize> = combined_indices
                    .into_iter()
                    .filter_map(|i| i.checked_sub(offset))
                    .collect();
                results.push((meta.clone(), score, title_indices));
                continue;
            }
            // Fallback: match against command ID.
            if let Some((score, _)) = fuzzy_score(trimmed, &meta.id) {
                results.push((meta.clone(), score.saturating_sub(5), Vec::new()));
            }
        }

        results.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.0.title.len().cmp(&b.0.title.len()))
                .then_with(|| a.0.title.cmp(&b.0.title))
        });
        results
    }
}

/// Simple subsequence fuzzy scorer returning `(score, matched_char_indices)`.
fn fuzzy_score(query: &str, candidate: &str) -> Option<(u32, Vec<usize>)> {
    let q_chars: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).collect();
    if q_chars.is_empty() {
        return Some((0, Vec::new()));
    }
    let c_chars: Vec<char> = candidate.chars().collect();
    let mut indices = Vec::with_capacity(q_chars.len());
    let mut score: i32 = 0;
    let mut qi = 0;
    let mut prev_match: Option<usize> = None;

    for (ci, &ch) in c_chars.iter().enumerate() {
        if qi >= q_chars.len() {
            break;
        }
        let qc = q_chars[qi];
        if ch.eq_ignore_ascii_case(&qc) || ch.to_lowercase().eq(qc.to_lowercase()) {
            indices.push(ci);
            score += 10;
            // Exact case match bonus.
            if ch == qc {
                score += 2;
            }
            // Start-of-string or word-boundary bonus.
            if ci == 0 {
                score += 15;
            } else {
                let prev = c_chars[ci - 1];
                if matches!(prev, ' ' | '_' | '-' | '.' | '/' | ':')
                    || (prev.is_lowercase() && ch.is_uppercase())
                {
                    score += 12;
                }
            }
            // Consecutive match bonus.
            if let Some(pm) = prev_match {
                if ci == pm + 1 {
                    score += 14;
                } else {
                    score -= ((ci - pm - 1) as i32).min(6);
                }
            }
            prev_match = Some(ci);
            qi += 1;
        }
    }

    if qi == q_chars.len() {
        Some((score.max(1) as u32, indices))
    } else {
        None
    }
}
