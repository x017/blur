//! Code folding ranges and structural outline extraction from tree-sitter syntax trees.

use std::collections::BTreeMap;
use std::ops::Range;

use ropey::Rope;
use tree_sitter::{Node, Tree};

/// A structural symbol in a file's outline (functions, structs, classes, headings, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    pub name: String,
    pub kind: String,
    /// 0-based line number where the item starts.
    pub line: usize,
    /// Nesting depth (0 for top-level items).
    pub depth: usize,
}

/// Returns `true` if a named node kind represents a foldable block/container.
fn is_foldable_kind(kind: &str) -> bool {
    matches!(
        kind,
        // General / Rust / C / C++ / JS / TS / Go
        "block"
            | "compound_statement"
            | "statement_block"
            | "declaration_list"
            | "field_declaration_list"
            | "enum_variant_list"
            | "match_block"
            | "function_item"
            | "function_definition"
            | "function_declaration"
            | "method_definition"
            | "method_declaration"
            | "struct_item"
            | "struct_specifier"
            | "enum_item"
            | "enum_specifier"
            | "trait_item"
            | "impl_item"
            | "mod_item"
            | "class_definition"
            | "class_declaration"
            | "class_specifier"
            | "interface_declaration"
            | "namespace_definition"
            | "type_declaration"
            | "type_spec"
            | "if_statement"
            | "if_expression"
            | "for_statement"
            | "for_expression"
            | "while_statement"
            | "while_expression"
            | "loop_expression"
            | "switch_statement"
            | "match_expression"
            | "try_statement"
            | "with_statement"
            | "closure_expression"
            | "arrow_function"
            // JSON / YAML / TOML / HTML / CSS / Markdown / Lua / Bash
            | "object"
            | "array"
            | "table"
            | "table_array_element"
            | "table_constructor"
            | "block_mapping"
            | "block_sequence"
            | "element"
            | "rule_set"
            | "media_statement"
            | "section"
            | "fenced_code_block"
            | "block_comment"
            | "comment"
            | "subshell"
            | "compound_statement_bash"
    )
}

/// Extracts sorted, deduplicated multi-line fold ranges (`start_row..end_row`) from `tree`.
pub(crate) fn extract_fold_ranges(tree: &Tree) -> Vec<Range<usize>> {
    // Map start_row -> largest end_row so each foldable start line has one canonical range.
    let mut by_start: BTreeMap<usize, usize> = BTreeMap::new();
    let mut cursor = tree.walk();
    let mut visited_children = false;

    loop {
        let node = cursor.node();
        if !visited_children && node.is_named() {
            let start_row = node.start_position().row;
            let end_row = node.end_position().row;
            if end_row > start_row && is_foldable_kind(node.kind()) {
                by_start
                    .entry(start_row)
                    .and_modify(|e| *e = (*e).max(end_row))
                    .or_insert(end_row);
            }
        }

        if !visited_children && cursor.goto_first_child() {
            visited_children = false;
        } else if cursor.goto_next_sibling() {
            visited_children = false;
        } else if cursor.goto_parent() {
            visited_children = true;
        } else {
            break;
        }
    }

    by_start.into_iter().map(|(s, e)| s..e).collect()
}

/// Per-language mapping from tree-sitter node kind to human-readable outline item kind.
fn outline_kind_for(lang_name: &str, node_kind: &str) -> Option<&'static str> {
    match lang_name {
        "rust" => match node_kind {
            "function_item" => Some("function"),
            "struct_item" => Some("struct"),
            "enum_item" => Some("enum"),
            "trait_item" => Some("trait"),
            "impl_item" => Some("impl"),
            "mod_item" => Some("module"),
            "type_item" => Some("type"),
            "const_item" => Some("constant"),
            "static_item" => Some("static"),
            "macro_definition" => Some("macro"),
            _ => None,
        },
        "python" => match node_kind {
            "function_definition" => Some("function"),
            "class_definition" => Some("class"),
            _ => None,
        },
        "go" => match node_kind {
            "function_declaration" => Some("function"),
            "method_declaration" => Some("method"),
            "type_spec" => Some("type"),
            _ => None,
        },
        "javascript" | "typescript" | "tsx" => match node_kind {
            "function_declaration" => Some("function"),
            "method_definition" => Some("method"),
            "class_declaration" => Some("class"),
            "interface_declaration" => Some("interface"),
            "type_alias_declaration" => Some("type"),
            "enum_declaration" => Some("enum"),
            _ => None,
        },
        "c" | "cpp" => match node_kind {
            "function_definition" => Some("function"),
            "struct_specifier" => Some("struct"),
            "class_specifier" => Some("class"),
            "enum_specifier" => Some("enum"),
            "namespace_definition" => Some("namespace"),
            _ => None,
        },
        "lua" => match node_kind {
            "function_declaration" => Some("function"),
            _ => None,
        },
        "bash" => match node_kind {
            "function_definition" => Some("function"),
            _ => None,
        },
        "toml" => match node_kind {
            "table" | "table_array_element" => Some("table"),
            _ => None,
        },
        "markdown" => match node_kind {
            "atx_heading" | "setext_heading" => Some("heading"),
            _ => None,
        },
        "css" => match node_kind {
            "rule_set" => Some("rule"),
            _ => None,
        },
        _ => None,
    }
}

