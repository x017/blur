//! Layered TOML configuration and settings model.
//!
//! Configuration is deep-merged at the `toml::Value` table level in the order:
//! built-in defaults <- user (`~/.config/blur/config.toml`) <- workspace (`<root>/.blur/config.toml`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tracing::warn;

pub use crate::keymap::NavigationMode;
use crate::paths;

/// Top-level Blur settings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub editor: EditorSettings,
    pub ui: UiSettings,
    pub terminal: TerminalSettings,
    pub git: GitSettings,
    pub lsp: LspSettings,
    pub files: FileSettings,
    pub extensions: ExtensionSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            editor: EditorSettings::default(),
            ui: UiSettings::default(),
            terminal: TerminalSettings::default(),
            git: GitSettings::default(),
            lsp: LspSettings::default(),
            files: FileSettings::default(),
            extensions: ExtensionSettings::default(),
        }
    }
}

impl Settings {
    /// Renders a fully commented default TOML configuration suitable for export or first run.
    pub fn default_toml() -> String {
        r#"# Blur Configuration
# User config: ~/.config/blur/config.toml
# Workspace overrides: <project>/.blur/config.toml

[editor]
# Monospace font family used in code editors.
font_family = "JetBrains Mono"
# Editor font size in points.
font_size = 13.5
# Line height multiplier.
line_height = 1.65
# Number of spaces per tab stop.
tab_size = 4
# Insert spaces instead of hard tab characters.
insert_spaces = true
# Strip trailing whitespace on save.
trim_trailing_whitespace = true
# Run language server formatter automatically on save.
format_on_save = false
# Display relative line numbers in the gutter.
relative_line_numbers = false
# Blink the text cursor.
cursor_blink = true
# Soft-wrap long lines at the viewport edge.
soft_wrap = false
# Keybinding / editing mode: "standard", "vim", "emacs", or "helix".
navigation_mode = "standard"
# Automatically close brackets and quotes.
auto_pairs = true
# Render whitespace characters.
show_whitespace = false
# Optional vertical ruler column (uncomment to enable).
# max_column = 100
# Show LSP inlay hints inline.
inlay_hints = true

[ui]
# Active color theme name.
theme = "Furusato Slate"
# Proportional font family used for UI chrome.
ui_font_family = "Geist"
# UI font size in points.
ui_font_size = 13.0
# Global UI scale multiplier (0.75 to 1.75).
ui_scale = 1.0
# Layout spacing density: "compact" or "comfortable".
density = "comfortable"
# Show the editor minimap.
show_minimap = false
# Start in distraction-free Zen Mode.
zen_mode = false
# Visible top-bar navigation tabs. Any of: "editor", "file_finder",
# "version_control", "protocols_lsp", "utilities", "theme", "settings".
nav_tabs = ["editor", "file_finder", "version_control", "protocols_lsp", "utilities", "theme", "settings"]
# Header / editor chrome item visibility (customizable in Settings > Tabs & Page Navigation).
top_bar_breadcrumb = true
top_bar_search = true
top_bar_stats = true
top_bar_actions = true
top_bar_quick_actions = true

[terminal]
# Override default shell executable (uses $SHELL when unset).
# shell = "/bin/bash"
# Additional arguments passed to the shell.
args = []
# Extra environment variables for terminal sessions.
env = {}
# Optional terminal font family override (inherits editor.font_family when unset).
# font_family = "JetBrains Mono"
# Optional terminal font size override (inherits editor.font_size when unset).
# font_size = 13.5
# Number of scrollback lines retained in memory.
scrollback = 10000
# Blink the terminal cursor.
cursor_blink = true
# Automatically copy selected text to clipboard.
copy_on_select = false

[git]
# Enable Git integration.
enabled = true
# Periodically fetch from remotes in the background.
auto_fetch = false
# Interval in seconds between automatic fetches.
auto_fetch_interval_secs = 300
# Show inline git blame annotation on the active line.
show_inline_blame = false
# Show added/modified/deleted indicators in the editor gutter.
show_gutter_diff = true
# Prompt before running git push/pull sync actions.
confirm_sync = false

[lsp]
# Enable Language Server Protocol integration.
enabled = true

# Override or add language servers by name, e.g.:
# [lsp.servers.rust-analyzer]
# command = "rust-analyzer"
# args = []
# env = {}
# languages = ["rust"]
# root_markers = ["Cargo.toml"]
# enabled = true

[files]
# Glob patterns excluded from file tree, index, and search.
exclude = [".git", "node_modules", "target", ".DS_Store"]
# Watch workspace files for external changes.
watch = true
# Show dotfiles and hidden directories.
show_hidden = false
# Respect .gitignore rules when listing and searching files.
respect_gitignore = true
# Persist unsaved workspace layout between sessions.
hot_exit = true

[extensions]
# Enable Lua extension host.
enabled = true
# Extension IDs to keep disabled.
disabled = []
# Granted permissions per extension ID.
granted_permissions = {}
"#
        .to_string()
    }
}

