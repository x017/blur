//! Multi-cursor editing operations on `Buffer` and `Selections`.
//!
//! Every mutating operation records the pre-edit selections for undo, applies edits to the buffer,
//! updates `sels` to post-edit positions, records the post-edit selections for redo, and returns
//! the resulting `Vec<EditDelta>` in sequential application order.

use std::collections::BTreeSet;
use std::ops::Range;

use crate::{movement, Buffer, EditDelta, Point, Selection, Selections};

/// Helper: computes the inclusive line range `(start_line, end_line)` touched by `sel`.
fn selection_line_range(buf: &Buffer, sel: Selection) -> (usize, usize) {
    let start_pt = buf.char_to_point(sel.start());
    if sel.is_empty() {
        return (start_pt.line, start_pt.line);
    }
    let end_pt = buf.char_to_point(sel.end());
    let end_line = if end_pt.col == 0 && end_pt.line > start_pt.line {
        end_pt.line - 1
    } else {
        end_pt.line
    };
    (start_pt.line, end_line)
}

/// Collects all unique sorted line indices touched by any selection.
fn touched_lines(buf: &Buffer, sels: &Selections) -> Vec<usize> {
    let mut set = BTreeSet::new();
    for &sel in sels.all() {
        let (a, b) = selection_line_range(buf, sel);
        for l in a..=b {
            set.insert(l);
        }
    }
    set.into_iter().collect()
}

/// Applies per-selection replacements `(range, replacement_text, cursor_offset_within_replacement)`
/// and updates `sels` + undo history.
fn apply_per_selection(
    buf: &mut Buffer,
    sels: &mut Selections,
    specs: Vec<(Range<usize>, String, usize)>,
) -> Vec<EditDelta> {
    buf.set_selections_for_undo(sels);
    let primary_idx = sels.primary_index();

    let mut edits = Vec::with_capacity(specs.len());
    let mut new_sels = Vec::with_capacity(specs.len());
    let mut delta: isize = 0;

    for (range, text, cursor_offset) in specs {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let old_len = range.end - range.start;
        let new_len = normalized.chars().count();
        let new_start = (range.start as isize + delta) as usize;
        let new_cursor = new_start + cursor_offset.min(new_len);

        if range.start != range.end || !normalized.is_empty() {
            edits.push((range, normalized));
        }
        new_sels.push(Selection::cursor(new_cursor));
        delta += new_len as isize - old_len as isize;
    }

    let deltas = buf.edit(edits);
    sels.set(new_sels, primary_idx);
    buf.set_selections_after_edit(sels);
    deltas
}

/// Inserts `text` at every selection (replacing any selected text).
pub fn insert(buf: &mut Buffer, sels: &mut Selections, text: &str) -> Vec<EditDelta> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let ins_len = normalized.chars().count();
    let specs = sels
        .all()
        .iter()
        .map(|s| (s.range(), normalized.clone(), ins_len))
        .collect();
    apply_per_selection(buf, sels, specs)
}

fn closing_pair(ch: char) -> Option<char> {
    match ch {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        _ => None,
    }
}

fn is_closing_bracket(ch: char) -> bool {
    matches!(ch, ')' | ']' | '}')
}

fn is_quote(ch: char) -> bool {
    matches!(ch, '"' | '\'' | '`')
}

