//! Theme & Appearance Studio view (1:1 implementation of `blur_theme_appearance_studio_1`
//! and `blur_theme_appearance_studio_2`).

use blur_theme::{Theme, ThemeRegistry};
use blur_ui::{badge, button, kbd, section_label, toggle_switch, BadgeTone, ButtonVariant};
use gpui::{
    div, prelude::*, px, Context, EventEmitter, FontWeight, MouseButton, ScrollDelta,
    ScrollHandle, ScrollWheelEvent, Window,
};

#[derive(Debug, Clone)]
pub enum ThemeStudioEvent {
    ThemeSelected(String),
    Notification(String),
    /// Geometry card picked in the studio → persist `ui.tab_radius` live.
    TabRadius(u32),
}

pub struct ThemeStudioView {
    active_section: usize,
    continuous_geometry: bool,
    selected_shape: usize, // 0: Rounded Pill (16px), 1: Subtle Soft (4px), 2: Sharp Minimal (0px), 3: Capsule Grid (12px)
    /// Wheel-scroll handle for the right configuration column.
    scroll_handle: ScrollHandle,
    tab_radius: u32,
    active_accent_idx: usize,
}

impl EventEmitter<ThemeStudioEvent> for ThemeStudioView {}

impl ThemeStudioView {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            active_section: 0,
            continuous_geometry: true,
            selected_shape: 0,
            tab_radius: 16,
            scroll_handle: ScrollHandle::new(),
            active_accent_idx: 0,
        }
    }
}