/// Editor appearance and behavior settings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct EditorSettings {
    pub font_family: String,
    pub font_size: f32,
    pub line_height: f32,
    pub tab_size: usize,
    pub insert_spaces: bool,
    pub trim_trailing_whitespace: bool,
    pub format_on_save: bool,
    pub relative_line_numbers: bool,
    pub cursor_blink: bool,
    pub soft_wrap: bool,
    pub navigation_mode: NavigationMode,
    pub auto_pairs: bool,
    pub show_whitespace: bool,
    pub max_column: Option<u32>,
    pub inlay_hints: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_family: "JetBrains Mono".to_string(),
            font_size: 13.5,
            line_height: 1.65,
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: true,
            format_on_save: false,
            relative_line_numbers: false,
            cursor_blink: true,
            soft_wrap: false,
            navigation_mode: NavigationMode::Standard,
            auto_pairs: true,
            show_whitespace: false,
            max_column: None,
            inlay_hints: true,
        }
    }
}

/// UI chrome spacing density.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UiDensity {
    Compact,
    #[default]
    Comfortable,
}

/// Default set of top-bar navigation tabs (all visible).
fn default_nav_tabs() -> Vec<String> {
    [
        "editor",
        "file_finder",
        "version_control",
        "protocols_lsp",
        "utilities",
        "theme",
        "settings",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// User interface theme and layout settings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct UiSettings {
    pub theme: String,
    pub ui_font_family: String,
    pub ui_font_size: f32,
    pub ui_scale: f32,
    pub density: UiDensity,
    pub show_minimap: bool,
    pub zen_mode: bool,
    /// Visible top-bar navigation tabs (keys: `editor`, `file_finder`,
    /// `version_control`, `protocols_lsp`, `utilities`, `theme`, `settings`).
    pub nav_tabs: Vec<String>,
    /// Show the active-file breadcrumb in the header bar.
    pub top_bar_breadcrumb: bool,
    /// Show the centered command-palette search trigger in the editor quiet bar.
    pub top_bar_search: bool,
    /// Show ambient stats (lines / tabs) in the editor quiet bar.
    pub top_bar_stats: bool,
    /// Show the preview / terminal quick-action buttons in the editor quiet bar.
    pub top_bar_actions: bool,
    /// Show the palette summoner + theme switcher buttons in the header bar.
    pub top_bar_quick_actions: bool,
    /// Corner radius (px) applied to the header nav pills / tab section.
    /// Driven live from the Theme Studio geometry cards.
    pub tab_radius: f32,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            theme: "Furusato Slate".to_string(),
            ui_font_family: "Geist".to_string(),
            ui_font_size: 13.0,
            ui_scale: 1.0,
            density: UiDensity::Comfortable,
            show_minimap: false,
            zen_mode: false,
            nav_tabs: default_nav_tabs(),
            top_bar_breadcrumb: true,
            top_bar_search: true,
            top_bar_stats: true,
            top_bar_actions: true,
            top_bar_quick_actions: true,
            tab_radius: 8.0,
        }
    }
}

/// Integrated terminal settings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct TerminalSettings {
    pub shell: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub scrollback: usize,
    pub cursor_blink: bool,
    pub copy_on_select: bool,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            shell: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            font_family: None,
            font_size: None,
            scrollback: 10_000,
            cursor_blink: true,
            copy_on_select: false,
        }
    }
}

/// Git integration settings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct GitSettings {
    pub enabled: bool,
    pub auto_fetch: bool,
    pub auto_fetch_interval_secs: u64,
    pub show_inline_blame: bool,
    pub show_gutter_diff: bool,
    pub confirm_sync: bool,
}

impl Default for GitSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_fetch: false,
            auto_fetch_interval_secs: 300,
            show_inline_blame: false,
            show_gutter_diff: true,
            confirm_sync: false,
        }
    }
}

/// LSP configuration; `servers` is empty by default and overrides `blur_lsp` presets by name.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LspSettings {
    pub enabled: bool,
    pub servers: BTreeMap<String, LspServerEntry>,
}

impl Default for LspSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            servers: BTreeMap::new(),
        }
    }
}

