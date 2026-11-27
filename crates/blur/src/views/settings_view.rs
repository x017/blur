//! Comprehensive System Settings view (1:1 implementation of `blur_comprehensive_settings`).

use blur_core::{ConfigStore, NavigationMode};
use blur_theme::{Theme, ThemeRegistry};
use blur_ui::{badge, button, kbd, section_label, toggle_switch, BadgeTone, ButtonVariant};
use gpui::{div, prelude::*, px, Context, EventEmitter, FontWeight, MouseButton, Window};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    Overview,
    ThemeStudio,
    RawToml,
}

#[derive(Debug, Clone)]
pub enum SettingsViewEvent {
    SettingsChanged,
    ThemeSelected(String),
}

pub struct SettingsView {
    category: SettingsCategory,
    config_toml: String,
    font_size: f32,
    ui_scale: f32,
    line_height: f32,
    tab_size: usize,
    trim_trailing_whitespace: bool,
    relative_line_numbers: bool,
    auto_pairs: bool,
    navigation_mode: NavigationMode,
    active_theme: String,
    _extension_names: Vec<String>,
}

impl EventEmitter<SettingsViewEvent> for SettingsView {}

impl SettingsView {
    pub fn new(store: &ConfigStore, extension_names: Vec<String>, _cx: &mut Context<Self>) -> Self {
        let s = store.settings();
        Self {
            category: SettingsCategory::Overview,
            config_toml: store.effective_toml(),
            font_size: s.editor.font_size,
            ui_scale: s.ui.ui_scale.clamp(0.75, 1.75),
            line_height: s.editor.line_height,
            tab_size: s.editor.tab_size,
            trim_trailing_whitespace: s.editor.trim_trailing_whitespace,
            relative_line_numbers: s.editor.relative_line_numbers,
            auto_pairs: s.editor.auto_pairs,
            navigation_mode: s.editor.navigation_mode,
            active_theme: s.ui.theme.clone(),
            _extension_names: extension_names,
        }
    }

    pub fn sync_from_store(&mut self, store: &ConfigStore, cx: &mut Context<Self>) {
        let s = store.settings();
        self.config_toml = store.effective_toml();
        self.active_theme = s.ui.theme.clone();
        self.ui_scale = s.ui.ui_scale.clamp(0.75, 1.75);
        cx.notify();
    }

    pub fn ui_scale(&self) -> f32 {
        self.ui_scale
    }

