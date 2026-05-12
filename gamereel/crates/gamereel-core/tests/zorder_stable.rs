//! M1 unit test: scene rendering is bit-deterministic across runs.
//!
//! Pre-M1, `Scene::children` was a `HashMap<u64, NodeGraph>`. HashMap
//! iteration order is unspecified and (with HashDoS protection) randomized
//! per process; this means the order in which nodes were composited onto
//! the framebuffer changed between runs, producing visually identical but
//! byte-divergent output. Because z-order matters when sprites overlap,
//! that randomness was a latent rendering bug as well as an obstacle to
//! regression testing.
//!
//! M1 changed `children` to `BTreeMap<u64, _>`, so iteration is sorted
//! ascending by node ID. This test validates two things:
//!   1. Two independent renders of the same scene produce per-frame
//!      RGBA byte buffers with identical SHA-256 digests.
//!   2. The number of unique frame digests across the timeline is > 1
//!      (otherwise we'd be falsely passing on a black/empty render).

use gamereel_core::ffmpeg_inc::stage_mgr::StageMgr;
use gamereel_core::stage;
use gamereel_core::RuntimeCtx;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const FRAMES_TO_HASH: u32 = 30; // 1 second at 30 fps

fn render_frame_hashes() -> Vec<[u8; 32]> {
    // Build a fresh runtime per call so any process-local state (e.g. RNGs
    // that previously perturbed HashMap ordering) starts identically.
    let project_root = env!("CARGO_MANIFEST_DIR");
    let scene_meta_path = PathBuf::from(project_root).join("tests/perf_main/scene.meta");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");

    rt.block_on(async {
        let scene_list = stage::import_scene(scene_meta_path)
            .await
            .expect("import scene");

        let mut rtx = RuntimeCtx::new(720, 1080, 1, 30);
        rtx.init(Some(PathBuf::from(project_root)))
            .expect("ffmpeg init");

        let mut stage_mgr = StageMgr::new(scene_list);
        stage_mgr.meta_scene_preload(&mut rtx, 0).expect("preload");

        let scene = stage_mgr
            .scenes
            .values_mut()
            .next()
            .expect("at least one scene");
        scene.on_init(&rtx);

        let mut hashes = Vec::with_capacity(FRAMES_TO_HASH as usize);
        for f in 0..FRAMES_TO_HASH {
            let img = scene
                .on_render(&mut rtx, f as f32 / 30.0)
                .expect("render frame");
            let rgba = img.to_rgba8();
            let mut h = Sha256::new();
            h.update(rgba.as_raw());
            let mut out = [0u8; 32];
            out.copy_from_slice(&h.finalize());
            hashes.push(out);
        }
        hashes
    })
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for byte in b {
        use std::fmt::Write;
        write!(&mut s, "{byte:02x}").unwrap();
    }
    s
}

#[test]
fn two_independent_renders_produce_identical_frame_hashes() {
    let a = render_frame_hashes();
    let b = render_frame_hashes();

    assert_eq!(a.len(), FRAMES_TO_HASH as usize);
    assert_eq!(b.len(), FRAMES_TO_HASH as usize);

    for (i, (ha, hb)) in a.iter().zip(b.iter()).enumerate() {
        assert_eq!(
            ha,
            hb,
            "frame {i} divergent: run-a={} vs run-b={} (BTreeMap iteration must be deterministic)",
            hex(ha),
            hex(hb),
        );
    }
}

#[test]
fn rendered_frames_actually_change_over_time() {
    // Sanity check: if every frame had the same hash, the determinism
    // assertion above could trivially pass even on a broken renderer.
    let hashes = render_frame_hashes();
    let unique: std::collections::BTreeSet<_> = hashes.iter().collect();
    assert!(
        unique.len() > 1,
        "scene produced only {} unique frame digest(s) across {FRAMES_TO_HASH} frames \
         — render is stuck or scene has no animation",
        unique.len()
    );
}
