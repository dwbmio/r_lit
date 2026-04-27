//! Undo / redo for paint and erase operations.
//!
//! v0.6 adds **stroke grouping**: a drag across many cells now
//! collapses into one logical undo entry instead of one-cell-at-a-time
//! stepping. The UI wraps each pointer-down → pointer-up interaction
//! with [`EditHistory::begin_stroke`] / [`EditHistory::end_stroke`];
//! between those calls every [`EditHistory::record`] call appends to
//! the open stroke. Plain clicks and non-UI callers can still record
//! individual ops — those become single-op strokes implicitly.
//!
//! The data structure is deliberately window-free. It lives in the
//! GUI binary today (because it's wired to Bevy resources and the
//! egui paint loop), but the tests in this module construct
//! `EditHistory` directly and drive it without any Bevy `App`,
//! satisfying the Headless Invariant's rule that feature correctness
//! is verified from a headless test surface.

use std::collections::VecDeque;

use bevy::prelude::*;

use maquette::grid::{Cell, Grid};

use crate::session::CurrentProject;

/// Ring-buffer depth (measured in strokes, not ops). 256 strokes
/// covers a full editing session; each stroke holds a small `Vec` of
/// `PaintOp`s (usually a dozen or two), so memory stays trivial.
pub const MAX_UNDO: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaintOp {
    pub x: usize,
    pub y: usize,
    pub before: Cell,
    pub after: Cell,
}

/// A user gesture — a run of cell changes committed together. A
/// click-and-drag that paints 30 cells becomes one `Stroke`; Ctrl+Z
/// rolls back the entire gesture in one step.
#[derive(Clone, Debug, Default)]
pub struct Stroke {
    ops: Vec<PaintOp>,
}

impl Stroke {
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    // Called only from the in-module tests today. Kept on the public
    // API because it's the natural way to observe stroke size from
    // debug overlays / future timeline UI.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.ops.len()
    }
}

#[derive(Resource, Default)]
pub struct EditHistory {
    undo: VecDeque<Stroke>,
    redo: VecDeque<Stroke>,
    /// Currently-open stroke, if any. `record` appends here while
    /// this is `Some`, falls back to a single-op stroke otherwise.
    open: Option<Stroke>,
    /// Monotonically increments each time a non-empty stroke is
    /// committed to the undo stack (including single-op strokes from
    /// a bare `record` call). The autosave plugin polls this
    /// counter; a delta since the last flush is its "stroke closed"
    /// signal. Kept as a plain counter (not a Bevy `Message`) so the
    /// history data structure stays window-free — `autosave.rs`
    /// observes it from the outside.
    ///
    /// A `u64` is absurd headroom (≈ 585 years at 10⁹ strokes/sec)
    /// but keeps the counter additive across `clear()` calls, so
    /// File → New doesn't trick autosave into re-flushing a stale
    /// snapshot.
    strokes_committed: u64,
}

impl EditHistory {
    /// Mark the start of a new stroke. Subsequent [`Self::record`]
    /// calls append to it. If a stroke was already open (shouldn't
    /// happen in normal UI flow, but can happen if begin/end get
    /// mis-paired during a crash recovery or keyboard conflict), it
    /// is silently committed first — we never silently drop work.
    pub fn begin_stroke(&mut self) {
        if self.open.is_some() {
            self.end_stroke();
        }
        self.open = Some(Stroke::default());
    }

    /// Commit the currently-open stroke to the undo history, clearing
    /// the redo stack. Empty strokes (nothing actually painted) are
    /// discarded so the user doesn't have to Ctrl+Z through null
    /// entries after a miss-click.
    pub fn end_stroke(&mut self) {
        if let Some(stroke) = self.open.take() {
            if stroke.is_empty() {
                return;
            }
            self.push_stroke(stroke);
        }
    }

    /// Record one cell change. If a stroke is open, appends to it;
    /// otherwise commits immediately as a single-op stroke.
    pub fn record(&mut self, op: PaintOp) {
        if let Some(stroke) = &mut self.open {
            stroke.ops.push(op);
        } else {
            self.push_stroke(Stroke { ops: vec![op] });
        }
    }

    fn push_stroke(&mut self, stroke: Stroke) {
        self.redo.clear();
        if self.undo.len() == MAX_UNDO {
            self.undo.pop_front();
        }
        self.undo.push_back(stroke);
        // Any committed stroke — stroke-group OR bare `record` — is
        // a potential autosave trigger. Ticking here (rather than in
        // `end_stroke`) covers the single-op-stroke path too.
        self.strokes_committed = self.strokes_committed.saturating_add(1);
    }