/// Inserts `text` with bracket/quote auto-pairing:
/// - Opening brackets `( [ {` and quotes `" ' \`` wrap non-empty selections or insert a matching pair.
/// - Typing a closing bracket or matching quote when the cursor is immediately before it skips over it.
pub fn insert_with_autopair(
    buf: &mut Buffer,
    sels: &mut Selections,
    text: &str,
) -> Vec<EditDelta> {
    let mut chars = text.chars();
    let Some(ch) = chars.next() else {
        return Vec::new();
    };
    if chars.next().is_some() {
        return insert(buf, sels, text);
    }

    let Some(close) = closing_pair(ch) else {
        if is_closing_bracket(ch) {
            // Skip over closing bracket if all selections are empty and immediately before `ch`.
            let all_skip = sels.all().iter().all(|s| {
                s.is_empty() && s.head < buf.len_chars() && buf.rope().char(s.head) == ch
            });
            if all_skip {
                let primary_idx = sels.primary_index();
                let moved = sels
                    .all()
                    .iter()
                    .map(|s| Selection::cursor(s.head + 1))
                    .collect();
                sels.set(moved, primary_idx);
                return Vec::new();
            }
        }
        return insert(buf, sels, text);
    };

    // Check if we should skip over an existing closing quote.
    if is_quote(ch) {
        let all_skip = sels.all().iter().all(|s| {
            s.is_empty() && s.head < buf.len_chars() && buf.rope().char(s.head) == ch
        });
        if all_skip {
            let primary_idx = sels.primary_index();
            let moved = sels
                .all()
                .iter()
                .map(|s| Selection::cursor(s.head + 1))
                .collect();
            sels.set(moved, primary_idx);
            return Vec::new();
        }
    }

    buf.set_selections_for_undo(sels);
    let primary_idx = sels.primary_index();

    let mut edits = Vec::new();
    let mut new_sels = Vec::with_capacity(sels.len());
    let mut delta: isize = 0;

    for &sel in sels.all() {
        let range = sel.range();
        let old_len = range.end - range.start;
        if !sel.is_empty() {
            // Wrap selection with open..close and keep inner text selected.
            let selected: String = buf.rope().slice(range.clone()).to_string();
            let mut wrapped = String::with_capacity(selected.len() + 2);
            wrapped.push(ch);
            wrapped.push_str(&selected);
            wrapped.push(close);
            let new_len = wrapped.chars().count();
            let new_start = (range.start as isize + delta) as usize;
            edits.push((range, wrapped));
            new_sels.push(Selection::new(new_start + 1, new_start + 1 + old_len));
            delta += new_len as isize - old_len as isize;
        } else {
            // Avoid auto-pairing single/double quotes immediately after a word char (e.g. `don't`).
            let prev_is_word = is_quote(ch)
                && sel.head > 0
                && {
                    let p = buf.rope().char(sel.head - 1);
                    p.is_alphanumeric() || p == '_'
                };
            if prev_is_word {
                let s = ch.to_string();
                let new_start = (range.start as isize + delta) as usize;
                edits.push((range, s));
                new_sels.push(Selection::cursor(new_start + 1));
                delta += 1;
            } else {
                let mut pair = String::with_capacity(2);
                pair.push(ch);
                pair.push(close);
                let new_start = (range.start as isize + delta) as usize;
                edits.push((range, pair));
                new_sels.push(Selection::cursor(new_start + 1));
                delta += 2;
            }
        }
    }

    let deltas = buf.edit(edits);
    sels.set(new_sels, primary_idx);
    buf.set_selections_after_edit(sels);
    deltas
}

/// Deletes the selection or the grapheme immediately to the left of each cursor.
pub fn backspace(buf: &mut Buffer, sels: &mut Selections) -> Vec<EditDelta> {
    let specs = sels
        .all()
        .iter()
        .map(|&sel| {
            let range = if !sel.is_empty() {
                sel.range()
            } else if sel.head > 0 {
                let prev = movement::left(buf, sel, false);
                prev.head..sel.head
            } else {
                0..0
            };
            (range, String::new(), 0)
        })
        .collect();
    apply_per_selection(buf, sels, specs)
}

/// Deletes the selection or the grapheme immediately to the right of each cursor.
pub fn delete_forward(buf: &mut Buffer, sels: &mut Selections) -> Vec<EditDelta> {
    let specs = sels
        .all()
        .iter()
        .map(|&sel| {
            let range = if !sel.is_empty() {
                sel.range()
            } else if sel.head < buf.len_chars() {
                let next = movement::right(buf, sel, false);
                sel.head..next.head
            } else {
                sel.head..sel.head
            };
            (range, String::new(), 0)
        })
        .collect();
    apply_per_selection(buf, sels, specs)
}

