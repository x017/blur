//! Mermaid flowchart parser, deterministic layered layout, and sequence diagram parser.

use std::collections::{HashMap, HashSet};

use anyhow::bail;

/// Flowchart layout direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Td,
    Tb,
    Bt,
    Lr,
    Rl,
}

/// Visual shape of a flowchart node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeShape {
    /// `[label]`
    Rect,
    /// `(label)`
    Round,
    /// `{label}`
    Diamond,
    /// `((label))`
    Circle,
    /// `>label]`
    Asymmetric,
    /// `([label])`
    Stadium,
}

/// Visual style of a flowchart edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeStyle {
    /// `-->`
    Solid,
    /// `---`
    Open,
    /// `-.->`
    Dotted,
    /// `==>`
    Thick,
}

/// A flowchart node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: NodeShape,
}

/// A directed/undirected edge between two flowchart nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub style: EdgeStyle,
}

/// Parsed Mermaid flowchart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flowchart {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Deterministic 2D layout for a [`Flowchart`].
#[derive(Debug, Clone, PartialEq)]
pub struct FlowLayout {
    /// Positioned nodes as `(id, x, y, width, height)`.
    pub nodes: Vec<(String, f32, f32, f32, f32)>,
    /// Edge polylines, one per edge in `Flowchart::edges`, as lists of `(x, y)` points.
    pub edges: Vec<Vec<(f32, f32)>>,
    pub width: f32,
    pub height: f32,
}

/// Parse a Mermaid `graph` or `flowchart` definition.
pub fn parse_flowchart(src: &str) -> anyhow::Result<Flowchart> {
    let mut direction = None;
    let mut node_indices: HashMap<String, usize> = HashMap::new();
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();

    for raw_line in src.lines() {
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        if direction.is_none() {
            let mut parts = line.split_whitespace();
            let kw = parts.next().unwrap_or("");
            if kw.eq_ignore_ascii_case("graph") || kw.eq_ignore_ascii_case("flowchart") {
                let dir_str = parts.next().unwrap_or("TD").to_ascii_uppercase();
                direction = Some(match dir_str.as_str() {
                    "TD" => Direction::Td,
                    "TB" => Direction::Tb,
                    "BT" => Direction::Bt,
                    "LR" => Direction::Lr,
                    "RL" => Direction::Rl,
                    other => bail!("unsupported flowchart direction: '{other}'"),
                });
                continue;
            } else {
                bail!("expected 'graph' or 'flowchart' header, found '{line}'");
            }
        }

        // Ignore subgraph / styling directives without error.
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("subgraph ")
            || lower == "subgraph"
            || lower == "end"
            || lower.starts_with("classdef ")
            || lower.starts_with("class ")
            || lower.starts_with("style ")
            || lower.starts_with("linkstyle ")
            || lower.starts_with("direction ")
        {
            continue;
        }

        for stmt in line.split(';') {
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }
            parse_statement(stmt, &mut node_indices, &mut nodes, &mut edges)?;
        }
    }

    let Some(direction) = direction else {
        bail!("empty flowchart: missing 'graph' or 'flowchart' header");
    };

    Ok(Flowchart {
        direction,
        nodes,
        edges,
    })
}

