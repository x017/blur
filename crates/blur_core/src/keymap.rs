//! Keymap model, default keybindings per navigation mode, user keymap loader, and key formatting.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Editor navigation / keybinding mode.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum NavigationMode {
    #[default]
    Standard,
    Vim,
    Emacs,
    Helix,
}

/// A single keybinding specification using GPUI keystroke syntax (e.g. `"ctrl-shift-p"`, `"ctrl-k ctrl-s"`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct KeyBindingSpec {
    pub keys: String,
    pub command: String,
    #[serde(default)]
    pub context: Option<String>,
}

impl KeyBindingSpec {
    /// Creates a global keybinding specification.
    pub fn new(keys: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            keys: keys.into(),
            command: command.into(),
            context: None,
        }
    }

    /// Creates a context-scoped keybinding specification.
    pub fn with_context(
        keys: impl Into<String>,
        command: impl Into<String>,
        context: impl Into<String>,
    ) -> Self {
        Self {
            keys: keys.into(),
            command: command.into(),
            context: Some(context.into()),
        }
    }
}

/// Returns the default keybindings for the given [`NavigationMode`].
///
/// Uses `cmd-` primary modifier on macOS and `ctrl-` on Linux/Windows.
pub fn default_bindings(mode: NavigationMode) -> Vec<KeyBindingSpec> {
    default_bindings_for_platform(mode, cfg!(target_os = "macos"))
}

/// Returns the default keybindings for the given [`NavigationMode`] and target platform (`mac`).
pub fn default_bindings_for_platform(mode: NavigationMode, mac: bool) -> Vec<KeyBindingSpec> {
    let p = if mac { "cmd" } else { "ctrl" };

    match mode {
        NavigationMode::Standard => standard_bindings(p),
        NavigationMode::Emacs => emacs_bindings(p),
        NavigationMode::Vim => vim_bindings(p),
        NavigationMode::Helix => helix_bindings(p),
    }
}

fn standard_bindings(p: &str) -> Vec<KeyBindingSpec> {
    let mut b = Vec::new();
    let mut add = |keys: String, cmd: &str| b.push(KeyBindingSpec::new(keys, cmd));

    // Workspace
    add(format!("{p}-shift-p"), "workspace.command_palette");
    add(format!("{p}-p"), "workspace.file_finder");
    add(format!("{p}-shift-f"), "workspace.project_search");
    add(format!("{p}-b"), "workspace.toggle_sidebar");
    add(format!("{p}-j"), "workspace.toggle_panel");
    add(format!("{p}-`"), "workspace.toggle_terminal");
    add(format!("{p}-shift-`"), "workspace.new_terminal");
    add(format!("{p}-o"), "workspace.open_folder");
    add(format!("{p}-,"), "workspace.open_settings");
    add(format!("{p}-k {p}-t"), "workspace.theme_selector");
    add(format!("{p}-k z"), "workspace.zen_mode");
    add(format!("{p}-1"), "workspace.view_editor");
    add(format!("{p}-2"), "workspace.view_git");
    add(format!("{p}-3"), "workspace.view_lsp");
    add(format!("{p}-4"), "workspace.view_utilities");
    add(format!("{p}-5"), "workspace.view_settings");
    add(format!("{p}-\\"), "workspace.split_right");
    add(format!("{p}-k {p}-\\"), "workspace.split_down");
    add(format!("{p}-k w"), "workspace.close_group");
    add(format!("{p}-k {p}-right"), "workspace.focus_next_group");
    add(format!("{p}-q"), "workspace.quit");

    // Editor
    add(format!("{p}-s"), "editor.save");
    add(format!("{p}-k s"), "editor.save_all");
    add(format!("{p}-="), "editor.increase_font_size");
    add(format!("{p}--"), "editor.decrease_font_size");
    add(format!("{p}-0"), "editor.reset_font_size");
    add(format!("{p}-w"), "editor.close_tab");
    add("ctrl-tab".to_string(), "editor.next_tab");
    add("ctrl-shift-tab".to_string(), "editor.prev_tab");
    add(format!("{p}-z"), "editor.undo");
    add(format!("{p}-shift-z"), "editor.redo");
    add(format!("{p}-x"), "editor.cut");
    add(format!("{p}-c"), "editor.copy");
    add(format!("{p}-v"), "editor.paste");
    add(format!("{p}-a"), "editor.select_all");
    add(format!("{p}-f"), "editor.find");
    add(format!("{p}-h"), "editor.replace");
    add("f3".to_string(), "editor.find_next");
    add("shift-f3".to_string(), "editor.find_prev");
    add(format!("{p}-g"), "editor.goto_line");
    add(format!("{p}-shift-o"), "editor.goto_symbol");
    add("f12".to_string(), "editor.goto_definition");
    add("shift-f12".to_string(), "editor.find_references");
    add("f2".to_string(), "editor.rename_symbol");
    add(format!("{p}-."), "editor.code_actions");
    add("alt-shift-f".to_string(), "editor.format");
    add(format!("{p}-k {p}-i"), "editor.hover");
    add("ctrl-space".to_string(), "editor.trigger_completion");
    add(format!("{p}-/"), "editor.toggle_comment");
    add("alt-shift-down".to_string(), "editor.duplicate_line");
    add("alt-up".to_string(), "editor.move_line_up");
    add("alt-down".to_string(), "editor.move_line_down");
    add(format!("{p}-shift-k"), "editor.delete_line");
    add(format!("{p}-alt-up"), "editor.add_cursor_above");
    add(format!("{p}-alt-down"), "editor.add_cursor_below");
    add(format!("{p}-d"), "editor.select_next_occurrence");
    add(format!("{p}-shift-["), "editor.fold");
    add(format!("{p}-shift-]"), "editor.unfold");
    add(format!("{p}-]"), "editor.indent");
    add(format!("{p}-["), "editor.outdent");
    add(format!("{p}-shift-v"), "editor.markdown_preview");

    // Git & Tasks
    add(format!("{p}-k {p}-g"), "git.graph");
    add(format!("{p}-shift-b"), "task.run");

    b
}