/// Deletes the selection or the word to the left of each cursor.
pub fn delete_word_left(buf: &mut Buffer, sels: &mut Selections) -> Vec<EditDelta> {
    let specs = sels
        .all()
        .iter()
        .map(|&sel| {
            let range = if !sel.is_empty() {
                sel.range()
            } else {
                let prev = movement::word_left(buf, sel, false);
                prev.head..sel.head
            };
            (range, String::new(), 0)
        })
        .collect();
    apply_per_selection(buf, sels, specs)
}

/// Deletes the selection or the word to the right of each cursor.
pub fn delete_word_right(buf: &mut Buffer, sels: &mut Selections) -> Vec<EditDelta> {
    let specs = sels
        .all()
        .iter()
        .map(|&sel| {
            let range = if !sel.is_empty() {
                sel.range()
            } else {
                let next = movement::word_right(buf, sel, false);
                sel.head..next.head
            };
            (range, String::new(), 0)
        })
        .collect();
    apply_per_selection(buf, sels, specs)
}

/// Inserts a newline at each cursor, preserving the current line's indentation and adding
/// an extra indent level when the non-whitespace character before the cursor is `{`, `(`, `[`, or `:`.
///
/// Infers tab vs 4-space indentation from the current line's leading whitespace.
pub fn newline(buf: &mut Buffer, sels: &mut Selections) -> Vec<EditDelta> {
    newline_with_indent(buf, sels, 4, true)
}

/// Like [`newline`], with explicit `tab_size` and `insert_spaces` settings.
pub fn newline_with_indent(
    buf: &mut Buffer,
    sels: &mut Selections,
    tab_size: usize,
    insert_spaces: bool,
) -> Vec<EditDelta> {
    let specs = sels
        .all()
        .iter()
        .map(|&sel| {
            let range = sel.range();
            let pt = buf.char_to_point(range.start);
            let line = buf.line_text(pt.line);
            let prefix_chars: String = line.chars().take(pt.col).collect();
            let indent: String = prefix_chars
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();

            let last_non_ws = prefix_chars.trim_end().chars().last();
            let extra = matches!(last_non_ws, Some('{' | '(' | '[' | ':'));

            let indent_unit = if indent.starts_with('\t') || !insert_spaces {
                "\t".to_string()
            } else {
                " ".repeat(tab_size)
            };

            let mut inserted = String::new();
            inserted.push('\n');
            inserted.push_str(&indent);
            if extra {
                inserted.push_str(&indent_unit);
            }
            let cursor_offset = inserted.chars().count();
            (range, inserted, cursor_offset)
        })
        .collect();

    apply_per_selection(buf, sels, specs)
}

/// Indents all lines touched by selections (or inserts a tab/spaces at empty cursors).
pub fn indent(
    buf: &mut Buffer,
    sels: &mut Selections,
    tab_size: usize,
    insert_spaces: bool,
) -> Vec<EditDelta> {
    let unit = if insert_spaces {
        " ".repeat(tab_size)
    } else {
        "\t".to_string()
    };

    // If all selections are empty cursors, insert indentation at each cursor.
    if sels.all().iter().all(|s| s.is_empty()) {
        let specs = sels
            .all()
            .iter()
            .map(|&sel| {
                let ins = if insert_spaces {
                    let pt = buf.char_to_point(sel.head);
                    let spaces = tab_size - (pt.col % tab_size.max(1));
                    " ".repeat(spaces)
                } else {
                    "\t".to_string()
                };
                let len = ins.chars().count();
                (sel.range(), ins, len)
            })
            .collect();
        return apply_per_selection(buf, sels, specs);
    }

    buf.set_selections_for_undo(sels);
    let lines = touched_lines(buf, sels);
    let unit_len = unit.chars().count();
    let mut edits = Vec::with_capacity(lines.len());
    for &line in &lines {
        let start = buf.rope().line_to_char(line);
        edits.push((start..start, unit.clone()));
    }

    let deltas = buf.edit(edits);
    // Shift selection endpoints by number of indented lines at or before each endpoint's line.
    let primary_idx = sels.primary_index();
    let new_sels = sels
        .all()
        .iter()
        .map(|&sel| {
            let shift = |pos: usize| -> usize {
                let count = lines
                    .iter()
                    .filter(|&&l| buf.rope().line_to_char(l) <= pos)
                    .count();
                (pos + count * unit_len).min(buf.len_chars())
            };
            Selection {
                anchor: shift(sel.anchor),
                head: shift(sel.head),
                goal_col: None,
            }
        })
        .collect();
    sels.set(new_sels, primary_idx);
    buf.set_selections_after_edit(sels);
    deltas
}

