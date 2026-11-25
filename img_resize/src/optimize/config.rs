use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};

use crate::error::{ReError, Result};
use crate::optimize::presets;

/// Resampling filter used when downscaling textures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownscaleFilter {
    Lanczos3,
    CatmullRom,
    Triangle,
    Nearest,
}

impl DownscaleFilter {
    pub fn to_image_filter(self) -> image::imageops::FilterType {
        match self {
            Self::Lanczos3 => image::imageops::FilterType::Lanczos3,
            Self::CatmullRom => image::imageops::FilterType::CatmullRom,
            Self::Triangle => image::imageops::FilterType::Triangle,
            Self::Nearest => image::imageops::FilterType::Nearest,
        }
    }
}

/// Target format directive for format conversion rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormatTarget {
    Keep,
    Png,
    Jpg,
    Webp,
}

/// PNG compression strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PngMode {
    /// Lossless PNG encoding + oxipng recompression only.
    Lossless,
    /// Perceptual palette quantization (imagequant) with optional PSNR guard + oxipng.
    Quantize,
}

/// Resolved dimension limits for a single image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    /// Longest-side pixel cap (0 = unlimited). Never upscales.
    pub max_dimension: u32,
    /// Optional explicit width cap (applied in addition to `max_dimension`).
    pub max_width: Option<u32>,
    /// Optional explicit height cap (applied in addition to `max_dimension`).
    pub max_height: Option<u32>,
    /// Align resized dimensions down to a multiple of N (0 or 1 = disabled).
    pub align_multiple: u32,
    /// If true, align dimensions even when image is already under `max_dimension`.
    pub force_align: bool,
    /// Downscale resampling filter.
    pub downscale_filter: DownscaleFilter,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitsPatch {
    pub max_dimension: Option<u32>,
    pub max_width: Option<Option<u32>>,
    pub max_height: Option<Option<u32>>,
    pub align_multiple: Option<u32>,
    pub force_align: Option<bool>,
    pub downscale_filter: Option<DownscaleFilter>,
}

impl LimitsConfig {
    pub fn apply(&mut self, patch: &LimitsPatch) {
        if let Some(v) = patch.max_dimension {
            self.max_dimension = v;
        }
        if let Some(v) = patch.max_width {
            self.max_width = v;
        }
        if let Some(v) = patch.max_height {
            self.max_height = v;
        }
        if let Some(v) = patch.align_multiple {
            self.align_multiple = v;
        }
        if let Some(v) = patch.force_align {
            self.force_align = v;
        }
        if let Some(v) = patch.downscale_filter {
            self.downscale_filter = v;
        }
    }
}

/// Resolved effective-pixel & alpha cleanup settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectivePixelsConfig {
    /// Alpha <= this threshold is treated as fully transparent (default: 4).
    pub alpha_threshold: u8,
    /// If all pixels have alpha >= this threshold, image is treated as fully opaque (default: 252).
    pub opaque_threshold: u8,
    /// Drop alpha channel (RGBA -> RGB) when image has no real transparency.
    pub strip_fake_alpha: bool,
    /// Zero RGB values on transparent pixels to boost lossless PNG/WebP compression.
    pub zero_transparent_rgb: bool,
    /// Dilate visible edge RGB into adjacent transparent pixels (in px) before zeroing the rest,
    /// preventing dark fringes under bilinear GPU texture sampling.
    pub edge_bleed_px: u32,
    /// Emit an audit warning when `effective_pixels / total_pixels < threshold`.
    pub warn_effective_ratio_below: f64,
    /// Emit an audit warning when `tight_bbox_area / total_area < threshold`.
    pub warn_bbox_ratio_below: f64,
    /// Trim transparent borders. Default false to protect Godot pivots, anchors, 9-patch & UVs.
    /// Enable selectively via `rules:` for safe directories.
    pub trim_transparent: bool,
    /// Padding in pixels kept around tight bbox when `trim_transparent` is true.
    pub trim_padding: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectivePixelsPatch {
    pub alpha_threshold: Option<u8>,
    pub opaque_threshold: Option<u8>,
    pub strip_fake_alpha: Option<bool>,
    pub zero_transparent_rgb: Option<bool>,
    pub edge_bleed_px: Option<u32>,
    pub warn_effective_ratio_below: Option<f64>,
    pub warn_bbox_ratio_below: Option<f64>,
    pub trim_transparent: Option<bool>,
    pub trim_padding: Option<u32>,
}

