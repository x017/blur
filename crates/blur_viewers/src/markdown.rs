//! GitHub-Flavored Markdown parser into render-ready blocks and inlines.

use pulldown_cmark::{
    Alignment as CmAlignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

/// A styled inline span inside a heading, paragraph, or table cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inline {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub link: Option<String>,
    pub math: bool,
}

impl Inline {
    /// Construct a plain unstyled inline span.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
            italic: false,
            code: false,
            strike: false,
            link: None,
            math: false,
        }
    }
}

/// Column alignment in a GFM table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    None,
    Left,
    Center,
    Right,
}

impl From<CmAlignment> for Alignment {
    fn from(a: CmAlignment) -> Self {
        match a {
            CmAlignment::None => Self::None,
            CmAlignment::Left => Self::Left,
            CmAlignment::Center => Self::Center,
            CmAlignment::Right => Self::Right,
        }
    }
}

/// A single item in a list, supporting GFM task checkboxes and nested blocks/lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    /// `Some(true)` for `[x]`, `Some(false)` for `[ ]`, `None` for regular items.
    pub checked: Option<bool>,
    /// Child blocks inside this item (paragraphs, nested lists, code blocks, etc.).
    pub blocks: Vec<Block>,
}

/// A top-level or nested Markdown block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading {
        level: u8,
        inlines: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    CodeBlock {
        lang: Option<String>,
        code: String,
    },
    BlockQuote(Vec<Block>),
    List {
        ordered: bool,
        start: u64,
        items: Vec<ListItem>,
    },
    Table {
        alignments: Vec<Alignment>,
        headers: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Rule,
    /// Display math (`$$...$$` or ```` ```math ````).
    Math(String),
    /// Mermaid diagram (```` ```mermaid ````).
    Mermaid(String),
    /// Footnote definition (`[^label]: ...`).
    FootnoteDefinition {
        label: String,
        blocks: Vec<Block>,
    },
}

#[derive(Default)]
struct InlineStyle {
    bold: usize,
    italic: usize,
    strike: usize,
    links: Vec<String>,
}

impl InlineStyle {
    fn make(&self, text: String, code: bool, math: bool) -> Inline {
        Inline {
            text,
            bold: self.bold > 0,
            italic: self.italic > 0,
            code,
            strike: self.strike > 0,
            link: self.links.last().cloned(),
            math,
        }
    }
}

enum Container {
    Root(Vec<Block>),
    Heading {
        level: u8,
        inlines: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    CodeBlock {
        lang: Option<String>,
        code: String,
    },
    BlockQuote(Vec<Block>),
    List {
        ordered: bool,
        start: u64,
        items: Vec<ListItem>,
    },
    ListItem {
        checked: Option<bool>,
        blocks: Vec<Block>,
        pending_inlines: Vec<Inline>,
    },
    Table {
        alignments: Vec<Alignment>,
        headers: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
        in_head: bool,
        current_row: Vec<Vec<Inline>>,
        current_cell: Option<Vec<Inline>>,
    },
    Footnote {
        label: String,
        blocks: Vec<Block>,
    },
}

fn push_block(stack: &mut [Container], block: Block) {
    if let Some(top) = stack.last_mut() {
        match top {
            Container::Root(blocks)
            | Container::BlockQuote(blocks)
            | Container::Footnote { blocks, .. } => blocks.push(block),
            Container::ListItem {
                blocks,
                pending_inlines,
                ..
            } => {
                if !pending_inlines.is_empty() {
                    let inlines = std::mem::take(pending_inlines);
                    blocks.push(Block::Paragraph(inlines));
                }
                blocks.push(block);
            }
            _ => {}
        }
    }
}

fn push_inline(stack: &mut [Container], inline: Inline) {
    if let Some(top) = stack.last_mut() {
        match top {
            Container::Heading { inlines, .. } | Container::Paragraph(inlines) => {
                inlines.push(inline);
            }
            Container::ListItem {
                pending_inlines, ..
            } => {
                pending_inlines.push(inline);
            }
            Container::Table {
                current_cell: Some(cell),
                ..
            } => {
                cell.push(inline);
            }
            Container::CodeBlock { code, .. } => {
                code.push_str(&inline.text);
            }
            _ => {}
        }
    }
}

fn heading_level_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Parse GitHub-Flavored Markdown (with math and Mermaid extensions) into a list of [`Block`]s.
pub fn parse(src: &str) -> Vec<Block> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_MATH;

