//! `proto-puzzle` — match-3 replay parser for the gamereel pipeline.
//!
//! v0 implementation (per `docs/protocols/match3-replay-spec.md`):
//!   * wire format: serde-json (lets us iterate the schema before
//!     committing to protobuf in v1)
//!   * decode: `serde_json::from_slice` → `wire::Replay`
//!   * translate: mechanical event-stream → `MetaSceneList` translation
//!     in `translate.rs`. No game-logic re-simulation.
//!   * register: `inventory::submit!` so `gamereel-cli` discovers the
//!     parser at link time.
//!
//! Public mock helper [`mock_replay`] generates a minimal valid replay
//! for tests / e2e plumbing without needing real game-server bytes.

pub mod translate;
pub mod wire;

use gamereel_core::error::GamereelError;
use gamereel_core::protocol::{ParsedReplay, ProtocolDescriptor, ProtocolParser};

pub struct PuzzleParser;

impl ProtocolParser for PuzzleParser {
    fn name(&self) -> &'static str { "puzzle" }
    fn description(&self) -> &'static str {
        "match-3 / 方块消除 — v0 wire format JSON, see docs/protocols/match3-replay-spec.md"
    }
    fn parse(&self, msg: &[u8]) -> Result<ParsedReplay, GamereelError> {
        let replay: wire::Replay = serde_json::from_slice(msg).map_err(|e| {
            GamereelError::CustomError(format!("proto-puzzle decode: {e}"))
        })?;
        let translation = translate::translate(&replay);
        let frames = (translation.duration_secs * 30.0).ceil() as u32;
        Ok(ParsedReplay {
            suggested_filename: format!("puzzle_{}", sanitize_id(&replay.match_id)),
            metadata: serde_json::json!({
                "parser": "puzzle",
                "schema_version": replay.version,
                "match_id": replay.match_id,
                "player": { "id": replay.player.id, "name": replay.player.name },
                "duration_ms": replay.duration_ms,
                "events_count": replay.events.len(),
                "board": { "rows": replay.board.rows, "cols": replay.board.cols },
            }),
            frames,
        })
    }
}

inventory::submit! {
    ProtocolDescriptor {
        name: "puzzle",
        description: "match-3 / 方块消除 (json wire format v0)",
        factory: || Box::new(PuzzleParser),
    }
}

/// Translate a decoded replay into the gamereel-core scene/timeline.
/// Re-exported for callers that already deserialized a `wire::Replay`
/// from a different source (test harness, integration tests, mocks).
pub fn translate_to_scene(replay: &wire::Replay) -> translate::Translation {
    translate::translate(replay)
}

/// Minimal valid replay for tests and gamereel-cli dry-runs.
/// 8x8 board, 2 swaps + matches, 1 cascade pair, 1 score change,
/// pause + match_end. duration ≈ 4 seconds.
pub fn mock_replay() -> wire::Replay {
    use wire::*;
    let cells: Vec<Vec<Cell>> = (0..8u32)
        .map(|r| {
            (0..8u32)
                .map(|c| Cell {
                    piece: ["red", "blue", "green", "yellow", "purple"][((r + c) as usize) % 5].into(),
                })
                .collect()
        })
        .collect();
    Replay {
        version: 1,
        match_id: "mock-match-001".into(),
        player: Player {
            id: "u-mock".into(),
            name: "MockPlayer".into(),
            avatar_tp: Some("avatar-default".into()),
        },
        board: Board { rows: 8, cols: 8, cell_size_px: 72 },
        duration_ms: 4000,
        init: BoardInit { cells: cells.clone() },
        events: vec![
            Event::Swap { t: 200, duration_ms: 200, from: [3, 4], to: [3, 5] },
            Event::Match {
                t: 420, cells: vec![[3, 4], [3, 5], [3, 6]],
                match_type: "horizontal_3".into(), score_gain: 30,
            },
            Event::CascadeDrop {
                t: 720, duration_ms: 180, col: 4,
                moves: vec![DropMove { from_row: 2, to_row: 3 }],
            },
            Event::CascadeSpawn {
                t: 920, duration_ms: 180, col: 4,
                spawns: vec![SpawnEntry { to_row: 0, piece: "purple".into() }],
            },
            Event::ScoreChange { t: 420, duration_ms: 400, from: 0, to: 30 },
            Event::Swap { t: 1200, duration_ms: 200, from: [5, 2], to: [5, 3] },
            Event::Match {
                t: 1420, cells: vec![[5, 2], [5, 3], [5, 4]],
                match_type: "horizontal_3".into(), score_gain: 30,
            },
            Event::TimePause { t: 1700, duration_ms: 300 },
            Event::MatchEnd {
                t: 4000, result: "win".into(), final_score: 60,
                stats: serde_json::json!({"matches": 2, "combos": 0}),
            },
        ],
    }
}

fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}