    /// Monotonic count of strokes that have been committed to the
    /// undo stack over the lifetime of this `EditHistory`. Used by
    /// the GUI's autosave plugin as a "did anything change?" probe.
    pub fn strokes_committed(&self) -> u64 {
        self.strokes_committed
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.open = None;
        // Intentionally do NOT reset `strokes_committed`: autosave
        // tracks its own baseline independently, and resetting here
        // could cause a pending-autosave race on File → New where
        // the counter "rolls back" past an already-seen value.
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// True while a stroke is currently being recorded. Reserved for
    /// debug overlays / a future "editing..." indicator in the status
    /// bar; the stroke machinery itself guards reentrancy internally.
    #[allow(dead_code)]
    pub fn is_stroke_open(&self) -> bool {
        self.open.is_some()
    }

    fn take_undo_stroke(&mut self) -> Option<Stroke> {
        self.undo.pop_back()
    }

    fn take_redo_stroke(&mut self) -> Option<Stroke> {
        self.redo.pop_back()
    }
}

#[derive(Message, Clone, Copy)]
pub enum HistoryAction {
    Undo,
    Redo,
}

pub struct HistoryPlugin;

impl Plugin for HistoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditHistory>()
            .add_message::<HistoryAction>()
            .add_systems(Update, handle_history_action);
    }
}

fn handle_history_action(
    mut events: MessageReader<HistoryAction>,
    mut history: ResMut<EditHistory>,
    mut grid: ResMut<Grid>,
    mut current: ResMut<CurrentProject>,
) {
    for action in events.read() {
        match action {
            HistoryAction::Undo => {
                // If a stroke is mid-flight (e.g. the user hit Ctrl+Z
                // while dragging), commit it first so Ctrl+Z reaches a
                // well-defined state.
                history.end_stroke();
                if let Some(stroke) = history.take_undo_stroke() {
                    for op in stroke.ops.iter().rev() {
                        apply_cell(&mut grid, op.x, op.y, op.before);
                    }
                    history.redo.push_back(stroke);
                    current.mark_dirty();
                }
            }
            HistoryAction::Redo => {
                history.end_stroke();
                if let Some(stroke) = history.take_redo_stroke() {
                    for op in stroke.ops.iter() {
                        apply_cell(&mut grid, op.x, op.y, op.after);
                    }
                    history.undo.push_back(stroke);
                    current.mark_dirty();
                }
            }
        }
    }
}

fn apply_cell(grid: &mut Grid, x: usize, y: usize, cell: Cell) {
    if !grid.in_bounds(x, y) {
        return;
    }
    let idx = y * grid.w + x;
    grid.cells[idx] = cell;
    grid.dirty = true;
}

#[cfg(test)]
mod tests {
    //! These tests exercise `EditHistory` directly, without a Bevy
    //! `App`. The plugin wiring in `handle_history_action` is a thin
    //! dispatcher around the pure data structure; correctness lives
    //! here.

    use super::*;
    use maquette::grid::Cell;

    fn op(x: usize, y: usize, color_before: Option<u8>, color_after: Option<u8>) -> PaintOp {
        PaintOp {
            x,
            y,
            before: Cell {
                color_idx: color_before,
                height: if color_before.is_some() { 1 } else { 0 },
                ..Cell::default()
            },
            after: Cell {
                color_idx: color_after,
                height: if color_after.is_some() { 1 } else { 0 },
                ..Cell::default()
            },
        }
    }

    #[test]
    fn record_outside_stroke_commits_single_op_stroke() {
        let mut h = EditHistory::default();
        h.record(op(0, 0, None, Some(1)));
        assert!(h.can_undo());
        assert!(!h.can_redo());
        assert_eq!(h.undo.len(), 1);
        assert_eq!(h.undo[0].len(), 1);
    }

    #[test]
    fn stroke_collapses_multi_cell_drag_into_one_undo_entry() {
        let mut h = EditHistory::default();
        h.begin_stroke();
        h.record(op(0, 0, None, Some(1)));
        h.record(op(1, 0, None, Some(1)));
        h.record(op(2, 0, None, Some(1)));
        h.end_stroke();
        assert_eq!(h.undo.len(), 1, "3 ops should coalesce into 1 stroke");
        assert_eq!(h.undo[0].len(), 3);
    }

