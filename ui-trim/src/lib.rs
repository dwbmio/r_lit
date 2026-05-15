use image::{ImageBuffer, Rgba, RgbaImage};
use serde::Serialize;
use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::time::Instant;

pub type Result<T> = std::result::Result<T, UiTrimError>;
type Mask = Vec<u8>;

#[derive(Debug, thiserror::Error)]
pub enum UiTrimError {
    #[error("failed to read input image {path}: {source}")]
    ReadImage {
        path: String,
        #[source]
        source: image::ImageError,
    },
    #[error("failed to write output image {path}: {source}")]
    WriteImage {
        path: String,
        #[source]
        source: image::ImageError,
    },
    #[error("failed to create output directory {path}: {source}")]
    CreateOutputDir {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("image dimensions are too large")]
    DimensionOverflow,
}

#[derive(Clone, Debug)]
pub struct TrimOptions {
    pub padding: u32,
    pub alpha_threshold: u8,
    pub feather: u32,
    pub max_bg_distance: f32,
    pub remove_red_guides: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TrimOptionsReport {
    pub padding_px: u32,
    pub alpha_threshold: u8,
    pub feather_px: u32,
    pub max_bg_distance: f32,
    pub remove_red_guides: bool,
    pub implementation: &'static str,
    pub acceleration: &'static str,
}

impl From<&TrimOptions> for TrimOptionsReport {
    fn from(options: &TrimOptions) -> Self {
        Self {
            padding_px: options.padding,
            alpha_threshold: options.alpha_threshold,
            feather_px: options.feather,
            max_bg_distance: options.max_bg_distance,
            remove_red_guides: options.remove_red_guides,
            implementation: "pure_rust_cpu",
            acceleration:
                "specialized_u8_mask_kernel; png codec dependencies may use CPU intrinsics internally",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TrimTimingsMs {
    pub decode: f64,
    pub sample_matte: f64,
    pub flood_fill: f64,
    pub morphology: f64,
    pub alpha_cleanup: f64,
    pub bbox_crop: f64,
    pub encode: f64,
    pub total: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TrimReport {
    pub ok: bool,
    pub input_width: u32,
    pub input_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub trim_bbox: [u32; 4],
    pub padding_px: u32,
    pub removed_pixels: u64,
    pub alpha_ratio: f32,
    pub throughput_mp_s: f64,
    pub options: TrimOptionsReport,
    pub timings_ms: TrimTimingsMs,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
struct ColorCluster {
    r: f32,
    g: f32,
    b: f32,
    count: u32,
}

impl ColorCluster {
    fn add(&mut self, r: u8, g: u8, b: u8) {
        let next = self.count + 1;
        self.r = (self.r * self.count as f32 + r as f32) / next as f32;
        self.g = (self.g * self.count as f32 + g as f32) / next as f32;
        self.b = (self.b * self.count as f32 + b as f32) / next as f32;
        self.count = next;
    }

    fn distance(&self, r: u8, g: u8, b: u8) -> f32 {
        let dr = self.r - r as f32;
        let dg = self.g - g as f32;
        let db = self.b - b as f32;
        (dr * dr + dg * dg + db * db).sqrt()
    }
}

pub fn trim_file(input: &Path, output: &Path, options: &TrimOptions) -> Result<TrimReport> {
    let total_start = Instant::now();
    let decode_start = Instant::now();
    let img = image::open(input)
        .map_err(|source| UiTrimError::ReadImage {
            path: input.display().to_string(),
            source,
        })?
        .to_rgba8();
    let decode_ms = elapsed_ms(decode_start);

    let (out, report) = trim_image(img, options)?;
    let mut report = report;
    report.timings_ms.decode = decode_ms;

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|source| UiTrimError::CreateOutputDir {
                path: parent.display().to_string(),
                source,
            })?;
        }
    }

    let encode_start = Instant::now();
    out.save(output).map_err(|source| UiTrimError::WriteImage {
        path: output.display().to_string(),
        source,
    })?;
    report.timings_ms.encode = elapsed_ms(encode_start);
    report.timings_ms.total = elapsed_ms(total_start);

    Ok(report)
}

pub fn trim_image(mut img: RgbaImage, options: &TrimOptions) -> Result<(RgbaImage, TrimReport)> {
    let total_start = Instant::now();
    let (w, h) = img.dimensions();
    let len = pixel_len(w, h)?;
    let sample_start = Instant::now();
    let clusters = sample_edge_matte(&img, options.alpha_threshold, options.max_bg_distance);
    let sample_matte_ms = elapsed_ms(sample_start);
    let mut warnings = Vec::new();
    if clusters.is_empty() {
        warnings.push("no_matte_cluster_sampled".to_string());
    }

    let flood_start = Instant::now();
    let mut bg = flood_fill_background(&img, &clusters, options)?;
    let flood_fill_ms = elapsed_ms(flood_start);
    let mut morphology_ms = 0.0;
    if bg.iter().any(|&v| v != 0) {
        let morphology_start = Instant::now();
        bg = open_mask(&close_mask(&bg, w, h), w, h);
        morphology_ms = elapsed_ms(morphology_start);
    } else {
        warnings.push("no_edge_background_removed".to_string());
    }

    let removed_pixels = bg.iter().filter(|&&v| v != 0).count() as u64;
    let cleanup_start = Instant::now();
    apply_alpha_cleanup(&mut img, &bg, w, h, options.feather);
    let alpha_cleanup_ms = elapsed_ms(cleanup_start);

    let bbox_start = Instant::now();
    let bbox = alpha_bbox(&img, options.alpha_threshold).unwrap_or([
        0,
        0,
        w.saturating_sub(1),
        h.saturating_sub(1),
    ]);
    if len == removed_pixels as usize {
        warnings.push("all_pixels_removed_by_background_mask".to_string());
    }

    let padded = pad_bbox(bbox, w, h, options.padding);
    let out = crop_rgba(&img, padded);
    let output_width = padded[2] - padded[0] + 1;
    let output_height = padded[3] - padded[1] + 1;
    let alpha_pixels = out
        .pixels()
        .filter(|p| p.0[3] > options.alpha_threshold)
        .count();
    let alpha_ratio = alpha_pixels as f32 / (output_width as f32 * output_height as f32);
    let bbox_crop_ms = elapsed_ms(bbox_start);
    let total_ms = elapsed_ms(total_start);
    let mpixels = (w as f64 * h as f64) / 1_000_000.0;
    let throughput_mp_s = if total_ms > 0.0 {
        mpixels / (total_ms / 1000.0)
    } else {
        0.0
    };

    let report = TrimReport {
        ok: true,
        input_width: w,
        input_height: h,
        output_width,
        output_height,
        trim_bbox: padded,
        padding_px: options.padding,
        removed_pixels,
        alpha_ratio,
        throughput_mp_s,
        options: TrimOptionsReport::from(options),
        timings_ms: TrimTimingsMs {
            sample_matte: sample_matte_ms,
            flood_fill: flood_fill_ms,
            morphology: morphology_ms,
            alpha_cleanup: alpha_cleanup_ms,
            bbox_crop: bbox_crop_ms,
            total: total_ms,
            ..TrimTimingsMs::default()
        },
        warnings,
    };
    Ok((out, report))
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn pixel_len(w: u32, h: u32) -> Result<usize> {
    let pixels = u64::from(w)
        .checked_mul(u64::from(h))
        .ok_or(UiTrimError::DimensionOverflow)?;
    usize::try_from(pixels).map_err(|_| UiTrimError::DimensionOverflow)
}

fn sample_edge_matte(img: &RgbaImage, alpha_threshold: u8, max_distance: f32) -> Vec<ColorCluster> {
    let (w, h) = img.dimensions();
    let mut samples = Vec::new();
    let step_x = (w / 64).max(1) as usize;
    let step_y = (h / 32).max(1) as usize;

    for x in (0..w).step_by(step_x) {
        samples.push(*img.get_pixel(x, 0));
        samples.push(*img.get_pixel(x, h - 1));
    }
    for y in (0..h).step_by(step_y) {
        samples.push(*img.get_pixel(0, y));
        samples.push(*img.get_pixel(w - 1, y));
    }

    let mut clusters: Vec<ColorCluster> = Vec::new();
    for Rgba([r, g, b, a]) in samples {
        if a <= alpha_threshold || is_saturated(r, g, b) || is_too_dark(r, g, b) {
            continue;
        }
        if let Some(cluster) = clusters
            .iter_mut()
            .find(|cluster| cluster.distance(r, g, b) <= max_distance)
        {
            cluster.add(r, g, b);
        } else if clusters.len() < 4 {
            clusters.push(ColorCluster {
                r: r as f32,
                g: g as f32,
                b: b as f32,
                count: 1,
            });
        }
    }
    clusters
}

fn flood_fill_background(
    img: &RgbaImage,
    clusters: &[ColorCluster],
    options: &TrimOptions,
) -> Result<Mask> {
    let (w, h) = img.dimensions();
    let mut visited = vec![0u8; pixel_len(w, h)?];
    let mut q = VecDeque::new();

    for x in 0..w {
        push_seed(img, clusters, options, &mut visited, &mut q, w, x, 0);
        push_seed(img, clusters, options, &mut visited, &mut q, w, x, h - 1);
    }
    for y in 0..h {
        push_seed(img, clusters, options, &mut visited, &mut q, w, 0, y);
        push_seed(img, clusters, options, &mut visited, &mut q, w, w - 1, y);
    }

    while let Some((x, y)) = q.pop_front() {
        if x > 0 {
            push_seed(img, clusters, options, &mut visited, &mut q, w, x - 1, y);
        }
        if x + 1 < w {
            push_seed(img, clusters, options, &mut visited, &mut q, w, x + 1, y);
        }
        if y > 0 {
            push_seed(img, clusters, options, &mut visited, &mut q, w, x, y - 1);
        }
        if y + 1 < h {
            push_seed(img, clusters, options, &mut visited, &mut q, w, x, y + 1);
        }
    }

    Ok(visited)
}

fn push_seed(
    img: &RgbaImage,
    clusters: &[ColorCluster],
    options: &TrimOptions,
    visited: &mut [u8],
    q: &mut VecDeque<(u32, u32)>,
    w: u32,
    x: u32,
    y: u32,
) {
    let idx = (y * w + x) as usize;
    if visited[idx] != 0 {
        return;
    }
    if !is_background_pixel(*img.get_pixel(x, y), clusters, options) {
        return;
    }
    visited[idx] = 1;
    q.push_back((x, y));
}

fn is_background_pixel(
    Rgba([r, g, b, a]): Rgba<u8>,
    clusters: &[ColorCluster],
    options: &TrimOptions,
) -> bool {
    if a <= options.alpha_threshold {
        return true;
    }
    if clusters
        .iter()
        .any(|cluster| cluster.distance(r, g, b) <= options.max_bg_distance)
    {
        return true;
    }
    if is_near_white_or_gray(r, g, b) {
        return true;
    }
    options.remove_red_guides && is_red_guide(r, g, b)
}

fn is_saturated(r: u8, g: u8, b: u8) -> bool {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    max.saturating_sub(min) > 80
}

fn is_too_dark(r: u8, g: u8, b: u8) -> bool {
    r.max(g).max(b) < 64
}

fn is_near_white_or_gray(r: u8, g: u8, b: u8) -> bool {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    r >= 220 && g >= 220 && b >= 220 && max.saturating_sub(min) <= 35
}

fn is_red_guide(r: u8, g: u8, b: u8) -> bool {
    r >= 180 && g <= 150 && b <= 150
}

fn close_mask(mask: &[u8], w: u32, h: u32) -> Mask {
    erode_mask(&dilate_mask(mask, w, h), w, h)
}

fn open_mask(mask: &[u8], w: u32, h: u32) -> Mask {
    dilate_mask(&erode_mask(mask, w, h), w, h)
}

fn dilate_mask(mask: &[u8], w: u32, h: u32) -> Mask {
    let tmp = morph_horizontal(mask, w, h, true);
    morph_vertical(&tmp, w, h, true)
}

fn erode_mask(mask: &[u8], w: u32, h: u32) -> Mask {
    let tmp = morph_horizontal(mask, w, h, false);
    morph_vertical(&tmp, w, h, false)
}

fn morph_horizontal(mask: &[u8], w: u32, h: u32, dilate: bool) -> Mask {
    let mut out = vec![0u8; mask.len()];
    let w_usize = w as usize;
    for y in 0..h {
        let row = y as usize * w_usize;
        for x in 0..w {
            let min_x = x.saturating_sub(1) as usize;
            let max_x = (x + 1).min(w - 1) as usize;
            let mut value = !dilate;
            for nx in min_x..=max_x {
                let on = mask[row + nx] != 0;
                if dilate {
                    if on {
                        value = true;
                        break;
                    }
                } else if !on {
                    value = false;
                    break;
                }
            }
            out[row + x as usize] = u8::from(value);
        }
    }
    out
}

fn morph_vertical(mask: &[u8], w: u32, h: u32, dilate: bool) -> Mask {
    let mut out = vec![0u8; mask.len()];
    let w_usize = w as usize;
    for y in 0..h {
        let min_y = y.saturating_sub(1);
        let max_y = (y + 1).min(h - 1);
        for x in 0..w {
            let mut value = !dilate;
            for ny in min_y..=max_y {
                let on = mask[ny as usize * w_usize + x as usize] != 0;
                if dilate {
                    if on {
                        value = true;
                        break;
                    }
                } else if !on {
                    value = false;
                    break;
                }
            }
            out[y as usize * w_usize + x as usize] = u8::from(value);
        }
    }
    out
}

pub fn make_synthetic_ui_asset(w: u32, h: u32) -> RgbaImage {
    let mut img = RgbaImage::from_fn(w, h, |x, y| {
        let checker = ((x / 16) + (y / 16)) % 2 == 0;
        if checker {
            Rgba([238, 238, 238, 255])
        } else {
            Rgba([222, 222, 222, 255])
        }
    });
    let min_x = w / 4;
    let max_x = w - min_x;
    let min_y = h / 4;
    let max_y = h - min_y;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let edge = x == min_x || x + 1 == max_x || y == min_y || y + 1 == max_y;
            let color = if edge {
                Rgba([245, 214, 92, 255])
            } else {
                Rgba([45, 78, 170, 255])
            };
            img.put_pixel(x, y, color);
        }
    }
    if w > 4 && h > 4 {
        for x in 0..w {
            img.put_pixel(x, 1, Rgba([220, 40, 40, 255]));
        }
        for y in 0..h {
            img.put_pixel(1, y, Rgba([220, 40, 40, 255]));
        }
    }
    img
}

pub fn default_options() -> TrimOptions {
    TrimOptions {
        padding: 6,
        alpha_threshold: 4,
        feather: 2,
        max_bg_distance: 48.0,
        remove_red_guides: true,
    }
}

fn apply_alpha_cleanup(img: &mut RgbaImage, bg: &[u8], w: u32, h: u32, feather: u32) {
    for y in 0..h {
        for x in 0..w {
            if bg[(y * w + x) as usize] != 0 {
                img.get_pixel_mut(x, y).0[3] = 0;
            }
        }
    }

    let radius = feather.min(3);
    if radius == 0 {
        return;
    }

    let mut feather_band = bg.to_vec();
    for _ in 0..radius {
        feather_band = dilate_mask(&feather_band, w, h);
    }

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            if bg[idx] != 0 || feather_band[idx] == 0 {
                continue;
            }
            let p = img.get_pixel_mut(x, y);
            let scale = match radius {
                1 => 220u16,
                2 => 204u16,
                _ => 192u16,
            };
            p.0[3] = ((u16::from(p.0[3]) * scale) / 255) as u8;
        }
    }
}

