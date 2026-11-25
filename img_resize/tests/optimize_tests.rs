use std::fs;
use std::path::{Path, PathBuf};

use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage};
use img_resize::optimize::config::LoadedProfile;
use img_resize::optimize::{run_optimize, OptimizeOptions};

fn temp_test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "img_resize_opt_test_{}_{}",
        name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

/// Create a realistic game-like texture (> 256 colors, painted panels + subtle surface grain)
/// that quantizes cleanly with high perceptual PSNR (> 38 dB) and strong byte reduction.
fn make_game_texture_rgba(w: u32, h: u32, alpha: u8) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let block_x = x / 64;
            let block_y = y / 64;
            let shade = ((x % 64) / 4) as u8;
            let (base_r, base_g, base_b) = match (block_x + block_y) % 4 {
                0 => (40u8, 120u8, 200u8),
                1 => (200u8, 90u8, 50u8),
                2 => (60u8, 175u8, 95u8),
                _ => (180u8, 150u8, 70u8),
            };
            // Subtle deterministic painterly surface grain (prevents trivial 1D PNG filter collapse)
            let grain_hash = x
                .wrapping_mul(1664525)
                .wrapping_add(y.wrapping_mul(1013904223))
                ^ ((x ^ y).wrapping_mul(2246822519));
            let gr = ((grain_hash & 7) as i16) - 3;
            let gg = (((grain_hash >> 3) & 7) as i16) - 3;
            let gb = (((grain_hash >> 6) & 7) as i16) - 3;
            let v_shade = ((y % 64) / 8) as i16;

            let r = (base_r as i16 + shade as i16 + gr).clamp(0, 255) as u8;
            let g = (base_g as i16 + v_shade + gg).clamp(0, 255) as u8;
            let b = (base_b as i16 - v_shade / 2 + gb).clamp(0, 255) as u8;
            img.put_pixel(x, y, Rgba([r, g, b, alpha]));
        }
    }
    img
}

/// Create a colorful 65K-color rainbow gradient image (hardest case for 256-color quantization).
fn make_colorful_rgba(w: u32, h: u32, alpha: u8) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let r = ((x * 255) / w.max(1)) as u8;
            let g = ((y * 255) / h.max(1)) as u8;
            let b = (((x + y) * 127) / (w + h).max(1)) as u8;
            img.put_pixel(x, y, Rgba([r, g, b, alpha]));
        }
    }
    img
}

/// Create a PNG with a small visible square in the center and noisy garbage RGB in transparent pixels.
fn make_dirty_transparent_png(w: u32, h: u32, box_size: u32) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    let x0 = (w - box_size) / 2;
    let y0 = (h - box_size) / 2;
    let x1 = x0 + box_size;
    let y1 = y0 + box_size;
    for y in 0..h {
        for x in 0..w {
            if x >= x0 && x < x1 && y >= y0 && y < y1 {
                img.put_pixel(x, y, Rgba([220, 40, 30, 255]));
            } else {
                // Pseudo-random garbage RGB hidden behind alpha = 0 (very common in PS/AI exports).
                let hval = x
                    .wrapping_mul(374761393)
                    .wrapping_add(y.wrapping_mul(668265263))
                    ^ (x << 13);
                img.put_pixel(
                    x,
                    y,
                    Rgba([
                        (hval & 0xFF) as u8,
                        ((hval >> 8) & 0xFF) as u8,
                        ((hval >> 16) & 0xFF) as u8,
                        0,
                    ]),
                );
            }
        }
    }
    img
}

fn save_png(img: &DynamicImage, path: &Path) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).expect("create parent");
    }
    img.save_with_format(path, ImageFormat::Png)
        .expect("save png");
}