impl Render for ThemeStudioView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();
        let registry = cx.global::<ThemeRegistry>().clone();

        let studio_sections = [
            ("Presets & Atmosphere", "01"),
            ("Tab Bar & Geometry", "02"),
            ("Colors & Tokens", "03"),
            ("Backdrop & Glass", "04"),
            ("Gutter & Statusline", "05"),
        ];

        let geo_options = [
            ("Rounded Pill", "Radius 16-20px", 16u32),
            ("Subtle Soft", "Radius 4px", 4u32),
            ("Sharp Minimal", "Radius 0px", 0u32),
            ("Capsule Grid", "Radius 10-12px", 12u32),
        ];

        let accents = [
            ("Cerulean", "#6c99bb", "Primary Core"),
            ("Cyan Ice", "#38bdf8", "Cryogenic Blue"),
            ("Pine Green", "#4ade80", "Natural Foliage"),
            ("Soft Amber", "#f59e0b", "Warm Glow"),
            ("Rose / Alert", "#f87171", "Calm Warning"),
        ];

        let syntax_tokens = [
            ("Keyword / Fn", "#9eccf0", "Bold", "Used for fn, pub, impl, struct, use."),
            ("Strings & Literals", "#9ed0c5", "Regex", "Soft sage for raw and interpolated text strings."),
            ("Types & Traits", "#e9c176", "Class", "Warm ochre for structs, interfaces & generics."),
            ("Constants & Numbers", "#d19a66", "Floats", "Terracotta for numeric literals and enums."),
            ("Comments & Doc", "#79a399", "Italic", "Muted slate with optional soft cursive tilt."),
            ("Operators & Delimiters", "#c1c7ce", "Braces", "Subdued off-white for arrows, colons & math."),
        ];

        let effective_radius = if self.continuous_geometry {
            self.tab_radius as f32
        } else {
            0.0
        };

        div()
            .size_full()
            .bg(theme.surface)
            .flex()
            .flex_col()
            .overflow_hidden()
            // Sub-header Breadcrumb Bar & Quick Operations (1:1 Stitch)
            .child(
                div()
                    .w_full()
                    .px(px(20.0))
                    .py(px(10.0))
                    .bg(theme.canvas)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .text_size(px(11.5))
                            .child(div().text_color(theme.text_muted).child("PREFERENCES"))
                            .child(div().text_color(Theme::with_alpha(theme.text_muted, 0.4)).child("/"))
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child("Appearance & Theme Studio"),
                            )
                            .child(div().text_color(Theme::with_alpha(theme.text_muted, 0.3)).child("·"))
                            .child(kbd("~/.config/blur/theme.toml", &theme))
                            .child(badge("sync", BadgeTone::Success, &theme)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                button("ts-export", "↓ Export Palette (.toml)", ButtonVariant::Secondary, &theme)
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.emit(ThemeStudioEvent::Notification(
                                            "Exported theme palette to ~/.config/blur/themes/".into(),
                                        ));
                                    })),
                            )
                            .child(
                                button("ts-reset", "↻ Reset Defaults", ButtonVariant::Secondary, &theme)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.tab_radius = 16;
                                        this.selected_shape = 0;
                                        this.continuous_geometry = true;
                                        cx.emit(ThemeStudioEvent::ThemeSelected("Furusato Slate".into()));
                                        cx.notify();
                                    })),
                            )
                            .child(button(
                                "ts-preview",
                                "◉ Preview Live Buffer",
                                ButtonVariant::Primary,
                                &theme,
                            )),
                    ),
            )
            // Studio Shell Layout (Left 256px sidebar + Right configuration grid)
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .px(px(32.0))
                    .py(px(20.0))
                    .flex()
                    .gap(px(22.0))
                    .overflow_hidden()
                    // Left Sidebar
                    .child(
                        div()
                            .w(px(248.0))
                            .flex()
                            .flex_col()
                            .gap(px(14.0))
                            // Studio Sections card
                            .child(
                                div()
                                    .p(px(10.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.0))
                                    .child(
                                        div()
                                            .px(px(6.0))
                                            .py(px(4.0))
                                            .child(section_label("STUDIO SECTIONS", &theme)),
                                    )
                                    .children(studio_sections.into_iter().enumerate().map(
                                        |(i, (title, num))| {
                                            let active = self.active_section == i;
                                            div()
                                                .id(("ts-sec", i))
                                                .px(px(10.0))
                                                .py(px(7.0))
                                                .rounded(px(4.0))
                                                .bg(if active {
                                                    theme.border_strong
                                                } else {
                                                    gpui::transparent_black()
                                                })
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(move |this, _, _, cx| {
                                                        this.active_section = i;
                                                        cx.notify();
                                                    }),
                                                )
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .text_size(px(12.0))
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
                                                                .bg(if active {
                                                                    theme.accent
                                                                } else {
                                                                    Theme::with_alpha(theme.text_muted, 0.4)
                                                                }),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_color(if active {
                                                                    theme.text
                                                                } else {
                                                                    theme.text_secondary
                                                                })
                                                                .child(title),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .text_color(if active {
                                                            theme.accent
                                                        } else {
                                                            theme.text_muted
                                                        })
                                                        .child(num),
                                                )
                                        },
                                    )),
                            )
                            // Aesthetic Profile Card
                            .child(
                                div()
                                    .p(px(14.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(section_label("AESTHETIC PROFILE", &theme))
                                            .child(badge("furusato-v3", BadgeTone::Success, &theme)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.text_secondary)
                                            .child(format!(
                                                "Active palette `{}` paired with zero-glare tokens. High eye-rest factor.",
                                                theme.name
                                            )),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(6.0))
                                            .rounded(px(3.0))
                                            .bg(theme.surface_elevated)
                                            .flex()
                                            .overflow_hidden()
                                            .child(div().w(px(110.0)).h_full().bg(theme.accent_secondary))
                                            .child(div().w(px(55.0)).h_full().bg(theme.success))
                                            .child(div().flex_1().h_full().bg(theme.warning)),
                                    ),
                            )
                            // Engine Constraints Card
                            .child(
                                div()
                                    .p(px(14.0))
                                    .rounded(px(8.0))
                                    .bg(theme.canvas)
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.0))
                                    .child(section_label("ENGINE CONSTRAINTS", &theme))
                                    .child(
                                        div()
                                            .flex()
                                            .justify_between()
                                            .text_size(px(11.5))
                                            .child(div().text_color(theme.text_muted).child("WCAG Target"))
                                            .child(div().text_color(theme.text).child("AAA (12.4:1)")),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .justify_between()
                                            .text_size(px(11.5))
                                            .child(div().text_color(theme.text_muted).child("Buffer Repaint"))
                                            .child(div().text_color(theme.success).child("< 1.2ms")),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .justify_between()
                                            .text_size(px(11.5))
                                            .child(div().text_color(theme.text_muted).child("GPU Shader"))
                                            .child(div().text_color(theme.text).child("Vulkan / Metal")),
                                    ),
                            ),
                    )
                    // Right Main Column
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .flex()
                            .flex_col()
                            .gap(px(16.0))
                            .overflow_hidden()
                            .id("ts-main-scroll")
                            .track_scroll(&self.scroll_handle)
                            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                                let dy = match event.delta {
                                    ScrollDelta::Lines(pt) => pt.y * 24.0,
                                    ScrollDelta::Pixels(pt) => f32::from(pt.y),
                                };
                                let cur = this.scroll_handle.offset();
                                this.scroll_handle.set_offset(gpui::Point {
                                    x: cur.x,
                                    y: cur.y + px(dy),
                                });
                                cx.notify();
                            }))
                            // SECTION 1: Colorway & Atmosphere Engine (6 Presets)
                            .child(
                                div()
                                    .p(px(18.0))
                                    .rounded(px(8.0))
                                    .bg(theme.surface_panel)
                                    .flex()
                                    .flex_col()
                                    .gap(px(12.0))
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
                                                            .w(px(8.0))
                                                            .h(px(8.0))
                                                            .rounded(px(4.0))
                                                            .bg(theme.success),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(16.0))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(theme.text)
                                                            .child("Colorway & Atmosphere Engine"),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.5))
                                                    .text_color(theme.text_muted)
                                                    .child(format!("{} Presets loaded", registry.list().len())),
                                            ),
                                    )
                                    // Preset cards row
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(10.0))
                                            .children(registry.list().iter().enumerate().map(|(i, def)| {
                                                let is_current = def.name.eq_ignore_ascii_case(&theme.name);
                                                let name = def.name.clone();
                                                let swatches = def.preview_swatches.iter().map(|hex| {
                                                    div()
                                                        .w(px(18.0))
                                                        .h(px(18.0))
                                                        .rounded(px(9.0))
                                                        .bg(blur_theme::parse_hex(hex))
                                                });
                                                div()
                                                    .id(("ts-preset", i))
                                                    .flex_1()
                                                    .p(px(12.0))
                                                    .rounded(px(6.0))
                                                    .bg(if is_current {
                                                        theme.surface_hover
                                                    } else {
                                                        theme.surface_elevated
                                                    })
                                                    .border_1()
                                                    .border_color(if is_current {
                                                        theme.accent
                                                    } else {
                                                        gpui::transparent_black()
                                                    })
                                                    .cursor_pointer()
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(move |_, _, _, cx| {
                                                            cx.emit(ThemeStudioEvent::ThemeSelected(name.clone()));
                                                        }),
                                                    )
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
                                                                    .text_size(px(12.5))
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .text_color(theme.text)
                                                                    .child(def.name.clone()),
                                                            )
                                                            .when(is_current, |d| {
                                                                d.child(badge("Current", BadgeTone::Accent, &theme))
                                                            }),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(11.0))
                                                            .text_color(theme.text_muted)
                                                            .child(def.description.clone()),
                                                    )
                                                    .child(div().flex().items_center().gap(px(5.0)).children(swatches))
                                            })),
                                    ),
                            )
                            // SECTION 2: Top Tab Bar & Window Geometry + Syntax Token Palette
                            .child(
                                div()
                                    .flex()
                                    .gap(px(16.0))
                                    // Left: Tab Bar & Window Geometry
                                    .child(
                                        div()
                                            .flex_1()
                                            .p(px(18.0))
                                            .rounded(px(8.0))
                                            .bg(theme.surface_panel)
                                            .flex()
                                            .flex_col()
                                            .gap(px(12.0))
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
                                                                    .w(px(8.0))
                                                                    .h(px(8.0))
                                                                    .rounded(px(4.0))
                                                                    .bg(theme.accent),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(px(15.0))
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .text_color(theme.text)
                                                                    .child("Top Tab Bar & Window Geometry"),
                                                            ),
                                                    )
                                                    .child(
                                                        toggle_switch(
                                                            "ts-cont-geo",
                                                            self.continuous_geometry,
                                                            &theme,
                                                        )
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.continuous_geometry = !this.continuous_geometry;
                                                            cx.notify();
                                                        })),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .gap(px(8.0))
                                                    .children(geo_options.into_iter().enumerate().map(
                                                        |(i, (title, sub, rad))| {
                                                            let active = self.selected_shape == i;
                                                            div()
                                                                .id(("ts-geo", i))
                                                                .flex_1()
                                                                .p(px(10.0))
                                                                .rounded(px(6.0))
                                                                .bg(if active {
                                                                    theme.surface_hover
                                                                } else {
                                                                    theme.surface_elevated
                                                                })
                                                                .cursor_pointer()
                                                                .on_mouse_down(
                                                                    MouseButton::Left,
                                                                    cx.listener(move |this, _, _, cx| {
                                                                        this.selected_shape = i;
                                                                        this.tab_radius = rad;
                                                                        // Apply the radius to the real header nav pills / tab section.
                                                                        cx.emit(ThemeStudioEvent::TabRadius(rad));
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
                                                                        .text_size(px(11.5))
                                                                        .text_color(theme.text)
                                                                        .child(title)
                                                                        .child(if active { "◉" } else { "○" }),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .h(px(20.0))
                                                                        .px(px(6.0))
                                                                        .rounded(px(rad as f32))
                                                                        .bg(theme.canvas)
                                                                        .flex()
                                                                        .items_center()
                                                                        .child(
                                                                            div()
                                                                                .w(px(36.0))
                                                                                .h(px(10.0))
                                                                                .rounded(px(rad as f32))
                                                                                .bg(Theme::with_alpha(
                                                                                    theme.accent,
                                                                                    0.45,
                                                                                )),
                                                                        ),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_size(px(10.5))
                                                                        .text_color(theme.text_muted)
                                                                        .child(sub),
                                                                )
                                                        },
                                                    )),
                                            )
                                            // Accent & Key Accents row
                                            .child(
                                                div()
                                                    .flex()
                                                    .gap(px(6.0))
                                                    .children(accents.into_iter().enumerate().map(
                                                        |(i, (label, hex, sub))| {
                                                            let active = self.active_accent_idx == i;
                                                            div()
                                                                .id(("ts-acc", i))
                                                                .flex_1()
                                                                .p(px(8.0))
                                                                .rounded(px(6.0))
                                                                .bg(theme.canvas)
                                                                .border_1()
                                                                .border_color(if active {
                                                                    theme.accent
                                                                } else {
                                                                    theme.border
                                                                })
                                                                .cursor_pointer()
                                                                .on_mouse_down(
                                                                    MouseButton::Left,
                                                                    cx.listener(move |this, _, _, cx| {
                                                                        this.active_accent_idx = i;
                                                                        cx.notify();
                                                                    }),
                                                                )
                                                                .flex()
                                                                .flex_col()
                                                                .gap(px(4.0))
                                                                .child(
                                                                    div()
                                                                        .text_size(px(11.0))
                                                                        .text_color(theme.text)
                                                                        .child(label),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .h(px(18.0))
                                                                        .rounded(px(3.0))
                                                                        .bg(blur_theme::parse_hex(hex))
                                                                        .flex()
                                                                        .items_center()
                                                                        .justify_center()
                                                                        .text_size(px(10.0))
                                                                        .text_color(theme.canvas)
                                                                        .child(hex),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_size(px(9.5))
                                                                        .text_color(theme.text_muted)
                                                                        .child(sub),
                                                                )
                                                        },
                                                    )),
                                            ),
                                    )
                                    // Right: Syntax Highlighting Token Palette
                                    .child(
                                        div()
                                            .w(px(420.0))
                                            .p(px(18.0))
                                            .rounded(px(8.0))
                                            .bg(theme.surface_panel)
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.0))
                                            .child(section_label(
                                                "SYNTAX HIGHLIGHTING TOKEN PALETTE · TREESITTER",
                                                &theme,
                                            ))
                                            .children(syntax_tokens.into_iter().map(
                                                |(name, hex, tag, desc)| {
                                                    div()
                                                        .p(px(8.0))
                                                        .rounded(px(4.0))
                                                        .bg(theme.canvas)
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
                                                                        .w(px(10.0))
                                                                        .h(px(10.0))
                                                                        .rounded(px(2.0))
                                                                        .bg(blur_theme::parse_hex(hex)),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .flex()
                                                                        .flex_col()
                                                                        .child(
                                                                            div()
                                                                                .text_size(px(11.5))
                                                                                .text_color(theme.text)
                                                                                .child(name),
                                                                        )
                                                                        .child(
                                                                            div()
                                                                                .text_size(px(10.0))
                                                                                .text_color(theme.text_muted)
                                                                                .child(desc),
                                                                        ),
                                                                ),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex()
                                                                .items_center()
                                                                .gap(px(6.0))
                                                                .child(
                                                                    div()
                                                                        .text_size(px(11.0))
                                                                        .text_color(blur_theme::parse_hex(hex))
                                                                        .child(hex),
                                                                )
                                                                .child(badge(tag, BadgeTone::Neutral, &theme)),
                                                        )
                                                },
                                            )),
                                    ),
                            )
                            // SECTION 3: Interactive Live Buffer & Syntax Previewer (1:1 Stitch)
                            .child(
                                div()
                                    .flex_1()
                                    .p(px(16.0))
                                    .rounded(px(8.0))
                                    .bg(theme.canvas)
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.0))
                                    .overflow_hidden()
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
                                                            .w(px(8.0))
                                                            .h(px(8.0))
                                                            .rounded(px(4.0))
                                                            .bg(theme.warning),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(14.0))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(theme.text)
                                                            .child("Interactive Live Buffer & Syntax Previewer"),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(11.5))
                                                            .text_color(theme.text_muted)
                                                            .child("· src/theme/engine.rs"),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .px(px(10.0))
                                                    .py(px(4.0))
                                                    .rounded(px(6.0))
                                                    .bg(theme.surface_panel)
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(10.0))
                                                    .text_size(px(11.0))
                                                    .child(div().text_color(theme.accent).child("● fn"))
                                                    .child(div().text_color(theme.success).child("● str"))
                                                    .child(div().text_color(theme.warning).child("● type"))
                                                    .child(div().text_color(theme.text_muted).child("● cmt")),
                                            ),
                                    )
                                    // Simulated Editor Frame with dynamic tab radius
                                    .child(
                                        div()
                                            .flex_1()
                                            .rounded(px(8.0))
                                            .bg(theme.surface)
                                            .flex()
                                            .flex_col()
                                            .overflow_hidden()
                                            .font_family(theme.mono_font.clone())
                                            // Live tab strip
                                            .child(
                                                div()
                                                    .h(px(36.0))
                                                    .px(px(10.0))
                                                    .bg(theme.surface_panel)
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.0))
                                                    .text_size(px(11.5))
                                                    .child(
                                                        div()
                                                            .px(px(10.0))
                                                            .py(px(3.0))
                                                            .rounded(px(effective_radius))
                                                            .text_color(theme.text_muted)
                                                            .child("<> main.rs"),
                                                    )
                                                    .child(
                                                        div()
                                                            .px(px(12.0))
                                                            .py(px(3.0))
                                                            .rounded(px(effective_radius))
                                                            .bg(theme.border_strong)
                                                            .text_color(theme.accent)
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(6.0))
                                                            .child("✎ theme.rs")
                                                            .child(
                                                                div()
                                                                    .w(px(5.0))
                                                                    .h(px(5.0))
                                                                    .rounded(px(2.5))
                                                                    .bg(theme.success),
                                                            )
                                                            .child("×"),
                                                    )
                                                    .child(
                                                        div()
                                                            .px(px(10.0))
                                                            .py(px(3.0))
                                                            .rounded(px(effective_radius))
                                                            .text_color(theme.text_muted)
                                                            .child("⚙ config.toml"),
                                                    )
                                                    .child(
                                                        div()
                                                            .px(px(10.0))
                                                            .py(px(3.0))
                                                            .rounded(px(effective_radius))
                                                            .text_color(theme.text_muted)
                                                            .child(">_ vulkan_surface.rs"),
                                                    ),
                                            )
                                            // Code sample
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .p(px(12.0))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(2.0))
                                                    .text_size(px(12.5))
                                                    .child(preview_code_line("01", "use blur_core::geometry::{ContinuousPill, TabMesh};", false, &theme))
                                                    .child(preview_code_line("02", "// Furusato aesthetic: zero cognitive drag, high ocular calm", false, &theme))
                                                    .child(preview_code_line("03", "pub struct ThemeRenderer { surface_corner_radius: f32 }", false, &theme))
                                                    .child(preview_code_line("07", "impl ThemeRenderer {", true, &theme))
                                                    .child(preview_code_line("08", "    pub fn apply_pill_geometry(&mut self, radius: f32) -> Result<(), GpuErr> {", false, &theme))
                                                    .child(preview_code_line("09", "        self.surface_corner_radius = radius.clamp(0.0, 24.0);", false, &theme))
                                                    .child(preview_code_line("10", "        Ok(())", false, &theme))
                                                    .child(preview_code_line("11", "    }", false, &theme)),
                                            )
                                            // Micro statusline
                                            .child(
                                                div()
                                                    .h(px(22.0))
                                                    .px(px(10.0))
                                                    .bg(theme.canvas)
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_size(px(10.5))
                                                    .text_color(theme.text_muted)
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(8.0))
                                                            .child(div().text_color(theme.accent).child("LIVE REPAINT"))
                                                            .child("|")
                                                            .child("engine.rs"),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(10.0))
                                                            .child("UTF-8")
                                                            .child(
                                                                div()
                                                                    .text_color(theme.success)
                                                                    .child(format!("Pill Contour: {}px", effective_radius as u32)),
                                                            )
                                                            .child("60 FPS"),
                                                    ),
                                            ),
                                    ),
                            ),
                    ),
            )
    }
}

fn preview_code_line(num: &str, code: &str, active: bool, theme: &Theme) -> gpui::Div {
    let color = if code.trim_start().starts_with("//") {
        theme.syn_comment
    } else if active {
        theme.accent
    } else {
        theme.text
    };
    div()
        .w_full()
        .px(px(6.0))
        .py(px(1.0))
        .rounded(px(3.0))
        .when(active, |d| d.bg(theme.surface_hover))
        .flex()
        .items_center()
        .gap(px(14.0))
        .child(
            div()
                .w(px(24.0))
                .text_color(if active {
                    theme.text
                } else {
                    Theme::with_alpha(theme.text_muted, 0.45)
                })
                .child(num.to_string()),
        )
        .child(div().text_color(color).child(code.to_string()))
}
