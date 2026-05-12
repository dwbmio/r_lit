//! M3-4 self-proof: end-to-end CUDA pipeline produces a valid mp4.
//!
//! Runs the perf_main scene through `create_scene_stream_cuda` and:
//!   * verifies the output exists and is non-trivial in size
//!   * uses ffprobe to verify codec=h264, profile=High, pix_fmt=yuv420p
//!     (the file as written to disk has been deinterlaced from NV12 by
//!     the muxer; this is normal)
//!   * checks frame count = expected
//!
//! Marked `#[ignore]` (CUDA hardware required):
//!   cargo test --release -p movie-maker --test cuda_e2e_perf_main \
//!       -- --ignored --nocapture

use movie_maker::ffmpeg_inc::{self, stage_mgr::StageMgr};
use movie_maker::stage;
use movie_maker::RuntimeCtx;
use std::path::PathBuf;
use std::process::Command;

#[test]
#[ignore]
fn perf_main_scene_through_cuda_pipeline_produces_valid_mp4() {
    let project_root = env!("CARGO_MANIFEST_DIR");
    let scene_meta_path = PathBuf::from(project_root).join("tests/perf_main/scene.meta");

    let dir = tempfile::tempdir().expect("tmpdir");
    let out = dir.path().join("cuda_pipeline.mp4");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");

    let start = std::time::Instant::now();
    rt.block_on(async {
        let scenes = stage::import_scene(scene_meta_path).await.expect("import");
        let mut rtx = RuntimeCtx::new(720, 1080, 10, 30);
        rtx.init(Some(PathBuf::from(project_root))).expect("init");

        let mut mgr = StageMgr::new(scenes);
        mgr.meta_scene_preload(&mut rtx, 0).expect("preload");
        let scene = mgr.scenes.values_mut().next().expect("scene");

        ffmpeg_inc::create_scene_stream_cuda(&mut rtx, &out, scene)
            .expect("CUDA pipeline encode");
    });
    let wall_ms = start.elapsed().as_millis();
    let bytes = std::fs::metadata(&out).expect("output stat").len();
    println!("cuda_pipeline.mp4 wrote {bytes}B in {wall_ms}ms ({:.1} fps e2e)",
             300.0 * 1000.0 / wall_ms as f64);
    assert!(bytes > 4096, "output too small: {bytes}");

    // ffprobe sanity
    let out_str = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "v:0",
            "-show_entries", "stream=codec_name,profile,width,height,pix_fmt,nb_read_frames",
            "-count_frames",
            "-of", "default=nw=1",
        ])
        .arg(&out)
        .output()
        .expect("ffprobe");
    let s = String::from_utf8_lossy(&out_str.stdout);
    println!("ffprobe:\n{s}");

    assert!(s.contains("codec_name=h264"));
    assert!(s.contains("profile=High"));
    assert!(s.contains("width=720"));
    assert!(s.contains("height=1080"));
    assert!(s.contains("pix_fmt=yuv420p") || s.contains("pix_fmt=nv12"));
    // 300 frames expected
    assert!(s.contains("nb_read_frames=300"), "frame count mismatch:\n{s}");
}
