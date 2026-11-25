use serde::Serialize;

use crate::optimize::config::{
    CompressionConfig, CompressionPatch, DownscaleFilter, EffectivePixelsConfig,
    EffectivePixelsPatch, FormatConfig, FormatPatch, FormatTarget, JpgCompressionConfig,
    LimitsConfig, LimitsPatch, PngCompressionConfig, PngCompressionPatch, PngMode, RuleSpec,
    WebpCompressionConfig,
};

/// Resolved preset defaults bundle.
pub struct PresetBundle {
    pub limits: LimitsConfig,
    pub effective_pixels: EffectivePixelsConfig,
    pub format: FormatConfig,
    pub compression: CompressionConfig,
    pub default_rules: Vec<RuleSpec>,
    pub default_excludes: Vec<String>,
}

/// Public descriptor of a built-in `$engine_$channel` preset.
#[derive(Debug, Clone, Serialize)]
pub struct PresetInfo {
    pub id: &'static str,
    pub engine: &'static str,
    pub channel: &'static str,
    pub filename: &'static str,
    pub summary: &'static str,
    pub max_dimension: u32,
    pub png_mode: PngMode,
    pub png_quality: (u8, u8),
    pub jpg_quality: u8,
    pub webp_quality: f32,
}

pub struct PresetEntry {
    pub id: &'static str,
    pub engine: &'static str,
    pub channel: &'static str,
    pub filename: &'static str,
    pub summary: &'static str,
    builder: fn() -> PresetBundle,
}

impl PresetEntry {
    pub fn build(&self) -> PresetBundle {
        (self.builder)()
    }

    pub fn info(&self) -> PresetInfo {
        let b = self.build();
        PresetInfo {
            id: self.id,
            engine: self.engine,
            channel: self.channel,
            filename: self.filename,
            summary: self.summary,
            max_dimension: b.limits.max_dimension,
            png_mode: b.compression.png.mode,
            png_quality: (b.compression.png.quality_min, b.compression.png.quality_max),
            jpg_quality: b.compression.jpg.quality,
            webp_quality: b.compression.webp.quality,
        }
    }
}

pub static PRESETS: &[PresetEntry] = &[
    PresetEntry {
        id: "godot_wx-minip",
        engine: "godot",
        channel: "wx-minip",
        filename: "godot_wx-minip.yml",
        summary: "Godot WASM -> WeChat Mini Game (4MB main pkg, strict iOS WASM/WebGL RAM; 1024px cap, perceptual PNG quantize + PSNR guard + oxipng)",
        builder: preset_godot_wx_minip,
    },
    PresetEntry {
        id: "godot_qq-minip",
        engine: "godot",
        channel: "qq-minip",
        filename: "godot_qq-minip.yml",
        summary: "Godot WASM -> QQ Mini Game (4MB main pkg & WASM memory budget similar to WeChat)",
        builder: preset_godot_qq_minip,
    },
    PresetEntry {
        id: "godot_dy-minip",
        engine: "godot",
        channel: "dy-minip",
        filename: "godot_dy-minip.yml",
        summary: "Godot WASM -> Douyin Mini Game (4MB main pkg, faster first-frame download & decode)",
        builder: preset_godot_dy_minip,
    },
    PresetEntry {
        id: "godot_zfb-minip",
        engine: "godot",
        channel: "zfb-minip",
        filename: "godot_zfb-minip.yml",
        summary: "Godot WASM -> Alipay Mini Game (strict pkg budget, conservative WebGL texture cap)",
        builder: preset_godot_zfb_minip,
    },
    PresetEntry {
        id: "godot_mobile",
        engine: "godot",
        channel: "mobile",
        filename: "godot_mobile.yml",
        summary: "Godot Native Mobile (iOS/Android; preserves lossless source fidelity for ASTC/ETC2 re-encoding, 2048px cap)",
        builder: preset_godot_mobile,
    },
    PresetEntry {
        id: "raw_png-jpg",
        engine: "raw",
        channel: "png-jpg",
        filename: "raw_png-jpg.yml",
        summary: "Generic PNG/JPG channel (no forced downscale, perceptual PNG quantize + oxipng + JPG re-encode)",
        builder: preset_raw_png_jpg,
    },
];

pub fn list_presets() -> Vec<PresetInfo> {
    PRESETS.iter().map(PresetEntry::info).collect()
}

pub fn resolve_preset(engine: &str, channel: &str) -> Option<&'static PresetEntry> {
    PRESETS
        .iter()
        .find(|p| p.engine == engine && p.channel == channel)
}