impl EffectivePixelsConfig {
    pub fn apply(&mut self, patch: &EffectivePixelsPatch) {
        if let Some(v) = patch.alpha_threshold {
            self.alpha_threshold = v;
        }
        if let Some(v) = patch.opaque_threshold {
            self.opaque_threshold = v;
        }
        if let Some(v) = patch.strip_fake_alpha {
            self.strip_fake_alpha = v;
        }
        if let Some(v) = patch.zero_transparent_rgb {
            self.zero_transparent_rgb = v;
        }
        if let Some(v) = patch.edge_bleed_px {
            self.edge_bleed_px = v;
        }
        if let Some(v) = patch.warn_effective_ratio_below {
            self.warn_effective_ratio_below = v;
        }
        if let Some(v) = patch.warn_bbox_ratio_below {
            self.warn_bbox_ratio_below = v;
        }
        if let Some(v) = patch.trim_transparent {
            self.trim_transparent = v;
        }
        if let Some(v) = patch.trim_padding {
            self.trim_padding = v;
        }
    }
}

/// Resolved format conversion and byte-regression settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatConfig {
    /// Target format for PNG images that have NO real transparency.
    pub opaque_png_to: FormatTarget,
    /// Target format for PNG images that DO have transparency.
    pub alpha_png_to: FormatTarget,
    /// Target format for JPEG inputs.
    pub jpg_to: FormatTarget,
    /// Target format for WebP inputs.
    pub webp_to: FormatTarget,
    /// Keep original file bytes if encoded output is not smaller (when not resized/trimmed/converted).
    pub no_regression: bool,
    /// Minimum percentage byte saving required to rewrite an unresized/unconverted image.
    pub min_saving_pct: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatPatch {
    pub opaque_png_to: Option<FormatTarget>,
    pub alpha_png_to: Option<FormatTarget>,
    pub jpg_to: Option<FormatTarget>,
    pub webp_to: Option<FormatTarget>,
    pub no_regression: Option<bool>,
    pub min_saving_pct: Option<f64>,
}

impl FormatConfig {
    pub fn apply(&mut self, patch: &FormatPatch) {
        if let Some(v) = patch.opaque_png_to {
            self.opaque_png_to = v;
        }
        if let Some(v) = patch.alpha_png_to {
            self.alpha_png_to = v;
        }
        if let Some(v) = patch.jpg_to {
            self.jpg_to = v;
        }
        if let Some(v) = patch.webp_to {
            self.webp_to = v;
        }
        if let Some(v) = patch.no_regression {
            self.no_regression = v;
        }
        if let Some(v) = patch.min_saving_pct {
            self.min_saving_pct = v;
        }
    }
}

/// PNG encoder & optimizer parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PngCompressionConfig {
    pub mode: PngMode,
    /// imagequant minimum acceptable quality (0..=100).
    pub quality_min: u8,
    /// imagequant target maximum quality (0..=100).
    pub quality_max: u8,
    /// imagequant speed (1 = slowest/best, 10 = fastest).
    pub speed: i32,
    /// Floyd-Steinberg dithering level (0.0..=1.0).
    pub dithering: f32,
    /// Perceptual quality safety guard in dB PSNR (e.g. 38.0).
    /// If quantized RGBA PSNR falls below this value, automatically falls back to lossless PNG.
    pub min_psnr_db: Option<f64>,
    /// oxipng optimization preset level (0 = disabled, 1..=6).
    pub oxipng_level: u8,
    /// Strip non-essential PNG ancillary chunks (tEXt, iTXt, tIME, pHYs, etc.).
    pub strip_metadata: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PngCompressionPatch {
    pub mode: Option<PngMode>,
    pub quality_min: Option<u8>,
    pub quality_max: Option<u8>,
    pub speed: Option<i32>,
    pub dithering: Option<f32>,
    pub min_psnr_db: Option<Option<f64>>,
    pub oxipng_level: Option<u8>,
    pub strip_metadata: Option<bool>,
}

impl PngCompressionConfig {
    pub fn apply(&mut self, patch: &PngCompressionPatch) {
        if let Some(v) = patch.mode {
            self.mode = v;
        }
        if let Some(v) = patch.quality_min {
            self.quality_min = v;
        }
        if let Some(v) = patch.quality_max {
            self.quality_max = v;
        }
        if let Some(v) = patch.speed {
            self.speed = v;
        }
        if let Some(v) = patch.dithering {
            self.dithering = v;
        }
        if let Some(v) = patch.min_psnr_db {
            self.min_psnr_db = v;
        }
        if let Some(v) = patch.oxipng_level {
            self.oxipng_level = v;
        }
        if let Some(v) = patch.strip_metadata {
            self.strip_metadata = v;
        }
    }
}

