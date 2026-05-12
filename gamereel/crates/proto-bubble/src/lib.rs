//! Skeleton parser for bubble-shooter game protocols. See
//! `proto-puzzle/src/lib.rs` for the rationale; this crate's structure
//! mirrors it exactly so the CLI end-to-end test can prove the
//! inventory mechanism scales beyond a single crate.

use gamereel_core::error::GamereelError;
use gamereel_core::protocol::{ParsedReplay, ProtocolDescriptor, ProtocolParser};

pub struct BubbleParser;

impl ProtocolParser for BubbleParser {
    fn name(&self) -> &'static str { "bubble" }
    fn description(&self) -> &'static str {
        "bubble (bubble-shooter style games) — skeleton, real decoder TBD"
    }
    fn parse(&self, msg: &[u8]) -> Result<ParsedReplay, GamereelError> {
        Ok(ParsedReplay {
            suggested_filename: "bubble_replay".into(),
            metadata: serde_json::json!({
                "parser": "bubble",
                "input_bytes": msg.len(),
                "note": "skeleton — real decoder pending"
            }),
            frames: 300,
        })
    }
}

inventory::submit! {
    ProtocolDescriptor {
        name: "bubble",
        description: "bubble (bubble-shooter style games)",
        factory: || Box::new(BubbleParser),
    }
}
