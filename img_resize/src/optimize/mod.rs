pub mod analysis;
pub mod codec;
pub mod config;
pub mod presets;

use std::fs;
use std::path::{Path, PathBuf};

use image::{DynamicImage, GenericImageView, ImageFormat};
use rayon::prelude::*;
use serde::Serialize;
use walkdir::WalkDir;

use crate::error::{ReError, Result};
use crate::optimize::analysis::{analyze_image, clean_transparent_rgb, trim_to_bbox};
use crate::optimize::codec::{
    encode_image, resize_if_needed, select_output_format, OutputFormat,
};
use crate::optimize::config::LoadedProfile;

/// Execution options for `img_resize optimize`.
#[derive(Debug, Clone)]
pub struct OptimizeOptions {
    pub profile_path: PathBuf,
    pub input_override: Option<PathBuf>,
    pub out_dir_override: Option<PathBuf>,
    pub dry_run: bool,
    pub report_path: Option<PathBuf>,
    pub jobs: Option<usize>,
    pub json_output: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileMetadata {
    pub engine: String,
    pub channel: String,
    pub preset_used: Option<String>,
    pub config_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileOptimizeResult {
    pub file: String,
    pub relative_path: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_size: Option<(u32, u32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_size: Option<(u32, u32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trim_bbox: Option<[u32; 4]>,
    pub original_bytes: u64,
    pub new_bytes: u64,
    pub saved_bytes: i64,
    pub saved_pct: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_pixel_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox_area_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_alpha: Option<bool>,
    pub fake_alpha_stripped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub psnr_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantize_fallback: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_rules: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditHotspot {
    pub file: String,
    pub original_bytes: u64,
    pub new_bytes: u64,
    pub effective_pixel_ratio: Option<f64>,
    pub bbox_area_ratio: Option<f64>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OptimizeSummary {
    pub total: usize,
    pub optimized: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub failed: usize,
    pub total_original_bytes: u64,
    pub total_output_bytes: u64,
    pub saved_bytes: i64,
    pub saved_pct: f64,
    pub fake_alpha_stripped_count: usize,
    pub low_effective_pixel_count: usize,
    pub quantize_fallback_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct OptimizeReport {
    pub profile: ProfileMetadata,
    pub dry_run: bool,
    pub input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<String>,
    pub summary: OptimizeSummary,
    pub audit_hotspots: Vec<AuditHotspot>,
    pub results: Vec<FileOptimizeResult>,
}

/// Run texture optimization according to `$engine_$channel.yml`.
pub fn run_optimize(opts: &OptimizeOptions) -> Result<OptimizeReport> {
    let profile = LoadedProfile::load(&opts.profile_path)?;

    let input_path = opts
        .input_override
        .clone()
        .or_else(|| profile.input.clone())
        .ok_or_else(|| {
            ReError::Usage(
                "missing input path: pass PATH on CLI or set `input:` in the profile YAML".into(),
            )
        })?;

    if !input_path.exists() {
        return Err(ReError::Input(format!(
            "input path does not exist: {}",
            input_path.display()
        )));
    }

    let output_dir = opts
        .out_dir_override
        .clone()
        .or_else(|| profile.output.clone());
    let dry_run = opts.dry_run || profile.dry_run;
    let input_is_file = input_path.is_file();

    let files = collect_input_files(&input_path, &profile)?;

    if !opts.json_output {
        log::info!(
            "optimize profile={}_{} (preset={:?}) files={} dry_run={}",
            profile.engine,
            profile.channel,
            profile.preset_used.as_deref().unwrap_or("custom"),
            files.len(),
            dry_run
        );
    }

    let process_all = || -> Vec<FileOptimizeResult> {
        files
            .par_iter()
            .map(|(abs_path, rel_path)| {
                process_single_file(
                    abs_path,
                    rel_path,
                    &profile,
                    output_dir.as_deref(),
                    input_is_file,
                    dry_run,
                )
            })
            .collect()
    };

    let results = if let Some(jobs) = opts.jobs {
        if jobs == 0 {
            return Err(ReError::Usage("--jobs must be >= 1".into()));
        }
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(jobs)
            .build()
            .map_err(|e| ReError::CustomError(format!("thread pool init failed: {}", e)))?;
        pool.install(process_all)
    } else {
        process_all()
    };

    let summary = build_summary(&results);
    let audit_hotspots = build_hotspots(&results);

    let report = OptimizeReport {
        profile: ProfileMetadata {
            engine: profile.engine,
            channel: profile.channel,
            preset_used: profile.preset_used,
            config_path: opts.profile_path.display().to_string(),
        },
        dry_run,
        input: input_path.display().to_string(),
        output_dir: output_dir.map(|p| p.display().to_string()),
        summary,
        audit_hotspots,
        results,
    };

    if let Some(report_path) = &opts.report_path {
        if let Some(parent) = report_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let pretty = serde_json::to_string_pretty(&report)?;
        fs::write(report_path, pretty)?;
    }

    Ok(report)
}

fn collect_input_files(input: &Path, profile: &LoadedProfile) -> Result<Vec<(PathBuf, String)>> {
    if input.is_file() {
        let rel = input
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("image")
            .to_string();
        return Ok(vec![(input.to_path_buf(), rel)]);
    }

    let mut collected = Vec::new();
    let walker = WalkDir::new(input)
        .into_iter()
        .filter_entry(|e| !is_hidden_entry(e, input));

    for entry in walker {
        let entry = entry?;
        if !entry.path().is_file() {
            continue;
        }
        let rel_path = entry
            .path()
            .strip_prefix(input)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");

        if profile.is_excluded(&rel_path) {
            continue;
        }
        collected.push((entry.path().to_path_buf(), rel_path));
    }

    collected.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(collected)
}

fn is_hidden_entry(entry: &walkdir::DirEntry, root: &Path) -> bool {
    // Never filter out the root directory itself (even if named `.` or inside a hidden temp dir).
    if entry.path() == root {
        return false;
    }
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.'))
        .unwrap_or(false)
}

fn detect_image_format(bytes: &[u8]) -> Option<ImageFormat> {
    if let Some(kind) = infer::get(bytes) {
        if kind.matcher_type() != infer::MatcherType::Image {
            return None;
        }
        if let Some(fmt) = ImageFormat::from_mime_type(kind.mime_type()) {
            return Some(fmt);
        }
    }
    image::guess_format(bytes).ok()
}

fn process_single_file(
    abs_path: &Path,
    rel_path: &str,
    profile: &LoadedProfile,
    output_dir: Option<&Path>,
    input_is_file: bool,
    dry_run: bool,
) -> FileOptimizeResult {
    let file_str = abs_path.display().to_string();
    let make_error = |orig_bytes: u64, msg: String| FileOptimizeResult {
        file: file_str.clone(),
        relative_path: rel_path.to_string(),
        status: "failed".to_string(),
        output: None,
        original_format: None,
        output_format: None,
        original_size: None,
        new_size: None,
        trim_bbox: None,
        original_bytes: orig_bytes,
        new_bytes: orig_bytes,
        saved_bytes: 0,
        saved_pct: 0.0,
        effective_pixel_ratio: None,
        bbox_area_ratio: None,
        has_alpha: None,
        fake_alpha_stripped: false,
        psnr_db: None,
        quantize_fallback: None,
        matched_rules: Vec::new(),
        actions: Vec::new(),
        warnings: Vec::new(),
        error: Some(msg),
    };

    let orig_bytes = match fs::read(abs_path) {
        Ok(b) => b,
        Err(e) => return make_error(0, format!("read failed: {}", e)),
    };
    let orig_len = orig_bytes.len() as u64;

    let Some(orig_fmt) = detect_image_format(&orig_bytes) else {
        return FileOptimizeResult {
            file: file_str,
            relative_path: rel_path.to_string(),
            status: "skipped".to_string(),
            output: None,
            original_format: None,
            output_format: None,
            original_size: None,
            new_size: None,
            trim_bbox: None,
            original_bytes: orig_len,
            new_bytes: orig_len,
            saved_bytes: 0,
            saved_pct: 0.0,
            effective_pixel_ratio: None,
            bbox_area_ratio: None,
            has_alpha: None,
            fake_alpha_stripped: false,
            psnr_db: None,
            quantize_fallback: None,
            matched_rules: Vec::new(),
            actions: Vec::new(),
            warnings: Vec::new(),
            error: Some("not a supported image file".to_string()),
        };
    };

    let eff = profile.resolve_for(rel_path);
    if eff.skip {
        return FileOptimizeResult {
            file: file_str,
            relative_path: rel_path.to_string(),
            status: "skipped".to_string(),
            output: None,
            original_format: Some(OutputFormat::from_image_format(orig_fmt).as_str().to_string()),
            output_format: None,
            original_size: None,
            new_size: None,
            trim_bbox: None,
            original_bytes: orig_len,
            new_bytes: orig_len,
            saved_bytes: 0,
            saved_pct: 0.0,
            effective_pixel_ratio: None,
            bbox_area_ratio: None,
            has_alpha: None,
            fake_alpha_stripped: false,
            psnr_db: None,
            quantize_fallback: None,
            matched_rules: eff.matched_rules,
            actions: vec!["skipped_by_rule".to_string()],
            warnings: Vec::new(),
            error: None,
        };
    }

    let decoded = match image::load_from_memory_with_format(&orig_bytes, orig_fmt)
        .or_else(|_| image::load_from_memory(&orig_bytes))
    {
        Ok(im) => im,
        Err(e) => return make_error(orig_len, format!("decode failed: {}", e)),
    };

    let orig_size = decoded.dimensions();
    let orig_analysis = analyze_image(&decoded, &eff.effective_pixels);

    let mut actions: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    if orig_analysis.has_real_alpha {
        if orig_analysis.effective_pixel_ratio < eff.effective_pixels.warn_effective_ratio_below {
            warnings.push(format!(
                "low_effective_pixel_ratio ({:.1}% < {:.1}% — consider atlas packing with mj_atlas)",
                orig_analysis.effective_pixel_ratio * 100.0,
                eff.effective_pixels.warn_effective_ratio_below * 100.0
            ));
        }
        if orig_analysis.bbox_area_ratio < eff.effective_pixels.warn_bbox_ratio_below {
            warnings.push(format!(
                "large_transparent_padding (bbox covers only {:.1}% of canvas)",
                orig_analysis.bbox_area_ratio * 100.0
            ));
        }
    }

    // Step 1: Optional transparent trim (OFF by default to protect Godot UV/pivot/anchor).
    let mut working = decoded;
    let mut trim_bbox: Option<[u32; 4]> = None;
    let mut trimmed = false;
    if eff.effective_pixels.trim_transparent && orig_analysis.has_real_alpha {
        if let Some(bbox) = orig_analysis.tight_bbox {
            if let Some((cropped, applied_bbox)) =
                trim_to_bbox(&working, bbox, eff.effective_pixels.trim_padding)
            {
                working = cropped;
                trim_bbox = Some(applied_bbox);
                trimmed = true;
                actions.push("trim_transparent".to_string());
            }
        }
    }

    // Step 2: Downscale & align to limits.
    let (resized_img, resized) = resize_if_needed(working, &eff.limits);
    working = resized_img;
    if resized {
        actions.push(format!("resize_{}x{}", working.width(), working.height()));
    }

    // Step 3: Safe alpha/transparent RGB cleanup (zero visual or layout change).
    let mut fake_alpha_stripped = false;
    let effective_has_alpha = if orig_analysis.has_real_alpha {
        if eff.effective_pixels.zero_transparent_rgb {
            let mut rgba = working.to_rgba8();
            if clean_transparent_rgb(
                &mut rgba,
                eff.effective_pixels.alpha_threshold,
                eff.effective_pixels.edge_bleed_px,
            ) {
                actions.push("zero_transparent_rgb".to_string());
            }
            working = DynamicImage::ImageRgba8(rgba);
        }
        true
    } else {
        if eff.effective_pixels.strip_fake_alpha && orig_analysis.source_has_alpha_channel {
            working = DynamicImage::ImageRgb8(working.to_rgb8());
            fake_alpha_stripped = true;
            actions.push("strip_fake_alpha".to_string());
        }
        false
    };

    // Re-evaluate palette/grayscale on final image if geometry changed (resizing can introduce colors).
    let (fits_256_palette, is_grayscale) = if resized || trimmed {
        let post = analyze_image(&working, &eff.effective_pixels);
        (post.fits_256_palette, post.is_grayscale)
    } else {
        (orig_analysis.fits_256_palette, orig_analysis.is_grayscale)
    };

    // Step 4: Choose output format.
    let orig_out_fmt = OutputFormat::from_image_format(orig_fmt);
    let target_fmt = select_output_format(orig_fmt, effective_has_alpha, &eff.format);
    let format_changed = target_fmt != orig_out_fmt
        || !matches!(
            orig_fmt,
            ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
        );
    if format_changed {
        actions.push(format!("convert_to_{}", target_fmt.as_str()));
    }

    // Audit suggestion for large opaque PNGs kept as PNG.
    if orig_fmt == ImageFormat::Png
        && !effective_has_alpha
        && target_fmt == OutputFormat::Png
        && orig_len >= 64 * 1024
        && orig_size.0.max(orig_size.1) >= 512
    {
        warnings.push(
            "opaque_png_candidate_for_jpg_or_webp (large opaque PNG; consider opaque_png_to: jpg/webp for CDN assets)"
                .to_string(),
        );
    }

    // Step 5: Encode & compress.
    let encoded = match encode_image(
        &working,
        target_fmt,
        effective_has_alpha,
        fits_256_palette,
        is_grayscale,
        &eff.compression,
    ) {
        Ok(e) => e,
        Err(e) => return make_error(orig_len, format!("encode failed: {}", e)),
    };

    if encoded.quantized {
        actions.push("png_quantize".to_string());
    }
    if encoded.oxipng_applied {
        actions.push("oxipng".to_string());
    }
    if encoded.quantize_fallback.is_some() {
        actions.push("quantize_fallback_lossless".to_string());
    }

    let encoded_len = encoded.bytes.len() as u64;
    let raw_saving_pct = if orig_len == 0 {
        0.0
    } else {
        (orig_len as f64 - encoded_len as f64) / orig_len as f64 * 100.0
    };

    // Step 6: No-regression guard.
    let keep_original = !resized
        && !trimmed
        && !format_changed
        && eff.format.no_regression
        && (encoded_len >= orig_len || raw_saving_pct < eff.format.min_saving_pct);

    let (final_bytes, status, final_fmt) = if keep_original {
        actions.push("kept_original_no_regression".to_string());
        (&orig_bytes[..], "unchanged", orig_out_fmt)
    } else {
        (&encoded.bytes[..], "optimized", target_fmt)
    };

    let dest_path = compute_dest_path(
        abs_path,
        rel_path,
        output_dir,
        input_is_file,
        profile.preserve_dir_structure,
        format_changed && !keep_original,
        final_fmt,
    );

    if !dry_run {
        let should_write = !(keep_original && dest_path == abs_path);
        if should_write {
            if let Some(parent) = dest_path.parent() {
                if !parent.as_os_str().is_empty() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        return make_error(orig_len, format!("create_dir_all failed: {}", e));
                    }
                }
            }
            if let Err(e) = fs::write(&dest_path, final_bytes) {
                return make_error(orig_len, format!("write failed: {}", e));
            }
        }
    }

    let final_len = final_bytes.len() as u64;
    let saved_bytes = orig_len as i64 - final_len as i64;
    let saved_pct = if orig_len == 0 {
        0.0
    } else {
        (saved_bytes as f64 / orig_len as f64) * 100.0
    };

    FileOptimizeResult {
        file: file_str,
        relative_path: rel_path.to_string(),
        status: status.to_string(),
        output: Some(dest_path.display().to_string()),
        original_format: Some(orig_out_fmt.as_str().to_string()),
        output_format: Some(final_fmt.as_str().to_string()),
        original_size: Some(orig_size),
        new_size: Some(working.dimensions()),
        trim_bbox,
        original_bytes: orig_len,
        new_bytes: final_len,
        saved_bytes,
        saved_pct: (saved_pct * 100.0).round() / 100.0,
        effective_pixel_ratio: Some((orig_analysis.effective_pixel_ratio * 10000.0).round() / 10000.0),
        bbox_area_ratio: Some((orig_analysis.bbox_area_ratio * 10000.0).round() / 10000.0),
        has_alpha: Some(orig_analysis.has_real_alpha),
        fake_alpha_stripped,
        psnr_db: encoded.psnr_db.map(|v| {
            if v.is_finite() {
                (v * 100.0).round() / 100.0
            } else {
                99.99
            }
        }),
        quantize_fallback: encoded.quantize_fallback,
        matched_rules: eff.matched_rules,
        actions,
        warnings,
        error: None,
    }
}

fn compute_dest_path(
    abs_path: &Path,
    rel_path: &str,
    output_dir: Option<&Path>,
    input_is_file: bool,
    preserve_dir_structure: bool,
    change_ext: bool,
    final_fmt: OutputFormat,
) -> PathBuf {
    let base = match output_dir {
        Some(dir) => {
            if input_is_file || !preserve_dir_structure {
                let fname = abs_path
                    .file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new("image"));
                dir.join(fname)
            } else {
                dir.join(rel_path)
            }
        }
        None => abs_path.to_path_buf(),
    };

    if change_ext {
        base.with_extension(final_fmt.extension())
    } else {
        base
    }
}

fn build_summary(results: &[FileOptimizeResult]) -> OptimizeSummary {
    let mut optimized = 0usize;
    let mut unchanged = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut total_original_bytes = 0u64;
    let mut total_output_bytes = 0u64;
    let mut fake_alpha_stripped_count = 0usize;
    let mut low_effective_pixel_count = 0usize;
    let mut quantize_fallback_count = 0usize;

    for r in results {
        match r.status.as_str() {
            "optimized" => {
                optimized += 1;
                total_original_bytes += r.original_bytes;
                total_output_bytes += r.new_bytes;
            }
            "unchanged" => {
                unchanged += 1;
                total_original_bytes += r.original_bytes;
                total_output_bytes += r.new_bytes;
            }
            "skipped" => skipped += 1,
            "failed" => failed += 1,
            _ => {}
        }
        if r.fake_alpha_stripped {
            fake_alpha_stripped_count += 1;
        }
        if r.warnings
            .iter()
            .any(|w| w.starts_with("low_effective_pixel_ratio"))
        {
            low_effective_pixel_count += 1;
        }
        if r.quantize_fallback.is_some() {
            quantize_fallback_count += 1;
        }
    }

    let saved_bytes = total_original_bytes as i64 - total_output_bytes as i64;
    let saved_pct = if total_original_bytes == 0 {
        0.0
    } else {
        ((saved_bytes as f64 / total_original_bytes as f64) * 10000.0).round() / 100.0
    };

    OptimizeSummary {
        total: results.len(),
        optimized,
        unchanged,
        skipped,
        failed,
        total_original_bytes,
        total_output_bytes,
        saved_bytes,
        saved_pct,
        fake_alpha_stripped_count,
        low_effective_pixel_count,
        quantize_fallback_count,
    }
}

fn build_hotspots(results: &[FileOptimizeResult]) -> Vec<AuditHotspot> {
    let mut candidates: Vec<AuditHotspot> = results
        .iter()
        .filter(|r| !r.warnings.is_empty())
        .map(|r| AuditHotspot {
            file: r.relative_path.clone(),
            original_bytes: r.original_bytes,
            new_bytes: r.new_bytes,
            effective_pixel_ratio: r.effective_pixel_ratio,
            bbox_area_ratio: r.bbox_area_ratio,
            warnings: r.warnings.clone(),
        })
        .collect();

    // Sort by largest output bytes first so the biggest optimization opportunities surface at top.
    candidates.sort_by(|a, b| b.new_bytes.cmp(&a.new_bytes));
    candidates.truncate(20);
    candidates
}
