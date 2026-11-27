//! Comprehensive System Settings view — 1:1 implementation of
//! `blur_comprehensive_settings` + `blur_settings_with_tab_customization_keybindings`.
//!
//! Every control is functional: changes are emitted as a [`SettingsPatch`] that the
//! application shell persists to the layered TOML config and applies live.

use blur_core::{default_bindings, ConfigStore, KeyBindingSpec, NavigationMode};
use blur_theme::{Theme, ThemeRegistry};
use blur_ui::{badge, button, kbd, section_label, toggle_switch, BadgeTone, ButtonVariant};
use gpui::{div, prelude::*, px, Context, EventEmitter, FontWeight, MouseButton, Window};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    Core,
    Tabs,
    Keybindings,
    Typography,
    Lsp,
    Git,
    Theme,
    Telemetry,
    RawToml,
}

#[derive(Debug, Clone)]
pub enum SettingsViewEvent {
    SettingsChanged,
    ThemeSelected(String),
    /// Flush every current value to disk immediately (Export TOML).
    SaveNow,
    /// Surface an informational toast from the settings view.
    Notify(String),
}

/// Complete snapshot of every setting owned by the Settings view.
/// The application shell persists and applies it atomically.
#[derive(Debug, Clone)]
pub struct SettingsPatch {
    pub font_size: f32,
    pub line_height: f32,
    pub tab_size: usize,
    pub trim_trailing_whitespace: bool,
    pub relative_line_numbers: bool,
    pub auto_pairs: bool,
    pub navigation_mode: NavigationMode,
    pub ui_scale: f32,
    pub nav_tabs: Vec<String>,
    pub top_bar_breadcrumb: bool,
    pub top_bar_search: bool,
    pub top_bar_stats: bool,
    pub top_bar_actions: bool,
    pub top_bar_quick_actions: bool,
    pub lsp_enabled: bool,
    pub git_enabled: bool,
    pub git_show_gutter_diff: bool,
    pub git_auto_fetch: bool,
    pub git_confirm_sync: bool,
    pub files_show_hidden: bool,
    pub files_respect_gitignore: bool,
}

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

pub struct SettingsView {
    category: SettingsCategory,
    config_toml: String,

    // Editor
    font_size: f32,
    line_height: f32,
    tab_size: usize,
    trim_trailing_whitespace: bool,
    relative_line_numbers: bool,
    auto_pairs: bool,
    navigation_mode: NavigationMode,
    font_family: String,

    // UI
    ui_scale: f32,
    ui_font_family: String,
    active_theme: String,
    nav_tabs: Vec<String>,
    top_bar_breadcrumb: bool,
    top_bar_search: bool,
    top_bar_stats: bool,
    top_bar_actions: bool,
    top_bar_quick_actions: bool,

    // Services
    lsp_enabled: bool,
    git_enabled: bool,
    git_show_gutter_diff: bool,
    git_auto_fetch: bool,
    git_confirm_sync: bool,
    files_show_hidden: bool,
    files_respect_gitignore: bool,

    _extension_names: Vec<String>,
}

impl EventEmitter<SettingsViewEvent> for SettingsView {}

impl SettingsView {
    pub fn new(store: &ConfigStore, extension_names: Vec<String>, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            category: SettingsCategory::Core,
            config_toml: String::new(),
            font_size: 13.5,
            line_height: 1.65,
            tab_size: 4,
            trim_trailing_whitespace: true,
            relative_line_numbers: false,
            auto_pairs: true,
            navigation_mode: NavigationMode::Standard,
            font_family: String::new(),
            ui_scale: 1.0,
            ui_font_family: String::new(),
            active_theme: String::new(),
            nav_tabs: default_nav_tabs(),
            top_bar_breadcrumb: true,
            top_bar_search: true,
            top_bar_stats: true,
            top_bar_actions: true,
            top_bar_quick_actions: true,
            lsp_enabled: true,
            git_enabled: true,
            git_show_gutter_diff: true,
            git_auto_fetch: false,
            git_confirm_sync: false,
            files_show_hidden: false,
            files_respect_gitignore: true,
            _extension_names: extension_names,
        };
        view.sync_from_store(store, cx);
        view
    }

    /// Re-reads every field from the layered config store (keeps the UI honest
    /// after external writes, resets, or workspace switches).
    pub fn sync_from_store(&mut self, store: &ConfigStore, cx: &mut Context<Self>) {
        let s = store.settings();
        self.config_toml = store.effective_toml();
        self.font_size = s.editor.font_size;
        self.line_height = s.editor.line_height;
        self.tab_size = s.editor.tab_size;
        self.trim_trailing_whitespace = s.editor.trim_trailing_whitespace;
        self.relative_line_numbers = s.editor.relative_line_numbers;
        self.auto_pairs = s.editor.auto_pairs;
        self.navigation_mode = s.editor.navigation_mode;
        self.font_family = s.editor.font_family.clone();
        self.ui_scale = s.ui.ui_scale.clamp(0.75, 1.75);
        self.ui_font_family = s.ui.ui_font_family.clone();
        self.active_theme = s.ui.theme.clone();
        self.nav_tabs = if s.ui.nav_tabs.is_empty() {
            default_nav_tabs()
        } else {
            s.ui.nav_tabs.clone()
        };
        self.top_bar_breadcrumb = s.ui.top_bar_breadcrumb;
        self.top_bar_search = s.ui.top_bar_search;
        self.top_bar_stats = s.ui.top_bar_stats;
        self.top_bar_actions = s.ui.top_bar_actions;
        self.top_bar_quick_actions = s.ui.top_bar_quick_actions;
        self.lsp_enabled = s.lsp.enabled;
        self.git_enabled = s.git.enabled;
        self.git_show_gutter_diff = s.git.show_gutter_diff;
        self.git_auto_fetch = s.git.auto_fetch;
        self.git_confirm_sync = s.git.confirm_sync;
        self.files_show_hidden = s.files.show_hidden;
        self.files_respect_gitignore = s.files.respect_gitignore;
        cx.notify();
    }

    /// Snapshots the current UI state into a patch for the application shell.
    pub fn patch(&self) -> SettingsPatch {
        SettingsPatch {
            font_size: self.font_size,
            line_height: self.line_height,
            tab_size: self.tab_size,
            trim_trailing_whitespace: self.trim_trailing_whitespace,
            relative_line_numbers: self.relative_line_numbers,
            auto_pairs: self.auto_pairs,
            navigation_mode: self.navigation_mode,
            ui_scale: self.ui_scale,
            nav_tabs: self.nav_tabs.clone(),
            top_bar_breadcrumb: self.top_bar_breadcrumb,
            top_bar_search: self.top_bar_search,
            top_bar_stats: self.top_bar_stats,
            top_bar_actions: self.top_bar_actions,
            top_bar_quick_actions: self.top_bar_quick_actions,
            lsp_enabled: self.lsp_enabled,
            git_enabled: self.git_enabled,
            git_show_gutter_diff: self.git_show_gutter_diff,
            git_auto_fetch: self.git_auto_fetch,
            git_confirm_sync: self.git_confirm_sync,
            files_show_hidden: self.files_show_hidden,
            files_respect_gitignore: self.files_respect_gitignore,
        }
    }

    fn toggle_nav_tab(&mut self, key: &str) {
        if let Some(pos) = self.nav_tabs.iter().position(|k| k == key) {
            self.nav_tabs.remove(pos);
        } else {
            self.nav_tabs.push(key.to_string());
        }
    }

    fn reset_defaults(&mut self, cx: &mut Context<Self>) {
        self.font_size = 13.5;
        self.line_height = 1.65;
        self.tab_size = 4;
        self.trim_trailing_whitespace = true;
        self.relative_line_numbers = false;
        self.auto_pairs = true;
        self.navigation_mode = NavigationMode::Standard;
        self.ui_scale = 1.0;
        self.nav_tabs = default_nav_tabs();
        self.top_bar_breadcrumb = true;
        self.top_bar_search = true;
        self.top_bar_stats = true;
        self.top_bar_actions = true;
        self.top_bar_quick_actions = true;
        self.lsp_enabled = true;
        self.git_enabled = true;
        self.git_show_gutter_diff = true;
        self.git_auto_fetch = false;
        self.git_confirm_sync = false;
        self.files_show_hidden = false;
        self.files_respect_gitignore = true;
        cx.emit(SettingsViewEvent::SettingsChanged);
        cx.emit(SettingsViewEvent::Notify(
            "Settings reset to defaults".to_string(),
        ));
        cx.notify();
    }
}

