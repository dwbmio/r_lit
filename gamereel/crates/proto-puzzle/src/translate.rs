//! Mechanical translation from `wire::Replay` → `gamereel-core`'s
//! scene/timeline format.
//!
//! No game-logic re-simulation. Every node + action is derived
//! directly from one event. If a future event class can't be rendered
//! without recomputing match outcomes, that's a bug in the spec, not
//! something we patch over here.

use crate::wire::{Event, Replay};
use gamereel_core::stage::model::meta_action::MetaAction;
use gamereel_core::stage::model::meta_node::{MetaNode, NodeAttr};
use gamereel_core::stage::model::meta_scene::{MetaScene, MetaSceneList};
use std::collections::HashMap;

/// Render dimensions for the canvas; caller can override post-translate.
const DEFAULT_W: u32 = 720;
const DEFAULT_H: u32 = 1080;

/// Top-left of the board on the canvas. Vertical 9:16 layout puts
/// the board roughly centered horizontally with header above and
/// footer (score / combo banner) below.
const BOARD_ORIGIN_X: f32 = 36.0;   // (canvas_w - board_w) / 2 for 8x72=576
const BOARD_ORIGIN_Y: f32 = 240.0;

/// Texture id used for the static background. Caller's manifest MUST
/// have an entry for this id pointing at a 720x1080 PNG.
const BG_TP_ID: &str = "bg-puzzle";

/// Texture id prefix for blocks; full id = "block-<piece>".
fn block_tp_id(piece: &str) -> String { format!("block-{}", piece) }

/// Stable cell node id. Encodes (row, col) so the renderer's BTreeMap
/// keeps z-order deterministic.
fn cell_id(row: u32, col: u32) -> u64 {
    // 16 bits for row, 16 for col; rest reserved for non-cell nodes.
    (1u64 << 32) | ((row as u64) << 16) | (col as u64)
}

/// Reserved id space:
///   0xFFFF_FFFF__0000_0000  background
///   0xFFFF_FFFF__0000_0001  score node
///   0xFFFF_FFFF__0000_0002  combo banner
///   0xFFFF_FFFF__0000_0003  result overlay
const ID_BG: u64       = 0xFFFF_FFFF_0000_0000;
const ID_SCORE: u64    = 0xFFFF_FFFF_0000_0001;
const ID_COMBO: u64    = 0xFFFF_FFFF_0000_0002;
const ID_RESULT: u64   = 0xFFFF_FFFF_0000_0003;

/// Bottom of the cell-id range. Anything ≥ this is a "cell" node;
/// less is a UI / overlay node.
const CELLS_MIN_ID: u64 = 1u64 << 32;

pub struct Translation {
    pub scene_list: MetaSceneList,
    /// Sum of all timeline durations + final pause. Caller sets
    /// RuntimeCtx duration to at least this many seconds.
    pub duration_secs: f32,
    /// Cell pixel size used in layout — caller may want to know.
    pub cell_size_px: u32,
}

