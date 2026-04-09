pub mod blur;
pub mod canvas;
pub mod text;

use cosmic_text::{FontSystem, SwashCache};
use log::info;

use crate::color;
use crate::effect::{self, EffectPhase};
use crate::error::{AppError, Result};

/// All render options parsed from CLI.
pub struct RenderOpts {
    pub text: String,
    pub output: String,
    pub font: Option<String>,
    pub font_size: f32,
    pub color: String,
    pub bg: String,
    pub transparent: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub padding: u32,
    pub effects: Vec<String>,
    pub json: bool,
}

/// Parsed background specification.
enum BgSpec {
    /// Transparent (no background)
    None,
    /// Solid color
    Solid(tiny_skia::Color),
    /// Linear gradient with multiple stops and angle
    Gradient { colors: Vec<tiny_skia::Color>, angle: f32 },
    /// Image file path
    Image(String),
}

/// Parse --bg value into a BgSpec.
/// - "#ff0000"                → Solid
/// - "#ff0000,#0000ff"        → 2-stop gradient, 0°
/// - "#ff0000,#0000ff@45"     → 2-stop gradient, 45°
/// - "#ff0000,#00ff00,#0000ff@90" → 3-stop gradient, 90°
/// - "./bg.jpg"               → Image file
fn parse_bg(bg: &str, transparent: bool) -> Result<BgSpec> {
    if transparent {
        return Ok(BgSpec::None);
    }

    // Check if it looks like a file path
    let path = std::path::Path::new(bg);
    if path.extension().is_some() && path.exists() {
        return Ok(BgSpec::Image(bg.to_string()));
    }

    // Check for gradient: contains comma → multi-color
    if bg.contains(',') {
        // Split off @angle if present
        let (color_part, angle) = if let Some(at_idx) = bg.rfind('@') {
            let angle_str = &bg[at_idx + 1..];
            let angle: f32 = angle_str.parse().map_err(|_| {
                AppError::ColorParse(format!("invalid gradient angle: {}", angle_str))
            })?;
            (&bg[..at_idx], angle)
        } else {
            (bg, 0.0f32)
        };

        let colors: Vec<tiny_skia::Color> = color_part
            .split(',')
            .map(|s| color::parse_skia_color(s.trim()))
            .collect::<Result<Vec<_>>>()?;

        if colors.len() < 2 {
            return Err(AppError::ColorParse("gradient needs at least 2 colors".into()));
        }

        return Ok(BgSpec::Gradient { colors, angle });
    }

    // Single color
    Ok(BgSpec::Solid(color::parse_skia_color(bg)?))
}

/// Maximum canvas width (1080p)
const MAX_WIDTH: u32 = 1920;
/// Maximum canvas height
const MAX_HEIGHT: u32 = 1920;

