//! Pure cursor movement functions operating on `(&Buffer, Selection) -> Selection`.
//!
//! When `extend` is `true`, the selection's `anchor` is preserved and only `head` moves.
//! When `extend` is `false`, `anchor` is moved to the new `head`.

use unicode_segmentation::UnicodeSegmentation;

use crate::{Buffer, Point, Selection};

fn finish(sel: Selection, new_head: usize, extend: bool, goal_col: Option<usize>) -> Selection {
    Selection {
        anchor: if extend { sel.anchor } else { new_head },
        head: new_head,
        goal_col,
    }
}

/// Moves one grapheme cluster left.
///
/// When `extend` is `false` and the selection is non-empty, collapses to the start of the selection.
pub fn left(buf: &Buffer, sel: Selection, extend: bool) -> Selection {
    if !extend && !sel.is_empty() {
        return Selection::cursor(sel.start());
    }
    if sel.head == 0 {
        return finish(sel, 0, extend, None);
    }

    let rope = buf.rope();
    let line_idx = rope.char_to_line(sel.head);
    let line_start = rope.line_to_char(line_idx);

    if sel.head == line_start {
        // Cross newline to end of previous line.
        return finish(sel, sel.head - 1, extend, None);
    }

    let line_str = buf.line_text(line_idx);
    let col_chars = sel.head - line_start;
    let byte_offset = char_offset_to_byte(&line_str, col_chars);
    let prefix = &line_str[..byte_offset];

    let prev_grapheme_chars = prefix
        .graphemes(true)
        .next_back()
        .map(|g| g.chars().count())
        .unwrap_or(1);

    let new_head = sel.head.saturating_sub(prev_grapheme_chars);
    finish(sel, new_head, extend, None)
}

/// Moves one grapheme cluster right.
///
/// When `extend` is `false` and the selection is non-empty, collapses to the end of the selection.
pub fn right(buf: &Buffer, sel: Selection, extend: bool) -> Selection {
    if !extend && !sel.is_empty() {
        return Selection::cursor(sel.end());
    }
    let len = buf.len_chars();
    if sel.head >= len {
        return finish(sel, len, extend, None);
    }

    let rope = buf.rope();
    let line_idx = rope.char_to_line(sel.head);
    let line_start = rope.line_to_char(line_idx);
    let line_len = buf.line_len_chars(line_idx);
    let col_chars = sel.head - line_start;

    if col_chars >= line_len {
        // Cross newline to start of next line.
        return finish(sel, (sel.head + 1).min(len), extend, None);
    }

    let line_str = buf.line_text(line_idx);
    let byte_offset = char_offset_to_byte(&line_str, col_chars);
    let suffix = &line_str[byte_offset..];

    let next_grapheme_chars = suffix
        .graphemes(true)
        .next()
        .map(|g| g.chars().count())
        .unwrap_or(1);

    let new_head = (sel.head + next_grapheme_chars).min(len);
    finish(sel, new_head, extend, None)
}

/// Moves up one line, preserving the horizontal goal column.
pub fn up(buf: &Buffer, sel: Selection, extend: bool) -> Selection {
    move_vertical(buf, sel, -1, extend)
}

/// Moves down one line, preserving the horizontal goal column.
pub fn down(buf: &Buffer, sel: Selection, extend: bool) -> Selection {
    move_vertical(buf, sel, 1, extend)
}

/// Moves up by `n_lines` lines, preserving the horizontal goal column.
pub fn page_up(buf: &Buffer, sel: Selection, n_lines: usize, extend: bool) -> Selection {
    move_vertical(buf, sel, -(n_lines as isize), extend)
}

/// Moves down by `n_lines` lines, preserving the horizontal goal column.
pub fn page_down(buf: &Buffer, sel: Selection, n_lines: usize, extend: bool) -> Selection {
    move_vertical(buf, sel, n_lines as isize, extend)
}

