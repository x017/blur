//! Utilities bridging `blur_lsp` protocol types (`Hover`, `TextEdit`,
//! `WorkspaceEdit`) with `blur_buffer::Buffer`.

use blur_buffer::{Buffer, EditDelta, LspPosition};
use blur_lsp::lsp_types::{Hover, HoverContents, MarkedString, TextEdit};

/// Structured hover content extracted from an LSP `Hover` response for
/// rendering the Stitch Floating Contextual Signature Card.
#[derive(Debug, Clone)]
pub struct HoverCard {
    pub line: usize,
    pub header_tag: String,
    pub signature: Option<String>,
    pub body: String,
    pub location_label: String,
    pub footer_tags: Vec<String>,
}

/// Extract clean signature + description markdown from an LSP `Hover`.
pub fn parse_hover(hover: &Hover, line: usize, location_label: String) -> Option<HoverCard> {
    let raw = match &hover.contents {
        HoverContents::Scalar(ms) => marked_to_string(ms),
        HoverContents::Array(arr) => arr
            .iter()
            .map(marked_to_string)
            .collect::<Vec<_>>()
            .join("\n\n"),
        HoverContents::Markup(m) => m.value.clone(),
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Split first fenced code block as signature if present
    let mut signature = None;
    let mut body_parts = Vec::new();
    let mut in_fence = false;
    let mut fence_buf = Vec::new();
    for l in trimmed.lines() {
        if l.trim_start().starts_with("```") {
            if in_fence {
                in_fence = false;
                if signature.is_none() && !fence_buf.is_empty() {
                    signature = Some(fence_buf.join("\n"));
                } else if !fence_buf.is_empty() {
                    body_parts.push(fence_buf.join("\n"));
                }
                fence_buf.clear();
            } else {
                in_fence = true;
            }
        } else if in_fence {
            fence_buf.push(l.to_string());
        } else if !l.trim().is_empty() {
            body_parts.push(l.to_string());
        }
    }

    let body = body_parts.join("\n");
    Some(HoverCard {
        line,
        header_tag: "lsp_types · hover inspection".to_string(),
        signature,
        body,
        location_label,
        footer_tags: vec![
            "Zero Alloc".to_string(),
            "Thread-Safe".to_string(),
            "UTF-16 Sync".to_string(),
        ],
    })
}

fn marked_to_string(ms: &MarkedString) -> String {
    match ms {
        MarkedString::String(s) => s.clone(),
        MarkedString::LanguageString(ls) => format!("```{}\n{}\n```", ls.language, ls.value),
    }
}

/// Apply a list of LSP `TextEdit`s (in original document UTF-16 coordinates)
/// to a `blur_buffer::Buffer`, returning sequential `EditDelta`s.
pub fn apply_text_edits(buffer: &mut Buffer, edits: &[TextEdit]) -> Vec<EditDelta> {
    if edits.is_empty() {
        return Vec::new();
    }
    let char_edits: Vec<_> = edits
        .iter()
        .map(|te| {
            let start = buffer.lsp_to_char(LspPosition {
                line: te.range.start.line,
                character: te.range.start.character,
            });
            let end = buffer.lsp_to_char(LspPosition {
                line: te.range.end.line,
                character: te.range.end.character,
            });
            (start..end, te.new_text.clone())
        })
        .collect();
    buffer.edit(char_edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use blur_lsp::lsp_types::{MarkupContent, MarkupKind, Position, Range};

    #[test]
    fn parses_hover_signature_and_body() {
        let h = Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "```rust\npub fn foo() -> i32\n```\n\nReturns forty-two.".to_string(),
            }),
            range: None,
        };
        let card = parse_hover(&h, 10, "main.rs:11".into()).unwrap();
        assert_eq!(card.signature.as_deref(), Some("pub fn foo() -> i32"));
        assert_eq!(card.body, "Returns forty-two.");
    }

    #[test]
    fn applies_lsp_text_edits() {
        let mut buf = Buffer::from_text("let foo = 1;\nlet bar = foo;\n");
        let edits = vec![
            TextEdit {
                range: Range::new(Position::new(0, 4), Position::new(0, 7)),
                new_text: "baz".into(),
            },
            TextEdit {
                range: Range::new(Position::new(1, 10), Position::new(1, 13)),
                new_text: "baz".into(),
            },
        ];
        let deltas = apply_text_edits(&mut buf, &edits);
        assert_eq!(deltas.len(), 2);
        assert_eq!(buf.text(), "let baz = 1;\nlet bar = baz;\n");
    }
}