    pub fn current_editor_overrides(&self) -> (f32, f32, usize, bool, bool, bool, NavigationMode) {
        (
            self.font_size,
            self.line_height,
            self.tab_size,
            self.trim_trailing_whitespace,
            self.relative_line_numbers,
            self.auto_pairs,
            self.navigation_mode,
        )
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();
        let registry = cx.global::<ThemeRegistry>().clone();

        let sidebar_items = [
            ("WORKSPACE CORE", "Core Editor & Buffer", "●", SettingsCategory::Overview, true),
            ("", "Typography & Fonts", "Mono", SettingsCategory::Overview, false),
            ("", "Keybindings & Modality", "Vim", SettingsCategory::Overview, false),
            ("INTELLIGENCE & VCS", "LSP & Language Daemons", "4 up", SettingsCategory::Overview, false),
            ("", "Version Control & Diff", "libgit2", SettingsCategory::Overview, false),
            ("SURFACE & SYSTEM", "Theme & Precision", "Furusato", SettingsCategory::ThemeStudio, false),
            ("", "Telemetry & Privacy", "Zero-air", SettingsCategory::Overview, false),
            ("", "Raw Config Inspector", ".toml", SettingsCategory::RawToml, false),
        ];

        let main_body = match self.category {
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

            SettingsCategory::ThemeStudio => div()
                .id("set-theme-scroll")
                .flex_1()
                .min_h_0()
                .h_full()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .overflow_y_scroll()
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

            SettingsCategory::Overview => {
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
                    // Section 1: Buffer & Rendering Engine
                    .child(
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
                                            .child("Buffer & Rendering Engine"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.text_muted)
                                            .child("Control glyph rasterization, subpixel kerning, and buffer spacing geometry."),
                                    ),
                            )
                            .child(
                                div()
                                    .font_family(theme.mono_font.clone())
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child("[buffer.render]"),
                            ),
                    )
                    // Primary Monospaced Typeface + Font Scale + Line Metric Ratio
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
                                                    .child(badge("Ligatures active", BadgeTone::Accent, &theme))
                                                    .child(badge("calt, dlig, ss01", BadgeTone::Neutral, &theme)),
                                            ),
                                    )
                                    .child(kbd("JetBrains Mono NL  ▾", &theme)),
                            )
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
                                    .justify_between()
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
                                                format!("{:.1} px", self.font_size),
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
                                                button("set-fs-m", "-", ButtonVariant::Secondary, &theme)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.font_size = (this.font_size - 0.5).max(11.0);
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
                                                    .child(div().w(px(60.0)).h_full().bg(theme.accent)),
                                            )
                                            .child(
                                                button("set-fs-p", "+", ButtonVariant::Secondary, &theme)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.font_size = (this.font_size + 0.5).min(20.0);
                                                        cx.emit(SettingsViewEvent::SettingsChanged);
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            )
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
                                    .justify_between()
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
                                            .child(badge("1.65 (Calm focus)", BadgeTone::Success, &theme)),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(4.0))
                                            .rounded(px(2.0))
                                            .bg(theme.surface_hover)
                                            .child(div().w(px(110.0)).h_full().bg(theme.success)),
                                    ),
                            ),
                    )
                    // Indentation Geometry + UI Scale + Relative Line Numbers
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
                                                this.relative_line_numbers = !this.relative_line_numbers;
                                                cx.emit(SettingsViewEvent::SettingsChanged);
                                                cx.notify();
                                            })),
                                    ),
                            )
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
                                    .justify_between()
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
                                                format!("{:.0}%", self.ui_scale * 100.0),
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
                                                button("set-us-m", "-", ButtonVariant::Secondary, &theme)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.ui_scale = ((this.ui_scale - 0.1).max(0.75) * 100.0).round() / 100.0;
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
                                                            .w(px(((self.ui_scale - 0.75) / 1.0 * 96.0).clamp(8.0, 96.0)))
                                                            .h_full()
                                                            .bg(theme.accent),
                                                    ),
                                            )
                                            .child(
                                                button("set-us-p", "+", ButtonVariant::Secondary, &theme)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.ui_scale = ((this.ui_scale + 0.1).min(1.75) * 100.0).round() / 100.0;
                                                        cx.emit(SettingsViewEvent::SettingsChanged);
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            ),
                    )
                    // Section 2: Modality & Keyboard Interaction (2x2 Grid)
                    .child(
                        div()
                            .flex_shrink_0()
                            .pt(px(4.0))
                            .pb(px(4.0))
                            .border_b_1()
                            .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child("Modality & Keyboard Interaction"),
                            )
                            .child(
                                div()
                                    .font_family(theme.mono_font.clone())
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child("[editor.modality]"),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .gap(px(10.0))
                            .children(modes.into_iter().enumerate().map(|(i, (m, title, tag, desc))| {
                                let active = self.navigation_mode == m;
                                div()
                                    .id(("set-mod-card", i))
                                    .flex_1()
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
                    // Section 3: Zero-Cloud Guarantee Banner + Registered Daemons
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
                                div()
                                    .w(px(340.0))
                                    .p(px(12.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .border_1()
                                    .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                                    .flex()
                                    .flex_col()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child("Registered Language Daemons"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(6.0))
                                            .child(badge("rust-analyzer ●", BadgeTone::Success, &theme))
                                            .child(badge("gopls ●", BadgeTone::Success, &theme))
                                            .child(badge("clangd ●", BadgeTone::Success, &theme))
                                            .child(badge("pyright ○", BadgeTone::Neutral, &theme)),
                                    ),
                            ),
                    )
                    // Section 4: Live Buffer Preview (src/engine.rs) — 1:1 Stitch bottom preview card
                    .child(
                        div()
                            .flex_shrink_0()
                            .min_h(px(190.0))
                            .p(px(14.0))
                            .rounded(px(8.0))
                            .bg(theme.canvas)
                            .border_1()
                            .border_color(Theme::with_alpha(theme.border_strong, 0.5))
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .overflow_hidden()
                            .child(
                                div()
                                    .pb(px(6.0))
                                    .border_b_1()
                                    .border_color(Theme::with_alpha(theme.border_strong, 0.3))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_size(px(11.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(
                                                div()
                                                    .w(px(7.0))
                                                    .h(px(7.0))
                                                    .rounded(px(3.5))
                                                    .bg(theme.success),
                                            )
                                            .child(section_label(
                                                "LIVE BUFFER PREVIEW (SRC/ENGINE.RS)",
                                                &theme,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .font_family(theme.mono_font.clone())
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .text_color(theme.text_muted)
                                            .child("Col 12  •  UTF-8  •")
                                            .child(
                                                div()
                                                    .text_color(theme.success)
                                                    .child("Interactive Playground"),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .p(px(10.0))
                                    .rounded(px(6.0))
                                    .bg(theme.surface)
                                    .font_family(theme.mono_font.clone())
                                    .text_size(px(self.font_size))
                                    .flex()
                                    .gap(px(14.0))
                                    .overflow_hidden()
                                    // Relative gutter (3, 2, 1, 42, 1, 2)
                                    .child(
                                        div()
                                            .pr(px(10.0))
                                            .border_r_1()
                                            .border_color(Theme::with_alpha(theme.border_strong, 0.4))
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.0))
                                            .text_color(Theme::with_alpha(theme.text_muted, 0.45))
                                            .child(" 3")
                                            .child(" 2")
                                            .child(" 1")
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(theme.warning)
                                                    .child("42"),
                                            )
                                            .child(" 1")
                                            .child(" 2"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.0))
                                            .text_color(theme.text_secondary)
                                            .child(div().text_color(theme.accent).child("use std::sync::atomic::{AtomicBool, Ordering};"))
                                            .child(div().text_color(theme.syn_comment).child("// HarfBuzz shaping test with ligatures: <= != ==> >="))
                                            .child(div().text_color(theme.warning).child("pub struct EditorBuffer<'a> {"))
                                            .child(div().text_color(theme.success).child("    rope: &'a RopeTree,  /* : &RopeTree */"))
                                            .child(div().text_color(theme.success).child("    is_clean: AtomicBool,"))
                                            .child("}"),
                                    ),
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
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.category = SettingsCategory::RawToml;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("set-reset-def", "↻ Reset Defaults", ButtonVariant::Secondary, &theme)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.font_size = 13.5;
                                        this.ui_scale = 1.0;
                                        this.line_height = 1.65;
                                        this.tab_size = 4;
                                        cx.emit(SettingsViewEvent::SettingsChanged);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            // Main Split: Left 3-col Sidebar + Right 9-col Body
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .gap(px(24.0))
                    .overflow_hidden()
                    // Left Sidebar (#0c0e13 rounded-xl p-4)
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
                                        |(i, (group, label, right_tag, cat, is_first))| {
                                            let active = (self.category == cat && is_first)
                                                || (cat != SettingsCategory::Overview
                                                    && self.category == cat);
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
                                                                .child(right_tag),
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
                                            .child("Changes applied instantly via SIGUSR1."),
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
                                            .child(div().text_color(theme.success).child("2.4 kB")),
                                    ),
                            ),
                    )
                    .child(main_body),
            )
    }
}