fn alpha_bbox(img: &RgbaImage, threshold: u8) -> Option<[u32; 4]> {
    let (w, h) = img.dimensions();
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;

    for y in 0..h {
        for x in 0..w {
            if img.get_pixel(x, y).0[3] > threshold {
                found = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    found.then_some([min_x, min_y, max_x, max_y])
}

fn pad_bbox(bbox: [u32; 4], w: u32, h: u32, padding: u32) -> [u32; 4] {
    [
        bbox[0].saturating_sub(padding),
        bbox[1].saturating_sub(padding),
        (bbox[2] + padding).min(w - 1),
        (bbox[3] + padding).min(h - 1),
    ]
}

fn crop_rgba(img: &RgbaImage, bbox: [u32; 4]) -> RgbaImage {
    let out_w = bbox[2] - bbox[0] + 1;
    let out_h = bbox[3] - bbox[1] + 1;
    ImageBuffer::from_fn(out_w, out_h, |x, y| {
        *img.get_pixel(bbox[0] + x, bbox[1] + y)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> TrimOptions {
        TrimOptions {
            padding: 1,
            alpha_threshold: 4,
            feather: 0,
            max_bg_distance: 48.0,
            remove_red_guides: true,
        }
    }

    #[test]
    fn trims_edge_connected_checker_background() {
        let mut img = RgbaImage::from_pixel(12, 10, Rgba([238, 238, 238, 255]));
        for y in 3..7 {
            for x in 4..8 {
                img.put_pixel(x, y, Rgba([30, 80, 180, 255]));
            }
        }

        let (out, report) = trim_image(img, &options()).expect("trim");

        assert_eq!(out.dimensions(), (6, 6));
        assert_eq!(report.trim_bbox, [3, 2, 8, 7]);
        assert!(report.removed_pixels > 0);
    }

    #[test]
    fn does_not_remove_interior_matte_colored_pixels() {
        let mut img = RgbaImage::from_pixel(10, 10, Rgba([240, 240, 240, 255]));
        for y in 2..8 {
            for x in 2..8 {
                img.put_pixel(x, y, Rgba([20, 30, 40, 255]));
            }
        }
        img.put_pixel(5, 5, Rgba([240, 240, 240, 255]));

        let (out, _) = trim_image(img, &options()).expect("trim");

        assert_eq!(out.get_pixel(3, 3).0[3], 255);
    }
}
