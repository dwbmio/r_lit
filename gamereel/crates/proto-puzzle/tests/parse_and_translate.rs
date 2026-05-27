//! S1-2 self-proof: proto-puzzle decodes mock JSON bytes and produces
//! a Scene whose shape matches what the spec promises.

use proto_puzzle::{mock_replay, translate_to_scene, PuzzleParser};
use gamereel_core::protocol::ProtocolParser;

#[test]
fn mock_replay_round_trips_through_json() {
    let mock = mock_replay();
    let bytes = serde_json::to_vec(&mock).expect("serialize");
    let parsed = PuzzleParser
        .parse(&bytes)
        .expect("parse");
    assert_eq!(parsed.metadata["match_id"], "mock-match-001");
    assert_eq!(parsed.metadata["events_count"], mock.events.len());
    // 4 s replay × 30 fps = 120 frames
    assert_eq!(parsed.frames, 120);
    assert!(parsed.suggested_filename.starts_with("puzzle_"));
}

#[test]
fn translation_produces_expected_scene_shape() {
    let mock = mock_replay();
    let t = translate_to_scene(&mock);
    let scene = &t.scene_list.meta_scene_list[0];

    // 1 background + 64 cells = 65 nodes initially. Cascade spawn
    // appends one more (a re-spawned cell) but reuses the cell id.
    // Vec<MetaNode> stores the spawn separately; total 66.
    assert!(
        scene.nodes.len() >= 65,
        "expected ≥ 65 nodes, got {}",
        scene.nodes.len()
    );

    // Background must be marked static + active.
    let bg = scene
        .nodes
        .iter()
        .find(|n| n.name == "bg")
        .expect("bg node");
    assert!(bg.attr.is_static);
    assert!(bg.attr.active);
    assert_eq!(bg.attr.size, Some([720.0, 1080.0]));

    // Timeline must reference at least the cells affected by mock
    // events (rows 3 col 4-6, row 5 col 2-4, the cascade).
    let touched = ["3-4", "3-5", "3-6", "5-2", "5-3", "5-4"];
    for cell in touched {
        let any = scene
            .nodes
            .iter()
            .any(|n| n.name.starts_with(&format!("cell-{cell}")));
        assert!(any, "missing cell node for cell-{cell}");
    }

    // Duration must round to ≥ 4 s.
    assert!(t.duration_secs >= 4.0);
    assert_eq!(t.cell_size_px, 72);
}

#[test]
fn parse_rejects_bad_json_with_helpful_error() {
    let res = PuzzleParser.parse(b"not-json");
    let err = match res {
        Ok(_) => panic!("expected error"),
        Err(e) => format!("{e}"),
    };
    assert!(err.contains("proto-puzzle decode"), "got {err}");
}
