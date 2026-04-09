use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping, SwashCache};
use tiny_skia::{Pixmap, Color, Transform, PixmapPaint, BlendMode};
use crate::font;

/// Measured text layout info.
pub struct TextLayout {
    pub width: f32,
    pub height: f32,
    pub buffer: Buffer,
}

/// Lay out text and compute its bounding box.
pub fn layout_text(
    font_system: &mut FontSystem,
    text: &str,
    font_spec: Option<&str>,
    font_size: f32,
) -> TextLayout {
    let family_name = font::resolve_font_family(font_system, font_spec);

    let metrics = Metrics::new(font_size, font_size * 1.2);
    let mut buffer = Buffer::new(font_system, metrics);

    let attrs = Attrs::new().family(cosmic_text::Family::Name(&family_name));
    buffer.set_text(font_system, text, attrs, Shaping::Advanced);

    // Set a large initial width so text doesn't wrap
    buffer.set_size(font_system, Some(10000.0), None);
    buffer.shape_until_scroll(font_system, false);

    // Compute bounding box
    let (mut max_w, mut total_h) = (0.0f32, 0.0f32);
    for run in buffer.layout_runs() {
        let line_w = run.line_w;
        if line_w > max_w {
            max_w = line_w;
        }
        total_h = run.line_y + font_size; // approximate bottom
    }

    TextLayout {
        width: max_w.ceil(),
        height: total_h.ceil(),
        buffer,
    }
}

/// Render text glyphs onto a pixmap at the given offset, with the given color.
/// Returns the list of glyph positions for effect processing.
pub fn render_text_to_pixmap(
    font_system: &mut FontSystem,
    cache: &mut SwashCache,
    buffer: &Buffer,
    pixmap: &mut Pixmap,
    offset_x: f32,
    offset_y: f32,
    color: Color,
) {
    let r = (color.red() * 255.0) as u8;
    let g = (color.green() * 255.0) as u8;
    let b = (color.blue() * 255.0) as u8;
    let a = (color.alpha() * 255.0) as u8;

    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            let physical = glyph.physical((offset_x, offset_y), 1.0);

            let Some(image) = cache.get_image(font_system, physical.cache_key) else {
                continue;
            };

            let glyph_x = physical.x + image.placement.left;
            let glyph_y = physical.y - image.placement.top + run.line_y as i32;

            // Rasterize glyph onto pixmap
            match image.content {
                cosmic_text::SwashContent::Mask => {
                    let gw = image.placement.width as usize;
                    let gh = image.placement.height as usize;

                    // Create a small pixmap for this glyph
                    if let Some(mut glyph_pm) = Pixmap::new(gw as u32, gh as u32) {
                        let glyph_data = glyph_pm.data_mut();
                        for i in 0..gw * gh {
                            let alpha = image.data[i];
                            let blended_a = ((alpha as u16 * a as u16) / 255) as u8;
                            // Premultiplied alpha
                            glyph_data[i * 4] = ((r as u16 * blended_a as u16) / 255) as u8;
                            glyph_data[i * 4 + 1] = ((g as u16 * blended_a as u16) / 255) as u8;
                            glyph_data[i * 4 + 2] = ((b as u16 * blended_a as u16) / 255) as u8;
                            glyph_data[i * 4 + 3] = blended_a;
                        }
                        pixmap.draw_pixmap(
                            glyph_x,
                            glyph_y,
                            glyph_pm.as_ref(),
                            &PixmapPaint {
                                blend_mode: BlendMode::SourceOver,
                                ..Default::default()
                            },
                            Transform::identity(),
                            None,
                        );
                    }
                }
                cosmic_text::SwashContent::Color => {
                    let gw = image.placement.width as usize;
                    let gh = image.placement.height as usize;
                    if let Some(mut glyph_pm) = Pixmap::new(gw as u32, gh as u32) {
                        let glyph_data = glyph_pm.data_mut();
                        // Color glyphs are RGBA
                        let len = (gw * gh * 4).min(image.data.len());
                        glyph_data[..len].copy_from_slice(&image.data[..len]);
                        pixmap.draw_pixmap(
                            glyph_x,
                            glyph_y,
                            glyph_pm.as_ref(),
                            &PixmapPaint::default(),
                            Transform::identity(),
                            None,
                        );
                    }
                }
                cosmic_text::SwashContent::SubpixelMask => {
                    // Treat as regular mask for simplicity
                }
            }
        }
    }
}