fn move_vertical(buf: &Buffer, sel: Selection, delta: isize, extend: bool) -> Selection {
    let pt = buf.char_to_point(sel.head);
    let goal = sel.goal_col.unwrap_or(pt.col);
    let max_line = buf.len_lines().saturating_sub(1);

    if delta < 0 && pt.line == 0 {
        return finish(sel, 0, extend, Some(goal));
    }
    if delta > 0 && pt.line >= max_line {
        return finish(sel, buf.len_chars(), extend, Some(goal));
    }

    let target_line = if delta < 0 {
        pt.line.saturating_sub(delta.unsigned_abs())
    } else {
        (pt.line + delta as usize).min(max_line)
    };

    let target_char = buf.point_to_char(Point {
        line: target_line,
        col: goal,
    });
    finish(sel, target_char, extend, Some(goal))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Whitespace,
    Word,
    Punctuation,
}

fn classify(c: char) -> CharClass {
    if c.is_whitespace() {
        CharClass::Whitespace
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punctuation
    }
}

/// Moves left to the start of the previous word (or punctuation run).
pub fn word_left(buf: &Buffer, sel: Selection, extend: bool) -> Selection {
    let mut pos = sel.head;
    if pos == 0 {
        return finish(sel, 0, extend, None);
    }

    let rope = buf.rope();
    // Skip any whitespace immediately to the left.
    while pos > 0 && classify(rope.char(pos - 1)) == CharClass::Whitespace {
        pos -= 1;
    }
    if pos > 0 {
        let cls = classify(rope.char(pos - 1));
        while pos > 0 && classify(rope.char(pos - 1)) == cls {
            pos -= 1;
        }
    }

    finish(sel, pos, extend, None)
}

/// Moves right to the end of the next word (or punctuation run).
pub fn word_right(buf: &Buffer, sel: Selection, extend: bool) -> Selection {
    let len = buf.len_chars();
    let mut pos = sel.head.min(len);
    if pos >= len {
        return finish(sel, len, extend, None);
    }

    let rope = buf.rope();
    let cls = classify(rope.char(pos));
    if cls == CharClass::Whitespace {
        while pos < len && classify(rope.char(pos)) == CharClass::Whitespace {
            pos += 1;
        }
        if pos < len {
            let next_cls = classify(rope.char(pos));
            while pos < len && classify(rope.char(pos)) == next_cls {
                pos += 1;
            }
        }
    } else {
        while pos < len && classify(rope.char(pos)) == cls {
            pos += 1;
        }
    }

    finish(sel, pos, extend, None)
}

/// Smart Home: moves to the first non-whitespace character on the line,
/// or to column 0 if already at the first non-whitespace character.
pub fn line_start(buf: &Buffer, sel: Selection, extend: bool) -> Selection {
    let pt = buf.char_to_point(sel.head);
    let line_start_char = buf.rope().line_to_char(pt.line);
    let line_str = buf.line_text(pt.line);

    let first_non_ws = line_str
        .chars()
        .position(|c| !c.is_whitespace())
        .unwrap_or(0);

    let target_col = if pt.col == first_non_ws {
        0
    } else {
        first_non_ws
    };
    finish(sel, line_start_char + target_col, extend, None)
}

/// Moves to the end of the current line (before any trailing newline).
pub fn line_end(buf: &Buffer, sel: Selection, extend: bool) -> Selection {
    let pt = buf.char_to_point(sel.head);
    let line_start_char = buf.rope().line_to_char(pt.line);
    let target = line_start_char + buf.line_len_chars(pt.line);
    finish(sel, target, extend, None)
}

/// Moves to the start of the document (char index 0).
pub fn doc_start(_buf: &Buffer, sel: Selection, extend: bool) -> Selection {
    finish(sel, 0, extend, None)
}

/// Moves to the end of the document (`buf.len_chars()`).
pub fn doc_end(buf: &Buffer, sel: Selection, extend: bool) -> Selection {
    finish(sel, buf.len_chars(), extend, None)
}

fn char_offset_to_byte(s: &str, char_offset: usize) -> usize {
    s.char_indices()
        .nth(char_offset)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}
