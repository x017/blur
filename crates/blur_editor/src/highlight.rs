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

/// Merge overlapping highlight ranges into disjoint atomic segments.
///
/// `StyledText::with_highlights` assumes its ranges are sorted and
/// **non-overlapping**: an overlap makes its run builder overshoot the line
/// length and desynchronize every later run — which is why selections used to
/// visually wipe out the keyword colors underneath them. Each atomic segment
/// here combines all covering styles field-by-field, with later pushes
/// (selection → find → bracket → diagnostic) winning only on the fields they
/// actually set, so a selection background coexists with the syntax color.
pub fn merge_overlapping(
    mut ranges: Vec<(std::ops::Range<usize>, HighlightStyle)>,
) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    if ranges.len() < 2 {
        return ranges;
    }
    // Outer ranges first (longest end wins at equal start), then inner ones —
    // so nested syntax spans are re-applied after their container.
    ranges.sort_by_key(|(r, _)| (r.start, std::cmp::Reverse(r.end)));
    let mut points: Vec<usize> = Vec::with_capacity(ranges.len() * 2);
    for (r, _) in &ranges {
        points.push(r.start);
        points.push(r.end);
    }
    points.sort_unstable();
    points.dedup();
    let mut out: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= b {
            continue;
        }
        let mut merged: Option<HighlightStyle> = None;
        for (r, st) in &ranges {
            if r.start <= a && r.end >= b {
                match &mut merged {
                    None => merged = Some(st.clone()),
                    Some(dst) => merge_fields(dst, st),
                }
            }
        }
        if let Some(st) = merged {
            out.push((a..b, st));
        }
    }
    out
}

/// Field-wise "later wins": only overwrite attributes the source actually sets.
fn merge_fields(dst: &mut HighlightStyle, src: &HighlightStyle) {
    if src.color.is_some() {
        dst.color = src.color;
    }
    if src.background_color.is_some() {
        dst.background_color = src.background_color;
    }
    if src.font_weight.is_some() {
        dst.font_weight = src.font_weight;
    }
    if src.font_style.is_some() {
        dst.font_style = src.font_style;
    }
    if src.underline.is_some() {
        dst.underline = src.underline;
    }
}
