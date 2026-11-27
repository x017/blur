//! Completion engine combining LSP `CompletionItem`s with fast local buffer
//! identifier extraction and live prefix filtering.

use std::collections::HashSet;

use blur_buffer::Buffer;
use blur_lsp::lsp_types::{CompletionItemKind, CompletionTextEdit};
use blur_lsp::CompletionItem;

/// A normalized completion candidate ready for rendering and insertion.
#[derive(Debug, Clone)]
pub struct CompletionCandidate {
    pub label: String,
    pub insert_text: String,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub kind: Option<CompletionItemKind>,
    pub kind_icon: &'static str,
    pub kind_tag: &'static str,
    pub from_lsp: bool,
    pub sort_key: String,
    pub score: i32,
}

/// Map `CompletionItemKind` to a VS Code-style icon glyph.
pub fn kind_icon(kind: Option<CompletionItemKind>) -> &'static str {
    match kind {
        Some(CompletionItemKind::FUNCTION) | Some(CompletionItemKind::METHOD) | Some(CompletionItemKind::CONSTRUCTOR) => "ƒ",
        Some(CompletionItemKind::STRUCT) | Some(CompletionItemKind::CLASS) | Some(CompletionItemKind::INTERFACE) => "◈",
        Some(CompletionItemKind::ENUM) | Some(CompletionItemKind::ENUM_MEMBER) => "∈",
        Some(CompletionItemKind::MODULE) => "{}",
        Some(CompletionItemKind::FIELD) | Some(CompletionItemKind::PROPERTY) => "▪",
        Some(CompletionItemKind::VARIABLE) => "𝑥",
        Some(CompletionItemKind::CONSTANT) => "π",
        Some(CompletionItemKind::KEYWORD) => "⌘",
        Some(CompletionItemKind::SNIPPET) => "✂",
        Some(CompletionItemKind::TYPE_PARAMETER) => "T",
        Some(CompletionItemKind::OPERATOR) => "±",
        _ => "≡",
    }
}

/// VS Code-style priority boost so functions, methods, and classes/structs appear first.
pub fn kind_priority(kind: Option<CompletionItemKind>) -> i32 {
    match kind {
        Some(CompletionItemKind::METHOD) | Some(CompletionItemKind::FUNCTION) | Some(CompletionItemKind::CONSTRUCTOR) => 400,
        Some(CompletionItemKind::STRUCT) | Some(CompletionItemKind::CLASS) | Some(CompletionItemKind::INTERFACE) | Some(CompletionItemKind::ENUM) | Some(CompletionItemKind::TYPE_PARAMETER) => 350,
        Some(CompletionItemKind::FIELD) | Some(CompletionItemKind::PROPERTY) | Some(CompletionItemKind::VARIABLE) | Some(CompletionItemKind::CONSTANT) => 300,
        Some(CompletionItemKind::MODULE) | Some(CompletionItemKind::ENUM_MEMBER) => 250,
        Some(CompletionItemKind::KEYWORD) => 100,
        Some(CompletionItemKind::SNIPPET) => 50,
        _ => 10,
    }
}

/// Map `CompletionItemKind` to a compact monospaced badge label.
pub fn kind_tag(kind: Option<CompletionItemKind>) -> &'static str {
    match kind {
        Some(CompletionItemKind::FUNCTION) => "fn",
        Some(CompletionItemKind::METHOD) => "meth",
        Some(CompletionItemKind::CONSTRUCTOR) => "new",
        Some(CompletionItemKind::STRUCT) => "struct",
        Some(CompletionItemKind::CLASS) => "class",
        Some(CompletionItemKind::INTERFACE) => "trait",
        Some(CompletionItemKind::ENUM) | Some(CompletionItemKind::ENUM_MEMBER) => "enum",
        Some(CompletionItemKind::MODULE) => "mod",
        Some(CompletionItemKind::FIELD) | Some(CompletionItemKind::PROPERTY) => "field",
        Some(CompletionItemKind::VARIABLE) => "var",
        Some(CompletionItemKind::CONSTANT) => "const",
        Some(CompletionItemKind::KEYWORD) => "kw",
        Some(CompletionItemKind::SNIPPET) => "snip",
        Some(CompletionItemKind::TYPE_PARAMETER) => "type",
        Some(CompletionItemKind::OPERATOR) => "op",
        _ => "txt",
    }
}