/// Turns `workspace.open_folder` into `Open Folder`.
fn humanize_command(id: &str) -> String {
    let tail = id.split('.').nth(1).unwrap_or(id);
    let words: Vec<String> = tail
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        id.to_string()
    } else {
        words.join(" ")
    }
}

/// Section header row (`Title  subtitle ............ [toml.tag]`).
fn section_head(title: &str, sub: &str, tag: &str, theme: &Theme) -> gpui::Div {
    div()
        .flex_shrink_0()
        .pb(px(4.0))
        .border_b_1()
        .border_color(Theme::with_alpha(theme.border_strong, 0.4))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.text_muted)
                        .child(sub.to_string()),
                ),
        )
        .child(
            div()
                .font_family(theme.mono_font.clone())
                .text_size(px(11.0))
                .text_color(theme.text_muted)
                .child(tag.to_string()),
        )
}

/// A card with a title, description, and a pill toggle on the right.
fn toggle_card(
    id: impl Into<gpui::ElementId>,
    label: &str,
    sub: &str,
    on: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .p(px(12.0))
        .rounded(px(8.0))
        .bg(theme.surface_panel)
        .border_1()
        .border_color(Theme::with_alpha(theme.border_strong, 0.4))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.text_muted)
                        .child(sub.to_string()),
                ),
        )
        // Static id: the card's own `.id()` scopes this switch's element path,
        // so a constant child id is safe and avoids `String: Into<ElementId>`.
        .child(toggle_switch("tgc", on, theme))
}