fn node_text(rope: &Rope, node: Node<'_>) -> String {
    let len = rope.len_bytes();
    let start = node.start_byte().min(len);
    let end = node.end_byte().min(len).max(start);
    rope.byte_slice(start..end).to_string()
}

fn extract_name(lang_name: &str, node: Node<'_>, rope: &Rope) -> Option<String> {
    // 1. Most grammars expose a "name" field.
    if let Some(name_node) = node.child_by_field_name("name") {
        let s = node_text(rope, name_node).trim().to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }

    // 2. Rust `impl_item`: combine trait + type (e.g. `impl Display for Foo` -> `Display for Foo`).
    if lang_name == "rust" && node.kind() == "impl_item" {
        let ty = node
            .child_by_field_name("type")
            .map(|n| node_text(rope, n).trim().to_string());
        let tr = node
            .child_by_field_name("trait")
            .map(|n| node_text(rope, n).trim().to_string());
        return match (tr, ty) {
            (Some(t), Some(y)) => Some(format!("{t} for {y}")),
            (None, Some(y)) => Some(y),
            _ => None,
        };
    }

    // 3. C/C++ `function_definition`: look inside `declarator` for identifier/field_identifier.
    if matches!(lang_name, "c" | "cpp") && node.kind() == "function_definition" {
        if let Some(decl) = node.child_by_field_name("declarator") {
            if let Some(inner) = find_first_identifier(decl) {
                let s = node_text(rope, inner).trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
    }

    // 4. Markdown headings, TOML tables, CSS selectors: use trimmed first line of text.
    if matches!(lang_name, "markdown" | "toml" | "css") {
        let raw = node_text(rope, node);
        let first_line = raw.lines().next().unwrap_or("").trim();
        let cleaned = if lang_name == "markdown" {
            first_line.trim_start_matches('#').trim()
        } else if lang_name == "css" {
            first_line.split('{').next().unwrap_or(first_line).trim()
        } else {
            first_line.trim_matches(|c| c == '[' || c == ']').trim()
        };
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }

    // 5. Fallback: first identifier-like child.
    find_first_identifier(node)
        .map(|n| node_text(rope, n).trim().to_string())
        .filter(|s| !s.is_empty())
}

fn find_first_identifier(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(
            child.kind(),
            "identifier"
                | "type_identifier"
                | "field_identifier"
                | "property_identifier"
                | "word"
        ) {
            return Some(child);
        }
        if matches!(
            child.kind(),
            "function_declarator" | "pointer_declarator" | "qualified_identifier"
        ) {
            if let Some(found) = find_first_identifier(child) {
                return Some(found);
            }
        }
    }
    None
}

/// Extracts structural [`OutlineItem`]s from `tree` for `lang_name`.
pub(crate) fn extract_outline(lang_name: &str, tree: &Tree, rope: &Rope) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    collect_outline(lang_name, tree.root_node(), rope, 0, &mut items);
    items
}

fn collect_outline(
    lang_name: &str,
    node: Node<'_>,
    rope: &Rope,
    depth: usize,
    out: &mut Vec<OutlineItem>,
) {
    let next_depth = if let Some(kind) = outline_kind_for(lang_name, node.kind()) {
        if let Some(name) = extract_name(lang_name, node, rope) {
            out.push(OutlineItem {
                name,
                kind: kind.to_string(),
                line: node.start_position().row,
                depth,
            });
            depth + 1
        } else {
            depth
        }
    } else {
        depth
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_named() {
            collect_outline(lang_name, child, rope, next_depth, out);
        }
    }
}
