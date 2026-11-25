use std::collections::HashSet;

use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use serde::Serialize;

use crate::optimize::config::EffectivePixelsConfig;

/// Pixel-level analysis metrics computed on an input texture.
#[derive(Debug, Clone, Serialize)]
pub struct PixelAnalysis {
    pub width: u32,
    pub height: u32,
    pub source_has_alpha_channel: bool,
    /// True if at least one pixel has alpha < `opaque_threshold`.
    pub has_real_alpha: bool,
    /// Minimum alpha value observed (255 for opaque images).
    pub min_alpha: u8,
    /// Fraction of pixels with `alpha > alpha_threshold` in `[0.0, 1.0]`.
    pub effective_pixel_ratio: f64,
    /// Fraction of area occupied by the tight bounding box of visible pixels in `[0.0, 1.0]`.
    pub bbox_area_ratio: f64,
    /// Tight bounding box `[x0, y0, x1_exclusive, y1_exclusive]` of visible pixels, if any.
    pub tight_bbox: Option<[u32; 4]>,
    /// True if total unique RGBA colors <= 256 (already lossless-palette friendly).
    pub fits_256_palette: bool,
    /// True if all visible pixels have R == G == B.
    pub is_grayscale: bool,
}

/// Analyze alpha usage, effective pixel ratio, tight bbox, palette size, and grayscale status.
pub fn analyze_image(img: &DynamicImage, cfg: &EffectivePixelsConfig) -> PixelAnalysis {
    let (w, h) = img.dimensions();
    let total = (w as u64).saturating_mul(h as u64);
    let source_has_alpha_channel = img.color().has_alpha();

    let rgba = img.to_rgba8();
    let raw = rgba.as_raw();

    let mut min_alpha: u8 = 255;
    let mut visible_count: u64 = 0;
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x: u32 = 0;
    let mut max_y: u32 = 0;
    let mut is_grayscale = true;
    let mut colors: HashSet<u32> = HashSet::with_capacity(260);
    let mut overflow_palette = false;

    for y in 0..h {
        let row_offset = (y as usize) * (w as usize) * 4;
        for x in 0..w {
            let idx = row_offset + (x as usize) * 4;
            let r = raw[idx];
            let g = raw[idx + 1];
            let b = raw[idx + 2];
            let a = raw[idx + 3];

            if a < min_alpha {
                min_alpha = a;
            }

            if a > cfg.alpha_threshold {
                visible_count += 1;
                if x < min_x {
                    min_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if x + 1 > max_x {
                    max_x = x + 1;
                }
                if y + 1 > max_y {
                    max_y = y + 1;
                }
                if is_grayscale && (r != g || g != b) {
                    is_grayscale = false;
                }
            }

            if !overflow_palette {
                let packed = if a <= cfg.alpha_threshold {
                    0u32
                } else {
                    ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | (a as u32)
                };
                colors.insert(packed);
                if colors.len() > 256 {
                    overflow_palette = true;
                }
            }
        }
    }

    let has_real_alpha = source_has_alpha_channel && min_alpha < cfg.opaque_threshold;
    let effective_pixel_ratio = if total == 0 {
        1.0
    } else {
        visible_count as f64 / total as f64
    };

    let (tight_bbox, bbox_area_ratio) = if visible_count == 0 || total == 0 {
        (None, 0.0)
    } else {
        let bw = (max_x - min_x) as u64;
        let bh = (max_y - min_y) as u64;
        (
            Some([min_x, min_y, max_x, max_y]),
            (bw * bh) as f64 / total as f64,
        )
    };

    PixelAnalysis {
        width: w,
        height: h,
        source_has_alpha_channel,
        has_real_alpha,
        min_alpha,
        effective_pixel_ratio,
        bbox_area_ratio,
        tight_bbox,
        fits_256_palette: !overflow_palette,
        is_grayscale,
    }
}

/// Trim transparent border around `tight_bbox` with `padding` pixels.
/// Returns `Some((cropped, [x0, y0, x1, y1]))` if cropping actually shrinks the canvas.
pub fn trim_to_bbox(
    img: &DynamicImage,
    tight_bbox: [u32; 4],
    padding: u32,
) -> Option<(DynamicImage, [u32; 4])> {
    let (w, h) = img.dimensions();
    let x0 = tight_bbox[0].saturating_sub(padding);
    let y0 = tight_bbox[1].saturating_sub(padding);
    let x1 = tight_bbox[2].saturating_add(padding).min(w);
    let y1 = tight_bbox[3].saturating_add(padding).min(h);
    let new_w = x1.saturating_sub(x0);
    let new_h = y1.saturating_sub(y0);
    if new_w == 0 || new_h == 0 || (new_w == w && new_h == h) {
        return None;
    }
    let cropped = img.crop_imm(x0, y0, new_w, new_h);
    Some((cropped, [x0, y0, x1, y1]))
}

/// Zero RGB on fully transparent pixels (`alpha <= alpha_threshold`) while optionally
/// dilating RGB from visible edge neighbors by `edge_bleed_px` pixels (with `alpha = 0`)
/// to prevent dark fringes under GPU bilinear filtering.
/// Returns true if any pixel was modified.
pub fn clean_transparent_rgb(rgba: &mut RgbaImage, alpha_threshold: u8, edge_bleed_px: u32) -> bool {
    let (w, h) = rgba.dimensions();
    let total = (w as usize).saturating_mul(h as usize);
    if total == 0 {
        return false;
    }

    // Track which pixels have valid RGB colors (initially all visible pixels).
    let mut has_color = vec![false; total];
    let mut visible_count = 0usize;
    for y in 0..h {
        for x in 0..w {
            let p = rgba.get_pixel(x, y);
            if p[3] > alpha_threshold {
                has_color[(y as usize) * (w as usize) + (x as usize)] = true;
                visible_count += 1;
            }
        }
    }

    if visible_count == total {
        // No transparent pixels at all.
        return false;
    }

    let mut modified = false;

    // First zero all transparent pixels cleanly; bleed passes will fill edge RGB from visible neighbors.
    let mut bleed_rgb: Vec<[u8; 3]> = vec![[0, 0, 0]; total];
    for y in 0..h {
        for x in 0..w {
            let idx = (y as usize) * (w as usize) + (x as usize);
            let p = rgba.get_pixel(x, y);
            if has_color[idx] {
                bleed_rgb[idx] = [p[0], p[1], p[2]];
            }
        }
    }

    let mut frontier_mask = has_color.clone();
    for _ in 0..edge_bleed_px {
        let mut next_mask = frontier_mask.clone();
        let mut next_rgb = bleed_rgb.clone();
        let mut any_new = false;

        for y in 0..h {
            for x in 0..w {
                let idx = (y as usize) * (w as usize) + (x as usize);
                if frontier_mask[idx] {
                    continue;
                }
                let mut sum_r: u32 = 0;
                let mut sum_g: u32 = 0;
                let mut sum_b: u32 = 0;
                let mut cnt: u32 = 0;

                let y0 = y.saturating_sub(1);
                let y1 = (y + 1).min(h - 1);
                let x0 = x.saturating_sub(1);
                let x1 = (x + 1).min(w - 1);

                for ny in y0..=y1 {
                    for nx in x0..=x1 {
                        if nx == x && ny == y {
                            continue;
                        }
                        let nidx = (ny as usize) * (w as usize) + (nx as usize);
                        if frontier_mask[nidx] {
                            let c = bleed_rgb[nidx];
                            sum_r += c[0] as u32;
                            sum_g += c[1] as u32;
                            sum_b += c[2] as u32;
                            cnt += 1;
                        }
                    }
                }

                if cnt > 0 {
                    next_mask[idx] = true;
                    next_rgb[idx] = [(sum_r / cnt) as u8, (sum_g / cnt) as u8, (sum_b / cnt) as u8];
                    any_new = true;
                }
            }
        }

        frontier_mask = next_mask;
        bleed_rgb = next_rgb;
        if !any_new {
            break;
        }
    }

    for y in 0..h {
        for x in 0..w {
            let idx = (y as usize) * (w as usize) + (x as usize);
            if !has_color[idx] {
                let target = if frontier_mask[idx] {
                    let c = bleed_rgb[idx];
                    Rgba([c[0], c[1], c[2], 0])
                } else {
                    Rgba([0, 0, 0, 0])
                };
                let cur = *rgba.get_pixel(x, y);
                if cur != target {
                    rgba.put_pixel(x, y, target);
                    modified = true;
                }
            }
        }
    }

    modified
}

/// Compute RGBA Peak Signal-to-Noise Ratio (dB) between original RGBA pixels and
/// palette-remapped pixels. Fully transparent pixels (`orig_a == 0 && quant_a == 0`)
/// contribute zero error since their RGB is invisible.
/// Returns `f64::INFINITY` if identical.
pub fn compute_palette_psnr_db(
    orig_rgba: &[u8],
    palette: &[imagequant::RGBA],
    indices: &[u8],
) -> f64 {
    let pixel_count = indices.len();
    if pixel_count == 0 || orig_rgba.len() < pixel_count * 4 {
        return f64::INFINITY;
    }

    let mut sse: f64 = 0.0;
    for (i, &idx) in indices.iter().enumerate() {
        let base = i * 4;
        let or = orig_rgba[base] as i32;
        let og = orig_rgba[base + 1] as i32;
        let ob = orig_rgba[base + 2] as i32;
        let oa = orig_rgba[base + 3] as i32;

        let p = palette.get(idx as usize).copied().unwrap_or(imagequant::RGBA {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        });
        let qr = p.r as i32;
        let qg = p.g as i32;
        let qb = p.b as i32;
        let qa = p.a as i32;

        if oa == 0 && qa == 0 {
            continue;
        }

        // Weight RGB error by max alpha so transparent-area RGB differences don't skew PSNR,
        // while keeping alpha error at full weight.
        let alpha_w = (oa.max(qa) as f64) / 255.0;
        let dr = (or - qr) as f64;
        let dg = (og - qg) as f64;
        let db = (ob - qb) as f64;
        let da = (oa - qa) as f64;

        sse += alpha_w * (dr * dr + dg * dg + db * db) + da * da;
    }

    let mse = sse / (pixel_count as f64 * 4.0);
    if mse <= 1e-12 {
        return f64::INFINITY;
    }
    10.0 * ((255.0_f64 * 255.0_f64) / mse).log10()
}