fn emacs_bindings(p: &str) -> Vec<KeyBindingSpec> {
    let mut b = vec![
        // Core Emacs chords & commands
        KeyBindingSpec::new("ctrl-x ctrl-s", "editor.save"),
        KeyBindingSpec::new("ctrl-x s", "editor.save_all"),
        KeyBindingSpec::new("ctrl-x ctrl-f", "workspace.file_finder"),
        KeyBindingSpec::new("ctrl-x k", "editor.close_tab"),
        KeyBindingSpec::new("ctrl-x ctrl-c", "workspace.quit"),
        KeyBindingSpec::new("ctrl-x 3", "workspace.split_right"),
        KeyBindingSpec::new("ctrl-x 2", "workspace.split_down"),
        KeyBindingSpec::new("ctrl-x 0", "workspace.close_group"),
        KeyBindingSpec::new("ctrl-x o", "workspace.focus_next_group"),
        KeyBindingSpec::new("ctrl-g", "editor.cancel"),
        KeyBindingSpec::new("alt-x", "workspace.command_palette"),
        // Movement
        KeyBindingSpec::with_context("ctrl-f", "editor.move_right", "Editor"),
        KeyBindingSpec::with_context("ctrl-b", "editor.move_left", "Editor"),
        KeyBindingSpec::with_context("ctrl-n", "editor.move_down", "Editor"),
        KeyBindingSpec::with_context("ctrl-p", "editor.move_up", "Editor"),
        KeyBindingSpec::with_context("ctrl-a", "editor.move_line_start", "Editor"),
        KeyBindingSpec::with_context("ctrl-e", "editor.move_line_end", "Editor"),
        KeyBindingSpec::with_context("alt-f", "editor.move_word_right", "Editor"),
        KeyBindingSpec::with_context("alt-b", "editor.move_word_left", "Editor"),
        // Editing / kill ring / search
        KeyBindingSpec::with_context("ctrl-k", "editor.kill_line", "Editor"),
        KeyBindingSpec::with_context("ctrl-space", "editor.set_mark", "Editor"),
        KeyBindingSpec::new("ctrl-s", "editor.find"),
        KeyBindingSpec::new("alt-%", "editor.replace"),
        KeyBindingSpec::new("ctrl-/", "editor.undo"),
        KeyBindingSpec::new("ctrl-w", "editor.cut"),
        KeyBindingSpec::new("alt-w", "editor.copy"),
        KeyBindingSpec::new("ctrl-y", "editor.paste"),
        KeyBindingSpec::new("alt-g g", "editor.goto_line"),
        KeyBindingSpec::new("alt-.", "editor.goto_definition"),
        KeyBindingSpec::new("alt-;", "editor.toggle_comment"),
    ];

    // Keep essential workspace shortcuts accessible
    b.push(KeyBindingSpec::new(
        format!("{p}-shift-p"),
        "workspace.command_palette",
    ));
    b.push(KeyBindingSpec::new(
        format!("{p}-shift-f"),
        "workspace.project_search",
    ));
    b.push(KeyBindingSpec::new(
        format!("{p}-`"),
        "workspace.toggle_terminal",
    ));
    b.push(KeyBindingSpec::new(
        format!("{p}-,"),
        "workspace.open_settings",
    ));
    b
}