#[test]
fn test_all_checked_in_profiles_load_cleanly() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let profiles_dir = manifest_dir.join("profiles");
    let expected = [
        ("godot_wx-minip.yml", "godot", "wx-minip", "godot_wx-minip"),
        ("godot_qq-minip.yml", "godot", "qq-minip", "godot_qq-minip"),
        ("godot_dy-minip.yml", "godot", "dy-minip", "godot_dy-minip"),
        (
            "godot_zfb-minip.yml",
            "godot",
            "zfb-minip",
            "godot_zfb-minip",
        ),
        ("godot_mobile.yml", "godot", "mobile", "godot_mobile"),
        ("raw_png-jpg.yml", "raw", "png-jpg", "raw_png-jpg"),
    ];

    for (file, eng, chan, preset) in expected {
        let p = profiles_dir.join(file);
        let loaded = LoadedProfile::load(&p).expect("profile should load");
        assert_eq!(loaded.engine, eng);
        assert_eq!(loaded.channel, chan);
        assert_eq!(loaded.preset_used.as_deref(), Some(preset));
    }
}

#[test]
fn test_filename_inferred_preset_and_normal_map_protection() {
    let root = temp_test_dir("infer_and_protect");
    let src = root.join("assets");
    let out = root.join("out");
    let profile_path = root.join("godot_wx-minip.yml");

    // Minimal YAML: engine/channel inferred solely from filename `godot_wx-minip.yml`.
    fs::write(&profile_path, "compression:\n  png:\n    oxipng_level: 1\n")
        .expect("write minimal yaml");

    let diffuse = DynamicImage::ImageRgba8(make_game_texture_rgba(256, 256, 255));
    let normal = DynamicImage::ImageRgba8(make_game_texture_rgba(256, 256, 255));
    save_png(&diffuse, &src.join("chars/hero_diffuse.png"));
    save_png(&normal, &src.join("chars/hero_normal.png"));

    let report = run_optimize(&OptimizeOptions {
        profile_path,
        input_override: Some(src),
        out_dir_override: Some(out.clone()),
        dry_run: false,
        report_path: None,
        jobs: Some(2),
        json_output: true,
    })
    .expect("optimize ok");

    assert_eq!(report.profile.engine, "godot");
    assert_eq!(report.profile.channel, "wx-minip");
    assert_eq!(report.profile.preset_used.as_deref(), Some("godot_wx-minip"));
    assert_eq!(report.summary.failed, 0);

    let res_diffuse = report
        .results
        .iter()
        .find(|r| r.relative_path == "chars/hero_diffuse.png")
        .expect("diffuse result");
    let res_normal = report
        .results
        .iter()
        .find(|r| r.relative_path == "chars/hero_normal.png")
        .expect("normal result");

    // Diffuse: fake alpha stripped + quantized!
    assert!(res_diffuse.fake_alpha_stripped);
    assert!(
        res_diffuse.actions.iter().any(|a| a == "png_quantize"),
        "expected png_quantize on diffuse, got {:?}",
        res_diffuse.actions
    );
    assert!(res_diffuse.new_bytes < res_diffuse.original_bytes);

    // Normal map: protected by default Godot rule (lossless, no quantize, no fake-alpha strip)!
    assert!(!res_normal.fake_alpha_stripped);
    assert!(
        !res_normal.actions.iter().any(|a| a == "png_quantize"),
        "normal map must NOT be quantized, got {:?}",
        res_normal.actions
    );
    assert!(out.join("chars/hero_diffuse.png").exists());
    assert!(out.join("chars/hero_normal.png").exists());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_transparent_rgb_cleanup_edge_bleed_and_audit_warning() {
    let root = temp_test_dir("dirty_alpha");
    let src = root.join("in");
    let out = root.join("out");
    let profile_path = root.join("godot_wx-minip.yml");

    // Force lossless so we isolate the pure lossless win of zeroing transparent RGB + oxipng.
    fs::write(
        &profile_path,
        r#"
effective_pixels:
  zero_transparent_rgb: true
  edge_bleed_px: 1
  trim_transparent: false
compression:
  png:
    mode: lossless
    oxipng_level: 2
"#,
    )
    .expect("write profile");

    let dirty = DynamicImage::ImageRgba8(make_dirty_transparent_png(256, 256, 64));
    save_png(&dirty, &src.join("ui/badge.png"));

    let report = run_optimize(&OptimizeOptions {
        profile_path,
        input_override: Some(src),
        out_dir_override: Some(out.clone()),
        dry_run: false,
        report_path: None,
        jobs: Some(1),
        json_output: true,
    })
    .expect("optimize ok");

    let r = &report.results[0];
    assert_eq!(r.status, "optimized");
    assert_eq!(r.new_size, Some((256, 256))); // Not trimmed by default!
    assert!(r.effective_pixel_ratio.expect("ratio") < 0.10);
    assert!(r
        .warnings
        .iter()
        .any(|w| w.starts_with("low_effective_pixel_ratio")));
    assert!(r.actions.iter().any(|a| a == "zero_transparent_rgb"));
    // Massive pure-lossless savings from clearing hidden garbage RGB!
    assert!(
        r.saved_pct > 80.0,
        "expected >80% lossless savings on dirty transparent PNG, got {:.2}%",
        r.saved_pct
    );

    // Inspect decoded pixels: far corner is (0,0,0,0), 1px neighbor of box (x0-1, y0) has bled red with alpha=0.
    let decoded = image::open(out.join("ui/badge.png"))
        .expect("decode output")
        .to_rgba8();
    assert_eq!(*decoded.get_pixel(0, 0), Rgba([0, 0, 0, 0]));
    let x0 = (256 - 64) / 2;
    let y0 = (256 - 64) / 2;
    assert_eq!(*decoded.get_pixel(x0, y0), Rgba([220, 40, 30, 255]));
    assert_eq!(*decoded.get_pixel(x0 - 1, y0), Rgba([220, 40, 30, 0]));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_rule_scoped_trim_transparent() {
    let root = temp_test_dir("rule_trim");
    let src = root.join("in");
    let out = root.join("out");
    let profile_path = root.join("godot_dy-minip.yml");

    fs::write(
        &profile_path,
        r#"
compression:
  png:
    oxipng_level: 1
rules:
  - match: "raw_icons/**"
    effective_pixels:
      trim_transparent: true
      trim_padding: 4
"#,
    )
    .expect("write profile");

    let sprite = DynamicImage::ImageRgba8(make_dirty_transparent_png(256, 256, 64));
    save_png(&sprite, &src.join("raw_icons/sword.png"));
    save_png(&sprite, &src.join("ui/panel.png"));

    let report = run_optimize(&OptimizeOptions {
        profile_path,
        input_override: Some(src),
        out_dir_override: Some(out),
        dry_run: false,
        report_path: None,
        jobs: Some(2),
        json_output: true,
    })
    .expect("optimize ok");

    let sword = report
        .results
        .iter()
        .find(|r| r.relative_path == "raw_icons/sword.png")
        .expect("sword");
    let panel = report
        .results
        .iter()
        .find(|r| r.relative_path == "ui/panel.png")
        .expect("panel");

    assert_eq!(sword.new_size, Some((72, 72))); // 64 + 2*4 padding
    assert!(sword.trim_bbox.is_some());
    assert_eq!(panel.new_size, Some((256, 256))); // Untrimmed!

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_psnr_guard_triggers_lossless_fallback() {
    let root = temp_test_dir("psnr_fallback");
    let src = root.join("in");
    let out = root.join("out");
    let profile_path = root.join("godot_zfb-minip.yml");

    // Set impossibly high PSNR threshold (75 dB) so quantization must fall back to lossless.
    fs::write(
        &profile_path,
        r#"
compression:
  png:
    mode: quantize
    quality_min: 50
    quality_max: 90
    min_psnr_db: 75.0
    oxipng_level: 1
"#,
    )
    .expect("write profile");

    let grad = DynamicImage::ImageRgba8(make_colorful_rgba(256, 256, 255));
    save_png(&grad, &src.join("grad.png"));

    let report = run_optimize(&OptimizeOptions {
        profile_path,
        input_override: Some(src),
        out_dir_override: Some(out.clone()),
        dry_run: false,
        report_path: None,
        jobs: Some(1),
        json_output: true,
    })
    .expect("optimize ok");

    let r = &report.results[0];
    assert!(
        r.quantize_fallback.is_some(),
        "expected PSNR fallback, got none (psnr={:?})",
        r.psnr_db
    );
    assert_eq!(report.summary.quantize_fallback_count, 1);
    assert!(out.join("grad.png").exists());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_format_conversion_jpg_and_webp_and_downscale_align() {
    let root = temp_test_dir("fmt_and_resize");
    let src = root.join("in");
    let out = root.join("out");
    let profile_path = root.join("godot_qq-minip.yml");

    fs::write(
        &profile_path,
        r#"
limits:
  max_dimension: 512
  align_multiple: 4
format:
  opaque_png_to: jpg
  alpha_png_to: webp
compression:
  jpg:
    quality: 82
  webp:
    lossless: false
    quality: 82.0
"#,
    )
    .expect("write profile");

    // 800x600 opaque PNG -> downscaled to 512x384 (multiple of 4) and converted to JPG.
    let bg = DynamicImage::ImageRgba8(make_colorful_rgba(800, 600, 255));
    save_png(&bg, &src.join("bg.png"));

    // 256x256 alpha PNG -> converted to WebP with alpha preserved.
    let sprite = DynamicImage::ImageRgba8(make_dirty_transparent_png(256, 256, 128));
    save_png(&sprite, &src.join("sprite.png"));

    let report = run_optimize(&OptimizeOptions {
        profile_path,
        input_override: Some(src),
        out_dir_override: Some(out.clone()),
        dry_run: false,
        report_path: None,
        jobs: Some(2),
        json_output: true,
    })
    .expect("optimize ok");

    assert_eq!(report.summary.failed, 0);

    let bg_res = report
        .results
        .iter()
        .find(|r| r.relative_path == "bg.png")
        .expect("bg");
    assert_eq!(bg_res.output_format.as_deref(), Some("jpg"));
    assert_eq!(bg_res.new_size, Some((512, 384)));
    assert!(out.join("bg.jpg").exists());
    let bg_dec = image::open(out.join("bg.jpg")).expect("decode jpg");
    assert_eq!(bg_dec.dimensions(), (512, 384));

    let sp_res = report
        .results
        .iter()
        .find(|r| r.relative_path == "sprite.png")
        .expect("sprite");
    assert_eq!(sp_res.output_format.as_deref(), Some("webp"));
    assert!(out.join("sprite.webp").exists());
    let sp_dec = image::open(out.join("sprite.webp")).expect("decode webp");
    assert_eq!(sp_dec.dimensions(), (256, 256));
    assert!(sp_dec.color().has_alpha());
    // Corner pixel should remain transparent after WebP encode/decode.
    assert!(sp_dec.to_rgba8().get_pixel(0, 0)[3] <= 4);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_dry_run_writes_no_files_and_generates_report() {
    let root = temp_test_dir("dry_run");
    let src = root.join("in");
    let out = root.join("out");
    let report_file = root.join("report.json");
    let profile_path = root.join("godot_wx-minip.yml");

    fs::write(&profile_path, "compression:\n  png:\n    oxipng_level: 1\n")
        .expect("write profile");
    let img = DynamicImage::ImageRgba8(make_colorful_rgba(128, 128, 255));
    save_png(&img, &src.join("a.png"));

    let report = run_optimize(&OptimizeOptions {
        profile_path,
        input_override: Some(src),
        out_dir_override: Some(out.clone()),
        dry_run: true,
        report_path: Some(report_file.clone()),
        jobs: Some(1),
        json_output: true,
    })
    .expect("dry run ok");

    assert!(report.dry_run);
    assert!(!out.exists(), "dry_run must not create output directory");
    assert!(report_file.exists(), "report file should be written");
    assert!(report.summary.saved_bytes > 0);

    let _ = fs::remove_dir_all(&root);
}
