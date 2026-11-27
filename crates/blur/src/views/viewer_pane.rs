//! Native GPUI Markdown, Mermaid, Math, JSON/YAML/TOML, and CSV preview pane
//! backed by `blur_viewers`.

use blur_theme::Theme;
use blur_ui::{badge, BadgeTone};
use blur_viewers::{csv, data, markdown, math, mermaid};
use gpui::{div, prelude::*, px, AnyElement, FontWeight};

/// Render a native preview for the given file extension and content.
pub fn render_preview(ext: &str, content: &str, theme: &Theme) -> AnyElement {
    match ext.to_ascii_lowercase().as_str() {
        "md" | "markdown" => render_markdown(content, theme),
        "json" => match data::parse_json(content) {
            Ok(node) => render_data_tree("JSON Inspector", &node, theme),
            Err(err) => render_error(&format!("JSON parse error: {err}"), theme),
        },
        "yaml" | "yml" => match data::parse_yaml(content) {
            Ok(node) => render_data_tree("YAML Inspector", &node, theme),
            Err(err) => render_error(&format!("YAML parse error: {err}"), theme),
        },
        "toml" => match data::parse_toml(content) {
            Ok(node) => render_data_tree("TOML Inspector", &node, theme),
            Err(err) => render_error(&format!("TOML parse error: {err}"), theme),
        },
        "csv" | "tsv" => match csv::parse(content, None) {
            Ok(table) => render_csv(&table, theme),
            Err(err) => render_error(&format!("CSV parse error: {err}"), theme),
        },
        _ => render_markdown(content, theme),
    }
}

fn render_error(msg: &str, theme: &Theme) -> AnyElement {
    div()
        .p(px(16.0))
        .text_size(px(12.5))
        .text_color(theme.error)
        .child(msg.to_string())
        .into_any_element()
}

fn render_markdown(src: &str, theme: &Theme) -> AnyElement {
    let blocks = markdown::parse(src);
    let children: Vec<AnyElement> = blocks
        .iter()
        .map(|b| render_block(b, theme))
        .collect();

    div()
        .size_full()
        .p(px(20.0))
        .bg(theme.surface)
        .flex()
        .flex_col()
        .gap(px(10.0))
        .overflow_hidden()
        .children(children)
        .into_any_element()
}

fn render_block(block: &markdown::Block, theme: &Theme) -> AnyElement {
    match block {
        markdown::Block::Heading { level, inlines } => {
            let size = match level {
                1 => 22.0,
                2 => 18.0,
                3 => 15.0,
                _ => 13.5,
            };
            div()
                .text_size(px(size))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.syn_heading)
                .pb(px(4.0))
                .when(*level <= 2, |d| d.border_b_1().border_color(theme.border))
                .child(render_inlines(inlines, theme))
                .into_any_element()
        }
        markdown::Block::Paragraph(inlines) => div()
            .text_size(px(13.0))
            .text_color(theme.text)
            .child(render_inlines(inlines, theme))
            .into_any_element(),
        markdown::Block::CodeBlock { lang, code } => div()
            .p(px(10.0))
            .rounded(px(4.0))
            .bg(theme.canvas)
            .border_1()
            .border_color(theme.border)
            .flex()
            .flex_col()
            .gap(px(4.0))
            .when_some(lang.clone(), |d, lang| {
                d.child(
                    div()
                        .text_size(px(10.0))
                        .text_color(theme.text_muted)
                        .child(lang),
                )
            })
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_secondary)
                    .child(code.clone()),
            )
            .into_any_element(),
        markdown::Block::BlockQuote(inner) => div()
            .pl(px(12.0))
            .border_l_2()
            .border_color(theme.accent)
            .flex()
            .flex_col()
            .gap(px(6.0))
            .children(inner.iter().map(|b| render_block(b, theme)))
            .into_any_element(),
        markdown::Block::List { ordered, items, .. } => div()
            .pl(px(14.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children(items.iter().enumerate().map(|(i, item)| {
                let bullet = match item.checked {
                    Some(true) => "[x]".to_string(),
                    Some(false) => "[ ]".to_string(),
                    None if *ordered => format!("{}.", i + 1),
                    None => "•".to_string(),
                };
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.accent)
                            .child(bullet),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .children(item.blocks.iter().map(|b| render_block(b, theme))),
                    )
            }))
            .into_any_element(),
        markdown::Block::Table { headers, rows, .. } => div()
            .rounded(px(4.0))
            .border_1()
            .border_color(theme.border)
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .bg(theme.surface_panel)
                    .border_b_1()
                    .border_color(theme.border)
                    .children(headers.iter().map(|h| {
                        div()
                            .flex_1()
                            .px(px(8.0))
                            .py(px(5.0))
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(render_inlines(h, theme))
                    })),
            )
            .children(rows.iter().map(|r| {
                div()
                    .flex()
                    .border_b_1()
                    .border_color(theme.border)
                    .children(r.iter().map(|cell| {
                        div()
                            .flex_1()
                            .px(px(8.0))
                            .py(px(4.0))
                            .text_size(px(12.0))
                            .text_color(theme.text_secondary)
                            .child(render_inlines(cell, theme))
                    }))
            }))
            .into_any_element(),
        markdown::Block::Rule => div().w_full().h(px(1.0)).bg(theme.border_strong).into_any_element(),
        markdown::Block::Math(tex) => div()
            .p(px(10.0))
            .rounded(px(4.0))
            .bg(theme.surface_panel)
            .border_1()
            .border_color(theme.border)
            .text_size(px(14.0))
            .text_color(theme.accent_secondary)
            .child(math::to_unicode(tex))
            .into_any_element(),
        markdown::Block::Mermaid(src) => render_mermaid(src, theme),
        markdown::Block::FootnoteDefinition { label, blocks } => div()
            .text_size(px(11.5))
            .text_color(theme.text_muted)
            .flex()
            .gap(px(6.0))
            .child(format!("[{label}]"))
            .children(blocks.iter().map(|b| render_block(b, theme)))
            .into_any_element(),
    }
}

