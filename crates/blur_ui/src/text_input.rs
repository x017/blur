//! Interactive native GPUI single-line text input component.
//!
//! Used by the Command Palette, File Finder, Project Search, Git Commit Box,
//! HTTP Client URL bar, Regex Tester, and Settings filter.

use blur_theme::Theme;
use gpui::{
    div, prelude::*, px, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable,
    KeyDownEvent, MouseButton, MouseDownEvent, SharedString, Window,
};

/// Events emitted by [`TextInput`].
#[derive(Debug, Clone)]
pub enum TextInputEvent {
    Changed(String),
    Submit(String),
    Escape,
    MoveUp,
    MoveDown,
    Tab,
}

/// A native GPUI single-line text input with cursor, selection, clipboard, and
/// keyboard navigation.
pub struct TextInput {
    focus_handle: FocusHandle,
    value: String,
    placeholder: SharedString,
    cursor: usize, // byte offset (always on char boundary)
    select_all: bool,
    borderless: bool,
    font_size: f32,
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            value: String::new(),
            placeholder: placeholder.into(),
            cursor: 0,
            select_all: false,
            borderless: false,
            font_size: 13.0,
        }
    }

    pub fn borderless(mut self) -> Self {
        self.borderless = true;
        self
    }

    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn set_value(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        self.value = value.into();
        self.cursor = self.value.len();
        self.select_all = false;
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.value.clear();
        self.cursor = 0;
        self.select_all = false;
        cx.emit(TextInputEvent::Changed(String::new()));
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    fn prev_char_boundary(&self, idx: usize) -> usize {
        if idx == 0 {
            return 0;
        }
        let mut i = idx - 1;
        while i > 0 && !self.value.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    fn next_char_boundary(&self, idx: usize) -> usize {
        if idx >= self.value.len() {
            return self.value.len();
        }
        let mut i = idx + 1;
        while i < self.value.len() && !self.value.is_char_boundary(i) {
            i += 1;
        }
        i
    }

    fn insert_str(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.select_all {
            self.value.clear();
            self.cursor = 0;
            self.select_all = false;
        }
        self.value.insert_str(self.cursor, text);
        self.cursor += text.len();
        cx.emit(TextInputEvent::Changed(self.value.clone()));
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let primary_mod = ks.modifiers.control || ks.modifiers.platform;

        match ks.key.as_str() {
            "escape" => {
                cx.emit(TextInputEvent::Escape);
                return;
            }
            "enter" => {
                cx.emit(TextInputEvent::Submit(self.value.clone()));
                return;
            }
            "up" => {
                cx.emit(TextInputEvent::MoveUp);
                return;
            }
            "down" => {
                cx.emit(TextInputEvent::MoveDown);
                return;
            }
            "tab" => {
                cx.emit(TextInputEvent::Tab);
                return;
            }
            "left" => {
                self.select_all = false;
                self.cursor = self.prev_char_boundary(self.cursor);
                cx.notify();
                return;
            }
            "right" => {
                self.select_all = false;
                self.cursor = self.next_char_boundary(self.cursor);
                cx.notify();
                return;
            }
            "home" => {
                self.select_all = false;
                self.cursor = 0;
                cx.notify();
                return;
            }
            "end" => {
                self.select_all = false;
                self.cursor = self.value.len();
                cx.notify();
                return;
            }
            "backspace" => {
                if self.select_all {
                    self.clear(cx);
                    return;
                }
                if self.cursor > 0 {
                    let prev = self.prev_char_boundary(self.cursor);
                    self.value.drain(prev..self.cursor);
                    self.cursor = prev;
                    cx.emit(TextInputEvent::Changed(self.value.clone()));
                    cx.notify();
                }
                return;
            }
            "delete" => {
                if self.select_all {
                    self.clear(cx);
                    return;
                }
                if self.cursor < self.value.len() {
                    let next = self.next_char_boundary(self.cursor);
                    self.value.drain(self.cursor..next);
                    cx.emit(TextInputEvent::Changed(self.value.clone()));
                    cx.notify();
                }
                return;
            }
            "a" if primary_mod => {
                self.select_all = !self.value.is_empty();
                self.cursor = self.value.len();
                cx.notify();
                return;
            }
            "c" if primary_mod => {
                if !self.value.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.value.clone()));
                }
                return;
            }
            "v" if primary_mod => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    let clean = text.replace(['\r', '\n'], " ");
                    self.insert_str(&clean, cx);
                }
                return;
            }
            "u" if primary_mod => {
                self.clear(cx);
                return;
            }
            _ => {}
        }

        if !primary_mod && !ks.modifiers.alt {
            if let Some(ch) = ks.key_char.as_deref() {
                if !ch.is_empty() && !ch.chars().all(|c| c.is_control()) {
                    self.insert_str(ch, cx);
                }
            } else if ks.key == "space" {
                self.insert_str(" ", cx);
            }
        }
    }

    fn on_mouse_down(
        &mut self,
        _event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.select_all = false;
        self.cursor = self.value.len();
        cx.notify();
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx);
        let focused = self.focus_handle.is_focused(window);
        let before = self.value[..self.cursor].to_string();
        let after = self.value[self.cursor..].to_string();

        let container = div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .w_full()
            .h(px(30.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .overflow_hidden()
            .cursor_text()
            .text_size(px(self.font_size));

        let container = if self.borderless {
            container
        } else {
            container
                .rounded(px(4.0))
                .bg(theme.canvas)
                .border_1()
                .border_color(if focused {
                    theme.accent
                } else {
                    theme.border_strong
                })
        };

        if self.value.is_empty() {
            container.child(
                div()
                    .flex()
                    .items_center()
                    .when(focused, |this| {
                        this.child(div().w(px(1.5)).h(px(15.0)).bg(theme.accent))
                    })
                    .child(
                        div()
                            .text_color(theme.text_muted)
                            .child(self.placeholder.clone()),
                    ),
            )
        } else if self.select_all {
            container.child(
                div()
                    .px(px(2.0))
                    .rounded(px(2.0))
                    .bg(theme.selection)
                    .text_color(theme.text)
                    .child(self.value.clone()),
            )
        } else {
            container.child(
                div()
                    .flex()
                    .items_center()
                    .text_color(theme.text)
                    .child(before)
                    .when(focused, |this| {
                        this.child(div().w(px(1.5)).h(px(15.0)).bg(theme.accent))
                    })
                    .child(after),
            )
        }
    }
}