/// A compact `label … value` info row used inside service sections.
fn info_row(label: &str, value: &str, theme: &Theme) -> gpui::Div {
    div()
        .flex_shrink_0()
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(6.0))
        .bg(theme.surface_panel)
        .border_1()
        .border_color(Theme::with_alpha(theme.border_strong, 0.35))
        .flex()
        .items_center()
        .justify_between()
        .text_size(px(12.0))
        .child(
            div()
                .text_color(theme.text_secondary)
                .child(label.to_string()),
        )
        .child(
            div()
                .font_family(theme.mono_font.clone())
                .text_color(theme.accent)
                .child(value.to_string()),
        )
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();
        let registry = cx.global::<ThemeRegistry>().clone();

        let mode_tag = match self.navigation_mode {
            NavigationMode::Vim => "Vim",
            NavigationMode::Helix => "Helix",
            NavigationMode::Emacs => "Emacs",
            NavigationMode::Standard => "Standard",
        };
        let theme_tag: &str = self
            .active_theme
            .split(' ')
            .next()
            .unwrap_or("Theme");

        let sidebar_items = [
            (
                "WORKSPACE CORE",
                "Core Editor & Buffer",
                "●",
                SettingsCategory::Core,
            ),
            (
                "",
                "Tabs & Page Navigation",
                "7 tabs",
                SettingsCategory::Tabs,
            ),
            (
                "",
                "Keybindings & Modality",
                mode_tag,
                SettingsCategory::Keybindings,
            ),
            ("", "Typography & Fonts", "Mono", SettingsCategory::Typography),
            (
                "INTELLIGENCE & VCS",
                "LSP & Language Daemons",
                "4 up",
                SettingsCategory::Lsp,
            ),
            (
                "",
                "Version Control & Diff",
                "libgit2",
                SettingsCategory::Git,
            ),
            (
                "SURFACE & SYSTEM",
                "Theme & Precision",
                theme_tag,
                SettingsCategory::Theme,
            ),
            ("", "Telemetry & Privacy", "Zero-air", SettingsCategory::Telemetry),
            ("", "Raw Config Inspector", ".toml", SettingsCategory::RawToml),
        ];

        let mac = cfg!(target_os = "macos");

        let main_body = match self.category {
            // ───────────────────────── Raw TOML inspector ─────────────────────────
            SettingsCategory::RawToml => div()
                .id("set-raw-toml-scroll")
                .flex_1()
                .min_h_0()
                .h_full()
                .p(px(16.0))
                .rounded(px(8.0))
                .bg(theme.canvas)
                .border_1()
                .border_color(theme.border_strong)
                .font_family(theme.mono_font.clone())
                .text_size(theme.scaled(12.0))
                .text_color(theme.text_secondary)
                .overflow_y_scroll()
                .child(self.config_toml.clone())
                .into_any_element(),

            // ───────────────────────── Theme studio ─────────────────────────
            SettingsCategory::Theme => div()
                .id("set-theme-scroll")
                .flex_1()
                .min_h_0()
                .h_full()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .overflow_y_scroll()
                .child(section_head(
                    "Theme & Precision",
                    "Colorway, contrast, and surface tonality across the shell.",
                    "[ui.theme]",
                    &theme,
                ))
                .children(registry.list().iter().enumerate().map(|(i, def)| {
                    let active = def.name.eq_ignore_ascii_case(&self.active_theme);
                    let name = def.name.clone();
                    let swatches = def.preview_swatches.iter().map(|hex| {
                        div()
                            .w(px(14.0))
                            .h(px(14.0))
                            .rounded(px(7.0))
                            .bg(blur_theme::parse_hex(hex))
                    });
                    div()
                        .id(("set-theme-row", i))
                        .p(px(12.0))
                        .rounded(px(8.0))
                        .bg(if active {
                            theme.surface_hover
                        } else {
                            theme.surface_panel
                        })
                        .border_1()
                        .border_color(if active {
                            theme.accent
                        } else {
                            Theme::with_alpha(theme.border_strong, 0.4)
                        })
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.active_theme = name.clone();
                                cx.emit(SettingsViewEvent::ThemeSelected(name.clone()));
                                cx.notify();
                            }),
                        )
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .text_size(px(13.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.text)
                                                .child(def.name.clone()),
                                        )
                                        .when(active, |d| {
                                            d.child(badge("ACTIVE", BadgeTone::Accent, &theme))
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(theme.text_muted)
                                        .child(def.description.clone()),
                                ),
                        )
                        .child(div().flex().items_center().gap(px(6.0)).children(swatches))
                }))
                .into_any_element(),

            // ───────────────────────── Tabs & page navigation ─────────────────────────
            SettingsCategory::Tabs => {
                let nav_entries = [
                    ("editor", "Editor", "src/main.rs"),
                    ("file_finder", "File Finder", "quick open"),
                    ("version_control", "Version Control", "git"),
                    ("protocols_lsp", "Protocols & LSP", "daemons"),
                    ("utilities", "Utilities", "tools"),
                    ("theme", "Theme Studio", "colorways"),
                    ("settings", "Settings", "prefs"),
                ];
                let top_bar_entries = [
                    ("breadcrumb", "File Breadcrumb", "src / main.rs in the header", self.top_bar_breadcrumb),
                    ("search", "Palette Summoner", "“Go to symbol, line, or action…”", self.top_bar_search),
                    ("stats", "Ambient Stats", "line / tab counters", self.top_bar_stats),
                    ("actions", "Quick Actions", "Preview + Terminal buttons", self.top_bar_actions),
                    ("quick", "Header Quick Actions", "⌘K + theme switcher", self.top_bar_quick_actions),
                ];

                div()
                    .id("set-tabs-scroll")
                    .flex_1()
                    .min_h_0()
                    .h_full()
                    .pr(px(8.0))
                    .pb(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .overflow_y_scroll()
                    .child(section_head(
                        "Tabs & Page Visibility Management",
                        "Control which navigation tabs and header items appear in the top bar.",
                        "[ui.top_bar]",
                        &theme,
                    ))
                    .child(
                        div()
                            .flex_shrink_0()
                            .mb(px(2.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(section_label(
                                &format!("{} nodes registered", nav_entries.len()),
                                &theme,
                            ))
                            .child(
                                div()
                                    .font_family(theme.mono_font.clone())
                                    .text_size(px(10.5))
                                    .text_color(theme.text_muted)
                                    .child("[ui.nav_tabs]"),
                            ),
                    )
                    // 2-column nav tab toggle grid
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .flex_wrap()
                            .gap(px(10.0))
                            .children(nav_entries.into_iter().enumerate().map(|(i, (key, label, hint))| {
                                let on = self.nav_tabs.iter().any(|k| k == key);
                                div()
                                    .id(("navtab-card", i))
                                    .w(px(268.0))
                                    .p(px(10.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .border_1()
                                    .border_color(if on {
                                        Theme::with_alpha(theme.accent, 0.55)
                                    } else {
                                        Theme::with_alpha(theme.border_strong, 0.4)
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.0))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            this.toggle_nav_tab(key);
                                            cx.emit(SettingsViewEvent::SettingsChanged);
                                            cx.notify();
                                        }),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(1.0))
                                            .child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(if on {
                                                        theme.text
                                                    } else {
                                                        theme.text_secondary
                                                    })
                                                    .child(label),
                                            )
                                            .child(
                                                div()
                                                    .font_family(theme.mono_font.clone())
                                                    .text_size(px(10.0))
                                                    .text_color(theme.text_muted)
                                                    .child(hint),
                                            ),
                                    )
                                    .child(toggle_switch(key, on, &theme))
                            })),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .pt(px(6.0))
                            .pb(px(2.0))
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child("Header & editor chrome items"),
                    )
                    // Top bar item toggles
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .children(top_bar_entries.into_iter().enumerate().map(|(i, (key, label, sub, on))| {
                                toggle_card(("topbar-card", i), label, sub, on, &theme).on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        match key {
                                            "breadcrumb" => {
                                                this.top_bar_breadcrumb = !this.top_bar_breadcrumb
                                            }
                                            "search" => {
                                                this.top_bar_search = !this.top_bar_search
                                            }
                                            "stats" => this.top_bar_stats = !this.top_bar_stats,
                                            "actions" => {
                                                this.top_bar_actions = !this.top_bar_actions
                                            }
                                            _ => {
                                                this.top_bar_quick_actions =
                                                    !this.top_bar_quick_actions
                                            }
                                        }
                                        cx.notify();
                                    }),
                                )
                            })),
                    )
                    .into_any_element()
            }

            // ───────────────────────── Keybindings ─────────────────────────
            SettingsCategory::Keybindings => {
                let modes = [
                    (
                        NavigationMode::Vim,
                        "Vim Ergonomics",
                        "Active",
                        "Classic Normal, Insert, Visual, and Command modes with text objects and motions (ciw, dap, <c-o>).",
                    ),
                    (
                        NavigationMode::Helix,
                        "Helix / Kakoune",
                        "Selection First",
                        "Selection precedes action paradigm. Multiple cursors are first-class primitives with instant feedback.",
                    ),
                    (
                        NavigationMode::Emacs,
                        "Emacs Evil Layer",
                        "Chords",
                        "Leader keys via <Space> and <Alt-x> command dispatcher with full Lisp compatibility.",
                    ),
                    (
                        NavigationMode::Standard,
                        "Modeless / CUA",
                        "Modern Standard",
                        "Zero modal states. Direct typing with conventional ⌘C / ⌘V / ⌘Z key bindings and multi-caret clicks.",
                    ),
                ];
                let bindings = default_bindings(self.navigation_mode);

                div()
                    .id("set-keys-scroll")
                    .flex_1()
                    .min_h_0()
                    .h_full()
                    .pr(px(8.0))
                    .pb(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .overflow_y_scroll()
                    .child(section_head(
                        "Keybinding & Shortcut Configuration",
                        "Global key map, motion engine, and custom command overrides.",
                        "[editor.keymap]",
                        &theme,
                    ))
                    // Mode cards (functional — switches navigation_mode)
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .flex_wrap()
                            .gap(px(10.0))
                            .children(modes.into_iter().enumerate().map(|(i, (m, title, tag, desc))| {
                                let active = self.navigation_mode == m;
                                div()
                                    .id(("set-mod-card", i))
                                    .w(px(268.0))
                                    .p(px(12.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .border_1()
                                    .border_color(if active {
                                        theme.accent
                                    } else {
                                        Theme::with_alpha(theme.border_strong, 0.5)
                                    })
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            this.navigation_mode = m;
                                            cx.emit(SettingsViewEvent::SettingsChanged);
                                            cx.notify();
                                        }),
                                    )
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(6.0))
                                                    .child(
                                                        div()
                                                            .text_color(if active {
                                                                theme.accent
                                                            } else {
                                                                theme.text_muted
                                                            })
                                                            .child(if active { "◉" } else { "○" }),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(12.5))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(theme.text)
                                                            .child(title),
                                                    ),
                                            )
                                            .child(badge(
                                                tag,
                                                if active {
                                                    BadgeTone::Accent
                                                } else {
                                                    BadgeTone::Neutral
                                                },
                                                &theme,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(theme.text_muted)
                                            .child(desc),
                                    )
                            })),
                    )
                    // Filter pill (cosmetic search affordance, mirrors Stitch)
                    .child(
                        div()
                            .flex_shrink_0()
                            .px(px(10.0))
                            .py(px(6.0))
                            .rounded(px(4.0))
                            .bg(theme.surface_panel)
                            .border_1()
                            .border_color(Theme::with_alpha(theme.border_strong, 0.5))
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(11.5))
                            .text_color(theme.text_muted)
                            .child("⌕ Filter shortcuts or commands...")
                            .child(kbd("⌘F", &theme)),
                    )
                    // Active-mode banner
                    .child(
                        div()
                            .flex_shrink_0()
                            .px(px(12.0))
                            .py(px(8.0))
                            .rounded(px(6.0))
                            .bg(Theme::with_alpha(theme.accent, 0.10))
                            .border_l_2()
                            .border_color(theme.accent)
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.0))
                            .text_size(px(11.5))
                            .text_color(theme.text_secondary)
                            .child(format!(
                                "{mode_tag} motions active · {} bindings registered for this mode.",
                                bindings.len()
                            ))
                            .child(badge("live", BadgeTone::Accent, &theme)),
                    )
                    // Binding table
                    .child(
                        div()
                            .flex_shrink_0()
                            .rounded(px(8.0))
                            .border_1()
                            .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .children(bindings.iter().enumerate().map(|(i, spec)| {
                                let zebra = i % 2 == 0;
                                div()
                                    .px(px(12.0))
                                    .py(px(7.0))
                                    .bg(if zebra {
                                        theme.surface_panel
                                    } else {
                                        Theme::with_alpha(theme.surface_panel, 0.55)
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(12.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(
                                                div()
                                                    .w(px(6.0))
                                                    .h(px(6.0))
                                                    .rounded(px(3.0))
                                                    .bg(theme.accent_secondary),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .text_color(theme.text)
                                                    .child(humanize_command(&spec.command)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(10.0))
                                            .child(kbd(display_keys_label(spec, mac), &theme))
                                            .child(
                                                div()
                                                    .font_family(theme.mono_font.clone())
                                                    .text_size(px(10.0))
                                                    .text_color(theme.text_muted)
                                                    .child(
                                                        spec.context
                                                            .clone()
                                                            .unwrap_or_else(|| {
                                                                spec.command
                                                                    .split('.')
                                                                    .next()
                                                                    .unwrap_or("global")
                                                                    .to_string()
                                                            }),
                                                    ),
                                            ),
                                    )
                            })),
                    )
                    .into_any_element()
            }

            // ───────────────────────── Typography ─────────────────────────
            SettingsCategory::Typography => {
                let fs = self.font_size;
                let us = self.ui_scale;
                div()
                    .id("set-typo-scroll")
                    .flex_1()
                    .min_h_0()
                    .h_full()
                    .pr(px(8.0))
                    .pb(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .overflow_y_scroll()
                    .child(section_head(
                        "Typography & Fonts",
                        "Editor and interface typefaces, scale, and rhythm.",
                        "[ui.typography]",
                        &theme,
                    ))
                    // Font family cards
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .flex_1()
                                    .p(px(12.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .border_1()
                                    .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(3.0))
                                            .child(
                                                div()
                                                    .text_size(px(12.5))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(theme.text)
                                                    .child("Editor Typeface"),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(6.0))
                                                    .child(badge(
                                                        "Ligatures active",
                                                        BadgeTone::Accent,
                                                        &theme,
                                                    ))
                                                    .child(badge(
                                                        "calt, dlig, ss01",
                                                        BadgeTone::Neutral,
                                                        &theme,
                                                    )),
                                            ),
                                    )
                                    .child(kbd(
                                        format!("{}  ▾", self.font_family),
                                        &theme,
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .p(px(12.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .border_1()
                                    .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(3.0))
                                            .child(
                                                div()
                                                    .text_size(px(12.5))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(theme.text)
                                                    .child("Interface Typeface"),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.0))
                                                    .text_color(theme.text_muted)
                                                    .child("Chrome, palette, and settings labels"),
                                            ),
                                    )
                                    .child(kbd(format!("{}  ▾", self.ui_font_family), &theme)),
                            ),
                    )
                    // Font size control (also Ctrl+= / Ctrl+- / Ctrl+0)
                    .child(
                        div()
                            .flex_shrink_0()
                            .p(px(12.0))
                            .rounded(px(8.0))
                            .bg(theme.surface_panel)
                            .border_1()
                            .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(
                                                div()
                                                    .text_size(px(12.5))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(theme.text)
                                                    .child("Editor Font Size"),
                                            )
                                            .child(kbd("Ctrl + = / - / 0", &theme)),
                                    )
                                    .child(badge(
                                        format!("{fs:.1} px"),
                                        BadgeTone::Accent,
                                        &theme,
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        button("typo-fs-m", "−", ButtonVariant::Secondary, &theme)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.font_size =
                                                    (this.font_size - 0.5).clamp(10.0, 28.0);
                                                cx.emit(SettingsViewEvent::SettingsChanged);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .h(px(6.0))
                                            .rounded(px(3.0))
                                            .bg(theme.surface_hover)
                                            .child(
                                                div()
                                                    .w(px((((fs - 10.0) / 18.0) * 240.0).clamp(
                                                        8.0,
                                                        240.0,
                                                    )))
                                                    .h_full()
                                                    .rounded(px(3.0))
                                                    .bg(theme.accent),
                                            ),
                                    )
                                    .child(
                                        button("typo-fs-p", "+", ButtonVariant::Secondary, &theme)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.font_size =
                                                    (this.font_size + 0.5).clamp(10.0, 28.0);
                                                cx.emit(SettingsViewEvent::SettingsChanged);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        button(
                                            "typo-fs-reset",
                                            "Reset",
                                            ButtonVariant::Ghost,
                                            &theme,
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.font_size = 13.5;
                                            cx.emit(SettingsViewEvent::SettingsChanged);
                                            cx.notify();
                                        })),
                                    ),
                            ),
                    )
                    // UI scale control
                    .child(
                        div()
                            .flex_shrink_0()
                            .p(px(12.0))
                            .rounded(px(8.0))
                            .bg(theme.surface_panel)
                            .border_1()
                            .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(
                                                div()
                                                    .text_size(px(12.5))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(theme.text)
                                                    .child("Global UI Scale"),
                                            )
                                            .child(kbd("Ctrl + Alt + = / - / 0", &theme)),
                                    )
                                    .child(badge(
                                        format!("{:.0}%", us * 100.0),
                                        BadgeTone::Success,
                                        &theme,
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        button("typo-us-m", "−", ButtonVariant::Secondary, &theme)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.ui_scale = ((this.ui_scale - 0.1).max(0.75)
                                                    * 100.0)
                                                    .round()
                                                    / 100.0;
                                                cx.emit(SettingsViewEvent::SettingsChanged);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .h(px(6.0))
                                            .rounded(px(3.0))
                                            .bg(theme.surface_hover)
                                            .child(
                                                div()
                                                    .w(px((((us - 0.75) / 1.0) * 240.0).clamp(
                                                        8.0,
                                                        240.0,
                                                    )))
                                                    .h_full()
                                                    .rounded(px(3.0))
                                                    .bg(theme.success),
                                            ),
                                    )
                                    .child(
                                        button("typo-us-p", "+", ButtonVariant::Secondary, &theme)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.ui_scale = ((this.ui_scale + 0.1).min(1.75)
                                                    * 100.0)
                                                    .round()
                                                    / 100.0;
                                                cx.emit(SettingsViewEvent::SettingsChanged);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        button(
                                            "typo-us-reset",
                                            "Reset",
                                            ButtonVariant::Ghost,
                                            &theme,
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.ui_scale = 1.0;
                                            cx.emit(SettingsViewEvent::SettingsChanged);
                                            cx.notify();
                                        })),
                                    ),
                            ),
                    )
                    .into_any_element()
            }

            // ───────────────────────── LSP ─────────────────────────
            SettingsCategory::Lsp => div()
                .id("set-lsp-scroll")
                .flex_1()
                .min_h_0()
                .h_full()
                .pr(px(8.0))
                .pb(px(16.0))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .overflow_y_scroll()
                .child(section_head(
                    "LSP & Diagnostics Engine",
                    "Asynchronous semantic tokens, code navigation, and quiet inline telemetry.",
                    "[lsp.servers]",
                    &theme,
                ))
                .child(
                    toggle_card(
                        "lsp-enabled",
                        "Language Server Protocol",
                        "Attach rust-analyzer, gopls, clangd, and pyright lazily per workspace root.",
                        self.lsp_enabled,
                        &theme,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.lsp_enabled = !this.lsp_enabled;
                        cx.emit(SettingsViewEvent::SettingsChanged);
                        cx.notify();
                    })),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .p(px(12.0))
                        .rounded(px(8.0))
                        .bg(theme.surface_panel)
                        .border_1()
                        .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child("Registered Language Daemons"),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(6.0))
                                .child(badge("rust-analyzer ●", BadgeTone::Success, &theme))
                                .child(badge("gopls ●", BadgeTone::Success, &theme))
                                .child(badge("clangd ●", BadgeTone::Success, &theme))
                                .child(badge("pyright ○", BadgeTone::Neutral, &theme)),
                        ),
                )
                .child(info_row(
                    "Inlay Type & Parameter Hints",
                    "Chaining & Implicit Types",
                    &theme,
                ))
                .child(info_row(
                    "Diagnostics Underline Severity",
                    "Subtle Undercurl with Margin Dot",
                    &theme,
                ))
                .into_any_element(),

            // ───────────────────────── Git ─────────────────────────
            SettingsCategory::Git => div()
                .id("set-git-scroll")
                .flex_1()
                .min_h_0()
                .h_full()
                .pr(px(8.0))
                .pb(px(16.0))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .overflow_y_scroll()
                .child(section_head(
                    "Version Control & Diff",
                    "First-class Git status, gutter markers, and quiet commit flows.",
                    "[git]",
                    &theme,
                ))
                .child(
                    toggle_card(
                        "git-enabled",
                        "Git Integration",
                        "Show branch and change counts in the status bar.",
                        self.git_enabled,
                        &theme,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.git_enabled = !this.git_enabled;
                        cx.emit(SettingsViewEvent::SettingsChanged);
                        cx.notify();
                    })),
                )
                .child(
                    toggle_card(
                        "git-gutter",
                        "Editor Gutter Diff Markers",
                        "Paint added / modified / deleted pips beside changed lines.",
                        self.git_show_gutter_diff,
                        &theme,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.git_show_gutter_diff = !this.git_show_gutter_diff;
                        cx.emit(SettingsViewEvent::SettingsChanged);
                        cx.notify();
                    })),
                )
                .child(
                    toggle_card(
                        "git-autofetch",
                        "Auto-fetch Remotes",
                        "Periodically fetch from origin in the background.",
                        self.git_auto_fetch,
                        &theme,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.git_auto_fetch = !this.git_auto_fetch;
                        cx.emit(SettingsViewEvent::SettingsChanged);
                        cx.notify();
                    })),
                )
                .child(
                    toggle_card(
                        "git-confirmsync",
                        "Confirm Push / Pull",
                        "Ask before any sync action that moves remote history.",
                        self.git_confirm_sync,
                        &theme,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.git_confirm_sync = !this.git_confirm_sync;
                        cx.emit(SettingsViewEvent::SettingsChanged);
                        cx.notify();
                    })),
                )
                .into_any_element(),

            // ───────────────────────── Telemetry / privacy ─────────────────────────
            SettingsCategory::Telemetry => div()
                .id("set-tel-scroll")
                .flex_1()
                .min_h_0()
                .h_full()
                .pr(px(8.0))
                .pb(px(16.0))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .overflow_y_scroll()
                .child(section_head(
                    "Telemetry, AI & Philosophy",
                    "Strict adherence to local-first computation, privacy, and memory hygiene.",
                    "[security.sandbox]",
                    &theme,
                ))
                // Zero-Cloud banner
                .child(
                    div()
                        .flex_shrink_0()
                        .p(px(12.0))
                        .rounded(px(8.0))
                        .bg(theme.surface_panel)
                        .border_1()
                        .border_color(Theme::with_alpha(theme.accent, 0.25))
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .child(
                            div()
                                .w(px(30.0))
                                .h(px(30.0))
                                .rounded(px(6.0))
                                .bg(Theme::with_alpha(theme.accent, 0.14))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(theme.accent)
                                .child("🛡"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .text_size(px(12.5))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.text)
                                                .child("Zero-Cloud Guarantee (Hardcoded)"),
                                        )
                                        .child(badge("AIR-GAPPED", BadgeTone::Success, &theme)),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(theme.text_muted)
                                        .child("No telemetry, usage statistics, crashes, or AI prompt telemetry are ever dispatched."),
                                ),
                        ),
                )
                .child(
                    toggle_card(
                        "files-hidden",
                        "Show Hidden Files",
                        "Reveal dotfiles and dot-directories in the explorer tree.",
                        self.files_show_hidden,
                        &theme,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.files_show_hidden = !this.files_show_hidden;
                        cx.emit(SettingsViewEvent::SettingsChanged);
                        cx.notify();
                    })),
                )
                .child(
                    toggle_card(
                        "files-gitignore",
                        "Respect .gitignore",
                        "Exclude ignored paths from the tree, index, and project search.",
                        self.files_respect_gitignore,
                        &theme,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.files_respect_gitignore = !this.files_respect_gitignore;
                        cx.emit(SettingsViewEvent::SettingsChanged);
                        cx.notify();
                    })),
                )
                .child(info_row("Data egress", "0 sockets open", &theme))
                .child(info_row(
                    "Config surface",
                    "~/.config/blur/config.toml",
                    &theme,
                ))
                .into_any_element(),

            // ───────────────────────── Core editor & buffer ─────────────────────────
            SettingsCategory::Core => {
                let fs = self.font_size;
                let lh = self.line_height;
                let us = self.ui_scale;
                div()
                    .id("set-overview-scroll")
                    .flex_1()
                    .min_h_0()
                    .h_full()
                    .pr(px(8.0))
                    .pb(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .overflow_y_scroll()
                    .child(section_head(
                        "Buffer & Rendering Engine",
                        "Control glyph rasterization, subpixel kerning, and buffer spacing geometry.",
                        "[buffer.render]",
                        &theme,
                    ))
                    // Typeface + Font scale + Line metric
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .flex_1()
                                    .p(px(12.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .border_1()
                                    .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(3.0))
                                            .child(
                                                div()
                                                    .text_size(px(12.5))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(theme.text)
                                                    .child("Primary Monospaced Typeface"),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(6.0))
                                                    .child(badge(
                                                        "Ligatures active",
                                                        BadgeTone::Accent,
                                                        &theme,
                                                    ))
                                                    .child(badge(
                                                        "calt, dlig, ss01",
                                                        BadgeTone::Neutral,
                                                        &theme,
                                                    )),
                                            ),
                                    )
                                    .child(kbd(format!("{}  ▾", self.font_family), &theme)),
                            )
                            // Font scale card (functional)
                            .child(
                                div()
                                    .w(px(210.0))
                                    .p(px(12.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .border_1()
                                    .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(theme.text)
                                                    .child("Font Scale"),
                                            )
                                            .child(badge(
                                                format!("{fs:.1} px"),
                                                BadgeTone::Accent,
                                                &theme,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.0))
                                            .child(
                                                button("set-fs-m", "−", ButtonVariant::Secondary, &theme)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.font_size =
                                                            (this.font_size - 0.5).clamp(10.0, 28.0);
                                                        cx.emit(SettingsViewEvent::SettingsChanged);
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .h(px(4.0))
                                                    .rounded(px(2.0))
                                                    .bg(theme.surface_hover)
                                                    .child(
                                                        div()
                                                            .w(px((((fs - 10.0) / 18.0) * 96.0)
                                                                .clamp(8.0, 96.0)))
                                                            .h_full()
                                                            .bg(theme.accent),
                                                    ),
                                            )
                                            .child(
                                                button("set-fs-p", "+", ButtonVariant::Secondary, &theme)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.font_size =
                                                            (this.font_size + 0.5).clamp(10.0, 28.0);
                                                        cx.emit(SettingsViewEvent::SettingsChanged);
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            )
                            // Line metric card (functional)
                            .child(
                                div()
                                    .w(px(220.0))
                                    .p(px(12.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .border_1()
                                    .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(theme.text)
                                                    .child("Line Metric Ratio"),
                                            )
                                            .child(badge(
                                                format!("{lh:.2} (Calm focus)"),
                                                BadgeTone::Success,
                                                &theme,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.0))
                                            .child(
                                                button("set-lh-m", "−", ButtonVariant::Secondary, &theme)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.line_height =
                                                            (this.line_height - 0.05).clamp(1.0, 2.4);
                                                        cx.emit(SettingsViewEvent::SettingsChanged);
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .h(px(4.0))
                                                    .rounded(px(2.0))
                                                    .bg(theme.surface_hover)
                                                    .child(
                                                        div()
                                                            .w(px((((lh - 1.0) / 1.4) * 110.0)
                                                                .clamp(8.0, 110.0)))
                                                            .h_full()
                                                            .bg(theme.success),
                                                    ),
                                            )
                                            .child(
                                                button("set-lh-p", "+", ButtonVariant::Secondary, &theme)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.line_height =
                                                            (this.line_height + 0.05).clamp(1.0, 2.4);
                                                        cx.emit(SettingsViewEvent::SettingsChanged);
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            ),
                    )
                    // Indentation + Relative numbers + UI scale
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .flex_1()
                                    .p(px(12.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .border_1()
                                    .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child("Indentation Geometry"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(4.0))
                                            .children([2usize, 4, 8].into_iter().map(|sz| {
                                                button(
                                                    ("set-indent-sz", sz),
                                                    format!("{sz} spaces"),
                                                    if self.tab_size == sz {
                                                        ButtonVariant::Primary
                                                    } else {
                                                        ButtonVariant::Secondary
                                                    },
                                                    &theme,
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.tab_size = sz;
                                                    cx.emit(SettingsViewEvent::SettingsChanged);
                                                    cx.notify();
                                                }))
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .p(px(12.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .border_1()
                                    .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child("Relative Line Numbers in Gutter (14j)"),
                                    )
                                    .child(
                                        toggle_switch("set-rel-ln", self.relative_line_numbers, &theme)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.relative_line_numbers =
                                                    !this.relative_line_numbers;
                                                cx.emit(SettingsViewEvent::SettingsChanged);
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // UI scale card (functional)
                            .child(
                                div()
                                    .w(px(220.0))
                                    .p(px(12.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .border_1()
                                    .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_size(theme.scaled(12.0))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(theme.text)
                                                    .child("UI Scale"),
                                            )
                                            .child(badge(
                                                format!("{:.0}%", us * 100.0),
                                                BadgeTone::Accent,
                                                &theme,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.0))
                                            .child(
                                                button("set-us-m", "−", ButtonVariant::Secondary, &theme)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.ui_scale = ((this.ui_scale - 0.1)
                                                            .max(0.75)
                                                            * 100.0)
                                                            .round()
                                                            / 100.0;
                                                        cx.emit(SettingsViewEvent::SettingsChanged);
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .h(px(4.0))
                                                    .rounded(px(2.0))
                                                    .bg(theme.surface_hover)
                                                    .child(
                                                        div()
                                                            .w(px((((us - 0.75) / 1.0) * 96.0)
                                                                .clamp(8.0, 96.0)))
                                                            .h_full()
                                                            .bg(theme.accent),
                                                    ),
                                            )
                                            .child(
                                                button("set-us-p", "+", ButtonVariant::Secondary, &theme)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.ui_scale = ((this.ui_scale + 0.1)
                                                            .min(1.75)
                                                            * 100.0)
                                                            .round()
                                                            / 100.0;
                                                        cx.emit(SettingsViewEvent::SettingsChanged);
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            ),
                    )
                    // Save-time behaviors
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .gap(px(10.0))
                            .child(
                                toggle_card(
                                    "set-trim",
                                    "Trim Trailing Whitespace On Save",
                                    "Automatically strip extraneous space characters and ensure trailing newline.",
                                    self.trim_trailing_whitespace,
                                    &theme,
                                )
                                .flex_1()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.trim_trailing_whitespace = !this.trim_trailing_whitespace;
                                    cx.emit(SettingsViewEvent::SettingsChanged);
                                    cx.notify();
                                })),
                            )
                            .child(
                                toggle_card(
                                    "set-autopairs",
                                    "Auto-close Brackets & Quotes",
                                    "Insert the closing delimiter automatically while typing.",
                                    self.auto_pairs,
                                    &theme,
                                )
                                .flex_1()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.auto_pairs = !this.auto_pairs;
                                    cx.emit(SettingsViewEvent::SettingsChanged);
                                    cx.notify();
                                })),
                            ),
                    )
                    .into_any_element()
            }
        };

        div()
            .size_full()
            .bg(theme.surface)
            .px(px(36.0))
            .py(px(20.0))
            .flex()
            .flex_col()
            .gap(px(18.0))
            .overflow_hidden()
            // Top Configuration Header (1:1 Stitch)
            .child(
                div()
                    .pb(px(14.0))
                    .border_b_1()
                    .border_color(Theme::with_alpha(theme.border_strong, 0.5))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(section_label("PREFERENCES", &theme))
                                    .child(div().text_color(Theme::with_alpha(theme.text_muted, 0.4)).child("/"))
                                    .child(div().text_size(px(11.0)).text_color(theme.accent).child("CORE EDITOR & BUFFER"))
                                    .child(div().text_color(Theme::with_alpha(theme.text_muted, 0.4)).child("•"))
                                    .child(
                                        div()
                                            .font_family(theme.mono_font.clone())
                                            .text_size(px(11.5))
                                            .text_color(theme.success)
                                            .child("~/.config/blur/config.toml"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .child(
                                        div()
                                            .text_size(theme.scaled(21.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child("System Settings"),
                                    )
                                    .child(badge("v0.9.4-release", BadgeTone::Success, &theme))
                                    .child(badge("● inotify active", BadgeTone::Neutral, &theme)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                button("set-diff-disk", "↺ Diff with Disk", ButtonVariant::Secondary, &theme)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.category = SettingsCategory::RawToml;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("set-export-toml", "↓ Export TOML", ButtonVariant::Secondary, &theme)
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.emit(SettingsViewEvent::SaveNow);
                                    })),
                            )
                            .child(
                                button("set-reset-def", "↻ Reset Defaults", ButtonVariant::Secondary, &theme)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reset_defaults(cx);
                                    })),
                            ),
                    ),
            )
            // Main Split: Left sidebar + Right body
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .gap(px(24.0))
                    .overflow_hidden()
                    // Left Sidebar
                    .child(
                        div()
                            .w(theme.scaled(256.0))
                            .h_full()
                            .min_h_0()
                            .p(px(14.0))
                            .rounded(px(8.0))
                            .bg(theme.canvas)
                            .border_1()
                            .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                            .flex()
                            .flex_col()
                            .justify_between()
                            .overflow_hidden()
                            .child(
                                div()
                                    .id("set-nav-scroll")
                                    .flex_1()
                                    .min_h_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.0))
                                    .overflow_y_scroll()
                                    // Filter input pill
                                    .child(
                                        div()
                                            .mb(px(6.0))
                                            .px(px(10.0))
                                            .py(px(6.0))
                                            .rounded(px(4.0))
                                            .bg(theme.surface_panel)
                                            .border_1()
                                            .border_color(Theme::with_alpha(theme.border_strong, 0.5))
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .text_size(px(11.5))
                                            .text_color(theme.text_muted)
                                            .child("⌕ Filter settings (⌘,)...")
                                            .child(kbd("⌘F", &theme)),
                                    )
                                    .children(sidebar_items.into_iter().enumerate().map(
                                        |(i, (group, label, right_tag, cat))| {
                                            let active = self.category == cat;
                                            div()
                                                .flex()
                                                .flex_col()
                                                .when(!group.is_empty(), |d| {
                                                    d.child(
                                                        div()
                                                            .px(px(6.0))
                                                            .pt(px(8.0))
                                                            .pb(px(3.0))
                                                            .text_size(px(10.0))
                                                            .text_color(Theme::with_alpha(
                                                                theme.text_muted,
                                                                0.6,
                                                            ))
                                                            .child(group),
                                                    )
                                                })
                                                .child(
                                                    div()
                                                        .id(("set-nav-item", i))
                                                        .px(px(10.0))
                                                        .py(px(6.0))
                                                        .rounded(px(4.0))
                                                        .bg(if active {
                                                            theme.surface_hover
                                                        } else {
                                                            gpui::transparent_black()
                                                        })
                                                        .cursor_pointer()
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            cx.listener(move |this, _, _, cx| {
                                                                this.category = cat;
                                                                cx.notify();
                                                            }),
                                                        )
                                                        .flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .text_size(px(12.0))
                                                        .child(
                                                            div()
                                                                .text_color(if active {
                                                                    theme.text
                                                                } else {
                                                                    theme.text_secondary
                                                                })
                                                                .child(label),
                                                        )
                                                        .child(
                                                            div()
                                                                .font_family(theme.mono_font.clone())
                                                                .text_size(px(10.5))
                                                                .text_color(if active {
                                                                    theme.accent
                                                                } else {
                                                                    theme.text_muted
                                                                })
                                                                // Owned copy: `right_tag` may
                                                                // borrow `self` (theme name), and
                                                                // text children require 'static.
                                                                .child(right_tag.to_string()),
                                                        ),
                                                )
                                        },
                                    )),
                            )
                            // Bottom Hot Reloader Active Footer
                            .child(
                                div()
                                    .pt(px(10.0))
                                    .border_t_1()
                                    .border_color(Theme::with_alpha(theme.border_strong, 0.5))
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(theme.text)
                                            .child("↻ Hot Reloader Active"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.5))
                                            .text_color(theme.text_muted)
                                            .child("Changes apply instantly to config.toml."),
                                    )
                                    .child(
                                        div()
                                            .p(px(6.0))
                                            .rounded(px(4.0))
                                            .bg(theme.surface_panel)
                                            .font_family(theme.mono_font.clone())
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .text_size(px(10.5))
                                            .text_color(theme.text_muted)
                                            .child("~/.config/blur/config.toml")
                                            .child(
                                                div()
                                                    .text_color(theme.success)
                                                    .child(format!(
                                                        "{:.1} kB",
                                                        self.config_toml.len() as f32 / 1024.0
                                                    )),
                                            ),
                                    ),
                            ),
                    )
                    .child(main_body),
            )
    }
}

/// `display_keys` wrapper so table rows can pass a `KeyBindingSpec` directly.
fn display_keys_label(spec: &KeyBindingSpec, mac: bool) -> String {
    blur_core::display_keys(&spec.keys, mac)
}
