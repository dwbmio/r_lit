//! M3-5 leak test: 100 sequential CUDA-pipeline encodes do not leak GPU
//! memory.
//!
//! Runs perf_main scene through `create_scene_stream_cuda` 100 times in
//! the same process. Each iteration constructs a fresh `CudaConverter`
//! and `CudaHwContext`, encodes 300 frames, drops both. After 100 cycles
//! the resident VRAM delta must be < 200 MB — anything more indicates a
//! frame ref that ffmpeg never released, a cudarc buffer that escaped its
//! Drop, or pool growth past the configured size.
//!
//! Reads VRAM via `nvidia-smi` (subprocess) because nvml-wrapper would
//! pull in another binding crate just for this one test.
//!
//! Marked `#[ignore]` (long, GPU-required):
//!   cargo test --release -p gamereel-core --test cuda_vram_leak \
//!       -- --ignored --nocapture --test-threads=1

use gamereel_core::ffmpeg_inc::{self, stage_mgr::StageMgr};
use gamereel_core::stage;
use gamereel_core::RuntimeCtx;
use std::path::PathBuf;
use std::process::Command;

const ITERATIONS: usize = 100;
const LEAK_THRESHOLD_MB: i64 = 200;

fn vram_used_mb() -> i64 {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=memory.used",
            "--format=csv,noheader,nounits",
            "-i", "0",
        ])
        .output()
        .expect("nvidia-smi");
    let s = String::from_utf8_lossy(&out.stdout);
    s.trim().parse::<i64>().unwrap_or(-1)
}

fn run_one_encode(rt: &tokio::runtime::Runtime, project_root: &str, out: &PathBuf) {
    rt.block_on(async {
        let scenes = stage::import_scene(
            PathBuf::from(project_root).join("tests/perf_main/scene.meta"),
        )
        .await
        .expect("import");
        let mut rtx = RuntimeCtx::new(720, 1080, 10, 30);
        rtx.init(Some(PathBuf::from(project_root))).expect("init");

        let mut mgr = StageMgr::new(scenes);
        mgr.meta_scene_preload(&mut rtx, 0).expect("preload");
        let scene = mgr.scenes.values_mut().next().expect("scene");

        ffmpeg_inc::create_scene_stream_cuda(&mut rtx, out, scene)
            .expect("CUDA pipeline encode");
    });
}

#[test]
#[ignore]
fn no_vram_leak_across_100_encodes() {
    let project_root = env!("CARGO_MANIFEST_DIR");
    let dir = tempfile::tempdir().expect("tmpdir");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");

    // Warmup encode — establishes baseline after CUDA + ffmpeg context init.
    run_one_encode(&rt, project_root, &dir.path().join("warmup.mp4"));

    let baseline_mb = vram_used_mb();
    println!("VRAM baseline after warmup: {baseline_mb} MB");

    let mut samples = Vec::with_capacity(11);
    samples.push((0usize, baseline_mb));

    for i in 1..=ITERATIONS {
        run_one_encode(&rt, project_root, &dir.path().join(format!("iter_{i}.mp4")));
        if i % 10 == 0 {
            let now_mb = vram_used_mb();
            samples.push((i, now_mb));
            println!("  after iter {i:>3}: {now_mb} MB (Δ {} MB from baseline)",
                     now_mb - baseline_mb);
        }
    }

    let final_mb = vram_used_mb();
    let delta = final_mb - baseline_mb;
    println!();
    println!("Final VRAM: {final_mb} MB (Δ {delta} MB after {ITERATIONS} encodes)");
    println!("Threshold:  Δ < {LEAK_THRESHOLD_MB} MB");

    assert!(
        delta < LEAK_THRESHOLD_MB,
        "VRAM grew by {delta} MB across {ITERATIONS} encodes (threshold {LEAK_THRESHOLD_MB} MB) — \
         a frame ref or buffer is escaping cleanup. Samples: {samples:?}"
    );
}