/// Configuration for a single language server entry.
///
/// Mirrors `blur_lsp::ServerConfig` fields (the map key in `LspSettings::servers` is the server name).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LspServerEntry {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub languages: Vec<String>,
    pub root_markers: Vec<String>,
    pub initialization_options: Option<toml::Value>,
    pub settings: Option<toml::Value>,
    pub enabled: bool,
}

impl Default for LspServerEntry {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            env: BTreeMap::new(),
            languages: Vec::new(),
            root_markers: Vec::new(),
            initialization_options: None,
            settings: None,
            enabled: true,
        }
    }
}

impl LspServerEntry {
    /// Converts `initialization_options` from `toml::Value` to `serde_json::Value`.
    pub fn initialization_options_json(&self) -> Option<serde_json::Value> {
        self.initialization_options.as_ref().map(toml_to_json)
    }

    /// Converts `settings` from `toml::Value` to `serde_json::Value`.
    pub fn settings_json(&self) -> Option<serde_json::Value> {
        self.settings.as_ref().map(toml_to_json)
    }
}

/// Converts a `toml::Value` into a `serde_json::Value`.
pub fn toml_to_json(value: &toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::Value::Number((*i).into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(toml_to_json).collect())
        }
        toml::Value::Table(table) => {
            let map = table
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
    }
}

/// File discovery, watching, and session settings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct FileSettings {
    pub exclude: Vec<String>,
    pub watch: bool,
    pub show_hidden: bool,
    pub respect_gitignore: bool,
    pub hot_exit: bool,
}

impl Default for FileSettings {
    fn default() -> Self {
        Self {
            exclude: vec![
                ".git".to_string(),
                "node_modules".to_string(),
                "target".to_string(),
                ".DS_Store".to_string(),
            ],
            watch: true,
            show_hidden: false,
            respect_gitignore: true,
            hot_exit: true,
        }
    }
}

/// Extension host settings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ExtensionSettings {
    pub enabled: bool,
    pub disabled: Vec<String>,
    pub granted_permissions: BTreeMap<String, Vec<String>>,
}

impl Default for ExtensionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            disabled: Vec::new(),
            granted_permissions: BTreeMap::new(),
        }
    }
}

/// Recursively merges `overlay` into `base` at the TOML table level.
///
/// Non-table values in `overlay` replace the corresponding entry in `base`.
pub fn deep_merge_toml(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base_map), toml::Value::Table(overlay_map)) => {
            for (key, value) in overlay_map {
                match base_map.get_mut(&key) {
                    Some(existing) => deep_merge_toml(existing, value),
                    None => {
                        base_map.insert(key, value);
                    }
                }
            }
        }
        (base_slot, overlay_val) => {
            *base_slot = overlay_val;
        }
    }
}

/// Layered configuration store managing built-in defaults, user config, and workspace overrides.
#[derive(Clone, Debug)]
pub struct ConfigStore {
    user_path: PathBuf,
    workspace_path: Option<PathBuf>,
    settings: Settings,
    user_raw: String,
    effective_value: toml::Value,
    errors: Vec<String>,
}

impl ConfigStore {
    /// Loads configuration from the standard user config path and optional workspace root.
    pub fn load(workspace_root: Option<&Path>) -> Self {
        let user_path = paths::user_config_file();
        let workspace_path = workspace_root.map(paths::workspace_config_file);
        Self::load_from_paths(user_path, workspace_path)
    }

    /// Loads configuration from explicit user and optional workspace config file paths.
    pub fn load_from_paths(user_path: PathBuf, workspace_path: Option<PathBuf>) -> Self {
        let default_settings = Settings::default();
        let effective_value =
            toml::Value::try_from(&default_settings).unwrap_or(toml::Value::Table(toml::Table::new()));
        let mut store = Self {
            user_path,
            workspace_path,
            settings: default_settings,
            user_raw: String::new(),
            effective_value,
            errors: Vec::new(),
        };
        let _ = store.reload();
        store
    }

