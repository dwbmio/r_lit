//! BMFont (AngelCode) export — bake stylized glyphs into a bitmap font.
//!
//! Renders each glyph in a charset with the full effect pipeline (gradient /
//! grain / shadow / ...), shelf-packs them into one atlas PNG, and emits a
//! `.fnt` (text format) with per-glyph metrics. Godot loads the `.fnt` directly
//! as a `FontFile`, so any `Label` / `RichTextLabel` can use the styled font —
//! and per-character animation (fly-in) just works on top of a normal Label.
//!
//! Effects are applied **per glyph** (each glyph gets its own gradient/grain),
//! which keeps a vertical gradient consistent across every character.

use cosmic_text::{FontSystem, SwashCache};
use log::info;
use tiny_skia::{BlendMode, Color, Pixmap, PixmapPaint, Transform};

use crate::color;
use crate::effect::{self, EffectPhase};
use crate::error::{AppError, Result};
use crate::render::text;

pub struct BmfontOpts {
    /// Characters to include (deduped). e.g. "是男人就下层0123456789".
    pub chars: String,
    /// Output basename; writes `<output>.fnt` + `<output>.png`.
    pub output: String,
    pub font: Option<String>,
    pub font_size: f32,
    pub color: String,
    pub padding: u32,
    pub effects: Vec<String>,
    pub json: bool,
}

/// Packed glyph: its cell pixmap + BMFont metrics.
struct GlyphCell {
    ch: char,
    pixmap: Option<Pixmap>, // None for whitespace / no-ink glyphs
    width: u32,
    height: u32,
    xoffset: i32,
    yoffset: i32,
    xadvance: i32,
    // atlas placement (filled by packer)
    x: u32,
    y: u32,
}

const ATLAS_MAX_W: u32 = 2048;
const SHELF_GAP: u32 = 2; // 1px transparent gutter between glyphs

pub fn execute(opts: &BmfontOpts) -> Result<()> {
    let text_color = color::parse_skia_color(&opts.color)?;
    let effects = effect::parse_effect_specs(&opts.effects)?;

    let mut font_system = FontSystem::new();
    let mut cache = SwashCache::new();

    let pad = opts.padding as i32;

    // Dedup chars, preserve first-seen order.
    let mut seen = std::collections::HashSet::new();
    let charset: Vec<char> = opts
        .chars
        .chars()
        .filter(|c| !c.is_control() && seen.insert(*c))
        .collect();
    if charset.is_empty() {
        return Err(AppError::Render("bmfont: empty charset".into()));
    }

    // Global vertical metrics (consistent across glyphs at fixed font/size).
    // Sampled from a representative CJK glyph layout.
    let (line_height, ascent) = {
        let layout = text::layout_text(&mut font_system, "国", opts.font.as_deref(), opts.font_size);
        let mut lh = (opts.font_size * 1.2).round();
        let mut asc = opts.font_size.round();
        if let Some(run) = layout.buffer.layout_runs().next() {
            lh = run.line_height.round();
            asc = (run.line_y - run.line_top).round();
        }
        (lh as i32, asc as i32)
    };

    let mut cells: Vec<GlyphCell> = Vec::with_capacity(charset.len());
    for ch in &charset {
        cells.push(render_glyph_cell(
            *ch,
            opts,
            &effects,
            text_color,
            pad,
            ascent,
            &mut font_system,
            &mut cache,
        )?);
    }

    // Shelf-pack into atlas.
    let (atlas_w, atlas_h) = shelf_pack(&mut cells);

    let mut atlas = Pixmap::new(atlas_w, atlas_h)
        .ok_or_else(|| AppError::Render("bmfont: failed to alloc atlas".into()))?;
    for cell in &cells {
        if let Some(pm) = &cell.pixmap {
            atlas.draw_pixmap(
                cell.x as i32,
                cell.y as i32,
                pm.as_ref(),
                &PixmapPaint {
                    blend_mode: BlendMode::Source,
                    ..Default::default()
                },
                Transform::identity(),
                None,
            );
        }
    }

    // Write atlas PNG.
    let png_path = format!("{}.png", opts.output);
    let fnt_path = format!("{}.fnt", opts.output);
    let png_data = atlas.encode_png().map_err(|e| AppError::PngEncode(e.to_string()))?;
    std::fs::write(&png_path, &png_data)?;

    // Write .fnt (AngelCode text format).
    let page_file = std::path::Path::new(&png_path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| png_path.clone());
    let face = opts.font.clone().unwrap_or_else(|| "stylized".into());

    let mut fnt = String::new();
    fnt.push_str(&format!(
        "info face=\"{}\" size={} bold=0 italic=0 charset=\"\" unicode=1 stretchH=100 smooth=1 aa=1 padding=0,0,0,0 spacing=0,0 outline=0\n",
        face, opts.font_size as i32
    ));
    fnt.push_str(&format!(
        "common lineHeight={} base={} scaleW={} scaleH={} pages=1 packed=0 alphaChnl=1 redChnl=0 greenChnl=0 blueChnl=0\n",
        line_height, ascent, atlas_w, atlas_h
    ));
    fnt.push_str(&format!("page id=0 file=\"{}\"\n", page_file));
    fnt.push_str(&format!("chars count={}\n", cells.len()));
    for cell in &cells {
        fnt.push_str(&format!(
            "char id={} x={} y={} width={} height={} xoffset={} yoffset={} xadvance={} page=0 chnl=15\n",
            cell.ch as u32,
            cell.x,
            cell.y,
            cell.width,
            cell.height,
            cell.xoffset,
            cell.yoffset,
            cell.xadvance,
        ));
    }
    fnt.push_str("kernings count=0\n");
    std::fs::write(&fnt_path, &fnt)?;

    if opts.json {
        println!(
            "{}",
            serde_json::json!({
                "status": "ok",
                "fnt": fnt_path,
                "atlas": png_path,
                "atlas_size": [atlas_w, atlas_h],
                "glyphs": cells.len(),
                "line_height": line_height,
                "base": ascent
            })
        );
    } else {
        info!(
            "Written {} + {} ({} glyphs, atlas {}x{}, lineHeight={} base={})",
            fnt_path, png_path, cells.len(), atlas_w, atlas_h, line_height, ascent
        );
    }

    Ok(())
}