pub fn translate(replay: &Replay) -> Translation {
    let cell_px = replay.board.cell_size_px as f32;
    let mut nodes: Vec<MetaNode> = Vec::with_capacity((replay.board.rows * replay.board.cols) as usize + 8);
    let mut timeline: HashMap<String, Vec<MetaAction>> = HashMap::new();

    // ---- Background ----
    nodes.push(MetaNode::new_with_attr(
        ID_BG,
        BG_TP_ID,
        "bg",
        NodeAttr {
            pos: [0.0, 0.0, 0.0],
            anchor: Some([0.0, 0.0]),
            size: Some([DEFAULT_W as f32, DEFAULT_H as f32]),
            active: true,
            is_static: true,
            ..Default::default()
        },
    ));

    // ---- Initial board cells from `replay.init` ----
    for (row_idx, row) in replay.init.cells.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            if cell.piece.is_empty() { continue; }
            let id = cell_id(row_idx as u32, col_idx as u32);
            let pos = cell_pos(row_idx as u32, col_idx as u32, cell_px);
            nodes.push(MetaNode::new_with_attr(
                id,
                &block_tp_id(&cell.piece),
                &format!("cell-{row_idx}-{col_idx}"),
                NodeAttr {
                    pos: [pos.0, pos.1, 0.0],
                    anchor: Some([0.5, 0.5]),
                    size: Some([cell_px, cell_px]),
                    active: true,
                    is_static: false,
                    ..Default::default()
                },
            ));
        }
    }

    // ---- Walk events, emitting timeline entries ----
    for evt in &replay.events {
        let t = ms_to_s(evt.t());
        match evt {
            Event::BoardInit { .. } => {
                // Already materialized via replay.init above; no-op.
            }
            Event::Swap { duration_ms, from, to, .. } => {
                let dur = ms_to_s(*duration_ms);
                let id_a = cell_id(from[0], from[1]);
                let id_b = cell_id(to[0], to[1]);
                let pos_a = cell_pos(from[0], from[1], cell_px);
                let pos_b = cell_pos(to[0], to[1], cell_px);
                push_action(
                    &mut timeline, id_a,
                    MetaAction::new_move_to(id_a, [pos_a.0, pos_a.1, 0.0], [pos_b.0, pos_b.1, 0.0], t, dur),
                );
                push_action(
                    &mut timeline, id_b,
                    MetaAction::new_move_to(id_b, [pos_b.0, pos_b.1, 0.0], [pos_a.0, pos_a.1, 0.0], t, dur),
                );
            }
            Event::Match { cells, .. } => {
                // Hide cleared cells. (M0 spec: instant deactivate.
                // A real renderer might add a flash sub-event later.)
                for [r, c] in cells {
                    let id = cell_id(*r, *c);
                    push_action(&mut timeline, id, MetaAction::new_activate(id, t, false));
                }
            }
            Event::CascadeDrop { duration_ms, col, moves, .. } => {
                let dur = ms_to_s(*duration_ms);
                for m in moves {
                    let id = cell_id(m.from_row, *col);
                    let p_from = cell_pos(m.from_row, *col, cell_px);
                    let p_to   = cell_pos(m.to_row,   *col, cell_px);
                    push_action(
                        &mut timeline, id,
                        MetaAction::new_move_to(id,
                            [p_from.0, p_from.1, 0.0],
                            [p_to.0, p_to.1, 0.0],
                            t, dur),
                    );
                }
            }
            Event::CascadeSpawn { duration_ms, col, spawns, .. } => {
                let dur = ms_to_s(*duration_ms);
                for s in spawns {
                    // Spawn nodes appear with a fresh id space — but we
                    // re-use the cell_id slot since the original cell
                    // was cleared by a Match event. Renderer's
                    // BTreeMap accepts overwrite via insert.
                    let id = cell_id(s.to_row, *col);
                    let above_pos = cell_pos(0, *col, cell_px); // start above row 0
                    let final_pos = cell_pos(s.to_row, *col, cell_px);
                    nodes.push(MetaNode::new_with_attr(
                        id,
                        &block_tp_id(&s.piece),
                        &format!("cell-spawn-{}-{}-{}", evt.t(), s.to_row, col),
                        NodeAttr {
                            pos: [above_pos.0, above_pos.1 - cell_px, 0.0],
                            anchor: Some([0.5, 0.5]),
                            size: Some([cell_px, cell_px]),
                            active: false, // turn on at spawn time
                            is_static: false,
                            ..Default::default()
                        },
                    ));
                    push_action(&mut timeline, id, MetaAction::new_activate(id, t, true));
                    push_action(
                        &mut timeline, id,
                        MetaAction::new_move_to(id,
                            [above_pos.0, above_pos.1 - cell_px, 0.0],
                            [final_pos.0, final_pos.1, 0.0],
                            t, dur),
                    );
                }
            }
            Event::PowerClear { duration_ms, cells_cleared, .. } => {
                let _ = duration_ms; // VFX overlay coming in v0.2
                for [r, c] in cells_cleared {
                    let id = cell_id(*r, *c);
                    push_action(&mut timeline, id, MetaAction::new_activate(id, t, false));
                }
            }
            Event::ScoreChange { .. } | Event::Combo { .. } => {
                // v0: no UI nodes yet (text rendering is M5 territory).
                // Reserved IDs ID_SCORE / ID_COMBO will host these
                // when the text renderer lands. Until then, score and
                // combo events are silently dropped.
            }
            Event::TimePause { .. } => {
                // No node-level work; renderer's frame pump treats
                // gaps as "show last frame".
            }
            Event::MatchEnd { .. } => {
                // v0: no result overlay. ID_RESULT reserved for v0.2.
            }
        }
    }

    let scene = MetaScene {
        name: "match3-replay".into(),
        clear_tp_id: Some(BG_TP_ID.into()),
        textures: vec![],   // caller resolves via manifest
        nodes,
        timeline,
    };
    let _ = (ID_SCORE, ID_COMBO, ID_RESULT, CELLS_MIN_ID); // silence unused-const warnings

    Translation {
        scene_list: MetaSceneList { meta_scene_list: vec![scene] },
        duration_secs: ms_to_s(replay.duration_ms),
        cell_size_px: replay.board.cell_size_px,
    }
}

fn ms_to_s(ms: u32) -> f32 { ms as f32 / 1000.0 }

fn cell_pos(row: u32, col: u32, cell_px: f32) -> (f32, f32) {
    // Anchor is centered (0.5, 0.5) so add half-cell offset.
    (
        BOARD_ORIGIN_X + col as f32 * cell_px + cell_px / 2.0,
        BOARD_ORIGIN_Y + row as f32 * cell_px + cell_px / 2.0,
    )
}

fn push_action(map: &mut HashMap<String, Vec<MetaAction>>, node_id: u64, action: MetaAction) {
    map.entry(node_id.to_string()).or_default().push(action);
}
