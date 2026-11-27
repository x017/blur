//! Maps `blur_syntax::HighlightKind` to GPUI `HighlightStyle` using the active `Theme`.

use blur_syntax::HighlightKind;
use blur_theme::Theme;
use gpui::{FontStyle, FontWeight, HighlightStyle};

pub fn style_for_kind(kind: HighlightKind, theme: &Theme) -> HighlightStyle {
    let mut style = HighlightStyle::default();
    match kind {
        HighlightKind::Keyword => {
            style.color = Some(theme.syn_keyword);
        }
        HighlightKind::Function => {
            style.color = Some(theme.syn_function);
        }
        HighlightKind::Type => {
            style.color = Some(theme.syn_type);
        }
        HighlightKind::String => {
            style.color = Some(theme.syn_string);
        }
        HighlightKind::Number => {
            style.color = Some(theme.syn_number);
        }
        HighlightKind::Comment => {
            style.color = Some(theme.syn_comment);
            style.font_style = Some(FontStyle::Italic);
        }
        HighlightKind::Constant => {
            style.color = Some(theme.syn_constant);
        }
        HighlightKind::Variable => {
            style.color = Some(theme.syn_variable);
        }
        HighlightKind::VariableBuiltin => {
            style.color = Some(theme.syn_variable_builtin);
        }
        HighlightKind::Property => {
            style.color = Some(theme.syn_property);
        }
        HighlightKind::Operator => {
            style.color = Some(theme.syn_operator);
        }
        HighlightKind::Punctuation => {
            style.color = Some(theme.syn_punctuation);
        }
        HighlightKind::Attribute => {
            style.color = Some(theme.syn_attribute);
        }
        HighlightKind::Tag => {
            style.color = Some(theme.syn_tag);
        }
        HighlightKind::Macro => {
            style.color = Some(theme.syn_macro);
        }
        HighlightKind::Namespace => {
            style.color = Some(theme.syn_namespace);
        }
        HighlightKind::Label => {
            style.color = Some(theme.syn_label);
        }
        HighlightKind::Escape => {
            style.color = Some(theme.syn_escape);
        }
        HighlightKind::Constructor => {
            style.color = Some(theme.syn_constructor);
        }
        HighlightKind::Embedded => {
            style.color = Some(theme.text_secondary);
        }
        HighlightKind::Heading => {
            style.color = Some(theme.syn_heading);
            style.font_weight = Some(FontWeight::BOLD);
        }
        HighlightKind::Link => {
            style.color = Some(theme.syn_link);
        }
        HighlightKind::Emphasis => {
            style.font_style = Some(FontStyle::Italic);
        }
        HighlightKind::Strong => {
            style.font_weight = Some(FontWeight::BOLD);
        }
    }
    style
}