/// Extract clean insert text from an LSP `CompletionItem` (stripping simple `$0`/`${1:foo}` snippet placeholders).
pub fn clean_insert_text(item: &CompletionItem) -> String {
    let raw = match item.text_edit.as_ref() {
        Some(CompletionTextEdit::Edit(edit)) => edit.new_text.as_str(),
        Some(CompletionTextEdit::InsertAndReplace(edit)) => edit.new_text.as_str(),
        None => item.insert_text.as_deref().unwrap_or(&item.label),
    };
    strip_snippet_placeholders(raw)
}

fn strip_snippet_placeholders(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                // ${1:default} -> default
                if let Some(end) = s[i + 2..].find('}') {
                    let inner = &s[i + 2..i + 2 + end];
                    if let Some(colon) = inner.find(':') {
                        out.push_str(&inner[colon + 1..]);
                    }
                    i = i + 2 + end + 1;
                    continue;
                }
            } else if i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
                i += 2;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Extract the identifier prefix immediately before `cursor_char` in `buffer`.
pub fn identifier_prefix_at(buffer: &Buffer, cursor_char: usize) -> (usize, String) {
    let rope = buffer.rope();
    let clamped = cursor_char.min(rope.len_chars());
    let mut start = clamped;
    while start > 0 {
        let ch = rope.char(start - 1);
        if ch.is_alphanumeric() || ch == '_' {
            start -= 1;
        } else {
            break;
        }
    }
    let prefix: String = rope.slice(start..clamped).chars().collect();
    (start, prefix)
}