fn vim_bindings(p: &str) -> Vec<KeyBindingSpec> {
    let mut b = vec![
        // Mode transitions (the vim state machine lives in blur_editor)
        KeyBindingSpec::with_context("escape", "vim.normal_mode", "Editor"),
        KeyBindingSpec::with_context("i", "vim.insert_mode", "Editor && vim_mode == normal"),
        KeyBindingSpec::with_context("v", "vim.visual_mode", "Editor && vim_mode == normal"),
        KeyBindingSpec::with_context(
            ":",
            "workspace.command_palette",
            "Editor && vim_mode == normal",
        ),
        KeyBindingSpec::with_context(
            "g d",
            "editor.goto_definition",
            "Editor && vim_mode == normal",
        ),
        KeyBindingSpec::with_context(
            "g r",
            "editor.find_references",
            "Editor && vim_mode == normal",
        ),
        KeyBindingSpec::with_context("K", "editor.hover", "Editor && vim_mode == normal"),
        KeyBindingSpec::with_context(
            "space f",
            "workspace.file_finder",
            "Editor && vim_mode == normal",
        ),
        KeyBindingSpec::with_context(
            "space /",
            "workspace.project_search",
            "Editor && vim_mode == normal",
        ),
        KeyBindingSpec::with_context(
            "space p",
            "workspace.command_palette",
            "Editor && vim_mode == normal",
        ),
    ];

    // Standard workspace & palette shortcuts
    b.push(KeyBindingSpec::new(
        format!("{p}-shift-p"),
        "workspace.command_palette",
    ));
    b.push(KeyBindingSpec::new(
        format!("{p}-p"),
        "workspace.file_finder",
    ));
    b.push(KeyBindingSpec::new(
        format!("{p}-shift-f"),
        "workspace.project_search",
    ));
    b.push(KeyBindingSpec::new(
        format!("{p}-b"),
        "workspace.toggle_sidebar",
    ));
    b.push(KeyBindingSpec::new(
        format!("{p}-j"),
        "workspace.toggle_panel",
    ));
    b.push(KeyBindingSpec::new(
        format!("{p}-`"),
        "workspace.toggle_terminal",
    ));
    b.push(KeyBindingSpec::new(format!("{p}-s"), "editor.save"));
    b.push(KeyBindingSpec::new(
        format!("{p}-,"),
        "workspace.open_settings",
    ));
    b.push(KeyBindingSpec::new(format!("{p}-q"), "workspace.quit"));
    b
}

fn helix_bindings(p: &str) -> Vec<KeyBindingSpec> {
    let mut b = vec![
        KeyBindingSpec::with_context("escape", "helix.normal_mode", "Editor"),
        KeyBindingSpec::with_context("i", "helix.insert_mode", "Editor && helix_mode == normal"),
        KeyBindingSpec::with_context(
            "space f",
            "workspace.file_finder",
            "Editor && helix_mode == normal",
        ),
        KeyBindingSpec::with_context(
            "space /",
            "workspace.project_search",
            "Editor && helix_mode == normal",
        ),
        KeyBindingSpec::with_context(
            "space p",
            "workspace.command_palette",
            "Editor && helix_mode == normal",
        ),
        KeyBindingSpec::with_context(
            "g d",
            "editor.goto_definition",
            "Editor && helix_mode == normal",
        ),
        KeyBindingSpec::with_context(
            "g r",
            "editor.find_references",
            "Editor && helix_mode == normal",
        ),
        KeyBindingSpec::with_context(
            "space k",
            "editor.hover",
            "Editor && helix_mode == normal",
        ),
    ];

    b.push(KeyBindingSpec::new(
        format!("{p}-shift-p"),
        "workspace.command_palette",
    ));
    b.push(KeyBindingSpec::new(
        format!("{p}-p"),
        "workspace.file_finder",
    ));
    b.push(KeyBindingSpec::new(format!("{p}-s"), "editor.save"));
    b.push(KeyBindingSpec::new(
        format!("{p}-`"),
        "workspace.toggle_terminal",
    ));
    b.push(KeyBindingSpec::new(
        format!("{p}-,"),
        "workspace.open_settings",
    ));
    b.push(KeyBindingSpec::new(format!("{p}-q"), "workspace.quit"));
    b
}