/// Render one glyph into its own padded cell and compute BMFont metrics.
fn render_glyph_cell(
    ch: char,
    opts: &BmfontOpts,
    effects: &[Box<dyn effect::Effect>],
    text_color: Color,
    pad: i32,
    ascent: i32,
    font_system: &mut FontSystem,
    cache: &mut SwashCache,
) -> Result<GlyphCell> {
    let s = ch.to_string();
    let layout = text::layout_text(font_system, &s, opts.font.as_deref(), opts.font_size);

    // Locate the single glyph: its advance + ink placement + line baseline.
    let mut advance = (opts.font_size * 0.5) as i32;
    let mut line_y = opts.font_size; // baseline within layout
    let mut placement: Option<(i32, i32, u32, u32)> = None; // left, top, w, h
    let mut phys0 = (0i32, 0i32);

    if let Some(run) = layout.buffer.layout_runs().next() {
        line_y = run.line_y;
        if let Some(glyph) = run.glyphs.first() {
            advance = glyph.w.round() as i32;
            let p = glyph.physical((0.0, 0.0), 1.0);
            phys0 = (p.x, p.y);
            if let Some(img) = cache.get_image(font_system, p.cache_key) {
                placement = Some((
                    img.placement.left,
                    img.placement.top,
                    img.placement.width,
                    img.placement.height,
                ));
            }
        }
    }

    // No ink (space, etc.) → empty cell, advance only.
    let Some((gleft, gtop, gw, gh)) = placement else {
        return Ok(GlyphCell {
            ch,
            pixmap: None,
            width: 0,
            height: 0,
            xoffset: 0,
            yoffset: 0,
            xadvance: advance,
            x: 0,
            y: 0,
        });
    };
    if gw == 0 || gh == 0 {
        return Ok(GlyphCell {
            ch,
            pixmap: None,
            width: 0,
            height: 0,
            xoffset: 0,
            yoffset: 0,
            xadvance: advance,
            x: 0,
            y: 0,
        });
    }

    let cell_w = gw as i32 + pad * 2;
    let cell_h = gh as i32 + pad * 2;
    let mut pixmap = Pixmap::new(cell_w as u32, cell_h as u32)
        .ok_or_else(|| AppError::Render("bmfont: failed to alloc glyph cell".into()))?;

    // Offsets so the glyph ink top-left lands at (pad, pad) in the cell.
    // render_text_to_pixmap: glyph_x = physical.x + left ; glyph_y = physical.y - top + line_y
    let text_x = pad as f32 - phys0.0 as f32 - gleft as f32;
    let text_y = pad as f32 - phys0.1 as f32 + gtop as f32 - line_y;

    // Effect pipeline (mirror render::execute order): Pre → Fill (or solid) → Post.
    for eff in effects.iter().filter(|e| e.phase() == EffectPhase::Pre) {
        eff.apply(&mut pixmap, font_system, cache, &layout.buffer, text_x, text_y, text_color)?;
    }
    let has_fill = effects.iter().any(|e| e.phase() == EffectPhase::Fill);
    if has_fill {
        for eff in effects.iter().filter(|e| e.phase() == EffectPhase::Fill) {
            eff.apply(&mut pixmap, font_system, cache, &layout.buffer, text_x, text_y, text_color)?;
        }
    } else {
        text::render_text_to_pixmap(font_system, cache, &layout.buffer, &mut pixmap, text_x, text_y, text_color);
    }
    for eff in effects.iter().filter(|e| e.phase() == EffectPhase::Post) {
        eff.apply(&mut pixmap, font_system, cache, &layout.buffer, text_x, text_y, text_color)?;
    }

    // BMFont metrics:
    //   xoffset = ink_left_bearing - pad   (cell-left relative to pen)
    //   yoffset = (ascent - top_bearing) - pad   (cell-top relative to line top)
    let xoffset = gleft - pad;
    let yoffset = (ascent - gtop) - pad;

    Ok(GlyphCell {
        ch,
        pixmap: Some(pixmap),
        width: cell_w as u32,
        height: cell_h as u32,
        xoffset,
        yoffset,
        xadvance: advance,
        x: 0,
        y: 0,
    })
}

/// Simple shelf packer; assigns each cell an (x, y). Returns atlas (w, h).
fn shelf_pack(cells: &mut [GlyphCell]) -> (u32, u32) {
    let mut cur_x = 0u32;
    let mut cur_y = 0u32;
    let mut row_h = 0u32;
    let mut atlas_w = 1u32;

    for cell in cells.iter_mut() {
        if cell.width == 0 || cell.height == 0 {
            continue;
        }
        if cur_x + cell.width > ATLAS_MAX_W {
            // new shelf
            cur_x = 0;
            cur_y += row_h + SHELF_GAP;
            row_h = 0;
        }
        cell.x = cur_x;
        cell.y = cur_y;
        cur_x += cell.width + SHELF_GAP;
        row_h = row_h.max(cell.height);
        atlas_w = atlas_w.max(cur_x);
    }

    let atlas_h = (cur_y + row_h).max(1);
    // pad atlas_w a touch so the last gutter doesn't clip
    (atlas_w.max(1), atlas_h)
}
