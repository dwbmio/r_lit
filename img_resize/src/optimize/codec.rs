use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, GenericImageView, ImageFormat};
use serde::Serialize;

use crate::error::{ReError, Result};
use crate::optimize::analysis::compute_palette_psnr_db;
use crate::optimize::config::{
    CompressionConfig, FormatConfig, FormatTarget, LimitsConfig, PngCompressionConfig, PngMode,
};

/// Canonical image format used by the optimization pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Png,
    Jpg,
    Webp,
}

impl OutputFormat {
    pub fn from_image_format(fmt: ImageFormat) -> Self {
        match fmt {
            ImageFormat::Jpeg => Self::Jpg,
            ImageFormat::WebP => Self::Webp,
            _ => Self::Png,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpg => "jpg",
            Self::Webp => "webp",
        }
    }

    pub fn as_str(self) -> &'static str {
        self.extension()
    }
}

/// Compute target dimensions respecting `max_dimension`, `max_width`, `max_height`,
/// and `align_multiple` without ever upscaling.
pub fn compute_target_dimensions(orig_w: u32, orig_h: u32, limits: &LimitsConfig) -> (u32, u32) {
    if orig_w == 0 || orig_h == 0 {
        return (orig_w, orig_h);
    }

    let mut scale: f64 = 1.0;

    if limits.max_dimension > 0 {
        let longest = orig_w.max(orig_h);
        if longest > limits.max_dimension {
            scale = scale.min(limits.max_dimension as f64 / longest as f64);
        }
    }
    if let Some(mw) = limits.max_width {
        if mw > 0 && orig_w > mw {
            scale = scale.min(mw as f64 / orig_w as f64);
        }
    }
    if let Some(mh) = limits.max_height {
        if mh > 0 && orig_h > mh {
            scale = scale.min(mh as f64 / orig_h as f64);
        }
    }

    let resized = scale < 1.0;
    let mut new_w = if resized {
        ((orig_w as f64 * scale).round() as u32).max(1)
    } else {
        orig_w
    };
    let mut new_h = if resized {
        ((orig_h as f64 * scale).round() as u32).max(1)
    } else {
        orig_h
    };

    if limits.align_multiple > 1 && (resized || limits.force_align) {
        let m = limits.align_multiple;
        if new_w >= m {
            new_w = (new_w / m) * m;
        }
        if new_h >= m {
            new_h = (new_h / m) * m;
        }
    }

    (new_w.max(1), new_h.max(1))
}

/// Resize image if target dimensions differ from current dimensions.
pub fn resize_if_needed(img: DynamicImage, limits: &LimitsConfig) -> (DynamicImage, bool) {
    let (w, h) = img.dimensions();
    let (tw, th) = compute_target_dimensions(w, h, limits);
    if (tw, th) == (w, h) {
        return (img, false);
    }
    let resized = img.resize_exact(tw, th, limits.downscale_filter.to_image_filter());
    (resized, true)
}

/// Select the output format based on original format, real alpha presence, and profile rules.
pub fn select_output_format(
    orig_fmt: ImageFormat,
    has_real_alpha: bool,
    cfg: &FormatConfig,
) -> OutputFormat {
    let apply_target = |target: FormatTarget, fallback: OutputFormat| match target {
        FormatTarget::Keep => fallback,
        FormatTarget::Png => OutputFormat::Png,
        FormatTarget::Jpg => OutputFormat::Jpg,
        FormatTarget::Webp => OutputFormat::Webp,
    };

    match orig_fmt {
        ImageFormat::Png => {
            if has_real_alpha {
                apply_target(cfg.alpha_png_to, OutputFormat::Png)
            } else {
                apply_target(cfg.opaque_png_to, OutputFormat::Png)
            }
        }
        ImageFormat::Jpeg => apply_target(cfg.jpg_to, OutputFormat::Jpg),
        ImageFormat::WebP => apply_target(cfg.webp_to, OutputFormat::Webp),
        _ => {
            if has_real_alpha {
                apply_target(cfg.alpha_png_to, OutputFormat::Png)
            } else {
                apply_target(cfg.opaque_png_to, OutputFormat::Png)
            }
        }
    }
}

/// Encoding outcome including optional perceptual diagnostics.
pub struct EncodeOutcome {
    pub bytes: Vec<u8>,
    pub psnr_db: Option<f64>,
    pub quantized: bool,
    pub quantize_fallback: Option<String>,
    pub oxipng_applied: bool,
}