fn render_inlines(inlines: &[markdown::Inline], theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(2.0))
        .children(inlines.iter().map(|inl| {
            let text = if inl.math {
                math::to_unicode(&inl.text)
            } else {
                inl.text.clone()
            };
            let el = div().child(text);
            let el = if inl.bold {
                el.font_weight(FontWeight::BOLD)
            } else {
                el
            };
            let el = if inl.italic { el.italic() } else { el };
            let el = if inl.code {
                el.px(px(4.0))
                    .rounded(px(2.0))
                    .bg(theme.canvas)
                    .text_color(theme.accent_secondary)
            } else if inl.link.is_some() {
                el.text_color(theme.syn_link)
            } else if inl.math {
                el.text_color(theme.accent)
            } else {
                el
            };
            el
        }))
        .into_any_element()
}

fn render_mermaid(src: &str, theme: &Theme) -> AnyElement {
    if let Ok(flow) = mermaid::parse_flowchart(src) {
        let layout = mermaid::layout(&flow);
        let nodes = layout.nodes.iter().map(|(id, x, y, w, h)| {
            let label = flow
                .nodes
                .iter()
                .find(|n| n.id == *id)
                .map(|n| n.label.clone())
                .unwrap_or_else(|| id.clone());
            div()
                .absolute()
                .left(px(*x))
                .top(px(*y))
                .w(px(*w))
                .h(px(*h))
                .rounded(px(4.0))
                .bg(theme.surface_elevated)
                .border_1()
                .border_color(theme.accent)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.5))
                .text_color(theme.text)
                .child(label)
        });

        return div()
            .p(px(10.0))
            .rounded(px(4.0))
            .bg(theme.canvas)
            .border_1()
            .border_color(theme.border)
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(badge("MERMAID FLOWCHART", BadgeTone::Accent, theme))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child(format!("{} nodes · {} edges", flow.nodes.len(), flow.edges.len())),
                    ),
            )
            .child(
                div()
                    .relative()
                    .w(px(layout.width.max(240.0)))
                    .h(px(layout.height.max(80.0)))
                    .children(nodes),
            )
            .into_any_element();
    }

    if let Ok(seq) = mermaid::parse_sequence(src) {
        return div()
            .p(px(10.0))
            .rounded(px(4.0))
            .bg(theme.canvas)
            .border_1()
            .border_color(theme.border)
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(badge("MERMAID SEQUENCE", BadgeTone::Accent, theme))
            .children(seq.messages.iter().map(|m| {
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_secondary)
                    .child(format!("{} -> {}: {}", m.from, m.to, m.text))
            }))
            .into_any_element();
    }

    div()
        .p(px(10.0))
        .rounded(px(4.0))
        .bg(theme.canvas)
        .text_size(px(11.5))
        .text_color(theme.text_muted)
        .child(src.to_string())
        .into_any_element()
}

fn render_data_tree(title: &str, root: &data::DataNode, theme: &Theme) -> AnyElement {
    let mut rows = Vec::new();
    collect_data_rows(root, 0, &mut rows, theme);
    div()
        .size_full()
        .p(px(16.0))
        .bg(theme.surface)
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .pb(px(6.0))
                .border_b_1()
                .border_color(theme.border)
                .child(badge(title.to_string(), BadgeTone::Accent, theme)),
        )
        .children(rows.into_iter().take(120))
        .into_any_element()
}

fn collect_data_rows(
    node: &data::DataNode,
    depth: usize,
    out: &mut Vec<AnyElement>,
    theme: &Theme,
) {
    if out.len() >= 120 {
        return;
    }
    let val_color = match node.kind {
        data::DataKind::String => theme.syn_string,
        data::DataKind::Number => theme.syn_number,
        data::DataKind::Bool => theme.syn_keyword,
        data::DataKind::Null => theme.text_muted,
        data::DataKind::Object | data::DataKind::Array => theme.accent,
    };
    let val_text = node.value.clone().unwrap_or_else(|| match node.kind {
        data::DataKind::Object => format!("{{ {} keys }}", node.children.len()),
        data::DataKind::Array => format!("[ {} items ]", node.children.len()),
        _ => String::new(),
    });
    out.push(
        div()
            .pl(px(depth as f32 * 14.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .text_size(px(12.0))
            .when_some(node.key.clone(), |d, k| {
                d.child(div().text_color(theme.syn_property).child(format!("{k}:")))
            })
            .child(div().text_color(val_color).child(val_text))
            .into_any_element(),
    );
    for child in &node.children {
        collect_data_rows(child, depth + 1, out, theme);
    }
}

fn render_csv(table: &csv::Table, theme: &Theme) -> AnyElement {
    div()
        .size_full()
        .p(px(16.0))
        .bg(theme.surface)
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .flex()
                .bg(theme.surface_panel)
                .border_1()
                .border_color(theme.border)
                .children(table.headers.iter().map(|h| {
                    div()
                        .flex_1()
                        .px(px(8.0))
                        .py(px(5.0))
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child(h.clone())
                })),
        )
        .children(table.rows.iter().take(60).map(|row| {
            div()
                .flex()
                .border_b_1()
                .border_color(theme.border)
                .children(row.iter().map(|cell| {
                    div()
                        .flex_1()
                        .px(px(8.0))
                        .py(px(4.0))
                        .text_size(px(11.5))
                        .text_color(theme.text_secondary)
                        .child(cell.clone())
                }))
        }))
        .into_any_element()
}
