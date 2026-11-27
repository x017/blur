//! `blur_syntax` — GPUI-free tree-sitter incremental parsing, range-restricted syntax
//! highlighting, code folding, and structural outline extraction.

mod highlight;
mod outline;
mod registry;

use std::ops::Range;
use std::sync::Arc;

use ropey::Rope;
use tree_sitter::{Node, Parser, QueryCursor, StreamingIterator, TextProvider, Tree};

pub use highlight::{HighlightKind, HighlightSpan};
pub use outline::OutlineItem;
pub use registry::{Language, LanguageRegistry};
pub use tree_sitter::{InputEdit, Point as TsPoint};

use highlight::{flatten_spans, RawSpan};

/// Zero-allocation `tree_sitter::TextProvider` backed directly by a [`Rope`].
struct RopeTextProvider<'a>(&'a Rope);

struct RopeChunkBytes<'a>(ropey::iter::Chunks<'a>);

impl<'a> Iterator for RopeChunkBytes<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(str::as_bytes)
    }
}

impl<'a> TextProvider<&'a [u8]> for RopeTextProvider<'a> {
    type I = RopeChunkBytes<'a>;

    fn text(&mut self, node: Node) -> Self::I {
        let len = self.0.len_bytes();
        let start = node.start_byte().min(len);
        let end = node.end_byte().min(len).max(start);
        RopeChunkBytes(self.0.byte_slice(start..end).chunks())
    }
}

/// Parses `rope` incrementally (if `old_tree` is `Some`) using a chunk callback without
/// allocating a contiguous copy of the entire document.
fn parse_rope(parser: &mut Parser, rope: &Rope, old_tree: Option<&Tree>) -> Option<Tree> {
    let total_bytes = rope.len_bytes();
    parser.parse_with_options(
        &mut |byte_idx, _pos| {
            if byte_idx >= total_bytes {
                return &[][..];
            }
            let (chunk, chunk_byte_idx, _, _) = rope.chunk_at_byte(byte_idx);
            &chunk.as_bytes()[byte_idx - chunk_byte_idx..]
        },
        old_tree,
        None,
    )
}

/// Holds a tree-sitter parser and syntax tree for one open buffer.
pub struct Syntax {
    parser: Parser,
    tree: Option<Tree>,
    language: Arc<Language>,
}

impl std::fmt::Debug for Syntax {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Syntax")
            .field("language", &self.language.name)
            .field("has_tree", &self.tree.is_some())
            .finish()
    }
}

impl Syntax {
    /// Creates a new `Syntax` instance and performs an initial full parse of `rope`.
    pub fn new(lang: Arc<Language>, rope: &Rope) -> Self {
        let mut parser = Parser::new();
        if let Err(err) = parser.set_language(&lang.ts_language) {
            tracing::warn!(
                language = lang.name,
                "failed to set tree-sitter language: {err}"
            );
            return Self {
                parser,
                tree: None,
                language: lang,
            };
        }
        let tree = parse_rope(&mut parser, rope, None);
        Self {
            parser,
            tree,
            language: lang,
        }
    }

    /// Notifies the existing syntax tree of a single applied [`InputEdit`].
    ///
    /// Call once per edit delta in sequential application order, then call [`Syntax::reparse`].
    pub fn edit(&mut self, edit: &InputEdit) {
        if let Some(tree) = &mut self.tree {
            tree.edit(edit);
        }
    }

    /// Incrementally reparses `rope` reusing the previously edited syntax tree.
    pub fn reparse(&mut self, rope: &Rope) {
        self.tree = parse_rope(&mut self.parser, rope, self.tree.as_ref());
    }

    /// Computes sorted, non-overlapping [`HighlightSpan`]s restricted to `byte_range`
    /// using the existing syntax tree (never reparses).
    pub fn highlights(&self, rope: &Rope, byte_range: Range<usize>) -> Vec<HighlightSpan> {
        let total = rope.len_bytes();
        let start = byte_range.start.min(total);
        let end = byte_range.end.min(total);
        if start >= end {
            return Vec::new();
        }
        let clipped = start..end;

        let (Some(tree), Some(query)) = (&self.tree, &self.language.highlight_query) else {
            return Vec::new();
        };

        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(clipped.clone());

        let mut raw = Vec::new();
        let mut order = 0usize;
        let provider = RopeTextProvider(rope);
        let mut captures = cursor.captures(query, tree.root_node(), provider);

        while let Some((m, cap_idx)) = captures.next() {
            let capture = m.captures[*cap_idx];
            if let Some(Some(kind)) = self.language.capture_kinds.get(capture.index as usize) {
                let node = capture.node;
                let ns = node.start_byte();
                let ne = node.end_byte();
                if ne > clipped.start && ns < clipped.end && ns < ne {
                    raw.push(RawSpan {
                        start: ns,
                        end: ne,
                        orig_len: ne - ns,
                        order,
                        kind: *kind,
                    });
                    order += 1;
                }
            }
        }

        flatten_spans(raw, clipped)
    }

    /// Returns sorted multi-line fold ranges (`start_row..end_row`) from the current tree.
    pub fn fold_ranges(&self) -> Vec<Range<usize>> {
        match &self.tree {
            Some(tree) => outline::extract_fold_ranges(tree),
            None => Vec::new(),
        }
    }

    /// Extracts structural [`OutlineItem`]s (functions, structs, classes, headings, etc.).
    pub fn outline(&self, rope: &Rope) -> Vec<OutlineItem> {
        match &self.tree {
            Some(tree) => outline::extract_outline(self.language.name, tree, rope),
            None => Vec::new(),
        }
    }

