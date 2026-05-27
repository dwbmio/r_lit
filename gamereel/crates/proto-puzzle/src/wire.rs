//! v0 wire types matching `docs/protocols/match3-replay-spec.md`.
//!
//! v0 = serde-json. v1 will swap in protobuf-generated types via
//! `prost`; the Scene-translation code in `translate.rs` only sees
//! these structs so it survives the format swap.

use serde::{Deserialize, Serialize};

/// Top-level replay envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replay {
    /// Schema version. v0 docs/protocols/match3-replay-spec.md.
    pub version: u32,
    pub match_id: String,
    pub player: Player,
    pub board: Board,
    pub duration_ms: u32,
    pub init: BoardInit,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: String,
    pub name: String,
    /// Texture id; renderer-side manifest maps to a file path.
    #[serde(default)]
    pub avatar_tp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Board {
    pub rows: u32,
    pub cols: u32,
    pub cell_size_px: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardInit {
    /// 2-D matrix [row][col] of starting pieces.
    pub cells: Vec<Vec<Cell>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cell {
    /// Logical piece id. Renderer-side manifest maps `"red"` →
    /// texture path. Empty string = empty cell (after a clear,
    /// before a cascade fills it).
    pub piece: String,
}

/// Discriminated union of every replay event variant. Tagged on
/// `kind` per spec; `t` is ms since match start.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Event {
    #[serde(rename = "board_init")]
    BoardInit { t: u32, cells: Vec<Vec<Cell>> },

    #[serde(rename = "swap")]
    Swap {
        t: u32,
        duration_ms: u32,
        from: [u32; 2], // [row, col]
        to: [u32; 2],
    },

    #[serde(rename = "match")]
    Match {
        t: u32,
        cells: Vec<[u32; 2]>,
        match_type: String,
        score_gain: u32,
    },

    #[serde(rename = "cascade_drop")]
    CascadeDrop {
        t: u32,
        duration_ms: u32,
        col: u32,
        moves: Vec<DropMove>,
    },

    #[serde(rename = "cascade_spawn")]
    CascadeSpawn {
        t: u32,
        duration_ms: u32,
        col: u32,
        spawns: Vec<SpawnEntry>,
    },

    #[serde(rename = "power_clear")]
    PowerClear {
        t: u32,
        duration_ms: u32,
        effect_id: String,
        origin: [u32; 2],
        cells_cleared: Vec<[u32; 2]>,
    },

    #[serde(rename = "score_change")]
    ScoreChange { t: u32, duration_ms: u32, from: u32, to: u32 },

    #[serde(rename = "combo")]
    Combo { t: u32, duration_ms: u32, count: u32, multiplier: f32 },

    #[serde(rename = "time_pause")]
    TimePause { t: u32, duration_ms: u32 },

    #[serde(rename = "match_end")]
    MatchEnd {
        t: u32,
        result: String, // "win" | "lose" | "timeout" | "abort"
        final_score: u32,
        #[serde(default)]
        stats: serde_json::Value,
    },
}

impl Event {
    /// Convenience accessor — every variant has a `t`.
    pub fn t(&self) -> u32 {
        match self {
            Event::BoardInit { t, .. }
            | Event::Swap { t, .. }
            | Event::Match { t, .. }
            | Event::CascadeDrop { t, .. }
            | Event::CascadeSpawn { t, .. }
            | Event::PowerClear { t, .. }
            | Event::ScoreChange { t, .. }
            | Event::Combo { t, .. }
            | Event::TimePause { t, .. }
            | Event::MatchEnd { t, .. } => *t,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropMove {
    pub from_row: u32,
    pub to_row: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnEntry {
    pub to_row: u32,
    pub piece: String,
}
