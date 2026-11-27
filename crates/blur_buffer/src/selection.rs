//! Selection and multi-cursor management.

use std::ops::Range;

/// A single text selection or caret, represented in character indices.
///
/// When `anchor == head`, the selection is an empty cursor (caret).
/// `goal_col` preserves the horizontal column across consecutive vertical movements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
    pub goal_col: Option<usize>,
}

impl Selection {
    /// Creates an empty cursor at character index `c`.
    pub fn cursor(c: usize) -> Self {
        Self {
            anchor: c,
            head: c,
            goal_col: None,
        }
    }

    /// Creates a selection from `anchor` to `head`.
    pub fn new(anchor: usize, head: usize) -> Self {
        Self {
            anchor,
            head,
            goal_col: None,
        }
    }

    /// Returns the ordered character range `start..end`.
    pub fn range(&self) -> Range<usize> {
        if self.anchor <= self.head {
            self.anchor..self.head
        } else {
            self.head..self.anchor
        }
    }

    /// Returns the smaller endpoint of the selection.
    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    /// Returns the larger endpoint of the selection.
    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    /// Returns `true` if this selection is an empty cursor.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// Returns `true` if `head < anchor`.
    pub fn is_reversed(&self) -> bool {
        self.head < self.anchor
    }
}

/// A non-empty, sorted, merged collection of selections with a designated primary selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selections {
    items: Vec<Selection>,
    primary: usize,
}

impl Default for Selections {
    fn default() -> Self {
        Self::single(Selection::cursor(0))
    }
}

impl Selections {
    /// Creates a `Selections` containing a single selection.
    pub fn single(sel: Selection) -> Self {
        Self {
            items: vec![sel],
            primary: 0,
        }
    }

    /// Returns all selections in sorted, non-overlapping order.
    pub fn all(&self) -> &[Selection] {
        &self.items
    }

    /// Returns the primary selection.
    pub fn primary(&self) -> &Selection {
        &self.items[self.primary]
    }

    /// Returns the index of the primary selection within `all()`.
    pub fn primary_index(&self) -> usize {
        self.primary
    }

    /// Adds a selection, making it the new primary, then sorts and merges overlaps.
    pub fn add(&mut self, sel: Selection) {
        self.items.push(sel);
        self.primary = self.items.len() - 1;
        self.normalize();
    }

    /// Replaces the selection set and primary index, then sorts and merges overlaps.
    ///
    /// If `v` is empty, resets to a single cursor at `0`.
    pub fn set(&mut self, v: Vec<Selection>, primary: usize) {
        if v.is_empty() {
            self.items = vec![Selection::cursor(0)];
            self.primary = 0;
            return;
        }
        self.primary = primary.min(v.len() - 1);
        self.items = v;
        self.normalize();
    }

    /// Collapses all selections down to just the primary selection.
    pub fn collapse_to_primary(&mut self) {
        let p = self.items[self.primary];
        self.items.clear();
        self.items.push(p);
        self.primary = 0;
    }

    /// Returns the number of selections (always `>= 1`).
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Always returns `false` because `Selections` always holds at least one selection.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Clamps all selection endpoints to `max_char` and re-normalizes.
    pub fn clamp(&mut self, max_char: usize) {
        for s in &mut self.items {
            s.anchor = s.anchor.min(max_char);
            s.head = s.head.min(max_char);
        }
        self.normalize();
    }

    /// Sorts selections by `(start, end)` and merges overlapping selections (and identical cursors),
    /// preserving the primary selection index.
    fn normalize(&mut self) {
        if self.items.is_empty() {
            self.items.push(Selection::cursor(0));
            self.primary = 0;
            return;
        }

        let mut indexed: Vec<(Selection, bool)> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, &s)| (s, i == self.primary))
            .collect();

        indexed.sort_by_key(|(s, _)| (s.start(), s.end()));

        let mut merged: Vec<(Selection, bool)> = Vec::with_capacity(indexed.len());
        for (sel, is_prim) in indexed {
            if let Some((last, last_prim)) = merged.last_mut() {
                let overlaps = if last.is_empty() && sel.is_empty() {
                    last.start() == sel.start()
                } else if last.is_empty() {
                    last.start() >= sel.start() && last.start() <= sel.end()
                } else if sel.is_empty() {
                    sel.start() >= last.start() && sel.start() <= last.end()
                } else {
                    sel.start() < last.end()
                };

                if overlaps {
                    let start = last.start().min(sel.start());
                    let end = last.end().max(sel.end());
                    let reversed = if is_prim {
                        sel.is_reversed()
                    } else {
                        last.is_reversed()
                    };
                    let (anchor, head) = if reversed { (end, start) } else { (start, end) };
                    let goal_col = if is_prim { sel.goal_col } else { last.goal_col };
                    *last = Selection {
                        anchor,
                        head,
                        goal_col,
                    };
                    *last_prim = *last_prim || is_prim;
                    continue;
                }
            }
            merged.push((sel, is_prim));
        }

        self.primary = merged.iter().position(|(_, p)| *p).unwrap_or(0);
        self.items = merged.into_iter().map(|(s, _)| s).collect();
    }
}