/// Collect unique identifier tokens from the current buffer for instant local
/// autocomplete (works offline and while LSP servers are starting up).
pub fn local_buffer_words(buffer: &Buffer, exclude: &str, max_words: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut words = Vec::new();
    let text = buffer.text();

    let mut start = None;
    for (idx, ch) in text.char_indices() {
        let is_ident = ch.is_alphanumeric() || ch == '_';
        match (start, is_ident) {
            (None, true) if !ch.is_ascii_digit() => start = Some(idx),
            (Some(s), false) => {
                let tok = &text[s..idx];
                if tok.len() >= 2 && tok != exclude && seen.insert(tok.to_string()) {
                    words.push(tok.to_string());
                    if words.len() >= max_words {
                        break;
                    }
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        let tok = &text[s..];
        if tok.len() >= 2 && tok != exclude && seen.insert(tok.to_string()) {
            words.push(tok.to_string());
        }
    }
    words
}

/// Score a candidate label against a typed prefix (higher is better; `None` means no match).
pub fn score_match(label: &str, prefix: &str) -> Option<i32> {
    if prefix.is_empty() {
        return Some(10);
    }
    let l_low = label.to_ascii_lowercase();
    let p_low = prefix.to_ascii_lowercase();
    if l_low == p_low {
        return Some(1000);
    }
    if label.starts_with(prefix) {
        return Some(800 - (label.len() as i32).min(200));
    }
    if l_low.starts_with(&p_low) {
        return Some(650 - (label.len() as i32).min(200));
    }
    if let Some(pos) = l_low.find(&p_low) {
        return Some(400 - pos as i32 * 4);
    }
    // Subsequence fuzzy match
    let mut p_chars = p_low.chars().peekable();
    let mut consecutive = 0i32;
    let mut bonus = 0i32;
    for ch in l_low.chars() {
        if p_chars.peek() == Some(&ch) {
            p_chars.next();
            consecutive += 1;
            bonus += consecutive * 10;
        } else {
            consecutive = 0;
        }
    }
    if p_chars.peek().is_none() {
        Some(100 + bonus - (label.len() as i32).min(80))
    } else {
        None
    }
}

/// Merge LSP items + local buffer identifiers, filter by `prefix`, and rank.
pub fn build_candidates(
    lsp_items: &[CompletionItem],
    buffer: &Buffer,
    prefix: &str,
    limit: usize,
) -> Vec<CompletionCandidate> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for item in lsp_items {
        let filter_key = item.filter_text.as_deref().unwrap_or(&item.label);
        if let Some(base_score) = score_match(filter_key, prefix) {
            let doc = item.documentation.as_ref().map(|d| match d {
                blur_lsp::lsp_types::Documentation::String(s) => s.clone(),
                blur_lsp::lsp_types::Documentation::MarkupContent(m) => m.value.clone(),
            });
            seen.insert(item.label.clone());
            out.push(CompletionCandidate {
                label: item.label.clone(),
                insert_text: clean_insert_text(item),
                detail: item.detail.clone(),
                documentation: doc,
                kind: item.kind,
                kind_icon: kind_icon(item.kind),
                kind_tag: kind_tag(item.kind),
                from_lsp: true,
                sort_key: item
                    .sort_text
                    .clone()
                    .unwrap_or_else(|| item.label.clone()),
                // Prefer LSP functions/classes first (VS Code style) over local buffer words
                score: base_score + 300 + kind_priority(item.kind),
            });
        }
    }

    for word in local_buffer_words(buffer, prefix, 400) {
        if seen.contains(&word) {
            continue;
        }
        if let Some(score) = score_match(&word, prefix) {
            seen.insert(word.clone());
            out.push(CompletionCandidate {
                label: word.clone(),
                insert_text: word.clone(),
                detail: Some("buffer".to_string()),
                documentation: None,
                kind: None,
                kind_icon: "≡",
                kind_tag: "word",
                from_lsp: false,
                sort_key: word,
                score,
            });
        }
    }

    out.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.sort_key.cmp(&b.sort_key))
            .then_with(|| a.label.cmp(&b.label))
    });
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_snippet_placeholders() {
        assert_eq!(
            strip_snippet_placeholders("dispatch_envelope(${1:msg}, $0)"),
            "dispatch_envelope(msg, )"
        );
    }

    #[test]
    fn builds_ranked_local_and_lsp_candidates() {
        let buf = Buffer::from_text("fn dispatch_envelope() { let dispatcher = 1; }");
        let lsp = vec![CompletionItem {
            label: "dispatch_async".to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some("async fn()".to_string()),
            ..Default::default()
        }];
        let cands = build_candidates(&lsp, &buf, "disp", 10);
        assert!(cands.len() >= 3);
        assert_eq!(cands[0].label, "dispatch_async");
        assert!(cands[0].from_lsp);
        assert_eq!(cands[0].kind_tag, "fn");
        assert_eq!(cands[0].kind_icon, "ƒ");
    }

    #[test]
    fn ranks_functions_and_classes_first_like_vscode() {
        let buf = Buffer::from_text("let foo_local = 1;");
        let lsp = vec![
            CompletionItem {
                label: "foo_kw".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            },
            CompletionItem {
                label: "FooClass".to_string(),
                kind: Some(CompletionItemKind::CLASS),
                ..Default::default()
            },
            CompletionItem {
                label: "foo_fn".to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                ..Default::default()
            },
        ];
        let cands = build_candidates(&lsp, &buf, "foo", 10);
        assert_eq!(cands[0].label, "foo_fn");
        assert_eq!(cands[1].label, "FooClass");
        assert_eq!(cands[2].label, "foo_kw");
        assert_eq!(cands[3].label, "foo_local");
    }
}
