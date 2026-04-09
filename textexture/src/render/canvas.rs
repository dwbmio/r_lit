use tiny_skia::{Pixmap, Color, Paint, Rect, Transform, PixmapPaint, BlendMode};
use crate::error::{AppError, Result};

/// Create a new canvas (Pixmap) with the given dimensions and background.
pub fn create_canvas(width: u32, height: u32, bg_color: Option<Color>) -> Result<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| AppError::Render(format!("Failed to create {}x{} canvas", width, height)))?;

    if let Some(color) = bg_color {
        pixmap.fill(color);
    }

    Ok(pixmap)
}

/// Draw a multi-stop linear gradient background.
pub fn draw_bg_gradient(pixmap: &mut Pixmap, colors: &[Color], angle: f32) {
    let w = pixmap.width() as f32;
    let h = pixmap.height() as f32;
    let data = pixmap.data_mut();

    let angle_rad = angle.to_radians();
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();
    let cx = w / 2.0;
    let cy = h / 2.0;
    let max_dist = (w * cos_a.abs() + h * sin_a.abs()).max(1.0);

    let n = colors.len();
    for y in 0..h as u32 {
        for x in 0..w as u32 {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let proj = dx * cos_a + dy * sin_a;
            let t = ((proj / max_dist) + 0.5).clamp(0.0, 1.0);

            // Find which segment of the gradient we're in
            let scaled = t * (n - 1) as f32;
            let seg = (scaled as usize).min(n - 2);
            let local_t = scaled - seg as f32;

            let c0 = &colors[seg];
            let c1 = &colors[seg + 1];
            let r = (c0.red() + (c1.red() - c0.red()) * local_t).clamp(0.0, 1.0);
            let g = (c0.green() + (c1.green() - c0.green()) * local_t).clamp(0.0, 1.0);
            let b = (c0.blue() + (c1.blue() - c0.blue()) * local_t).clamp(0.0, 1.0);
            let a = (c0.alpha() + (c1.alpha() - c0.alpha()) * local_t).clamp(0.0, 1.0);

            let idx = (y * w as u32 + x) as usize * 4;
            // Premultiplied alpha
            data[idx]     = (r * a * 255.0) as u8;
            data[idx + 1] = (g * a * 255.0) as u8;
            data[idx + 2] = (b * a * 255.0) as u8;
            data[idx + 3] = (a * 255.0) as u8;
        }
    }
}

/// Draw a background image stretched to fill the canvas.
pub fn draw_bg_image(pixmap: &mut Pixmap, path: &str) -> Result<()> {
    let img = image::open(path)
        .map_err(|e| AppError::Render(format!("Failed to load bg image '{}': {}", path, e)))?;

    let resized = img.resize_exact(
        pixmap.width(),
        pixmap.height(),
        image::imageops::FilterType::Lanczos3,
    );
    let rgba = resized.to_rgba8();

    // Convert to premultiplied alpha and copy into a temp pixmap
    let mut bg_pm = Pixmap::new(pixmap.width(), pixmap.height())
        .ok_or_else(|| AppError::Render("Failed to create bg image pixmap".into()))?;

    let dst = bg_pm.data_mut();
    let src = rgba.as_raw();
    for i in (0..dst.len()).step_by(4) {
        let a = src[i + 3] as u16;
        dst[i]     = ((src[i] as u16 * a) / 255) as u8;
        dst[i + 1] = ((src[i + 1] as u16 * a) / 255) as u8;
        dst[i + 2] = ((src[i + 2] as u16 * a) / 255) as u8;
        dst[i + 3] = src[i + 3];
    }

    pixmap.draw_pixmap(
        0, 0,
        bg_pm.as_ref(),
        &PixmapPaint { blend_mode: BlendMode::SourceOver, ..Default::default() },
        Transform::identity(),
        None,
    );

    Ok(())
}

/// Fill a pixmap with a solid color rectangle.
pub fn fill_rect(pixmap: &mut Pixmap, rect: Rect, color: Color) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;

    let path = tiny_skia::PathBuilder::from_rect(rect);
    pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
}