    /// Returns the effective merged settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Reloads user and workspace config files from disk.
    ///
    /// Parse or deserialization errors are recorded in [`ConfigStore::errors`] and previous
    /// valid (or default) settings are preserved.
    pub fn reload(&mut self) -> Result<()> {
        self.errors.clear();
        let mut merged = toml::Value::try_from(&Settings::default())
            .context("failed to serialize default settings")?;

        // Layer 1: user config
        if self.user_path.exists() {
            match std::fs::read_to_string(&self.user_path) {
                Ok(raw) => {
                    self.user_raw = raw.clone();
                    match toml::from_str::<toml::Value>(&raw) {
                        Ok(val) => deep_merge_toml(&mut merged, val),
                        Err(err) => {
                            let msg = format!(
                                "Failed to parse user config {}: {}",
                                self.user_path.display(),
                                err
                            );
                            warn!("{msg}");
                            self.errors.push(msg);
                        }
                    }
                }
                Err(err) => {
                    let msg = format!(
                        "Failed to read user config {}: {}",
                        self.user_path.display(),
                        err
                    );
                    warn!("{msg}");
                    self.errors.push(msg);
                }
            }
        } else {
            self.user_raw.clear();
        }

        // Layer 2: workspace config
        if let Some(ws_path) = &self.workspace_path {
            if ws_path.exists() {
                match std::fs::read_to_string(ws_path) {
                    Ok(raw) => match toml::from_str::<toml::Value>(&raw) {
                        Ok(val) => deep_merge_toml(&mut merged, val),
                        Err(err) => {
                            let msg = format!(
                                "Failed to parse workspace config {}: {}",
                                ws_path.display(),
                                err
                            );
                            warn!("{msg}");
                            self.errors.push(msg);
                        }
                    },
                    Err(err) => {
                        let msg = format!(
                            "Failed to read workspace config {}: {}",
                            ws_path.display(),
                            err
                        );
                        warn!("{msg}");
                        self.errors.push(msg);
                    }
                }
            }
        }

        match merged.clone().try_into::<Settings>() {
            Ok(parsed) => {
                self.settings = parsed;
                self.effective_value = merged;
            }
            Err(err) => {
                let msg = format!("Invalid configuration value: {err}");
                warn!("{msg}");
                self.errors.push(msg);
            }
        }

        Ok(())
    }

    /// Returns the raw contents of the user config file (empty string if missing).
    pub fn user_toml(&self) -> String {
        self.user_raw.clone()
    }

    /// Returns the effective merged configuration serialized as pretty TOML.
    pub fn effective_toml(&self) -> String {
        toml::to_string_pretty(&self.effective_value)
            .or_else(|_| toml::to_string_pretty(&self.settings))
            .unwrap_or_default()
    }

    /// Updates a single dotted key (e.g. `"editor.font_size"`) in the user config file,
    /// preserving all other keys, writes the file, and reloads the store.
    pub fn set_user_value(&mut self, dotted_key: &str, value: toml::Value) -> Result<()> {
        self.set_user_values(&[(dotted_key, value)])
    }

    /// Updates several dotted keys in the user config file in a single read/modify/write
    /// cycle, then reloads the store once. Keys are applied in order, so later keys win
    /// when they address the same leaf.
    pub fn set_user_values(&mut self, values: &[(&str, toml::Value)]) -> Result<()> {
        if values.is_empty() {
            return Ok(());
        }
        for (key, _) in values {
            let segments: Vec<&str> = key.split('.').filter(|s| !s.is_empty()).collect();
            if segments.is_empty() {
                return Err(anyhow!("configuration key cannot be empty"));
            }
        }

        let mut root_table = if self.user_path.exists() {
            let raw = std::fs::read_to_string(&self.user_path).with_context(|| {
                format!("failed to read user config {}", self.user_path.display())
            })?;
            if raw.trim().is_empty() {
                toml::Table::new()
            } else {
                toml::from_str::<toml::Table>(&raw).with_context(|| {
                    format!(
                        "cannot edit user config {} because it contains invalid TOML",
                        self.user_path.display()
                    )
                })?
            }
        } else {
            toml::Table::new()
        };

        for (dotted_key, value) in values {
            let segments: Vec<&str> = dotted_key.split('.').filter(|s| !s.is_empty()).collect();
            let mut current = &mut root_table;
            for &segment in &segments[..segments.len() - 1] {
                let entry = current
                    .entry(segment.to_string())
                    .or_insert_with(|| toml::Value::Table(toml::Table::new()));
                if !entry.is_table() {
                    *entry = toml::Value::Table(toml::Table::new());
                }
                current = entry
                    .as_table_mut()
                    .ok_or_else(|| anyhow!("failed to descend into table '{segment}'"))?;
            }

            let leaf = segments[segments.len() - 1];
            current.insert(leaf.to_string(), value.clone());
        }

        if let Some(parent) = self.user_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }

        let serialized = toml::to_string_pretty(&root_table)
            .context("failed to serialize updated user config")?;
        std::fs::write(&self.user_path, &serialized).with_context(|| {
            format!("failed to write user config {}", self.user_path.display())
        })?;

        self.reload()
    }

    /// Returns parse or validation errors encountered during the most recent load/reload.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Returns the path of the user config file backing this store.
    pub fn user_path(&self) -> &Path {
        &self.user_path
    }

    /// Returns the path of the workspace config file, if any.
    pub fn workspace_path(&self) -> Option<&Path> {
        self.workspace_path.as_deref()
    }
}