    #[test]
    fn empty_stroke_is_discarded() {
        let mut h = EditHistory::default();
        h.begin_stroke();
        h.end_stroke();
        assert!(!h.can_undo(), "empty strokes should not land in history");
    }

    #[test]
    fn begin_stroke_while_open_flushes_previous_stroke() {
        // Defensive behaviour: if the UI ever fails to pair
        // begin/end (e.g. focus loss mid-drag), the previous
        // stroke is still committed, never silently dropped.
        let mut h = EditHistory::default();
        h.begin_stroke();
        h.record(op(0, 0, None, Some(1)));
        h.begin_stroke(); // no matching end_stroke
        h.record(op(1, 1, None, Some(2)));
        h.end_stroke();
        assert_eq!(h.undo.len(), 2, "each stroke should survive");
        assert_eq!(h.undo[0].len(), 1);
        assert_eq!(h.undo[1].len(), 1);
    }

    #[test]
    fn recording_after_undo_clears_redo_stack() {
        let mut h = EditHistory::default();
        h.record(op(0, 0, None, Some(1)));
        h.record(op(1, 0, None, Some(2)));

        // Simulate undo by draining one stroke from the undo queue
        // into the redo queue. We're testing the history data model
        // here, not the Bevy system that would apply the ops.
        let s = h.take_undo_stroke().unwrap();
        h.redo.push_back(s);
        assert!(h.can_redo());

        // Any new record call after an undo must drop the redo
        // history — standard editor semantics.
        h.record(op(5, 5, None, Some(3)));
        assert!(!h.can_redo(), "redo should be cleared by a new edit");
    }

    #[test]
    fn clear_resets_both_stacks_and_open_stroke() {
        let mut h = EditHistory::default();
        h.begin_stroke();
        h.record(op(0, 0, None, Some(1)));
        h.clear();
        assert!(!h.can_undo());
        assert!(!h.can_redo());
        assert!(!h.is_stroke_open());
    }

    #[test]
    fn max_undo_truncates_oldest_stroke() {
        let mut h = EditHistory::default();
        for i in 0..(MAX_UNDO + 5) {
            h.record(op(i % 8, i % 8, None, Some(1)));
        }
        assert_eq!(h.undo.len(), MAX_UNDO);
    }

    #[test]
    fn stroke_preserves_op_order() {
        // Undo replays in reverse, redo in forward — that contract is
        // enforced by `handle_history_action`, but the data structure
        // needs to keep the ops ordered as recorded for that logic to
        // work.
        let mut h = EditHistory::default();
        h.begin_stroke();
        for i in 0..5 {
            h.record(op(i, 0, None, Some(1)));
        }
        h.end_stroke();
        let stroke = h.take_undo_stroke().unwrap();
        for (i, o) in stroke.ops.iter().enumerate() {
            assert_eq!(o.x, i, "stroke op order changed");
        }
    }

    #[test]
    fn is_stroke_open_tracks_begin_end() {
        let mut h = EditHistory::default();
        assert!(!h.is_stroke_open());
        h.begin_stroke();
        assert!(h.is_stroke_open());
        h.end_stroke();
        assert!(!h.is_stroke_open());
    }

    #[test]
    fn strokes_committed_counts_both_grouped_and_bare_commits() {
        let mut h = EditHistory::default();
        assert_eq!(h.strokes_committed(), 0);

        // Grouped multi-op stroke counts as one.
        h.begin_stroke();
        h.record(op(0, 0, None, Some(1)));
        h.record(op(1, 0, None, Some(1)));
        h.end_stroke();
        assert_eq!(h.strokes_committed(), 1);

        // Empty strokes must NOT tick the counter — otherwise a
        // miss-click would wake the autosave system.
        h.begin_stroke();
        h.end_stroke();
        assert_eq!(h.strokes_committed(), 1);

        // Bare `record` outside a stroke commits a single-op stroke
        // and ticks the counter.
        h.record(op(2, 0, None, Some(1)));
        assert_eq!(h.strokes_committed(), 2);
    }

    #[test]
    fn strokes_committed_survives_clear() {
        let mut h = EditHistory::default();
        h.record(op(0, 0, None, Some(1)));
        h.record(op(1, 0, None, Some(2)));
        let before = h.strokes_committed();
        assert_eq!(before, 2);
        h.clear();
        // Autosave relies on this monotonicity — File → New must
        // not make the counter appear to go backwards.
        assert_eq!(h.strokes_committed(), before);
    }
}
