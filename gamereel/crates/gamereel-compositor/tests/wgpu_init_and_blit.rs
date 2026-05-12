//! M4-1 / M4-2 self-proof:
//!   1. wgpu picks an adapter (Vulkan on Linux + NVIDIA, fallback acceptable)
//!   2. WgpuCompositor::new finishes init in a sane time bound (< 5s)
//!   3. A trivial single-sprite compose reads back the expected RGBA pixels
//!
//! No GPU strictly required — wgpu falls back to a CPU adapter (llvmpipe)
//! on hosts without one, so this test runs in CI too.

use gamereel_compositor::{SpriteDraw, WgpuCompositor};
use image::DynamicImage;
use std::time::Instant;

fn solid_red() -> DynamicImage {
    let mut img = image::RgbaImage::new(32, 32);
    for p in img.pixels_mut() {
        *p = image::Rgba([255, 0, 0, 255]);
    }
    DynamicImage::ImageRgba8(img)
}

#[test]
fn init_and_blit_single_red_square_top_left() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .is_test(true)
        .try_init();

    let t = Instant::now();
    let mut c = WgpuCompositor::new(64, 64).expect("WgpuCompositor::new");
    let init_ms = t.elapsed().as_millis();
    println!("init: {init_ms} ms");
    assert!(init_ms < 5000, "init took {init_ms} ms — wgpu adapter selection regressed");

    c.upload_texture("red", &solid_red()).expect("upload");

    let draws = vec![SpriteDraw {
        texture_id: "red".into(),
        pos: [0.0, 0.0],
        size: None,
        scale: [1.0, 1.0],
        rotation_deg: 0.0,
        anchor: [0.0, 0.0], // top-left anchor → square at (0,0..32,32)
        opacity: 255,
    }];
    let rgba = c.compose_to_host(&draws).expect("compose");
    assert_eq!(rgba.len(), 64 * 64 * 4);

    // top-left pixel (0,0) should be red.
    assert_eq!(&rgba[0..4], &[255, 0, 0, 255], "top-left should be red");

    // pixel inside the square — say (16,16).
    let off = (16 * 64 + 16) * 4;
    assert_eq!(&rgba[off..off + 4], &[255, 0, 0, 255], "(16,16) should be red");

    // Pixel far outside the square (50,50) should be transparent black
    // (we cleared with TRANSPARENT).
    let off_out = (50 * 64 + 50) * 4;
    assert_eq!(
        &rgba[off_out..off_out + 4],
        &[0, 0, 0, 0],
        "(50,50) should be cleared transparent"
    );

    println!("blit OK: top-left red, far-corner clear");
}

#[test]
fn anchor_centered_places_sprite_at_center() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .is_test(true)
        .try_init();
    let mut c = WgpuCompositor::new(64, 64).expect("WgpuCompositor::new");
    c.upload_texture("red", &solid_red()).expect("upload");

    let draws = vec![SpriteDraw {
        texture_id: "red".into(),
        pos: [32.0, 32.0],          // place anchor at scene center
        size: None,
        scale: [1.0, 1.0],
        rotation_deg: 0.0,
        anchor: [0.5, 0.5],          // center of the 32x32 sprite
        opacity: 255,
    }];
    let rgba = c.compose_to_host(&draws).expect("compose");

    // Sprite top-left should land at (16,16) ⇒ pixel (16,16) is red.
    let inside = (16 * 64 + 16) * 4;
    assert_eq!(&rgba[inside..inside + 4], &[255, 0, 0, 255]);

    // Pixel (15,15) is just outside the centered sprite ⇒ clear.
    let outside = (15 * 64 + 15) * 4;
    assert_eq!(&rgba[outside..outside + 4], &[0, 0, 0, 0]);
}