/// Outdents all lines touched by selections by up to one tab stop (`tab_size` spaces or one `\t`).
pub fn outdent(
    buf: &mut Buffer,
    sels: &mut Selections,
    tab_size: usize,
    _insert_spaces: bool,
) -> Vec<EditDelta> {
    buf.set_selections_for_undo(sels);
    let lines = touched_lines(buf, sels);
    let mut edits = Vec::new();
    let mut removals: Vec<(usize, usize)> = Vec::new(); // (line_start_char, removed_chars)

    for &line in &lines {
        let text = buf.line_text(line);
        let remove = if text.starts_with('\t') {
            1
        } else {
            text.chars()
                .take(tab_size)
                .take_while(|&c| c == ' ')
                .count()
        };
        if remove > 0 {
            let start = buf.rope().line_to_char(line);
            edits.push((start..(start + remove), String::new()));
            removals.push((start, remove));
        }
    }

    if edits.is_empty() {
        return Vec::new();
    }

    let deltas = buf.edit(edits);
    let primary_idx = sels.primary_index();
    let new_sels = sels
        .all()
        .iter()
        .map(|&sel| {
            let adjust = |pos: usize| -> usize {
                let mut offset = 0usize;
                for &(start, rem) in &removals {
                    if pos >= start + rem {
                        offset += rem;
                    } else if pos > start {
                        offset += pos - start;
                    }
                }
                pos.saturating_sub(offset).min(buf.len_chars())
            };
            Selection {
                anchor: adjust(sel.anchor),
                head: adjust(sel.head),
                goal_col: None,
            }
        })
        .collect();
    sels.set(new_sels, primary_idx);
    buf.set_selections_after_edit(sels);
    deltas
}

/// Toggles line comments with `token` (e.g. `"//"` or `"#"`) across all lines touched by selections.
pub fn toggle_line_comment(
    buf: &mut Buffer,
    sels: &mut Selections,
    token: &str,
) -> Vec<EditDelta> {
    if token.is_empty() {
        return Vec::new();
    }
    buf.set_selections_for_undo(sels);
    let lines = touched_lines(buf, sels);
    let token_chars = token.chars().count();

    let non_empty_lines: Vec<usize> = lines
        .iter()
        .copied()
        .filter(|&l| !buf.line_text(l).trim().is_empty())
        .collect();

    let target_lines = if non_empty_lines.is_empty() {
        lines.clone()
    } else {
        non_empty_lines
    };

    let all_commented = target_lines.iter().all(|&l| {
        let text = buf.line_text(l);
        text.trim_start().starts_with(token)
    });

    let mut edits = Vec::new();
    let mut shifts: Vec<(usize, isize)> = Vec::new();

    if all_commented {
        for &l in &target_lines {
            let text = buf.line_text(l);
            let ws_chars = text.chars().take_while(|c| c.is_whitespace()).count();
            let after_ws: String = text.chars().skip(ws_chars).collect();
            if after_ws.starts_with(token) {
                let has_space = after_ws.chars().nth(token_chars) == Some(' ');
                let del_len = token_chars + usize::from(has_space);
                let start = buf.rope().line_to_char(l) + ws_chars;
                edits.push((start..(start + del_len), String::new()));
                shifts.push((start, -(del_len as isize)));
            }
        }
    } else {
        let min_indent = target_lines
            .iter()
            .map(|&l| {
                buf.line_text(l)
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .count()
            })
            .min()
            .unwrap_or(0);
        let prefix = format!("{token} ");
        let add_len = prefix.chars().count() as isize;
        for &l in &target_lines {
            let line_len = buf.line_len_chars(l);
            let col = min_indent.min(line_len);
            let start = buf.rope().line_to_char(l) + col;
            edits.push((start..start, prefix.clone()));
            shifts.push((start, add_len));
        }
    }

    let deltas = buf.edit(edits);
    let primary_idx = sels.primary_index();
    let new_sels = sels
        .all()
        .iter()
        .map(|&sel| {
            let adjust = |pos: usize| -> usize {
                let mut cur = pos as isize;
                for &(at, d) in &shifts {
                    if pos >= at {
                        cur += d;
                    }
                }
                cur.max(0).min(buf.len_chars() as isize) as usize
            };
            Selection {
                anchor: adjust(sel.anchor),
                head: adjust(sel.head),
                goal_col: None,
            }
        })
        .collect();
    sels.set(new_sels, primary_idx);
    buf.set_selections_after_edit(sels);
    deltas
}

