//! S1-4 closed loop:
//!   proto-puzzle bytes → decode → render an actual scene via
//!   gamereel-farm::LocalWorker → upload via LocalDiskSink → assert
//!   receipt URL points at a real MP4.
//!
//! Uses the hs-mvp scene file as the actual rendering target (because
//! that's the only scene we have textures for today). The proto-puzzle
//! bytes are decoded for metadata only — proving the parser is wired
//! into the loop. Once a real game-side caller produces a scene.meta
//! whose texture ids resolve, swap the scene path here.
//!
//! Marked `#[ignore]` (CUDA hardware required):
//!   cargo test --release -p gamereel-output --test e2e_render_to_sink \
//!     -- --ignored --nocapture --test-threads=1

use gamereel_farm::worker::local::LocalWorker;
use gamereel_farm::worker::Worker;
use gamereel_farm::{JobPriority, RenderJob};
use gamereel_output::{LocalDiskSink, OutputSink};
use proto_puzzle::{mock_replay, PuzzleParser};
use gamereel_core::protocol::ProtocolParser;
use std::path::PathBuf;
use std::time::Instant;

fn hs_mvp_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/hs-mvp")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn proto_bytes_through_render_through_sink() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .is_test(true)
        .try_init();

    // ---- 1) Encode + decode mock proto-puzzle bytes ----
    let mock = mock_replay();
    let bytes = serde_json::to_vec(&mock).expect("serialize");
    let parsed = PuzzleParser.parse(&bytes).expect("parse mock");
    let job_id = format!("e2e-{}", parsed.suggested_filename);
    println!("decoded replay: id={job_id} frames={} events_count={}",
             parsed.frames, parsed.metadata["events_count"]);

    // ---- 2) Render via LocalWorker on the hs-mvp scene ----
    let total_t = Instant::now();
    let mut worker = LocalWorker::new("e2e-worker", 720, 1080)
        .expect("LocalWorker::new — need NVIDIA + libnvrtc");
    println!("worker init: {} ms", worker.init_wall_ms());

    let dir = tempfile::tempdir().expect("tmpdir");
    let render_out = dir.path().join("rendered.mp4");
    let job = RenderJob {
        id: job_id.clone(),
        scene_meta_path: hs_mvp_root().join("tests/hs-proj/scene.meta"),
        output_path: render_out.clone(),
        source_root: Some(hs_mvp_root()),
        width: Some(720),
        height: Some(1080),
        fps: Some(30),
        duration_s: Some(10),
        priority: JobPriority::Normal,
        tag: parsed.metadata.clone(),
    };
    let render_result = worker.render(job).await.expect("render");
    println!(
        "render: ok={} bytes={} wall={}ms render_loop={}ms",
        render_result.ok, render_result.output_bytes,
        render_result.wall.as_millis(), render_result.render_loop.as_millis(),
    );
    assert!(render_result.ok);
    assert!(render_result.output_bytes > 4096, "render output suspiciously small");
    assert_eq!(render_result.tag["match_id"], "mock-match-001");

    // ---- 3) Upload via LocalDiskSink ----
    let sink_root = dir.path().join("sink");
    let sink = LocalDiskSink::new(&sink_root);
    let mp4_bytes = std::fs::read(&render_out).expect("read render output");
    let receipt = sink.deliver(&render_result, &mp4_bytes).await.expect("upload");

    println!("delivered to {} ({} bytes)", receipt.location, receipt.bytes);
    assert_eq!(receipt.sink, "local_disk");
    assert_eq!(receipt.bytes, mp4_bytes.len() as u64);
    assert_eq!(receipt.job_id, job_id);

    // Final invariant: the URL the receipt promises is a real file
    // with the same bytes we encoded.
    let url_path = std::path::PathBuf::from(&receipt.location);
    assert!(url_path.exists());
    assert_eq!(std::fs::metadata(&url_path).unwrap().len(), mp4_bytes.len() as u64);

    println!("\ne2e total wall: {} ms (decode + render + upload)", total_t.elapsed().as_millis());
}