/// Encode image into target format with full channel compression pipeline.
pub fn encode_image(
    img: &DynamicImage,
    target_fmt: OutputFormat,
    has_real_alpha: bool,
    fits_256_palette: bool,
    is_grayscale: bool,
    comp: &CompressionConfig,
) -> Result<EncodeOutcome> {
    match target_fmt {
        OutputFormat::Png => encode_png(
            img,
            has_real_alpha,
            fits_256_palette,
            is_grayscale,
            &comp.png,
        ),
        OutputFormat::Jpg => encode_jpg(img, comp.jpg.quality),
        OutputFormat::Webp => encode_webp(img, has_real_alpha, comp.webp.lossless, comp.webp.quality),
    }
}

fn encode_png(
    img: &DynamicImage,
    has_real_alpha: bool,
    fits_256_palette: bool,
    is_grayscale: bool,
    cfg: &PngCompressionConfig,
) -> Result<EncodeOutcome> {
    let mut psnr_db: Option<f64> = None;
    let mut quantized = false;
    let mut quantize_fallback: Option<String> = None;

    // If mode == Quantize and the image already has > 256 colors, try imagequant first.
    // (When <= 256 colors, lossless encoding + oxipng already emits an optimal lossless palette PNG-8.)
    let raw_png = if cfg.mode == PngMode::Quantize && !fits_256_palette {
        match quantize_png_to_memory(img, cfg) {
            QuantizeAttempt::Quantized { bytes, psnr } => {
                if let Some(min_psnr) = cfg.min_psnr_db {
                    if psnr < min_psnr {
                        quantize_fallback = Some(format!(
                            "psnr_below_threshold ({:.2} dB < {:.2} dB)",
                            psnr, min_psnr
                        ));
                        encode_lossless_png_raw(img, has_real_alpha, is_grayscale)?
                    } else {
                        psnr_db = Some(psnr);
                        quantized = true;
                        bytes
                    }
                } else {
                    psnr_db = Some(psnr);
                    quantized = true;
                    bytes
                }
            }
            QuantizeAttempt::Fallback(reason) => {
                quantize_fallback = Some(reason);
                encode_lossless_png_raw(img, has_real_alpha, is_grayscale)?
            }
        }
    } else {
        encode_lossless_png_raw(img, has_real_alpha, is_grayscale)?
    };

    if cfg.oxipng_level > 0 {
        let optimized = run_oxipng(&raw_png, cfg.oxipng_level, cfg.strip_metadata)?;
        // Keep whichever is smaller (oxipng almost always wins or matches).
        let bytes = if optimized.len() <= raw_png.len() {
            optimized
        } else {
            raw_png
        };
        Ok(EncodeOutcome {
            bytes,
            psnr_db,
            quantized,
            quantize_fallback,
            oxipng_applied: true,
        })
    } else {
        Ok(EncodeOutcome {
            bytes: raw_png,
            psnr_db,
            quantized,
            quantize_fallback,
            oxipng_applied: false,
        })
    }
}

enum QuantizeAttempt {
    Quantized { bytes: Vec<u8>, psnr: f64 },
    Fallback(String),
}