/// Duplicates the lines touched by each selection immediately below them.
pub fn duplicate_lines(buf: &mut Buffer, sels: &mut Selections) -> Vec<EditDelta> {
    buf.set_selections_for_undo(sels);
    let mut ranges: Vec<(usize, usize)> = sels
        .all()
        .iter()
        .map(|&s| selection_line_range(buf, s))
        .collect();
    ranges.sort_by_key(|&(a, b)| (a, b));
    // Merge overlapping line blocks.
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (a, b) in ranges {
        if let Some(last) = merged.last_mut() {
            if a <= last.1 + 1 {
                last.1 = last.1.max(b);
                continue;
            }
        }
        merged.push((a, b));
    }

    let mut edits = Vec::with_capacity(merged.len());
    let mut inserted_shifts: Vec<(usize, usize)> = Vec::new();

    for (start_line, end_line) in merged {
        let mut block = String::new();
        for l in start_line..=end_line {
            block.push('\n');
            block.push_str(&buf.line_text(l));
        }
        let insert_pos = buf.rope().line_to_char(end_line) + buf.line_len_chars(end_line);
        let ins_len = block.chars().count();
        edits.push((insert_pos..insert_pos, block));
        inserted_shifts.push((insert_pos, ins_len));
    }

    let deltas = buf.edit(edits);
    let primary_idx = sels.primary_index();
    let new_sels = sels
        .all()
        .iter()
        .map(|&sel| {
            // Move selection down onto the duplicated lines.
            let mut shift = 0usize;
            for &(at, ins) in &inserted_shifts {
                if sel.start() <= at {
                    shift += ins;
                    break;
                } else {
                    shift += ins;
                }
            }
            Selection {
                anchor: (sel.anchor + shift).min(buf.len_chars()),
                head: (sel.head + shift).min(buf.len_chars()),
                goal_col: None,
            }
        })
        .collect();
    sels.set(new_sels, primary_idx);
    buf.set_selections_after_edit(sels);
    deltas
}

/// Moves the lines covered by the primary selection (or all touched lines) up by one line.
pub fn move_lines_up(buf: &mut Buffer, sels: &mut Selections) -> Vec<EditDelta> {
    let lines = touched_lines(buf, sels);
    let Some(&first) = lines.first() else {
        return Vec::new();
    };
    if first == 0 {
        return Vec::new();
    }
    let last = *lines.last().unwrap();

    buf.set_selections_for_undo(sels);
    let above_line = first - 1;
    let above_text = buf.line_text(above_line);
    let above_start = buf.rope().line_to_char(above_line);
    let block_end = buf.rope().line_to_char(last) + buf.line_len_chars(last);

    let mut replacement = String::new();
    for l in first..=last {
        replacement.push_str(&buf.line_text(l));
        replacement.push('\n');
    }
    replacement.push_str(&above_text);

    let above_delta = above_text.chars().count() + 1;
    let deltas = buf.edit(vec![(above_start..block_end, replacement)]);

    let primary_idx = sels.primary_index();
    let new_sels = sels
        .all()
        .iter()
        .map(|&sel| Selection {
            anchor: sel.anchor.saturating_sub(above_delta),
            head: sel.head.saturating_sub(above_delta),
            goal_col: None,
        })
        .collect();
    sels.set(new_sels, primary_idx);
    buf.set_selections_after_edit(sels);
    deltas
}

