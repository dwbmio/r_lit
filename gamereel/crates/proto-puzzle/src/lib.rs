//! Skeleton parser for puzzle-style game protocols (block matching,
//! falling pieces). Real protocol decoding lives in a follow-up commit;
//! this crate exists today to:
//!   1. Prove the inventory plug-in path works end-to-end.
//!   2. Reserve the namespace + Cargo entry so the rename PR is
//!      structurally complete.
//!
//! Adding new code here does NOT require modifying gamereel-core or any
//! consuming app — just edit this file and `cargo build`.

use gamereel_core::error::GamereelError;
use gamereel_core::protocol::{ParsedReplay, ProtocolDescriptor, ProtocolParser};

pub struct PuzzleParser;

impl ProtocolParser for PuzzleParser {
    fn name(&self) -> &'static str { "puzzle" }
    fn description(&self) -> &'static str {
        "puzzle (block-matching / falling-piece games) — skeleton, real decoder TBD"
    }
    fn parse(&self, msg: &[u8]) -> Result<ParsedReplay, GamereelError> {
        // Skeleton: produce a deterministic placeholder replay so the
        // CLI end-to-end path can run without a real binary protocol.
        Ok(ParsedReplay {
            suggested_filename: "puzzle_replay".into(),
            metadata: serde_json::json!({
                "parser": "puzzle",
                "input_bytes": msg.len(),
                "note": "skeleton — real decoder pending"
            }),
            frames: 300, // 10 s @ 30 fps placeholder
        })
    }
}

inventory::submit! {
    ProtocolDescriptor {
        name: "puzzle",
        description: "puzzle (block-matching / falling-piece games)",
        factory: || Box::new(PuzzleParser),
    }
}