fn strip_comment(line: &str) -> &str {
    match line.find("%%") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

fn upsert_node(
    node_indices: &mut HashMap<String, usize>,
    nodes: &mut Vec<Node>,
    parsed: Node,
    has_explicit_shape: bool,
) -> String {
    if let Some(&idx) = node_indices.get(&parsed.id) {
        if has_explicit_shape {
            nodes[idx].label = parsed.label;
            nodes[idx].shape = parsed.shape;
        }
        return nodes[idx].id.clone();
    }
    let id = parsed.id.clone();
    let idx = nodes.len();
    nodes.push(parsed);
    node_indices.insert(id.clone(), idx);
    id
}

fn parse_statement(
    stmt: &str,
    node_indices: &mut HashMap<String, usize>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) -> anyhow::Result<()> {
    let mut rest = stmt.trim();
    let (first_node, first_explicit, after_first) = parse_node_token(rest)?;
    let mut prev_id = upsert_node(node_indices, nodes, first_node, first_explicit);
    rest = after_first.trim_start();

    while !rest.is_empty() {
        let Some((style, label, after_edge)) = parse_edge_token(rest) else {
            break;
        };
        let after_edge = after_edge.trim_start();
        if after_edge.is_empty() {
            bail!("expected target node after edge in '{stmt}'");
        }
        let (next_node, next_explicit, after_next) = parse_node_token(after_edge)?;
        let next_id = upsert_node(node_indices, nodes, next_node, next_explicit);
        edges.push(Edge {
            from: prev_id,
            to: next_id.clone(),
            label,
            style,
        });
        prev_id = next_id;
        rest = after_next.trim_start();
    }

    Ok(())
}

fn parse_node_token(s: &str) -> anyhow::Result<(Node, bool, &str)> {
    let s = s.trim_start();
    let id_end = s
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(s.len());
    // Stop before `--` edge operators if id ends with `-`.
    let id_raw = &s[..id_end];
    let id_clean = if let Some(pos) = id_raw.find("--") {
        &id_raw[..pos]
    } else if let Some(pos) = id_raw.find("-.") {
        &id_raw[..pos]
    } else {
        id_raw
    };
    if id_clean.is_empty() {
        bail!("expected node id near '{s}'");
    }
    let rest = &s[id_clean.len()..];

    // Check shape delimiters
    let shapes: &[(&str, &str, NodeShape)] = &[
        ("((", "))", NodeShape::Circle),
        ("([", "])", NodeShape::Stadium),
        ("[", "]", NodeShape::Rect),
        ("(", ")", NodeShape::Round),
        ("{", "}", NodeShape::Diamond),
        (">", "]", NodeShape::Asymmetric),
    ];

    for &(open, close, shape) in shapes {
        if let Some(after_open) = rest.strip_prefix(open) {
            if let Some(close_idx) = after_open.find(close) {
                let raw_label = after_open[..close_idx].trim();
                let label = raw_label
                    .strip_prefix('"')
                    .and_then(|t| t.strip_suffix('"'))
                    .unwrap_or(raw_label)
                    .to_string();
                let remaining = &after_open[close_idx + close.len()..];
                return Ok((
                    Node {
                        id: id_clean.to_string(),
                        label,
                        shape,
                    },
                    true,
                    remaining,
                ));
            }
        }
    }

    Ok((
        Node {
            id: id_clean.to_string(),
            label: id_clean.to_string(),
            shape: NodeShape::Rect,
        },
        false,
        rest,
    ))
}

fn parse_edge_token(s: &str) -> Option<(EdgeStyle, Option<String>, &str)> {
    let s = s.trim_start();

    // Form 1: `-- text -->`, `-- text ---`, `-. text .->`, `== text ==>`
    if let Some(after) = s.strip_prefix("-- ") {
        if let Some(end) = after.find("-->") {
            let label = after[..end].trim();
            let label = if label.is_empty() {
                None
            } else {
                Some(label.to_string())
            };
            return Some((EdgeStyle::Solid, label, &after[end + 3..]));
        }
        if let Some(end) = after.find("---") {
            let label = after[..end].trim();
            let label = if label.is_empty() {
                None
            } else {
                Some(label.to_string())
            };
            return Some((EdgeStyle::Open, label, &after[end + 3..]));
        }
    }
    if let Some(after) = s.strip_prefix("-. ") {
        if let Some(end) = after.find(".->") {
            let label = after[..end].trim();
            let label = if label.is_empty() {
                None
            } else {
                Some(label.to_string())
            };
            return Some((EdgeStyle::Dotted, label, &after[end + 3..]));
        }
    }
    if let Some(after) = s.strip_prefix("== ") {
        if let Some(end) = after.find("==>") {
            let label = after[..end].trim();
            let label = if label.is_empty() {
                None
            } else {
                Some(label.to_string())
            };
            return Some((EdgeStyle::Thick, label, &after[end + 3..]));
        }
    }

    // Form 2: operator followed by optional `|label|`
    let ops: &[(&str, EdgeStyle)] = &[
        ("-.->", EdgeStyle::Dotted),
        ("==>", EdgeStyle::Thick),
        ("-->", EdgeStyle::Solid),
        ("---", EdgeStyle::Open),
    ];

    for &(op, style) in ops {
        if let Some(after_op) = s.strip_prefix(op) {
            let after_op_trim = after_op.trim_start();
            if let Some(after_pipe) = after_op_trim.strip_prefix('|') {
                if let Some(close_pipe) = after_pipe.find('|') {
                    let label = after_pipe[..close_pipe].trim().to_string();
                    return Some((style, Some(label), &after_pipe[close_pipe + 1..]));
                }
            }
            return Some((style, None, after_op));
        }
    }

    None
}

/// Compute a deterministic Sugiyama-lite layered layout for a [`Flowchart`].
pub fn layout(chart: &Flowchart) -> FlowLayout {
    if chart.nodes.is_empty() {
        return FlowLayout {
            nodes: Vec::new(),
            edges: Vec::new(),
            width: 0.0,
            height: 0.0,
        };
    }

    let id_to_idx: HashMap<&str, usize> = chart
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();

    let n = chart.nodes.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_deg: Vec<usize> = vec![0; n];

    for edge in &chart.edges {
        if let (Some(&u), Some(&v)) = (
            id_to_idx.get(edge.from.as_str()),
            id_to_idx.get(edge.to.as_str()),
        ) {
            if u != v {
                adj[u].push(v);
                in_deg[v] += 1;
            }
        }
    }

    // Longest-path layering with cycle safety.
    let mut layer: Vec<usize> = vec![0; n];
    let mut visited: Vec<bool> = vec![false; n];
    let mut in_stack: Vec<bool> = vec![false; n];

    fn dfs_layer(
        u: usize,
        adj: &[Vec<usize>],
        layer: &mut [usize],
        visited: &mut [bool],
        in_stack: &mut [bool],
    ) {
        visited[u] = true;
        in_stack[u] = true;
        for &v in &adj[u] {
            if !in_stack[v] {
                if layer[v] < layer[u] + 1 {
                    layer[v] = layer[u] + 1;
                }
                if !visited[v] {
                    dfs_layer(v, adj, layer, visited, in_stack);
                } else {
                    // Propagate increased layer downstream safely
                    propagate(v, adj, layer, &mut HashSet::new());
                }
            }
        }
        in_stack[u] = false;
    }

    fn propagate(u: usize, adj: &[Vec<usize>], layer: &mut [usize], seen: &mut HashSet<usize>) {
        if !seen.insert(u) {
            return;
        }
        for &v in &adj[u] {
            if layer[v] < layer[u] + 1 {
                layer[v] = layer[u] + 1;
                propagate(v, adj, layer, seen);
            }
        }
    }

    // Start from roots first for stable layering
    for i in 0..n {
        if in_deg[i] == 0 && !visited[i] {
            dfs_layer(i, &adj, &mut layer, &mut visited, &mut in_stack);
        }
    }
    for i in 0..n {
        if !visited[i] {
            dfs_layer(i, &adj, &mut layer, &mut visited, &mut in_stack);
        }
    }

    let max_layer = layer.iter().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_layer + 1];
    for (idx, &l) in layer.iter().enumerate() {
        layers[l].push(idx);
    }

    // One deterministic barycenter ordering pass per layer
    let mut pos_in_layer: Vec<usize> = vec![0; n];
    for l in 0..=max_layer {
        if l > 0 {
            let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
            for (u, neighbors) in adj.iter().enumerate() {
                for &v in neighbors {
                    preds[v].push(u);
                }
            }
            layers[l].sort_by(|&a, &b| {
                let score = |node: usize| -> f32 {
                    let ps = &preds[node];
                    if ps.is_empty() {
                        node as f32
                    } else {
                        let sum: usize = ps.iter().map(|&p| pos_in_layer[p]).sum();
                        sum as f32 / ps.len() as f32
                    }
                };
                score(a)
                    .partial_cmp(&score(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.cmp(&b))
            });
        }
        for (p, &node_idx) in layers[l].iter().enumerate() {
            pos_in_layer[node_idx] = p;
        }
    }

    // Compute node sizes
    let sizes: Vec<(f32, f32)> = chart
        .nodes
        .iter()
        .map(|node| {
            let w = (node.label.chars().count() as f32 * 8.0 + 28.0).max(84.0);
            let h = match node.shape {
                NodeShape::Diamond => 52.0,
                NodeShape::Circle => w.max(52.0),
                _ => 40.0,
            };
            (w, h)
        })
        .collect();

    let pad = 24.0_f32;
    let gap_major = 72.0_f32;
    let gap_minor = 36.0_f32;

    let is_horizontal = matches!(chart.direction, Direction::Lr | Direction::Rl);
    let is_reversed = matches!(chart.direction, Direction::Bt | Direction::Rl);

    let layer_order: Vec<usize> = if is_reversed {
        (0..=max_layer).rev().collect()
    } else {
        (0..=max_layer).collect()
    };

    let mut coords: Vec<(f32, f32, f32, f32)> = vec![(0.0, 0.0, 0.0, 0.0); n];
    let mut cursor_major = pad;
    let mut max_minor_extent = 0.0_f32;

    for &l in &layer_order {
        let mut cursor_minor = pad;
        let mut layer_thickness = 0.0_f32;
        for &idx in &layers[l] {
            let (w, h) = sizes[idx];
            if is_horizontal {
                coords[idx] = (cursor_major, cursor_minor, w, h);
                cursor_minor += h + gap_minor;
                layer_thickness = layer_thickness.max(w);
            } else {
                coords[idx] = (cursor_minor, cursor_major, w, h);
                cursor_minor += w + gap_minor;
                layer_thickness = layer_thickness.max(h);
            }
        }
        let minor_extent = (cursor_minor - gap_minor).max(pad);
        max_minor_extent = max_minor_extent.max(minor_extent);
        cursor_major += layer_thickness + gap_major;
    }

    let major_extent = (cursor_major - gap_major + pad).max(pad * 2.0);
    let minor_extent = max_minor_extent + pad;
    let (width, height) = if is_horizontal {
        (major_extent, minor_extent)
    } else {
        (minor_extent, major_extent)
    };

    let positioned_nodes: Vec<(String, f32, f32, f32, f32)> = chart
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let (x, y, w, h) = coords[i];
            (node.id.clone(), x, y, w, h)
        })
        .collect();

    let positioned_edges: Vec<Vec<(f32, f32)>> = chart
        .edges
        .iter()
        .map(|edge| {
            let (Some(&u), Some(&v)) = (
                id_to_idx.get(edge.from.as_str()),
                id_to_idx.get(edge.to.as_str()),
            ) else {
                return Vec::new();
            };
            let (x1, y1, w1, h1) = coords[u];
            let (x2, y2, w2, h2) = coords[v];
            let (sx, sy, tx, ty) = match chart.direction {
                Direction::Td | Direction::Tb => {
                    (x1 + w1 / 2.0, y1 + h1, x2 + w2 / 2.0, y2)
                }
                Direction::Bt => (x1 + w1 / 2.0, y1, x2 + w2 / 2.0, y2 + h2),
                Direction::Lr => (x1 + w1, y1 + h1 / 2.0, x2, y2 + h2 / 2.0),
                Direction::Rl => (x1, y1 + h1 / 2.0, x2 + w2, y2 + h2 / 2.0),
            };
            if is_horizontal {
                let mx = (sx + tx) / 2.0;
                vec![(sx, sy), (mx, sy), (mx, ty), (tx, ty)]
            } else {
                let my = (sy + ty) / 2.0;
                vec![(sx, sy), (sx, my), (tx, my), (tx, ty)]
            }
        })
        .collect();

    FlowLayout {
        nodes: positioned_nodes,
        edges: positioned_edges,
        width,
        height,
    }
}