pub fn find_preset_by_name(name: &str) -> Option<&'static PresetEntry> {
    let cleaned = name
        .trim()
        .trim_end_matches(".yaml")
        .trim_end_matches(".yml")
        .to_lowercase();
    if let Some((eng, chan)) = cleaned.split_once('_') {
        let norm_chan = crate::optimize::config::normalize_channel(chan);
        return resolve_preset(eng, &norm_chan);
    }
    PRESETS.iter().find(|p| p.id == cleaned)
}

/// Shared protection rules for Godot projects:
/// normal/ORM/mask/height maps, bitmap fonts, and pixel-art assets must NOT be lossily quantized.
fn godot_default_protection_rules() -> Vec<RuleSpec> {
    vec![
        RuleSpec {
            pattern: "**/*_{normal,n,norm,orm,roughness,metallic,mask,height,bump,ao}.*".into(),
            skip: None,
            limits: None,
            effective_pixels: Some(EffectivePixelsPatch {
                strip_fake_alpha: Some(false),
                trim_transparent: Some(false),
                ..Default::default()
            }),
            format: Some(FormatPatch {
                opaque_png_to: Some(FormatTarget::Keep),
                alpha_png_to: Some(FormatTarget::Keep),
                ..Default::default()
            }),
            compression: Some(CompressionPatch {
                png: Some(PngCompressionPatch {
                    mode: Some(PngMode::Lossless),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        },
        RuleSpec {
            pattern: "**/fonts/**".into(),
            skip: None,
            limits: None,
            effective_pixels: Some(EffectivePixelsPatch {
                trim_transparent: Some(false),
                ..Default::default()
            }),
            format: Some(FormatPatch {
                opaque_png_to: Some(FormatTarget::Keep),
                alpha_png_to: Some(FormatTarget::Keep),
                ..Default::default()
            }),
            compression: Some(CompressionPatch {
                png: Some(PngCompressionPatch {
                    mode: Some(PngMode::Lossless),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        },
        RuleSpec {
            pattern: "**/*_pixel*.*".into(),
            skip: None,
            limits: Some(LimitsPatch {
                downscale_filter: Some(DownscaleFilter::Nearest),
                ..Default::default()
            }),
            effective_pixels: None,
            format: None,
            compression: Some(CompressionPatch {
                png: Some(PngCompressionPatch {
                    mode: Some(PngMode::Lossless),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        },
    ]
}

fn default_excludes() -> Vec<String> {
    vec![
        ".godot/**".into(),
        "**/.godot/**".into(),
        ".git/**".into(),
        "**/.git/**".into(),
        ".import/**".into(),
        "**/.import/**".into(),
    ]
}

fn base_effective_pixels() -> EffectivePixelsConfig {
    EffectivePixelsConfig {
        alpha_threshold: 4,
        opaque_threshold: 252,
        strip_fake_alpha: true,
        zero_transparent_rgb: true,
        edge_bleed_px: 1,
        warn_effective_ratio_below: 0.35,
        warn_bbox_ratio_below: 0.50,
        trim_transparent: false,
        trim_padding: 2,
    }
}

fn base_format() -> FormatConfig {
    FormatConfig {
        opaque_png_to: FormatTarget::Keep,
        alpha_png_to: FormatTarget::Keep,
        jpg_to: FormatTarget::Keep,
        webp_to: FormatTarget::Keep,
        no_regression: true,
        min_saving_pct: 1.0,
    }
}

fn preset_godot_wx_minip() -> PresetBundle {
    PresetBundle {
        limits: LimitsConfig {
            max_dimension: 1024,
            max_width: None,
            max_height: None,
            align_multiple: 4,
            force_align: false,
            downscale_filter: DownscaleFilter::Lanczos3,
        },
        effective_pixels: base_effective_pixels(),
        format: base_format(),
        compression: CompressionConfig {
            png: PngCompressionConfig {
                mode: PngMode::Quantize,
                quality_min: 78,
                quality_max: 92,
                speed: 3,
                dithering: 0.85,
                min_psnr_db: Some(38.0),
                oxipng_level: 3,
                strip_metadata: true,
            },
            jpg: JpgCompressionConfig {
                quality: 82,
                strip_metadata: true,
            },
            webp: WebpCompressionConfig {
                lossless: false,
                quality: 82.0,
            },
        },
        default_rules: godot_default_protection_rules(),
        default_excludes: default_excludes(),
    }
}

fn preset_godot_qq_minip() -> PresetBundle {
    // QQ Mini Game shares WeChat's 4MB main-package & WASM memory constraints.
    preset_godot_wx_minip()
}

fn preset_godot_dy_minip() -> PresetBundle {
    let mut b = preset_godot_wx_minip();
    // Slightly tighter byte budget for faster first-frame download on Douyin Mini Game.
    b.compression.png.quality_min = 75;
    b.compression.png.quality_max = 90;
    b.compression.png.min_psnr_db = Some(37.0);
    b.compression.jpg.quality = 80;
    b.compression.webp.quality = 80.0;
    b
}

fn preset_godot_zfb_minip() -> PresetBundle {
    let mut b = preset_godot_wx_minip();
    // Conservative cap & slightly stronger lossless deflate pass for Alipay Mini Game.
    b.compression.png.quality_min = 76;
    b.compression.png.quality_max = 90;
    b.compression.png.min_psnr_db = Some(37.5);
    b.compression.png.oxipng_level = 4;
    b.compression.jpg.quality = 80;
    b.compression.webp.quality = 80.0;
    b
}

fn preset_godot_mobile() -> PresetBundle {
    PresetBundle {
        limits: LimitsConfig {
            max_dimension: 2048,
            max_width: None,
            max_height: None,
            align_multiple: 4,
            force_align: false,
            downscale_filter: DownscaleFilter::Lanczos3,
        },
        effective_pixels: base_effective_pixels(),
        format: base_format(),
        compression: CompressionConfig {
            png: PngCompressionConfig {
                // Keep lossless source PNG so Godot's mobile ASTC/ETC2 compressor starts from clean pixels.
                mode: PngMode::Lossless,
                quality_min: 85,
                quality_max: 98,
                speed: 3,
                dithering: 0.85,
                min_psnr_db: Some(40.0),
                oxipng_level: 3,
                strip_metadata: true,
            },
            jpg: JpgCompressionConfig {
                quality: 88,
                strip_metadata: true,
            },
            webp: WebpCompressionConfig {
                lossless: false,
                quality: 88.0,
            },
        },
        default_rules: godot_default_protection_rules(),
        default_excludes: default_excludes(),
    }
}

fn preset_raw_png_jpg() -> PresetBundle {
    PresetBundle {
        limits: LimitsConfig {
            max_dimension: 0,
            max_width: None,
            max_height: None,
            align_multiple: 0,
            force_align: false,
            downscale_filter: DownscaleFilter::Lanczos3,
        },
        effective_pixels: base_effective_pixels(),
        format: base_format(),
        compression: CompressionConfig {
            png: PngCompressionConfig {
                mode: PngMode::Quantize,
                quality_min: 80,
                quality_max: 95,
                speed: 3,
                dithering: 0.85,
                min_psnr_db: Some(39.0),
                oxipng_level: 4,
                strip_metadata: true,
            },
            jpg: JpgCompressionConfig {
                quality: 84,
                strip_metadata: true,
            },
            webp: WebpCompressionConfig {
                lossless: false,
                quality: 84.0,
            },
        },
        default_rules: Vec::new(),
        default_excludes: default_excludes(),
    }
}

pub fn default_fallback_bundle() -> PresetBundle {
    preset_raw_png_jpg()
}

/// Render a well-commented `$engine_$channel.yml` starter template for a preset.
pub fn render_yaml_template(entry: &PresetEntry) -> String {
    let b = entry.build();
    let png_mode = match b.compression.png.mode {
        PngMode::Lossless => "lossless",
        PngMode::Quantize => "quantize",
    };
    let min_psnr = b
        .compression
        .png
        .min_psnr_db
        .map(|v| format!("{:.1}", v))
        .unwrap_or_else(|| "null".to_string());

    let extra_rules_snippet = if entry.engine == "godot" {
        r#"# Built-in Godot protection rules (normal/ORM/mask/fonts/pixel-art -> lossless) are inherited
# automatically when `inherit_default_rules: true`. Add project-specific overrides below:
rules:
  # Large opaque backgrounds: allow slightly larger dim and convert opaque PNG -> JPG (for CDN bundles)
  # - match: "bg/**"
  #   limits:
  #     max_dimension: 1280
  #   format:
  #     opaque_png_to: jpg

  # Safe standalone icon folder where transparent trimming will not break Godot pivots/anchors:
  # - match: "raw_icons/**"
  #   effective_pixels:
  #     trim_transparent: true
  #     trim_padding: 2

  # Remote CDN sprites targeting WebP directly:
  # - match: "cdn/**"
  #   format:
  #     opaque_png_to: webp
  #     alpha_png_to: webp
"#
    } else {
        r#"# Optional per-path glob overrides (later matching rules win):
rules:
  # - match: "photos/**"
  #   format:
  #     opaque_png_to: jpg
"#
    };

    format!(
        r#"# img_resize texture optimization profile: {filename}
# {summary}
# Usage:
#   img_resize optimize {filename} ./assets -o ./assets_opt --dry-run --json
#   img_resize optimize {filename} ./assets -o ./assets_opt --json

engine: {engine}
channel: {channel}

# Optional default paths (overridden when CLI PATH / -o are provided)
# input: ./assets
# output: ./assets_opt
preserve_dir_structure: true
dry_run: false
inherit_default_rules: true

limits:
  # Longest-side pixel cap (0 = unlimited; never upscales)
  max_dimension: {max_dim}
  # Align resized dimensions down to a multiple of N (4 is friendly to WebGL/ETC2/ASTC)
  align_multiple: {align}
  force_align: false
  downscale_filter: lanczos3

effective_pixels:
  # Pixels with alpha <= alpha_threshold count as transparent
  alpha_threshold: {alpha_th}
  # Images whose minimum alpha >= opaque_threshold are treated as fake-alpha (fully opaque)
  opaque_threshold: {opaque_th}
  # Drop useless alpha channel (RGBA -> RGB) on fully opaque PNGs (zero layout impact)
  strip_fake_alpha: true
  # Zero RGB on fully transparent pixels after edge bleed (pure lossless PNG size win)
  zero_transparent_rgb: true
  edge_bleed_px: {bleed}
  # Audit warning thresholds in JSON/CLI report (prompts atlas packing via mj_atlas)
  warn_effective_ratio_below: {warn_eff:.2}
  warn_bbox_ratio_below: {warn_bbox:.2}
  # Keep false by default to protect Godot pivots/anchors/9-patch/UVs; enable per-rule if safe
  trim_transparent: false
  trim_padding: 2

format:
  # Keep `.png` by default so Godot `res://*.png` references stay intact;
  # set `opaque_png_to: jpg` or `webp` for CDN/remote texture bundles or in `rules:`
  opaque_png_to: keep       # keep | jpg | webp
  alpha_png_to: keep        # keep | png | webp
  jpg_to: keep              # keep | jpg | webp
  webp_to: keep             # keep | webp | png | jpg
  # Never write a larger file when image was not resized/trimmed/format-converted
  no_regression: true
  min_saving_pct: 1.0

compression:
  png:
    mode: {png_mode}          # lossless | quantize (local imagequant, TinyPNG-equivalent)
    quality_min: {png_qmin}
    quality_max: {png_qmax}
    speed: {png_speed}                 # 1 (slowest/best) .. 10 (fastest)
    dithering: {png_dither:.2}
    # Perceptual quality guard: if quantized RGBA PSNR < min_psnr_db, fallback to lossless PNG
    min_psnr_db: {min_psnr}
    oxipng_level: {oxi}          # 0 (off) .. 6 (max lossless deflate/filter optimization)
    strip_metadata: true
  jpg:
    quality: {jpg_q}
    strip_metadata: true
  webp:
    lossless: false
    quality: {webp_q:.1}

{extra_rules_snippet}"#,
        filename = entry.filename,
        summary = entry.summary,
        engine = entry.engine,
        channel = entry.channel,
        max_dim = b.limits.max_dimension,
        align = b.limits.align_multiple,
        alpha_th = b.effective_pixels.alpha_threshold,
        opaque_th = b.effective_pixels.opaque_threshold,
        bleed = b.effective_pixels.edge_bleed_px,
        warn_eff = b.effective_pixels.warn_effective_ratio_below,
        warn_bbox = b.effective_pixels.warn_bbox_ratio_below,
        png_mode = png_mode,
        png_qmin = b.compression.png.quality_min,
        png_qmax = b.compression.png.quality_max,
        png_speed = b.compression.png.speed,
        png_dither = b.compression.png.dithering,
        min_psnr = min_psnr,
        oxi = b.compression.png.oxipng_level,
        jpg_q = b.compression.jpg.quality,
        webp_q = b.compression.webp.quality,
        extra_rules_snippet = extra_rules_snippet,
    )
}
