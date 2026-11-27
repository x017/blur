//! Undo/redo transaction history with time-based coalescing.

use std::ops::Range;
use std::time::{Duration, Instant};

use crate::selection::Selections;

/// Time window within which consecutive edits coalesce into one undo group.
const COALESCE_WINDOW: Duration = Duration::from_millis(500);

/// One atomic edit step recorded for undo/redo replay.
#[derive(Debug, Clone)]
pub(crate) struct RecordedEdit {
    /// Character range in the document right BEFORE this step was applied.
    pub range: Range<usize>,
    /// Text that was replaced (needed to undo).
    pub old_text: String,
    /// Text that was inserted (needed to redo).
    pub new_text: String,
}

/// A single edit batch (one `Buffer::edit` call) within an undo group.
#[derive(Debug, Clone)]
pub(crate) struct EditBatch {
    /// Edits in the order they were applied to the rope.
    pub steps: Vec<RecordedEdit>,
}

/// A grouped undo transaction.
#[derive(Debug, Clone)]
pub(crate) struct UndoGroup {
    pub batches: Vec<EditBatch>,
    pub selections_before: Selections,
    pub selections_after: Selections,
    pub last_edit_at: Instant,
}

/// Manages undo and redo stacks with 500ms coalescing and dirty-state tracking.
#[derive(Debug, Clone)]
pub(crate) struct History {
    pub undo_stack: Vec<UndoGroup>,
    pub redo_stack: Vec<UndoGroup>,
    pub group_broken: bool,
    pub pending_selections_before: Option<Selections>,
    /// Undo stack depth corresponding to the last saved state, if still reachable.
    pub saved_depth: Option<usize>,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            group_broken: true,
            pending_selections_before: None,
            saved_depth: Some(0),
        }
    }

    pub fn mark_saved(&mut self) {
        self.saved_depth = Some(self.undo_stack.len());
    }

    pub fn is_dirty(&self) -> bool {
        self.saved_depth != Some(self.undo_stack.len())
    }

    pub fn break_group(&mut self) {
        self.group_broken = true;
    }

    pub fn set_selections_before(&mut self, before: &Selections) {
        self.pending_selections_before = Some(before.clone());
    }

    pub fn set_selections_after(&mut self, after: &Selections) {
        if let Some(top) = self.undo_stack.last_mut() {
            top.selections_after = after.clone();
        }
    }

    pub fn record_batch(&mut self, batch: EditBatch, default_after: Selections) {
        let now = Instant::now();
        let before = self
            .pending_selections_before
            .take()
            .unwrap_or_else(|| default_after.clone());

        // Any new edit invalidates the redo stack.
        self.redo_stack.clear();

        // If saved_depth was ahead of current undo_stack (in redo), it is now unreachable.
        if let Some(depth) = self.saved_depth {
            if depth > self.undo_stack.len() {
                self.saved_depth = None;
            }
        }

        let can_coalesce = !self.group_broken
            && self
                .undo_stack
                .last()
                .is_some_and(|g| now.duration_since(g.last_edit_at) <= COALESCE_WINDOW)
            && self.saved_depth != Some(self.undo_stack.len());

        if can_coalesce {
            if let Some(top) = self.undo_stack.last_mut() {
                top.batches.push(batch);
                top.selections_after = default_after;
                top.last_edit_at = now;
                return;
            }
        }

        self.undo_stack.push(UndoGroup {
            batches: vec![batch],
            selections_before: before,
            selections_after: default_after,
            last_edit_at: now,
        });
        self.group_broken = false;
    }

    pub fn pop_undo(&mut self) -> Option<UndoGroup> {
        self.group_broken = true;
        self.pending_selections_before = None;
        self.undo_stack.pop()
    }

    pub fn push_redo(&mut self, group: UndoGroup) {
        self.redo_stack.push(group);
    }

    pub fn pop_redo(&mut self) -> Option<UndoGroup> {
        self.group_broken = true;
        self.pending_selections_before = None;
        self.redo_stack.pop()
    }

    pub fn push_undo(&mut self, group: UndoGroup) {
        self.undo_stack.push(group);
    }
}