/// Arrow style in a Mermaid sequence diagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageArrow {
    /// `->>`
    Solid,
    /// `-->>`
    Dotted,
    /// `->`
    SolidOpen,
    /// `-->`
    DottedOpen,
}

/// A message exchanged between two participants in a sequence diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceMessage {
    pub from: String,
    pub to: String,
    pub text: String,
    pub arrow: MessageArrow,
}

/// Parsed Mermaid `sequenceDiagram`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceDiagram {
    pub participants: Vec<String>,
    pub messages: Vec<SequenceMessage>,
}

/// Parse a Mermaid `sequenceDiagram` definition.
pub fn parse_sequence(src: &str) -> anyhow::Result<SequenceDiagram> {
    let mut saw_header = false;
    let mut participants: Vec<String> = Vec::new();
    let mut aliases: HashMap<String, String> = HashMap::new();
    let mut messages: Vec<SequenceMessage> = Vec::new();

    let add_participant = |name: &str, participants: &mut Vec<String>| {
        if !participants.iter().any(|p| p == name) {
            participants.push(name.to_string());
        }
    };

    for raw_line in src.lines() {
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if !saw_header {
            if line.eq_ignore_ascii_case("sequencediagram") {
                saw_header = true;
                continue;
            } else {
                bail!("expected 'sequenceDiagram' header, found '{line}'");
            }
        }

        let lower = line.to_ascii_lowercase();
        if lower.starts_with("participant ") || lower.starts_with("actor ") {
            let rest = line.split_once(' ').map(|(_, r)| r.trim()).unwrap_or("");
            if let Some((id, alias)) = rest.split_once(" as ") {
                let id = id.trim();
                let alias = alias.trim();
                aliases.insert(id.to_string(), alias.to_string());
                add_participant(alias, &mut participants);
            } else if !rest.is_empty() {
                add_participant(rest, &mut participants);
            }
            continue;
        }

        if lower.starts_with("note ")
            || lower.starts_with("loop ")
            || lower.starts_with("alt ")
            || lower.starts_with("else")
            || lower.starts_with("opt ")
            || lower.starts_with("par ")
            || lower.starts_with("and ")
            || lower.starts_with("activate ")
            || lower.starts_with("deactivate ")
            || lower == "end"
        {
            continue;
        }

        // Parse `From ->> To: message`
        let arrows: &[(&str, MessageArrow)] = &[
            ("-->>", MessageArrow::Dotted),
            ("->>", MessageArrow::Solid),
            ("-->", MessageArrow::DottedOpen),
            ("->", MessageArrow::SolidOpen),
        ];

        for &(op, arrow) in arrows {
            if let Some(pos) = line.find(op) {
                let from_raw = line[..pos].trim();
                let after = &line[pos + op.len()..];
                let (to_raw, text) = match after.split_once(':') {
                    Some((t, msg)) => (t.trim(), msg.trim().to_string()),
                    None => (after.trim(), String::new()),
                };
                // Strip optional +/- activation markers on target
                let to_clean = to_raw.trim_start_matches(['+', '-']).trim();
                let from = aliases
                    .get(from_raw)
                    .cloned()
                    .unwrap_or_else(|| from_raw.to_string());
                let to = aliases
                    .get(to_clean)
                    .cloned()
                    .unwrap_or_else(|| to_clean.to_string());
                add_participant(&from, &mut participants);
                add_participant(&to, &mut participants);
                messages.push(SequenceMessage {
                    from,
                    to,
                    text,
                    arrow,
                });
                break;
            }
        }
    }

    if !saw_header {
        bail!("empty sequence diagram: missing 'sequenceDiagram' header");
    }

    Ok(SequenceDiagram {
        participants,
        messages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flowchart_shapes_edges_chains_and_subgraphs() {
        let src = r#"
flowchart LR
    subgraph Main
        A[Start] --> B{Decision} -->|Yes| C((Done))
        B -- No --> D(Retry) -.-> A
        C ==> E>Flag]
    end
"#;
        let chart = parse_flowchart(src).unwrap();
        assert_eq!(chart.direction, Direction::Lr);
        assert_eq!(chart.nodes.len(), 5);
        assert_eq!(chart.edges.len(), 5);
        assert_eq!(chart.nodes[0].shape, NodeShape::Rect);
        assert_eq!(chart.nodes[1].shape, NodeShape::Diamond);
        assert_eq!(chart.nodes[2].shape, NodeShape::Circle);
        assert_eq!(chart.nodes[3].shape, NodeShape::Round);
        assert_eq!(chart.nodes[4].shape, NodeShape::Asymmetric);
        assert_eq!(chart.edges[1].label.as_deref(), Some("Yes"));
        assert_eq!(chart.edges[2].label.as_deref(), Some("No"));
        assert_eq!(chart.edges[3].style, EdgeStyle::Dotted);
        assert_eq!(chart.edges[4].style, EdgeStyle::Thick);

        let lay = layout(&chart);
        assert_eq!(lay.nodes.len(), 5);
        assert_eq!(lay.edges.len(), 5);
        assert!(lay.width > 0.0 && lay.height > 0.0);
        // Deterministic layout check
        assert_eq!(lay, layout(&chart));
    }

    #[test]
    fn parses_sequence_diagram() {
        let src = r#"
sequenceDiagram
    participant U as User
    participant S as Server
    U->>S: Request
    S-->>U: Response
"#;
        let seq = parse_sequence(src).unwrap();
        assert_eq!(seq.participants, vec!["User", "Server"]);
        assert_eq!(seq.messages.len(), 2);
        assert_eq!(seq.messages[0].from, "User");
        assert_eq!(seq.messages[0].to, "Server");
        assert_eq!(seq.messages[0].text, "Request");
        assert_eq!(seq.messages[0].arrow, MessageArrow::Solid);
        assert_eq!(seq.messages[1].arrow, MessageArrow::Dotted);
    }
}