/// Moves the lines covered by the selections down by one line.
pub fn move_lines_down(buf: &mut Buffer, sels: &mut Selections) -> Vec<EditDelta> {
    let lines = touched_lines(buf, sels);
    let Some(&first) = lines.first() else {
        return Vec::new();
    };
    let last = *lines.last().unwrap();
    if last + 1 >= buf.len_lines() {
        return Vec::new();
    }

    buf.set_selections_for_undo(sels);
    let below_line = last + 1;
    let below_text = buf.line_text(below_line);
    let block_start = buf.rope().line_to_char(first);
    let below_end = buf.rope().line_to_char(below_line) + buf.line_len_chars(below_line);

    let mut replacement = String::new();
    replacement.push_str(&below_text);
    for l in first..=last {
        replacement.push('\n');
        replacement.push_str(&buf.line_text(l));
    }

    let below_delta = below_text.chars().count() + 1;
    let deltas = buf.edit(vec![(block_start..below_end, replacement)]);

    let primary_idx = sels.primary_index();
    let new_sels = sels
        .all()
        .iter()
        .map(|&sel| Selection {
            anchor: (sel.anchor + below_delta).min(buf.len_chars()),
            head: (sel.head + below_delta).min(buf.len_chars()),
            goal_col: None,
        })
        .collect();
    sels.set(new_sels, primary_idx);
    buf.set_selections_after_edit(sels);
    deltas
}

/// Deletes all lines touched by selections.
pub fn delete_lines(buf: &mut Buffer, sels: &mut Selections) -> Vec<EditDelta> {
    buf.set_selections_for_undo(sels);
    let lines = touched_lines(buf, sels);
    if lines.is_empty() {
        return Vec::new();
    }

    let total_lines = buf.len_lines();
    let mut edits = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let start_l = lines[i];
        let mut end_l = start_l;
        while i + 1 < lines.len() && lines[i + 1] == end_l + 1 {
            i += 1;
            end_l = lines[i];
        }
        i += 1;

        let range = if end_l + 1 < total_lines {
            let s = buf.rope().line_to_char(start_l);
            let e = buf.rope().line_to_char(end_l + 1);
            s..e
        } else if start_l > 0 {
            // Deleting up to the last line: also consume the preceding newline.
            let s = buf.rope().line_to_char(start_l) - 1;
            let e = buf.len_chars();
            s..e
        } else {
            0..buf.len_chars()
        };
        edits.push((range, String::new()));
    }

    let first_line = lines[0];
    let deltas = buf.edit(edits);
    let target_line = first_line.min(buf.len_lines().saturating_sub(1));
    let pos = buf.point_to_char(Point {
        line: target_line,
        col: 0,
    });
    sels.set(vec![Selection::cursor(pos)], 0);
    buf.set_selections_after_edit(sels);
    deltas
}

/// Expands each selection to cover its full line(s) (including trailing newline if present).
pub fn select_line(buf: &Buffer, sels: &mut Selections) {
    let primary_idx = sels.primary_index();
    let total_lines = buf.len_lines();
    let new_sels = sels
        .all()
        .iter()
        .map(|&sel| {
            let (a, b) = selection_line_range(buf, sel);
            let start = buf.rope().line_to_char(a);
            let end = if b + 1 < total_lines {
                buf.rope().line_to_char(b + 1)
            } else {
                buf.len_chars()
            };
            Selection::new(start, end)
        })
        .collect();
    sels.set(new_sels, primary_idx);
}

/// Expands each cursor to select the word under it.
pub fn select_word(buf: &Buffer, sels: &mut Selections) {
    let primary_idx = sels.primary_index();
    let new_sels = sels
        .all()
        .iter()
        .map(|&sel| {
            if let Some(r) = buf.word_range_at(sel.head) {
                Selection::new(r.start, r.end)
            } else {
                sel
            }
        })
        .collect();
    sels.set(new_sels, primary_idx);
}

/// Selects the entire buffer.
pub fn select_all(buf: &Buffer, sels: &mut Selections) {
    sels.set(vec![Selection::new(0, buf.len_chars())], 0);
}

