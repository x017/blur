//! Native GPU-rendered integrated terminal panel backed by `blur_terminal`.

use std::path::PathBuf;
use std::time::Duration;

use blur_terminal::{encode_key, TermColor, Terminal, TerminalOptions};
use blur_theme::Theme;
use gpui::{
    div, prelude::*, px, rgb, ClipboardItem, Context, FocusHandle, Focusable, FontStyle,
    FontWeight, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, ScrollDelta, ScrollWheelEvent,
    Window,
};

/// A native GPUI terminal emulator view.
pub struct TerminalView {
    focus_handle: FocusHandle,
    terminal: Option<Terminal>,
    cwd: PathBuf,
    error: Option<String>,
    /// Typeface override pushed from Settings (None = theme mono font).
    font_family: Option<String>,
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TerminalView {
    pub fn new(cwd: PathBuf, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            terminal: None,
            cwd,
            error: None,
            font_family: None,
        };
        this.spawn_shell(cx);
        this
    }

    /// Typeface pushed by the app shell on settings changes.
    pub fn set_font_family(&mut self, family: String, cx: &mut Context<Self>) {
        self.font_family = Some(family);
        cx.notify();
    }

    pub fn spawn_shell(&mut self, cx: &mut Context<Self>) {
        let opts = TerminalOptions {
            cwd: Some(self.cwd.clone()),
            cols: 120,
            rows: 16,
            ..Default::default()
        };
        match Terminal::spawn(opts, || {}) {
            Ok(term) => {
                self.terminal = Some(term);
                self.error = None;
                // Poll terminal snapshot at ~30fps while alive
                cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(33))
                            .await;
                        let keep_going = this
                            .update(cx, |view, cx| {
                                cx.notify();
                                view.terminal.as_ref().map(|t| t.is_alive()).unwrap_or(false)
                            })
                            .unwrap_or(false);
                        if !keep_going {
                            break;
                        }
                    }
                })
                .detach();
            }
            Err(err) => {
                self.error = Some(format!("Failed to spawn terminal: {err}"));
            }
        }
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    #[allow(dead_code)]
    pub fn is_alive(&self) -> bool {
        self.terminal.as_ref().map(|t| t.is_alive()).unwrap_or(false)
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(term) = &self.terminal else { return };
        let ks = &event.keystroke;

        // Paste with Ctrl+Shift+V
        if (ks.modifiers.control || ks.modifiers.platform)
            && ks.modifiers.shift
            && ks.key.eq_ignore_ascii_case("v")
        {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                term.paste(&text);
                cx.notify();
            }
            return;
        }

        let app_cursor = term.mode().app_cursor;
        if let Some(bytes) = encode_key(
            &ks.key,
            ks.modifiers.control,
            ks.modifiers.alt,
            ks.modifiers.shift,
            app_cursor,
        ) {
            term.write(&bytes);
            term.scroll_to_bottom();
            cx.notify();
        } else if !ks.modifiers.control && !ks.modifiers.alt {
            if let Some(ch) = ks.key_char.as_deref() {
                if !ch.is_empty() {
                    term.write(ch.as_bytes());
                    term.scroll_to_bottom();
                    cx.notify();
                }
            }
        }
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(term) = &self.terminal {
            let lines = match event.delta {
                ScrollDelta::Lines(pt) => (pt.y * 3.0).round() as i32,
                ScrollDelta::Pixels(pt) => (f32::from(pt.y) / 16.0).round() as i32,
            };
            term.scroll(lines);
            cx.notify();
        }
    }

    fn on_mouse_down(
        &mut self,
        _ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        cx.notify();
    }

    #[allow(dead_code)]
    pub fn copy_visible(&self, cx: &mut Context<Self>) {
        if let Some(term) = &self.terminal {
            let snap = term.snapshot();
            let text = snap
                .lines
                .iter()
                .map(|row| {
                    row.iter()
                        .filter(|c| !c.wide_spacer)
                        .map(|c| c.ch)
                        .collect::<String>()
                        .trim_end()
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join("\n");
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }
}

fn resolve_term_color(color: TermColor, theme: &Theme, is_bg: bool) -> Hsla {
    match color {
        TermColor::Default => {
            if is_bg {
                theme.term_bg
            } else {
                theme.term_fg
            }
        }
        TermColor::DefaultBg => theme.term_bg,
        TermColor::Cursor => theme.term_cursor,
        TermColor::Named(idx) => theme.term_ansi[(idx as usize).min(15)],
        TermColor::Indexed(idx) => {
            if (idx as usize) < 16 {
                theme.term_ansi[idx as usize]
            } else if idx < 232 {
                let n = idx - 16;
                let r = (n / 36) * 51;
                let g = ((n / 6) % 6) * 51;
                let b = (n % 6) * 51;
                let hex = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
                rgb(hex).into()
            } else {
                let gray = 8 + (idx - 232) * 10;
                let hex = ((gray as u32) << 16) | ((gray as u32) << 8) | (gray as u32);
                rgb(hex).into()
            }
        }
        TermColor::Rgb(r, g, b) => {
            let hex = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            rgb(hex).into()
        }
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();
        let focused = self.focus_handle.is_focused(window);

        let body = if let Some(err) = &self.error {
            div()
                .p(px(12.0))
                .text_size(px(12.0))
                .text_color(theme.error)
                .child(err.clone())
                .into_any_element()
        } else if let Some(term) = &self.terminal {
            let snap = term.snapshot();
            let mut rows = Vec::with_capacity(snap.lines.len());

            for (row_idx, row) in snap.lines.iter().enumerate() {
                // Group contiguous cells with identical style into runs for fast rendering
                let mut runs = Vec::new();
                let mut cur_text = String::new();
                let mut cur_fg = theme.term_fg;
                let mut cur_bg = theme.term_bg;
                let mut cur_bold = false;
                let mut cur_italic = false;

                for (col_idx, cell) in row.iter().enumerate() {
                    if cell.wide_spacer {
                        continue;
                    }
                    let is_cursor = focused
                        && snap.display_offset == 0
                        && snap.cursor == Some((col_idx, row_idx));

                    let (mut fg, mut bg) = (
                        resolve_term_color(cell.fg, &theme, false),
                        resolve_term_color(cell.bg, &theme, true),
                    );
                    if cell.inverse || is_cursor {
                        std::mem::swap(&mut fg, &mut bg);
                        if is_cursor {
                            bg = theme.term_cursor;
                            fg = theme.on_accent;
                        }
                    }

                    if !cur_text.is_empty()
                        && (fg != cur_fg
                            || bg != cur_bg
                            || cell.bold != cur_bold
                            || cell.italic != cur_italic)
                    {
                        runs.push(
                            div()
                                .when(cur_bg != theme.term_bg, |d| d.bg(cur_bg))
                                .text_color(cur_fg)
                                .when(cur_bold, |d| d.font_weight(FontWeight::BOLD))
                                .when(cur_italic, |d| d.italic())
                                .child(std::mem::take(&mut cur_text)),
                        );
                    }
                    if cur_text.is_empty() {
                        cur_fg = fg;
                        cur_bg = bg;
                        cur_bold = cell.bold;
                        cur_italic = cell.italic;
                    }
                    cur_text.push(cell.ch);
                }
                if !cur_text.is_empty() {
                    let trimmed = cur_text.trim_end();
                    let out = if trimmed.is_empty() && cur_bg == theme.term_bg {
                        " ".to_string()
                    } else {
                        cur_text
                    };
                    runs.push(
                        div()
                            .when(cur_bg != theme.term_bg, |d| d.bg(cur_bg))
                            .text_color(cur_fg)
                            .when(cur_bold, |d| d.font_weight(FontWeight::BOLD))
                            .when(cur_italic, |d| d.italic())
                            .child(out),
                    );
                }

                rows.push(
                    div()
                        .w_full()
                        .h(px(16.0))
                        .flex()
                        .items_center()
                        .text_size(px(12.0))
                        .children(runs),
                );
            }

            div()
                .size_full()
                .px(px(10.0))
                .py(px(6.0))
                .flex()
                .flex_col()
                .overflow_hidden()
                .children(rows)
                .into_any_element()
        } else {
            div()
                .p(px(12.0))
                .text_size(px(12.0))
                .text_color(theme.text_muted)
                .child("Terminal closed.")
                .into_any_element()
        };

        // Keep FontStyle referenced cleanly
        let _ = FontStyle::Normal;

        div()
            .id("terminal-surface")
            .size_full()
            .bg(theme.term_bg)
            .font_family(
                self.font_family
                    .clone()
                    .filter(|f| !f.trim().is_empty())
                    .unwrap_or_else(|| theme.mono_font.clone()),
            )
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .child(body)
    }
}