    /// Returns the [`Language`] associated with this syntax tree.
    pub fn language(&self) -> &Arc<Language> {
        &self.language
    }

    /// Returns a reference to the current `tree_sitter::Tree`, if available.
    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn all_builtin_language_queries_compile() {
        let reg = LanguageRegistry::new();
        let all = reg.all();
        assert_eq!(all.len(), 16);
        for lang in &all {
            assert!(
                lang.has_highlight_query(),
                "highlight query failed to compile for language '{}'",
                lang.name
            );
        }
    }

    #[test]
    fn language_detection_by_path_and_name() {
        let reg = LanguageRegistry::new();
        assert_eq!(reg.for_path(Path::new("src/main.rs")).unwrap().name, "rust");
        assert_eq!(reg.for_path(Path::new("app.tsx")).unwrap().name, "tsx");
        assert_eq!(reg.for_path(Path::new("Cargo.lock")).unwrap().name, "toml");
        assert_eq!(reg.for_path(Path::new(".bashrc")).unwrap().name, "bash");
        assert_eq!(reg.for_path(Path::new("README.MD")).unwrap().name, "markdown");
        assert!(reg.for_path(Path::new("unknown.xyz")).is_none());

        assert_eq!(reg.by_name("typescriptreact").unwrap().name, "tsx");
        assert_eq!(reg.by_name("Rust").unwrap().name, "rust");
    }

    #[test]
    fn rust_highlights_contain_keyword_function_string_and_are_sorted_non_overlapping() {
        let reg = LanguageRegistry::new();
        let rust = reg.by_name("rust").unwrap();
        let src = r#"fn greet(name: &str) -> String {
    let msg = "hello";
    format!("{msg} {name}")
}
"#;
        let rope = Rope::from_str(src);
        let syntax = Syntax::new(rust, &rope);
        let spans = syntax.highlights(&rope, 0..rope.len_bytes());

        assert!(!spans.is_empty());
        // Verify sorted and strictly non-overlapping.
        for w in spans.windows(2) {
            assert!(w[0].range.start < w[0].range.end);
            assert!(w[0].range.end <= w[1].range.start);
        }

        let has_keyword = spans.iter().any(|s| s.kind == HighlightKind::Keyword);
        let has_function = spans.iter().any(|s| s.kind == HighlightKind::Function);
        let has_string = spans.iter().any(|s| s.kind == HighlightKind::String);
        assert!(has_keyword, "expected Keyword highlight in Rust snippet");
        assert!(has_function, "expected Function highlight in Rust snippet");
        assert!(has_string, "expected String highlight in Rust snippet");
    }

    #[test]
    fn incremental_edit_keeps_tree_valid() {
        let reg = LanguageRegistry::new();
        let rust = reg.by_name("rust").unwrap();
        let mut rope = Rope::from_str("fn foo() { let x = 1; }\n");
        let mut syntax = Syntax::new(rust, &rope);
        assert!(!syntax.tree().unwrap().root_node().has_error());

        // Replace "foo" (bytes 3..6) with "bar_baz" (bytes 3..10).
        rope.remove(3..6);
        rope.insert(3, "bar_baz");
        syntax.edit(&InputEdit {
            start_byte: 3,
            old_end_byte: 6,
            new_end_byte: 10,
            start_position: TsPoint::new(0, 3),
            old_end_position: TsPoint::new(0, 6),
            new_end_position: TsPoint::new(0, 10),
        });
        syntax.reparse(&rope);

        let tree = syntax.tree().unwrap();
        assert!(!tree.root_node().has_error());
        let outline = syntax.outline(&rope);
        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].name, "bar_baz");
        assert_eq!(outline[0].kind, "function");
    }

    #[test]
    fn fold_ranges_and_outline() {
        let reg = LanguageRegistry::new();
        let rust = reg.by_name("rust").unwrap();
        let src = r#"struct Point {
    x: i32,
    y: i32,
}

impl Point {
    fn sum(&self) -> i32 {
        self.x + self.y
    }
}
"#;
        let rope = Rope::from_str(src);
        let syntax = Syntax::new(rust, &rope);

        let folds = syntax.fold_ranges();
        assert!(folds.contains(&(0..3)), "expected struct fold 0..3, got {folds:?}");
        assert!(folds.contains(&(5..9)), "expected impl fold 5..9, got {folds:?}");
        assert!(folds.contains(&(6..8)), "expected fn fold 6..8, got {folds:?}");

        let outline = syntax.outline(&rope);
        assert_eq!(outline.len(), 3);
        assert_eq!(outline[0].name, "Point");
        assert_eq!(outline[0].kind, "struct");
        assert_eq!(outline[0].depth, 0);
        assert_eq!(outline[1].name, "Point");
        assert_eq!(outline[1].kind, "impl");
        assert_eq!(outline[1].depth, 0);
        assert_eq!(outline[2].name, "sum");
        assert_eq!(outline[2].kind, "function");
        assert_eq!(outline[2].depth, 1);

        // Verify highlights restricted to a sub-range stay strictly within that byte range.
        let line6_start = rope.line_to_byte(6);
        let line7_end = rope.line_to_byte(8);
        let sub_spans = syntax.highlights(&rope, line6_start..line7_end);
        assert!(!sub_spans.is_empty());
        for sp in &sub_spans {
            assert!(sp.range.start >= line6_start && sp.range.end <= line7_end);
        }
    }
}