/// JPEG encoder parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JpgCompressionConfig {
    /// JPEG quality (1..=100).
    pub quality: u8,
    /// Strip EXIF/ICC/XMP metadata (always stripped on re-encode).
    pub strip_metadata: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JpgCompressionPatch {
    pub quality: Option<u8>,
    pub strip_metadata: Option<bool>,
}

impl JpgCompressionConfig {
    pub fn apply(&mut self, patch: &JpgCompressionPatch) {
        if let Some(v) = patch.quality {
            self.quality = v;
        }
        if let Some(v) = patch.strip_metadata {
            self.strip_metadata = v;
        }
    }
}

/// WebP encoder parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebpCompressionConfig {
    /// Encode lossless VP8L instead of lossy VP8.
    pub lossless: bool,
    /// Lossy WebP quality (0.0..=100.0).
    pub quality: f32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebpCompressionPatch {
    pub lossless: Option<bool>,
    pub quality: Option<f32>,
}

impl WebpCompressionConfig {
    pub fn apply(&mut self, patch: &WebpCompressionPatch) {
        if let Some(v) = patch.lossless {
            self.lossless = v;
        }
        if let Some(v) = patch.quality {
            self.quality = v;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionConfig {
    pub png: PngCompressionConfig,
    pub jpg: JpgCompressionConfig,
    pub webp: WebpCompressionConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompressionPatch {
    pub png: Option<PngCompressionPatch>,
    pub jpg: Option<JpgCompressionPatch>,
    pub webp: Option<WebpCompressionPatch>,
}

impl CompressionConfig {
    pub fn apply(&mut self, patch: &CompressionPatch) {
        if let Some(p) = &patch.png {
            self.png.apply(p);
        }
        if let Some(p) = &patch.jpg {
            self.jpg.apply(p);
        }
        if let Some(p) = &patch.webp {
            self.webp.apply(p);
        }
    }
}

/// One path-scoped rule override in YAML.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSpec {
    /// Glob pattern matched against relative path (forward slashes), e.g. `**/*_normal.*` or `bg/**`.
    #[serde(rename = "match")]
    pub pattern: String,
    #[serde(default)]
    pub skip: Option<bool>,
    #[serde(default)]
    pub limits: Option<LimitsPatch>,
    #[serde(default)]
    pub effective_pixels: Option<EffectivePixelsPatch>,
    #[serde(default)]
    pub format: Option<FormatPatch>,
    #[serde(default)]
    pub compression: Option<CompressionPatch>,
}

/// Compiled rule ready for fast matching.
pub struct CompiledRule {
    pub pattern: String,
    pub matcher: GlobMatcher,
    pub spec: RuleSpec,
}

/// Raw YAML file representation (`$engine_$channel.yml`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawProfileYaml {
    pub engine: Option<String>,
    pub channel: Option<String>,
    pub input: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub preserve_dir_structure: Option<bool>,
    pub dry_run: Option<bool>,
    pub inherit_default_rules: Option<bool>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub limits: Option<LimitsPatch>,
    #[serde(default)]
    pub effective_pixels: Option<EffectivePixelsPatch>,
    #[serde(default)]
    pub format: Option<FormatPatch>,
    #[serde(default)]
    pub compression: Option<CompressionPatch>,
    #[serde(default)]
    pub rules: Vec<RuleSpec>,
}

/// Effective optimization parameters for a specific file after merging preset + YAML + matching rules.
#[derive(Debug, Clone, Serialize)]
pub struct EffectiveSettings {
    pub skip: bool,
    pub limits: LimitsConfig,
    pub effective_pixels: EffectivePixelsConfig,
    pub format: FormatConfig,
    pub compression: CompressionConfig,
    pub matched_rules: Vec<String>,
}

/// Fully loaded and validated profile ready to execute.
pub struct LoadedProfile {
    pub config_path: PathBuf,
    pub engine: String,
    pub channel: String,
    pub preset_used: Option<String>,
    pub input: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub preserve_dir_structure: bool,
    pub dry_run: bool,
    pub exclude_matchers: Vec<GlobMatcher>,
    pub base_limits: LimitsConfig,
    pub base_effective_pixels: EffectivePixelsConfig,
    pub base_format: FormatConfig,
    pub base_compression: CompressionConfig,
    pub rules: Vec<CompiledRule>,
}

impl LoadedProfile {
    /// Load `$engine_$channel.yml`, infer engine/channel from filename if omitted,
    /// layer preset defaults underneath YAML overrides, and compile glob rules.
    pub fn load(config_path: &Path) -> Result<Self> {
        let content = fs::read_to_string(config_path).map_err(|e| {
            ReError::Input(format!(
                "cannot read profile config {}: {}",
                config_path.display(),
                e
            ))
        })?;

        let raw: RawProfileYaml = if content.trim().is_empty() {
            RawProfileYaml::default()
        } else {
            serde_yaml_ng::from_str(&content).map_err(|e| {
                ReError::Input(format!(
                    "invalid YAML in profile {}: {}",
                    config_path.display(),
                    e
                ))
            })?
        };

        let (inferred_engine, inferred_channel) = infer_engine_channel_from_filename(config_path);

        let engine = raw
            .engine
            .or(inferred_engine)
            .ok_or_else(|| {
                ReError::Input(format!(
                    "profile {} must specify `engine` or follow `$engine_$channel.yml` naming (e.g. godot_wx-minip.yml)",
                    config_path.display()
                ))
            })?
            .to_lowercase();

        let channel_raw = raw
            .channel
            .or(inferred_channel)
            .ok_or_else(|| {
                ReError::Input(format!(
                    "profile {} must specify `channel` or follow `$engine_$channel.yml` naming (e.g. godot_wx-minip.yml)",
                    config_path.display()
                ))
            })?;
        let channel = normalize_channel(&channel_raw);

        let preset_bundle = presets::resolve_preset(&engine, &channel);
        let preset_used = preset_bundle.as_ref().map(|p| p.id.to_string());

        let mut base = preset_bundle
            .map(|p| p.build())
            .unwrap_or_else(presets::default_fallback_bundle);

        if let Some(p) = &raw.limits {
            base.limits.apply(p);
        }
        if let Some(p) = &raw.effective_pixels {
            base.effective_pixels.apply(p);
        }
        if let Some(p) = &raw.format {
            base.format.apply(p);
        }
        if let Some(p) = &raw.compression {
            base.compression.apply(p);
        }

        // Validate numeric ranges up front so errors are caught cleanly as Input (exit 3).
        validate_settings(&base.limits, &base.effective_pixels, &base.compression)?;

        let inherit_rules = raw.inherit_default_rules.unwrap_or(true);
        let mut all_rule_specs = if inherit_rules {
            base.default_rules
        } else {
            Vec::new()
        };
        all_rule_specs.extend(raw.rules);

        let mut rules = Vec::with_capacity(all_rule_specs.len());
        for spec in all_rule_specs {
            let matcher = Glob::new(&spec.pattern)
                .map_err(|e| {
                    ReError::Input(format!(
                        "invalid glob pattern {:?} in {}: {}",
                        spec.pattern,
                        config_path.display(),
                        e
                    ))
                })?
                .compile_matcher();
            rules.push(CompiledRule {
                pattern: spec.pattern.clone(),
                matcher,
                spec,
            });
        }

        let mut excludes = base.default_excludes;
        excludes.extend(raw.exclude);
        let mut exclude_matchers = Vec::with_capacity(excludes.len());
        for pat in excludes {
            let m = Glob::new(&pat)
                .map_err(|e| {
                    ReError::Input(format!(
                        "invalid exclude glob {:?} in {}: {}",
                        pat,
                        config_path.display(),
                        e
                    ))
                })?
                .compile_matcher();
            exclude_matchers.push(m);
        }

        // Resolve relative input/output paths against the YAML file's parent directory?
        // Keep them as given (standard CLI expectation relative to cwd).
        Ok(Self {
            config_path: config_path.to_path_buf(),
            engine,
            channel,
            preset_used,
            input: raw.input,
            output: raw.output,
            preserve_dir_structure: raw.preserve_dir_structure.unwrap_or(true),
            dry_run: raw.dry_run.unwrap_or(false),
            exclude_matchers,
            base_limits: base.limits,
            base_effective_pixels: base.effective_pixels,
            base_format: base.format,
            base_compression: base.compression,
            rules,
        })
    }

    /// Returns true if a relative path (normalized with forward slashes) matches any exclude pattern.
    pub fn is_excluded(&self, rel_path: &str) -> bool {
        self.exclude_matchers.iter().any(|m| m.is_match(rel_path))
    }

    /// Resolve effective settings for a given relative path (forward slashes).
    pub fn resolve_for(&self, rel_path: &str) -> EffectiveSettings {
        let mut eff = EffectiveSettings {
            skip: false,
            limits: self.base_limits.clone(),
            effective_pixels: self.base_effective_pixels.clone(),
            format: self.base_format.clone(),
            compression: self.base_compression.clone(),
            matched_rules: Vec::new(),
        };

        for rule in &self.rules {
            if rule.matcher.is_match(rel_path) {
                eff.matched_rules.push(rule.pattern.clone());
                if let Some(s) = rule.spec.skip {
                    eff.skip = s;
                }
                if let Some(p) = &rule.spec.limits {
                    eff.limits.apply(p);
                }
                if let Some(p) = &rule.spec.effective_pixels {
                    eff.effective_pixels.apply(p);
                }
                if let Some(p) = &rule.spec.format {
                    eff.format.apply(p);
                }
                if let Some(p) = &rule.spec.compression {
                    eff.compression.apply(p);
                }
            }
        }

        eff
    }
}

/// Infer `(engine, channel)` from `$engine_$channel.yml` / `.yaml`.
pub fn infer_engine_channel_from_filename(path: &Path) -> (Option<String>, Option<String>) {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return (None, None);
    };
    let Some((eng, chan)) = stem.split_once('_') else {
        return (None, None);
    };
    if eng.is_empty() || chan.is_empty() {
        return (None, None);
    }
    (Some(eng.to_lowercase()), Some(chan.to_lowercase()))
}

/// Canonicalize common channel aliases (e.g. `wx_minip` / `wx` -> `wx-minip`).
pub fn normalize_channel(raw: &str) -> String {
    let lower = raw.trim().to_lowercase().replace('_', "-");
    match lower.as_str() {
        "wx" | "wechat" | "wx-minip" | "wxminip" | "wechat-minigame" => "wx-minip".to_string(),
        "qq" | "qq-minip" | "qqminip" | "qq-minigame" => "qq-minip".to_string(),
        "dy" | "douyin" | "tt" | "dy-minip" | "dyminip" | "tt-minip" => "dy-minip".to_string(),
        "zfb" | "alipay" | "zfb-minip" | "zfbminip" => "zfb-minip".to_string(),
        "mobile" | "ios" | "android" => "mobile".to_string(),
        "png-jpg" | "pngjpg" | "generic" => "png-jpg".to_string(),
        _ => lower,
    }
}

fn validate_settings(
    _limits: &LimitsConfig,
    eff: &EffectivePixelsConfig,
    comp: &CompressionConfig,
) -> Result<()> {
    if comp.png.quality_min > 100 || comp.png.quality_max > 100 {
        return Err(ReError::Input(
            "compression.png quality_min/quality_max must be in 0..=100".into(),
        ));
    }
    if comp.png.quality_min > comp.png.quality_max {
        return Err(ReError::Input(
            "compression.png.quality_min cannot exceed quality_max".into(),
        ));
    }
    if !(1..=10).contains(&comp.png.speed) {
        return Err(ReError::Input(
            "compression.png.speed must be in 1..=10".into(),
        ));
    }
    if !(0.0..=1.0).contains(&comp.png.dithering) {
        return Err(ReError::Input(
            "compression.png.dithering must be in 0.0..=1.0".into(),
        ));
    }
    if comp.png.oxipng_level > 6 {
        return Err(ReError::Input(
            "compression.png.oxipng_level must be in 0..=6".into(),
        ));
    }
    if comp.jpg.quality == 0 || comp.jpg.quality > 100 {
        return Err(ReError::Input(
            "compression.jpg.quality must be in 1..=100".into(),
        ));
    }
    if !(0.0..=100.0).contains(&comp.webp.quality) {
        return Err(ReError::Input(
            "compression.webp.quality must be in 0.0..=100.0".into(),
        ));
    }
    if !(0.0..=1.0).contains(&eff.warn_effective_ratio_below)
        || !(0.0..=1.0).contains(&eff.warn_bbox_ratio_below)
    {
        return Err(ReError::Input(
            "effective_pixels warning ratio thresholds must be in 0.0..=1.0".into(),
        ));
    }
    Ok(())
}
