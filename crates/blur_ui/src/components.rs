//! Reusable stateless GPUI UI components matching the Stitch Quiet Precision design language.
//!
//! Uses tonal surface elevation (`#0c0e13` -> `#111318` -> `#1a1b21` -> `#1e2025` -> `#282a2f`)
//! instead of harsh borders, with crisp 4-8px rounded containers and subtle active pips.

use blur_theme::Theme;
use gpui::{
    div, prelude::*, px, AnyElement, Div, ElementId, FontWeight, Hsla, SharedString, Stateful,
};

/// Visual variants for Blur buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// Semantic tones for compact tags/badges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Neutral,
    Accent,
    Success,
    Warning,
    Error,
}

/// Toast notification severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// A transient user notification shown in the bottom-right toast stack.
#[derive(Debug, Clone)]
pub struct NotificationItem {
    pub id: u64,
    pub message: String,
    pub level: NotificationLevel,
}

/// Styled interactive button matching Stitch (`bg-primary-container` / `bg-surface-container-low`).
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    theme: &Theme,
) -> Stateful<Div> {
    let (bg, fg, hover_bg) = match variant {
        ButtonVariant::Primary => (
            theme.accent_secondary,
            theme.on_accent,
            theme.accent,
        ),
        ButtonVariant::Secondary => (
            theme.surface_panel,
            theme.text_secondary,
            theme.surface_elevated,
        ),
        ButtonVariant::Ghost => (
            gpui::transparent_black(),
            theme.text_muted,
            theme.surface_elevated,
        ),
        ButtonVariant::Danger => (
            Theme::with_alpha(theme.error, 0.14),
            theme.error,
            Theme::with_alpha(theme.error, 0.22),
        ),
    };

    div()
        .id(id)
        .h(theme.scaled(28.0))
        .px(theme.scaled(12.0))
        .flex()
        .items_center()
        .justify_center()
        .gap(theme.scaled(6.0))
        .rounded(px(4.0))
        .bg(bg)
        .text_size(theme.scaled(12.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .child(label.into())
}

/// Compact tonal badge matching Stitch (`bg-surface-container` / tinted translucent fills, no harsh borders).
pub fn badge(label: impl Into<SharedString>, tone: BadgeTone, theme: &Theme) -> Div {
    let (fg, bg) = match tone {
        BadgeTone::Neutral => (theme.text_muted, theme.surface_elevated),
        BadgeTone::Accent => (theme.accent, Theme::with_alpha(theme.accent, 0.14)),
        BadgeTone::Success => (theme.success, Theme::with_alpha(theme.success, 0.14)),
        BadgeTone::Warning => (theme.warning, Theme::with_alpha(theme.warning, 0.14)),
        BadgeTone::Error => (theme.error, Theme::with_alpha(theme.error, 0.14)),
    };

    div()
        .px(theme.scaled(6.0))
        .py(theme.scaled(2.0))
        .rounded(px(3.0))
        .bg(bg)
        .text_size(theme.scaled(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .child(label.into())
}

/// Quiet keycap shortcut badge matching Stitch (`bg-surface-container-highest`, `text-outline`).
pub fn kbd(keys: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .px(theme.scaled(6.0))
        .py(theme.scaled(2.0))
        .rounded(px(3.0))
        .bg(theme.surface_panel)
        .text_size(theme.scaled(11.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.text_muted)
        .child(keys.into())
}

/// 1px subtle horizontal divider.
pub fn divider_h(theme: &Theme) -> Div {
    div()
        .w_full()
        .h(px(1.0))
        .bg(Theme::with_alpha(theme.border_strong, 0.45))
}

/// 1px subtle vertical divider.
pub fn divider_v(theme: &Theme) -> Div {
    div()
        .h_full()
        .w(px(1.0))
        .bg(Theme::with_alpha(theme.border_strong, 0.45))
}

/// Quiet uppercase tracked section label (`11px`, `text-outline`).
pub fn section_label(label: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_size(theme.scaled(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.text_muted)
        .child(label.into())
}

/// Selectable list / tree row with subtle left accent pip (`bg-surface-container` when active).
pub fn list_row(id: impl Into<ElementId>, active: bool, theme: &Theme) -> Stateful<Div> {
    let hover_bg = Theme::with_alpha(theme.surface_elevated, 0.65);
    let base = div()
        .id(id)
        .w_full()
        .h(theme.scaled(28.0))
        .px(theme.scaled(8.0))
        .rounded(px(4.0))
        .flex()
        .items_center()
        .justify_between()
        .gap(theme.scaled(8.0))
        .border_l_2()
        .cursor_pointer();

    if active {
        base.border_color(theme.accent)
            .bg(theme.surface_elevated)
            .text_color(theme.text)
    } else {
        base.border_color(gpui::transparent_black())
            .text_color(theme.text_muted)
            .hover(move |s| s.bg(hover_bg))
    }
}

/// Smooth pill toggle switch matching Stitch Settings (`w-10 h-5 rounded-full`).
pub fn toggle_switch(id: impl Into<ElementId>, enabled: bool, theme: &Theme) -> Stateful<Div> {
    let track_bg = if enabled {
        Theme::with_alpha(theme.accent, 0.25)
    } else {
        theme.surface_hover
    };
    let thumb_color = if enabled {
        theme.accent
    } else {
        theme.text_muted
    };

    div()
        .id(id)
        .w(px(38.0))
        .h(px(20.0))
        .px(px(2.0))
        .rounded(px(10.0))
        .bg(track_bg)
        .flex()
        .items_center()
        .justify_end()
        .when(!enabled, |this| this.justify_start())
        .cursor_pointer()
        .child(div().w(px(16.0)).h(px(16.0)).rounded(px(8.0)).bg(thumb_color))
}

/// Status bar item with quiet monospaced typography.
pub fn status_item(label: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .h_full()
        .flex()
        .items_center()
        .text_size(px(11.5))
        .text_color(color)
        .child(label.into())
}

/// Floating notification toast card.
pub fn notification_toast(item: &NotificationItem, theme: &Theme) -> AnyElement {
    let accent = match item.level {
        NotificationLevel::Info => theme.accent,
        NotificationLevel::Success => theme.success,
        NotificationLevel::Warning => theme.warning,
        NotificationLevel::Error => theme.error,
    };

    div()
        .w(px(320.0))
        .px(px(14.0))
        .py(px(10.0))
        .rounded(px(6.0))
        .bg(theme.surface_hover)
        .border_l_2()
        .border_color(accent)
        .flex()
        .items_center()
        .gap(px(8.0))
        .text_size(px(12.0))
        .text_color(theme.text)
        .child(item.message.clone())
        .into_any_element()
}
