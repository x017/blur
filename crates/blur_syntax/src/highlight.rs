//! Highlight kinds, capture-name prefix mapping, and span flattening.

use std::ops::Range;

/// Semantic highlight category mapped from tree-sitter highlight query captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HighlightKind {
    Keyword,
    Function,
    Type,
    String,
    Number,
    Comment,
    Constant,
    Variable,
    VariableBuiltin,
    Property,
    Operator,
    Punctuation,
    Attribute,
    Tag,
    Macro,
    Namespace,
    Label,
    Escape,
    Constructor,
    Embedded,
    Heading,
    Link,
    Emphasis,
    Strong,
}

/// A highlighted byte range in the source buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    /// UTF-8 byte range in the buffer.
    pub range: Range<usize>,
    pub kind: HighlightKind,
}

/// Ordered prefix table for longest-prefix matching of tree-sitter capture names.
///
/// More specific dotted prefixes (e.g. `"variable.builtin"`, `"function.macro"`,
/// `"string.escape"`) take precedence over shorter prefixes (`"variable"`, `"function"`, `"string"`).
const CAPTURE_PREFIXES: &[(&str, HighlightKind)] = &[
    // Specific multi-segment captures
    ("variable.builtin", HighlightKind::VariableBuiltin),
    ("variable.parameter", HighlightKind::Variable),
    ("variable.member", HighlightKind::Property),
    ("function.macro", HighlightKind::Macro),
    ("function.builtin", HighlightKind::Function),
    ("function.method", HighlightKind::Function),
    ("type.builtin", HighlightKind::Type),
    ("constant.builtin", HighlightKind::Constant),
    ("string.escape", HighlightKind::Escape),
    ("string.special.key", HighlightKind::Property),
    ("string.special.url", HighlightKind::Link),
    ("string.special", HighlightKind::String),
    ("markup.heading", HighlightKind::Heading),
    ("markup.link", HighlightKind::Link),
    ("markup.italic", HighlightKind::Emphasis),
    ("markup.bold", HighlightKind::Strong),
    ("markup.strong", HighlightKind::Strong),
    ("markup.raw", HighlightKind::Embedded),
    ("text.title", HighlightKind::Heading),
    ("text.uri", HighlightKind::Link),
    ("text.reference", HighlightKind::Link),
    ("text.emphasis", HighlightKind::Emphasis),
    ("text.strong", HighlightKind::Strong),
    ("text.literal", HighlightKind::Embedded),
    // Top-level captures
    ("keyword", HighlightKind::Keyword),
    ("function", HighlightKind::Function),
    ("method", HighlightKind::Function),
    ("type", HighlightKind::Type),
    ("string", HighlightKind::String),
    ("character", HighlightKind::String),
    ("number", HighlightKind::Number),
    ("float", HighlightKind::Number),
    ("boolean", HighlightKind::Constant),
    ("comment", HighlightKind::Comment),
    ("constant", HighlightKind::Constant),
    ("variable", HighlightKind::Variable),
    ("property", HighlightKind::Property),
    ("field", HighlightKind::Property),
    ("operator", HighlightKind::Operator),
    ("punctuation", HighlightKind::Punctuation),
    ("attribute", HighlightKind::Attribute),
    ("tag", HighlightKind::Tag),
    ("macro", HighlightKind::Macro),
    ("namespace", HighlightKind::Namespace),
    ("module", HighlightKind::Namespace),
    ("label", HighlightKind::Label),
    ("escape", HighlightKind::Escape),
    ("constructor", HighlightKind::Constructor),
    ("embedded", HighlightKind::Embedded),
    ("heading", HighlightKind::Heading),
    ("link", HighlightKind::Link),
    ("emphasis", HighlightKind::Emphasis),
    ("strong", HighlightKind::Strong),
];

/// Maps a tree-sitter capture name (e.g. `"keyword.function"`, `"string.escape"`)
/// to a [`HighlightKind`] by longest-prefix match on `.`-delimited segments.
pub fn map_capture_name(name: &str) -> Option<HighlightKind> {
    let mut best: Option<(usize, HighlightKind)> = None;
    for &(prefix, kind) in CAPTURE_PREFIXES {
        let matches = name == prefix
            || (name.len() > prefix.len()
                && name.starts_with(prefix)
                && name.as_bytes()[prefix.len()] == b'.');
        if matches && best.is_none_or(|(len, _)| prefix.len() > len) {
            best = Some((prefix.len(), kind));
        }
    }
    best.map(|(_, k)| k)
}

/// A raw capture candidate prior to overlap resolution.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RawSpan {
    pub start: usize,
    pub end: usize,
    /// Original capture byte length (smaller = more specific / innermost).
    pub orig_len: usize,
    /// Capture sequence order (higher = later query rule, wins ties).
    pub order: usize,
    pub kind: HighlightKind,
}

/// Flattens potentially overlapping raw spans into sorted, non-overlapping [`HighlightSpan`]s
/// clamped to `clip_range`, where the innermost (smallest original byte span) capture wins,
/// and later query rules win ties. Adjacent spans of identical [`HighlightKind`] are merged.
pub(crate) fn flatten_spans(raw: Vec<RawSpan>, clip_range: Range<usize>) -> Vec<HighlightSpan> {
    if raw.is_empty() || clip_range.start >= clip_range.end {
        return Vec::new();
    }

    // Collect all unique boundary points within clip_range.
    let mut points: Vec<usize> = Vec::with_capacity(raw.len() * 2);
    for s in &raw {
        let a = s.start.max(clip_range.start);
        let b = s.end.min(clip_range.end);
        if a < b {
            points.push(a);
            points.push(b);
        }
    }
    if points.is_empty() {
        return Vec::new();
    }
    points.sort_unstable();
    points.dedup();

    // Sort raw spans by start so we can advance a window efficiently.
    let mut sorted = raw;
    sorted.sort_by_key(|s| s.start);

    let mut active: Vec<RawSpan> = Vec::new();
    let mut raw_idx = 0usize;
    let mut result: Vec<HighlightSpan> = Vec::new();

    for win in points.windows(2) {
        let seg_start = win[0];
        let seg_end = win[1];
        if seg_start >= seg_end {
            continue;
        }

        // Add spans that start at or before seg_start.
        while raw_idx < sorted.len() && sorted[raw_idx].start <= seg_start {
            if sorted[raw_idx].end > seg_start {
                active.push(sorted[raw_idx]);
            }
            raw_idx += 1;
        }
        // Drop spans that ended at or before seg_start.
        active.retain(|s| s.end > seg_start);

        // Pick the most specific active span (smallest orig_len, then largest order).
        if let Some(winner) = active.iter().min_by(|a, b| {
            a.orig_len
                .cmp(&b.orig_len)
                .then_with(|| b.order.cmp(&a.order))
        }) {
            if let Some(last) = result.last_mut() {
                if last.kind == winner.kind && last.range.end == seg_start {
                    last.range.end = seg_end;
                    continue;
                }
            }
            result.push(HighlightSpan {
                range: seg_start..seg_end,
                kind: winner.kind,
            });
        }
    }

    result
}