    let parser = Parser::new_ext(src, options);
    let mut stack: Vec<Container> = vec![Container::Root(Vec::new())];
    let mut style = InlineStyle::default();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    stack.push(Container::Heading {
                        level: heading_level_u8(level),
                        inlines: Vec::new(),
                    });
                }
                Tag::Paragraph => {
                    stack.push(Container::Paragraph(Vec::new()));
                }
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(info) => {
                            let token = info.split_whitespace().next().unwrap_or("").trim();
                            if token.is_empty() {
                                None
                            } else {
                                Some(token.to_string())
                            }
                        }
                        CodeBlockKind::Indented => None,
                    };
                    stack.push(Container::CodeBlock {
                        lang,
                        code: String::new(),
                    });
                }
                Tag::BlockQuote(_) => {
                    stack.push(Container::BlockQuote(Vec::new()));
                }
                Tag::List(start) => {
                    stack.push(Container::List {
                        ordered: start.is_some(),
                        start: start.unwrap_or(1),
                        items: Vec::new(),
                    });
                }
                Tag::Item => {
                    stack.push(Container::ListItem {
                        checked: None,
                        blocks: Vec::new(),
                        pending_inlines: Vec::new(),
                    });
                }
                Tag::Table(alignments) => {
                    stack.push(Container::Table {
                        alignments: alignments.into_iter().map(Alignment::from).collect(),
                        headers: Vec::new(),
                        rows: Vec::new(),
                        in_head: false,
                        current_row: Vec::new(),
                        current_cell: None,
                    });
                }
                Tag::TableHead => {
                    if let Some(Container::Table {
                        in_head,
                        current_row,
                        ..
                    }) = stack.last_mut()
                    {
                        *in_head = true;
                        current_row.clear();
                    }
                }
                Tag::TableRow => {
                    if let Some(Container::Table { current_row, .. }) = stack.last_mut() {
                        current_row.clear();
                    }
                }
                Tag::TableCell => {
                    if let Some(Container::Table { current_cell, .. }) = stack.last_mut() {
                        *current_cell = Some(Vec::new());
                    }
                }
                Tag::FootnoteDefinition(label) => {
                    stack.push(Container::Footnote {
                        label: label.to_string(),
                        blocks: Vec::new(),
                    });
                }
                Tag::Emphasis => style.italic += 1,
                Tag::Strong => style.bold += 1,
                Tag::Strikethrough => style.strike += 1,
                Tag::Link { dest_url, .. } => style.links.push(dest_url.to_string()),
                Tag::Image { dest_url, .. } => style.links.push(dest_url.to_string()),
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    if let Some(Container::Heading { level, inlines }) = stack.pop() {
                        push_block(&mut stack, Block::Heading { level, inlines });
                    }
                }
                TagEnd::Paragraph => {
                    if let Some(Container::Paragraph(inlines)) = stack.pop() {
                        if !inlines.is_empty() {
                            push_block(&mut stack, Block::Paragraph(inlines));
                        }
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some(Container::CodeBlock { lang, code }) = stack.pop() {
                        let block = match lang.as_deref().map(str::to_ascii_lowercase).as_deref() {
                            Some("mermaid") => Block::Mermaid(code),
                            Some("math") | Some("latex") | Some("tex") => Block::Math(code),
                            _ => Block::CodeBlock { lang, code },
                        };
                        push_block(&mut stack, block);
                    }
                }
                TagEnd::BlockQuote(_) => {
                    if let Some(Container::BlockQuote(blocks)) = stack.pop() {
                        push_block(&mut stack, Block::BlockQuote(blocks));
                    }
                }
                TagEnd::List(_) => {
                    if let Some(Container::List {
                        ordered,
                        start,
                        items,
                    }) = stack.pop()
                    {
                        push_block(
                            &mut stack,
                            Block::List {
                                ordered,
                                start,
                                items,
                            },
                        );
                    }
                }
                TagEnd::Item => {
                    if let Some(Container::ListItem {
                        checked,
                        mut blocks,
                        pending_inlines,
                    }) = stack.pop()
                    {
                        if !pending_inlines.is_empty() {
                            blocks.push(Block::Paragraph(pending_inlines));
                        }
                        if let Some(Container::List { items, .. }) = stack.last_mut() {
                            items.push(ListItem { checked, blocks });
                        }
                    }
                }
                TagEnd::TableCell => {
                    if let Some(Container::Table {
                        current_row,
                        current_cell,
                        ..
                    }) = stack.last_mut()
                    {
                        if let Some(cell) = current_cell.take() {
                            current_row.push(cell);
                        }
                    }
                }
                TagEnd::TableHead => {
                    if let Some(Container::Table {
                        headers,
                        in_head,
                        current_row,
                        ..
                    }) = stack.last_mut()
                    {
                        *headers = std::mem::take(current_row);
                        *in_head = false;
                    }
                }
                TagEnd::TableRow => {
                    if let Some(Container::Table {
                        rows,
                        in_head,
                        current_row,
                        ..
                    }) = stack.last_mut()
                    {
                        if !*in_head {
                            rows.push(std::mem::take(current_row));
                        }
                    }
                }
                TagEnd::Table => {
                    if let Some(Container::Table {
                        alignments,
                        headers,
                        rows,
                        ..
                    }) = stack.pop()
                    {
                        push_block(
                            &mut stack,
                            Block::Table {
                                alignments,
                                headers,
                                rows,
                            },
                        );
                    }
                }
                TagEnd::FootnoteDefinition => {
                    if let Some(Container::Footnote { label, blocks }) = stack.pop() {
                        push_block(&mut stack, Block::FootnoteDefinition { label, blocks });
                    }
                }
                TagEnd::Emphasis => style.italic = style.italic.saturating_sub(1),
                TagEnd::Strong => style.bold = style.bold.saturating_sub(1),
                TagEnd::Strikethrough => style.strike = style.strike.saturating_sub(1),
                TagEnd::Link | TagEnd::Image => {
                    style.links.pop();
                }
                _ => {}
            },
            Event::Text(text) => {
                if let Some(Container::CodeBlock { code, .. }) = stack.last_mut() {
                    code.push_str(&text);
                } else {
                    push_inline(&mut stack, style.make(text.to_string(), false, false));
                }
            }
            Event::Code(code) => {
                push_inline(&mut stack, style.make(code.to_string(), true, false));
            }
            Event::InlineMath(tex) => {
                push_inline(&mut stack, style.make(tex.to_string(), false, true));
            }
            Event::DisplayMath(tex) => {
                // If inside a paragraph that only holds display math, flush it as a Block::Math
                // after closing or directly into the parent container.
                if matches!(stack.last(), Some(Container::Paragraph(_))) {
                    if let Some(Container::Paragraph(inlines)) = stack.pop() {
                        if !inlines.is_empty() {
                            push_block(&mut stack, Block::Paragraph(inlines));
                        }
                        push_block(&mut stack, Block::Math(tex.to_string()));
                        stack.push(Container::Paragraph(Vec::new()));
                    }
                } else {
                    push_block(&mut stack, Block::Math(tex.to_string()));
                }
            }
            Event::SoftBreak => {
                push_inline(&mut stack, style.make(" ".to_string(), false, false));
            }
            Event::HardBreak => {
                push_inline(&mut stack, style.make("\n".to_string(), false, false));
            }
            Event::Rule => {
                push_block(&mut stack, Block::Rule);
            }
            Event::TaskListMarker(checked) => {
                if let Some(Container::ListItem { checked: slot, .. }) = stack.last_mut() {
                    *slot = Some(checked);
                }
            }
            Event::FootnoteReference(label) => {
                push_inline(
                    &mut stack,
                    style.make(format!("[^{label}]"), false, false),
                );
            }
            Event::Html(_) | Event::InlineHtml(_) => {}
        }
    }

    match stack.into_iter().next() {
        Some(Container::Root(blocks)) => blocks,
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gfm_headings_lists_tables_math_and_mermaid() {
        let md = r#"# Title

Hello **bold** and *italic* and ~~strike~~ and `code` and $x^2$ and [link](https://example.com).

- [x] Done
  - Nested item
- [ ] Todo

| Left | Center | Right |
| :--- | :---: | ---: |
| a | b | c |

$$
\sum_{i=1}^{n} i
$$

```mermaid
graph TD
    A --> B
```

```math
E = mc^2
```

---
"#;

        let blocks = parse(md);
        assert!(matches!(&blocks[0], Block::Heading { level: 1, .. }));
        assert!(matches!(&blocks[1], Block::Paragraph(inlines) if inlines.iter().any(|i| i.bold) && inlines.iter().any(|i| i.math && i.text == "x^2")));

        // Nested task list
        let list = blocks.iter().find(|b| matches!(b, Block::List { .. })).unwrap();
        if let Block::List { items, .. } = list {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].checked, Some(true));
            assert!(items[0].blocks.iter().any(|b| matches!(b, Block::List { .. })));
            assert_eq!(items[1].checked, Some(false));
        }

        // Table
        let table = blocks.iter().find(|b| matches!(b, Block::Table { .. })).unwrap();
        if let Block::Table { alignments, headers, rows } = table {
            assert_eq!(alignments, &vec![Alignment::Left, Alignment::Center, Alignment::Right]);
            assert_eq!(headers.len(), 3);
            assert_eq!(rows.len(), 1);
        }

        // Math & Mermaid blocks
        assert!(blocks.iter().any(|b| matches!(b, Block::Mermaid(s) if s.contains("A --> B"))));
        let math_count = blocks.iter().filter(|b| matches!(b, Block::Math(_))).count();
        assert_eq!(math_count, 2);
        assert!(blocks.iter().any(|b| matches!(b, Block::Rule)));
    }
}
