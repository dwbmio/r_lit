//! M4-4 parity test: wgpu compositor produces output close enough to
//! CPU image_effect for the same hs-mvp scene state.
//!
//! "Close enough" = mean absolute pixel difference < 8/255 across 30
//! frames, AND > 99% of pixels within ±16 levels per channel. We don't
//! enforce strict SSIM here because:
//!   * CPU path uses image_effect.rs's pure-Rust alpha blend math.
//!   * wgpu fragment shader uses GPU bilinear sampling and IEEE-754
//!     blend; the rounding diverges at low bits.
//!   * Both produce visually identical output but bit-different bytes.
//!
//! The test fails LOUDLY if the wgpu path produces visibly wrong
//! output (e.g. wrong z-order, missing layer, wrong scaling).

use gamereel_compositor::{compose_scene_frame, upload_scene_textures, WgpuCompositor};
use gamereel_core::ffmpeg_inc::stage_mgr::StageMgr;
use gamereel_core::stage::model::meta_scene::MetaSceneList;
use gamereel_core::RuntimeCtx;
use std::path::PathBuf;

#[path = "../src/nodes/mod.rs"]
mod nodes;
#[path = "../src/report/mod.rs"]
mod report;

fn project_root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")) }

fn build_scene_mgr() -> (RuntimeCtx, StageMgr) {
    let bytes = std::fs::read(project_root().join("tests/hs-proj/scene.meta")).expect("read meta");
    let scene_meta: MetaSceneList = serde_json::from_slice(&bytes).expect("parse meta");

    let mut rtx = RuntimeCtx::new(720, 1080, 10, 30);
    rtx.set_source_path(project_root());
    let report = report::Report::new();
    report.gen_report_dynamic_images(&mut rtx);
    let mut mgr = StageMgr::new(scene_meta);
    report.gen_nodes(&mut rtx, &mut mgr.scenes_meta.meta_scene_list[0]);
    mgr.meta_scene_preload(&mut rtx, 0).expect("preload");
    (rtx, mgr)
}

fn rgba_from_dynamic(img: &image::DynamicImage) -> Vec<u8> {
    img.to_rgba8().into_raw()
}

#[test]
fn wgpu_matches_cpu_within_pixel_tolerance() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .is_test(true)
        .try_init();

    // -------- CPU path --------
    let (mut rtx_cpu, mut mgr_cpu) = build_scene_mgr();
    let scene_cpu = mgr_cpu.scenes.values_mut().next().expect("scene");
    scene_cpu.on_init(&rtx_cpu);
    let mut cpu_frames: Vec<Vec<u8>> = Vec::with_capacity(30);
    for f in 0..30u32 {
        let img = scene_cpu.on_render(&mut rtx_cpu, f as f32 / 30.0).expect("cpu render");
        cpu_frames.push(rgba_from_dynamic(&img));
    }

    // -------- wgpu path --------
    let (mut rtx_gpu, mut mgr_gpu) = build_scene_mgr();
    let scene_gpu = mgr_gpu.scenes.values_mut().next().expect("scene");
    scene_gpu.on_init(&rtx_gpu);

    let mut compositor = WgpuCompositor::new(720, 1080).expect("WgpuCompositor::new");
    upload_scene_textures(&mut compositor, scene_gpu, &rtx_gpu).expect("upload");

    let mut gpu_frames: Vec<Vec<u8>> = Vec::with_capacity(30);
    for f in 0..30u32 {
        let bytes = compose_scene_frame(
            &mut compositor,
            scene_gpu,
            &mut rtx_gpu,
            f as f32 / 30.0,
        )
        .expect("gpu compose");
        gpu_frames.push(bytes);
    }

    // -------- Compare --------
    assert_eq!(cpu_frames.len(), gpu_frames.len());
    let mut total_abs_diff: u64 = 0;
    let mut total_samples: u64 = 0;
    let mut over_16_per_frame: Vec<f64> = Vec::with_capacity(30);

    for (i, (cpu, gpu)) in cpu_frames.iter().zip(gpu_frames.iter()).enumerate() {
        assert_eq!(cpu.len(), gpu.len(),
            "frame {i}: lengths differ ({} vs {})", cpu.len(), gpu.len());
        let mut over_16 = 0u64;
        let mut frame_abs: u64 = 0;
        for (a, b) in cpu.iter().zip(gpu.iter()) {
            let d = (*a as i32 - *b as i32).unsigned_abs() as u8;
            frame_abs += d as u64;
            if d > 16 { over_16 += 1; }
        }
        total_abs_diff += frame_abs;
        total_samples += cpu.len() as u64;
        let pct_over = 100.0 * over_16 as f64 / cpu.len() as f64;
        over_16_per_frame.push(pct_over);
        if i < 3 || i == 29 {
            println!(
                "  frame {i:>2}: mean |diff| = {:.3}, samples >16 = {} ({:.3}%)",
                frame_abs as f64 / cpu.len() as f64,
                over_16,
                pct_over,
            );
        }
    }

    let mean_abs = total_abs_diff as f64 / total_samples as f64;
    let mean_over_16 =
        over_16_per_frame.iter().sum::<f64>() / over_16_per_frame.len() as f64;
    println!();
    println!("=== overall (30 frames, 720x1080 RGBA) ===");
    println!("  mean |diff| per channel:        {mean_abs:.3} / 255");
    println!("  mean % samples > 16 levels off: {mean_over_16:.3}%");

    // Tolerance: mean diff must be small (visually invisible);
    // outlier pixels (often anti-aliased edges) allowed up to 1%.
    assert!(
        mean_abs < 8.0,
        "wgpu mean per-channel diff {mean_abs:.3} > 8 — gamma / blend math diverged"
    );
    assert!(
        mean_over_16 < 1.0,
        "wgpu has {mean_over_16:.2}% pixels diverging by > 16 levels — \
         likely a wrong z-order or layer is missing"
    );
}