#[derive(Deserialize)]
struct KeymapFile {
    #[serde(default)]
    binding: Vec<KeyBindingSpec>,
}

/// Loads user keybindings from a TOML file containing `[[binding]]` entries.
///
/// Returns an empty list if `path` does not exist.
pub fn load_user_keymap(path: &Path) -> Result<Vec<KeyBindingSpec>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read keymap file {}", path.display()))?;
    let parsed: KeymapFile = toml::from_str(&raw)
        .with_context(|| format!("failed to parse keymap file {}", path.display()))?;
    Ok(parsed.binding)
}

/// Formats GPUI keystroke syntax for display in menus and the command palette.
///
/// Examples:
/// - `display_keys("ctrl-shift-p", false)` -> `"Ctrl+Shift+P"`
/// - `display_keys("ctrl-shift-p", true)` -> `"⌃⇧P"`
/// - `display_keys("ctrl-k ctrl-s", false)` -> `"Ctrl+K Ctrl+S"`
pub fn display_keys(keys: &str, mac: bool) -> String {
    keys.split_whitespace()
        .map(|chord| format_chord(chord, mac))
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_chord(chord: &str, mac: bool) -> String {
    let tokens = split_chord_tokens(chord);
    let formatted: Vec<String> = tokens.into_iter().map(|t| format_token(t, mac)).collect();
    if mac {
        formatted.join("")
    } else {
        formatted.join("+")
    }
}

/// Splits a single keystroke chord on `-`, preserving a literal `-` key at the end (e.g. `ctrl--`).
fn split_chord_tokens(chord: &str) -> Vec<&str> {
    if chord == "-" {
        return vec!["-"];
    }
    if let Some(prefix) = chord.strip_suffix("--") {
        let mut parts: Vec<&str> = prefix.split('-').filter(|s| !s.is_empty()).collect();
        parts.push("-");
        return parts;
    }
    chord.split('-').filter(|s| !s.is_empty()).collect()
}

fn format_token(token: &str, mac: bool) -> String {
    let lower = token.to_ascii_lowercase();
    if mac {
        match lower.as_str() {
            "ctrl" | "control" => "⌃".to_string(),
            "shift" => "⇧".to_string(),
            "alt" | "option" => "⌥".to_string(),
            "cmd" | "super" | "meta" | "secondary" => "⌘".to_string(),
            "enter" | "return" => "↩".to_string(),
            "escape" | "esc" => "⎋".to_string(),
            "backspace" => "⌫".to_string(),
            "delete" | "del" => "⌦".to_string(),
            "tab" => "⇥".to_string(),
            "space" => "Space".to_string(),
            "up" => "↑".to_string(),
            "down" => "↓".to_string(),
            "left" => "←".to_string(),
            "right" => "→".to_string(),
            _ => format_key_name(token),
        }
    } else {
        match lower.as_str() {
            "ctrl" | "control" | "secondary" => "Ctrl".to_string(),
            "shift" => "Shift".to_string(),
            "alt" | "option" => "Alt".to_string(),
            "cmd" | "super" | "meta" => "Super".to_string(),
            "enter" | "return" => "Enter".to_string(),
            "escape" | "esc" => "Esc".to_string(),
            "backspace" => "Backspace".to_string(),
            "delete" | "del" => "Del".to_string(),
            "tab" => "Tab".to_string(),
            "space" => "Space".to_string(),
            "up" => "Up".to_string(),
            "down" => "Down".to_string(),
            "left" => "Left".to_string(),
            "right" => "Right".to_string(),
            "pageup" => "PageUp".to_string(),
            "pagedown" => "PageDown".to_string(),
            "home" => "Home".to_string(),
            "end" => "End".to_string(),
            "insert" => "Ins".to_string(),
            _ => format_key_name(token),
        }
    }
}

fn format_key_name(token: &str) -> String {
    if token.len() == 1 {
        return token.to_ascii_uppercase();
    }
    let lower = token.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix('f') {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return format!("F{rest}");
        }
    }
    token.to_string()
}