fn quantize_png_to_memory(img: &DynamicImage, cfg: &PngCompressionConfig) -> QuantizeAttempt {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let raw = rgba.as_raw();

    let pixels: Vec<imagequant::RGBA> = raw
        .chunks_exact(4)
        .map(|c| imagequant::RGBA {
            r: c[0],
            g: c[1],
            b: c[2],
            a: c[3],
        })
        .collect();

    let mut liq = imagequant::new();
    if let Err(e) = liq.set_speed(cfg.speed) {
        return QuantizeAttempt::Fallback(format!("imagequant set_speed failed: {}", e));
    }
    if let Err(e) = liq.set_quality(cfg.quality_min, cfg.quality_max) {
        return QuantizeAttempt::Fallback(format!("imagequant set_quality failed: {}", e));
    }

    let mut liq_img = match liq.new_image_borrowed(&pixels, w as usize, h as usize, 0.0) {
        Ok(i) => i,
        Err(e) => return QuantizeAttempt::Fallback(format!("imagequant new_image failed: {}", e)),
    };

    let mut res = match liq.quantize(&mut liq_img) {
        Ok(r) => r,
        Err(imagequant::Error::QualityTooLow) => {
            return QuantizeAttempt::Fallback(format!(
                "imagequant quality_too_low (target >= {})",
                cfg.quality_min
            ));
        }
        Err(e) => return QuantizeAttempt::Fallback(format!("imagequant quantize failed: {}", e)),
    };

    if let Err(e) = res.set_dithering_level(cfg.dithering) {
        return QuantizeAttempt::Fallback(format!("imagequant dithering failed: {}", e));
    }

    let (palette, indices) = match res.remapped(&mut liq_img) {
        Ok(r) => r,
        Err(e) => return QuantizeAttempt::Fallback(format!("imagequant remap failed: {}", e)),
    };

    let psnr = compute_palette_psnr_db(raw, &palette, &indices);

    // Encode palette PNG with lodepng (same reliable pattern as mj_atlas).
    let mut encoder = lodepng::Encoder::new();
    encoder.set_auto_convert(false);
    {
        let info = encoder.info_raw_mut();
        info.set_colortype(lodepng::ColorType::PALETTE);
        info.set_bitdepth(8);
        for entry in &palette {
            if let Err(e) = info.palette_add(lodepng::RGBA {
                r: entry.r,
                g: entry.g,
                b: entry.b,
                a: entry.a,
            }) {
                return QuantizeAttempt::Fallback(format!("lodepng palette_add failed: {}", e));
            }
        }
    }
    {
        let info = &mut encoder.info_png_mut().color;
        info.set_colortype(lodepng::ColorType::PALETTE);
        info.set_bitdepth(8);
        for entry in &palette {
            if let Err(e) = info.palette_add(lodepng::RGBA {
                r: entry.r,
                g: entry.g,
                b: entry.b,
                a: entry.a,
            }) {
                return QuantizeAttempt::Fallback(format!("lodepng palette_add failed: {}", e));
            }
        }
    }

    match encoder.encode(&indices, w as usize, h as usize) {
        Ok(bytes) => QuantizeAttempt::Quantized { bytes, psnr },
        Err(e) => QuantizeAttempt::Fallback(format!("lodepng encode failed: {}", e)),
    }
}

fn encode_lossless_png_raw(
    img: &DynamicImage,
    has_real_alpha: bool,
    is_grayscale: bool,
) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    match (is_grayscale, has_real_alpha) {
        (true, false) => {
            let gray = img.to_luma8();
            gray.write_to(&mut cursor, ImageFormat::Png)?;
        }
        (true, true) => {
            let gray_a = img.to_luma_alpha8();
            gray_a.write_to(&mut cursor, ImageFormat::Png)?;
        }
        (false, false) => {
            let rgb = img.to_rgb8();
            rgb.write_to(&mut cursor, ImageFormat::Png)?;
        }
        (false, true) => {
            let rgba = img.to_rgba8();
            rgba.write_to(&mut cursor, ImageFormat::Png)?;
        }
    }
    Ok(buf)
}

fn run_oxipng(png_bytes: &[u8], level: u8, strip_metadata: bool) -> Result<Vec<u8>> {
    let mut opts = oxipng::Options::from_preset(level.min(6));
    if strip_metadata {
        opts.strip = oxipng::StripChunks::Safe;
    }
    oxipng::optimize_from_memory(png_bytes, &opts)
        .map_err(|e| ReError::CustomError(format!("oxipng failed: {}", e)))
}

fn encode_jpg(img: &DynamicImage, quality: u8) -> Result<EncodeOutcome> {
    let rgb = img.to_rgb8();
    let mut buf = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
    enc.encode_image(&rgb)?;
    Ok(EncodeOutcome {
        bytes: buf,
        psnr_db: None,
        quantized: false,
        quantize_fallback: None,
        oxipng_applied: false,
    })
}

fn encode_webp(
    img: &DynamicImage,
    has_real_alpha: bool,
    lossless: bool,
    quality: f32,
) -> Result<EncodeOutcome> {
    let bytes = if has_real_alpha {
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let enc = webp::Encoder::from_rgba(rgba.as_raw(), w, h);
        let mem = if lossless {
            enc.encode_lossless()
        } else {
            enc.encode(quality.clamp(0.0, 100.0))
        };
        mem.to_vec()
    } else {
        let rgb = img.to_rgb8();
        let (w, h) = rgb.dimensions();
        let enc = webp::Encoder::from_rgb(rgb.as_raw(), w, h);
        let mem = if lossless {
            enc.encode_lossless()
        } else {
            enc.encode(quality.clamp(0.0, 100.0))
        };
        mem.to_vec()
    };

    Ok(EncodeOutcome {
        bytes,
        psnr_db: None,
        quantized: false,
        quantize_fallback: None,
        oxipng_applied: false,
    })
}