/// Adds a new cursor on the line above the topmost cursor (preserving column).
pub fn add_cursor_above(buf: &Buffer, sels: &mut Selections) {
    let top = sels.all()[0];
    let pt = buf.char_to_point(top.head);
    if pt.line == 0 {
        return;
    }
    let goal = top.goal_col.unwrap_or(pt.col);
    let target = buf.point_to_char(Point {
        line: pt.line - 1,
        col: goal,
    });
    sels.add(Selection {
        anchor: target,
        head: target,
        goal_col: Some(goal),
    });
}

/// Adds a new cursor on the line below the bottommost cursor (preserving column).
pub fn add_cursor_below(buf: &Buffer, sels: &mut Selections) {
    let bottom = *sels.all().last().unwrap();
    let pt = buf.char_to_point(bottom.head);
    if pt.line + 1 >= buf.len_lines() {
        return;
    }
    let goal = bottom.goal_col.unwrap_or(pt.col);
    let target = buf.point_to_char(Point {
        line: pt.line + 1,
        col: goal,
    });
    sels.add(Selection {
        anchor: target,
        head: target,
        goal_col: Some(goal),
    });
}

/// If the primary selection is empty, selects the word at the primary cursor.
/// Otherwise finds the next occurrence of the primary selection's text (wrapping around)
/// and adds it as a new selection.
pub fn select_next_occurrence(buf: &Buffer, sels: &mut Selections) {
    let primary = *sels.primary();
    if primary.is_empty() {
        if let Some(r) = buf.word_range_at(primary.head) {
            let idx = sels.primary_index();
            let mut all = sels.all().to_vec();
            all[idx] = Selection::new(r.start, r.end);
            sels.set(all, idx);
        }
        return;
    }

    let query: String = buf.rope().slice(primary.range()).to_string();
    if query.is_empty() {
        return;
    }

    let matches = buf.find_all(
        &query,
        crate::SearchOptions {
            case_sensitive: true,
            whole_word: false,
            regex: false,
        },
    );
    if matches.is_empty() {
        return;
    }

    let after = primary.end();
    // Search after `primary.end()` first, then wrap around from 0.
    let candidate = matches
        .iter()
        .filter(|r| r.start >= after)
        .chain(matches.iter().filter(|r| r.start < after))
        .find(|r| !sels.all().iter().any(|s| s.range() == **r));

    if let Some(next_range) = candidate {
        sels.add(Selection::new(next_range.start, next_range.end));
    }
}

/// Returns the text to copy to the clipboard.
///
/// If all selections are empty, copies the full lines of each cursor (joined by `\n`).
/// Otherwise joins all non-empty selected texts by `\n`.
pub fn copy_text(buf: &Buffer, sels: &Selections) -> String {
    if sels.all().iter().all(|s| s.is_empty()) {
        let lines = touched_lines(buf, sels);
        lines
            .into_iter()
            .map(|l| buf.line_text(l))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        sels.all()
            .iter()
            .map(|s| buf.rope().slice(s.range()).to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Cuts the selected text (or full lines if all selections are empty) and returns `(cut_text, deltas)`.
pub fn cut_text(buf: &mut Buffer, sels: &mut Selections) -> (String, Vec<EditDelta>) {
    let text = copy_text(buf, sels);
    let deltas = if sels.all().iter().all(|s| s.is_empty()) {
        delete_lines(buf, sels)
    } else {
        let specs = sels
            .all()
            .iter()
            .map(|s| (s.range(), String::new(), 0))
            .collect();
        apply_per_selection(buf, sels, specs)
    };
    (text, deltas)
}

/// Pastes `text` at every selection.
///
/// Multi-cursor aware: if `sels.len() > 1` and the pasted text has the exact same number of
/// lines as cursors, each cursor receives its corresponding line.
pub fn paste(buf: &mut Buffer, sels: &mut Selections, text: &str) -> Vec<EditDelta> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if sels.len() > 1 {
        let lines: Vec<&str> = normalized.split('\n').collect();
        if lines.len() == sels.len() {
            let specs = sels
                .all()
                .iter()
                .zip(lines)
                .map(|(s, line)| {
                    let ins = line.to_string();
                    let len = ins.chars().count();
                    (s.range(), ins, len)
                })
                .collect();
            return apply_per_selection(buf, sels, specs);
        }
    }
    insert(buf, sels, &normalized)
}
