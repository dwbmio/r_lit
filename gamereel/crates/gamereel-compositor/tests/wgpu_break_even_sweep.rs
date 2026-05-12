//! Find the exact break-even point: at what N of full-screen overlapping
//! sprites does wgpu start beating CPU image_effect?
//!
//! Output is a small table the operator can map to their own scene
//! complexity. Runs as a regular `cargo test --release` because it
//! needs a wgpu device and produces useful data each run.

use gamereel_compositor::{SpriteDraw, WgpuCompositor};
use gamereel_core::ffmpeg_inc::image_effect::blend_images;
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use std::time::Instant;

const W: u32 = 720;
const H: u32 = 1080;
const FRAMES: u32 = 30;

fn make_sprite(seed: usize) -> DynamicImage {
    let mut img = RgbaImage::new(W, H);
    // Compute everything in u32 to dodge debug-mode u8 overflow panics
    // (release mode wraps silently — test passed there but debug
    // tripped the overflow check on `seed as u8 * 50` for seed ≥ 6).
    let tint = ((50 + seed * 35) % 256) as u8;
    let alpha = (90u32.saturating_sub(seed as u32 * 5)).max(40) as u8;
    let blue = ((seed * 50) % 256) as u8;
    for p in img.pixels_mut() {
        *p = Rgba([tint, 255 - tint, blue, alpha]);
    }
    DynamicImage::ImageRgba8(img)
}

fn cpu_run(overlays: &[DynamicImage]) -> u128 {
    let mut base = DynamicImage::ImageRgba8(RgbaImage::new(W, H));
    let t = Instant::now();
    for f in 0..FRAMES {
        for p in base.as_mut_rgba8().unwrap().pixels_mut() {
            *p = Rgba([20, 20, 30, 255]);
        }
        for (i, ov) in overlays.iter().enumerate() {
            let dx = ((f as i32 + i as i32) % 8) as f32;
            let dy = ((f as i32 * 2 + i as i32) % 8) as f32;
            blend_images(&mut base, ov, dx, dy, None, None,
                Some(1.0), Some(1.0), None, None, Some(0.0), Some(0.0));
        }
    }
    t.elapsed().as_millis()
}

fn wgpu_run(comp: &mut WgpuCompositor, overlays: &[DynamicImage], ids: &[String]) -> u128 {
    let t = Instant::now();
    for f in 0..FRAMES {
        let mut draws: Vec<SpriteDraw> = Vec::with_capacity(overlays.len());
        for i in 0..overlays.len() {
            let dx = ((f as i32 + i as i32) % 8) as f32;
            let dy = ((f as i32 * 2 + i as i32) % 8) as f32;
            draws.push(SpriteDraw {
                texture_id: ids[i].clone(),
                pos: [dx, dy],
                size: Some([W as f32, H as f32]),
                scale: [1.0, 1.0],
                rotation_deg: 0.0,
                anchor: [0.0, 0.0],
                opacity: 255,
            });
        }
        let _ = comp.compose_to_host(&draws).expect("compose");
    }
    t.elapsed().as_millis()
}

#[test]
fn sweep_break_even_full_screen_overlays() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .is_test(true)
        .try_init();

    let mut comp = WgpuCompositor::new(W, H).expect("wgpu init");
    let mut all_overlays: Vec<DynamicImage> = Vec::new();
    let mut all_ids: Vec<String> = Vec::new();
    for i in 0..10 {
        let s = make_sprite(i);
        comp.upload_texture(format!("ov-{i}"), &s).expect("upload");
        all_overlays.push(s);
        all_ids.push(format!("ov-{i}"));
    }

    println!();
    println!("=== full-screen overlay sweep, {FRAMES} frames @ 720×1080 ===");
    println!("{:>4}  {:>10}  {:>10}  {:>9}  verdict",
             "N", "cpu(ms)", "wgpu(ms)", "ratio");
    let mut break_even: Option<usize> = None;
    for n in 1..=10 {
        let overlays = &all_overlays[..n];
        let ids = &all_ids[..n];
        let cpu_ms = cpu_run(overlays);
        let wgpu_ms = wgpu_run(&mut comp, overlays, ids);
        let ratio = cpu_ms as f64 / wgpu_ms.max(1) as f64;
        let verdict = if ratio >= 1.0 { "wgpu ✓" } else { "cpu  ✓" };
        println!("{:>4}  {:>10}  {:>10}  {:>8.2}×  {verdict}", n, cpu_ms, wgpu_ms, ratio);
        if break_even.is_none() && ratio >= 1.0 {
            break_even = Some(n);
        }
    }
    println!();
    if let Some(n) = break_even {
        println!("→ break-even at N = {n} full-screen overlays per frame");
        println!("→ recommendation: GAMEREEL_WORKER_COMPOSITOR=wgpu when expected per-frame");
        println!("  composited pixel area > {} × 720×1080 = ~{} K pixels",
                 n - 1, (n - 1) as u32 * 720 * 1080 / 1000);
    } else {
        println!("→ wgpu never won across N = 1..10 — should NOT happen on real GPU");
        panic!("break-even not found");
    }
}