/// Execute the render pipeline.
pub fn execute(opts: &RenderOpts) -> Result<()> {
    // 1. Parse colors
    let text_color = color::parse_skia_color(&opts.color)?;
    let bg_spec = parse_bg(&opts.bg, opts.transparent)?;

    // 2. Parse effects
    let effects = effect::parse_effect_specs(&opts.effects)?;

    // 3. Text layout — auto-fit font size to width constraint
    let mut font_system = FontSystem::new();
    let mut cache = SwashCache::new();

    let pad = opts.padding as f32;
    let target_w = opts.width.map(|w| w.min(MAX_WIDTH));

    let (layout, actual_font_size) = if let Some(tw) = target_w {
        // Shrink font to fit within target width (never upscale)
        let max_text_w = tw as f32 - pad * 2.0;
        let mut fs = opts.font_size;

        let mut layout = text::layout_text(&mut font_system, &opts.text, opts.font.as_deref(), fs);
        if layout.width > max_text_w && max_text_w > 0.0 {
            // Scale font proportionally
            fs = fs * (max_text_w / layout.width);
            fs = fs.max(8.0); // floor at 8px
            layout = text::layout_text(&mut font_system, &opts.text, opts.font.as_deref(), fs);
            info!("Auto-fit font size: {:.0}px → {:.1}px to fit {}px width", opts.font_size, fs, tw);
        }
        (layout, fs)
    } else {
        // No width constraint — fit canvas to text, but clamp to MAX_WIDTH
        let layout = text::layout_text(&mut font_system, &opts.text, opts.font.as_deref(), opts.font_size);
        let natural_w = (layout.width + pad * 2.0).ceil() as u32;
        if natural_w > MAX_WIDTH {
            // Text too wide at this font size, shrink to fit MAX_WIDTH
            let max_text_w = MAX_WIDTH as f32 - pad * 2.0;
            let fs = (opts.font_size * (max_text_w / layout.width)).max(8.0);
            let layout = text::layout_text(&mut font_system, &opts.text, opts.font.as_deref(), fs);
            info!("Auto-fit font size: {:.0}px → {:.1}px (capped at {}px width)", opts.font_size, fs, MAX_WIDTH);
            (layout, fs)
        } else {
            (layout, opts.font_size)
        }
    };

    // 4. Canvas dimensions — always accommodate full text, never clip
    let text_needed_w = (layout.width + pad * 2.0).ceil() as u32;
    let text_needed_h = (layout.height + pad * 2.0).ceil() as u32;

    let img_w = match opts.width {
        Some(w) => w.min(MAX_WIDTH).max(text_needed_w.min(MAX_WIDTH)),
        None => text_needed_w.min(MAX_WIDTH),
    };
    let img_h = match opts.height {
        Some(h) => h.min(MAX_HEIGHT).max(text_needed_h.min(MAX_HEIGHT)),
        None => text_needed_h.min(MAX_HEIGHT),
    };

    info!(
        "Canvas: {}x{}, text bbox: {:.0}x{:.0}, font: {:.1}px",
        img_w, img_h, layout.width, layout.height, actual_font_size
    );

    // 5. Create canvas with background
    let solid_bg = match &bg_spec {
        BgSpec::Solid(c) => Some(*c),
        _ => None,
    };
    let mut pixmap = canvas::create_canvas(img_w, img_h, solid_bg)?;

    match &bg_spec {
        BgSpec::Gradient { colors, angle } => {
            canvas::draw_bg_gradient(&mut pixmap, colors, *angle);
        }
        BgSpec::Image(path) => {
            canvas::draw_bg_image(&mut pixmap, path)?;
        }
        _ => {}
    }

    // Text offset (centered)
    let text_x = (img_w as f32 - layout.width) / 2.0;
    let text_y = (img_h as f32 - layout.height) / 2.0;

    // 6. Pre-text effects (shadow, 3d)
    for eff in effects.iter().filter(|e| e.phase() == EffectPhase::Pre) {
        eff.apply(&mut pixmap, &mut font_system, &mut cache, &layout.buffer, text_x, text_y, text_color)?;
    }

    // 7. Check if any fill effect exists
    let has_fill = effects.iter().any(|e| e.phase() == EffectPhase::Fill);

    if has_fill {
        for eff in effects.iter().filter(|e| e.phase() == EffectPhase::Fill) {
            eff.apply(&mut pixmap, &mut font_system, &mut cache, &layout.buffer, text_x, text_y, text_color)?;
        }
    } else {
        // Default: render with solid color
        text::render_text_to_pixmap(
            &mut font_system,
            &mut cache,
            &layout.buffer,
            &mut pixmap,
            text_x,
            text_y,
            text_color,
        );
    }

    // 8. Post-text effects (outline, glow, neon)
    for eff in effects.iter().filter(|e| e.phase() == EffectPhase::Post) {
        eff.apply(&mut pixmap, &mut font_system, &mut cache, &layout.buffer, text_x, text_y, text_color)?;
    }

    // 9. Output
    let png_data = pixmap.encode_png()
        .map_err(|e| AppError::PngEncode(e.to_string()))?;
    std::fs::write(&opts.output, &png_data)?;

    if opts.json {
        println!(
            "{}",
            serde_json::json!({
                "status": "ok",
                "output": opts.output,
                "width": img_w,
                "height": img_h
            })
        );
    } else {
        info!("Written {} ({}x{})", opts.output, img_w, img_h);
    }

    Ok(())
}
