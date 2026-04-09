use std::collections::HashMap;
use cosmic_text::{Buffer, FontSystem, SwashCache};
use tiny_skia::{Color, Pixmap, PixmapPaint, BlendMode, Transform};

use crate::color;
use crate::effect::{Effect, EffectPhase, param_or};
use crate::error::Result;
use crate::render::text;

pub struct Gradient {
    start_color: Color,
    end_color: Color,
    angle: f32,
}

impl Gradient {
    pub fn from_params(params: &HashMap<String, String>) -> Result<Self> {
        let start_color = match params.get("start") {
            Some(c) => color::parse_skia_color(c)?,
            None => Color::from_rgba8(255, 0, 0, 255),
        };
        let end_color = match params.get("end") {
            Some(c) => color::parse_skia_color(c)?,
            None => Color::from_rgba8(0, 0, 255, 255),
        };
        let angle: f32 = param_or(params, "angle", 0.0)?;
        Ok(Self { start_color, end_color, angle })
    }
}

impl Effect for Gradient {
    fn name(&self) -> &str { "gradient" }
    fn phase(&self) -> EffectPhase { EffectPhase::Fill }

    fn apply(
        &self,
        pixmap: &mut Pixmap,
        font_system: &mut FontSystem,
        cache: &mut SwashCache,
        buffer: &Buffer,
        text_x: f32,
        text_y: f32,
        _text_color: Color,
    ) -> Result<()> {
        let w = pixmap.width();
        let h = pixmap.height();

        // Render text as white mask
        let mut mask_pm = Pixmap::new(w, h)
            .ok_or_else(|| crate::error::AppError::Render("failed to create gradient mask".into()))?;

        text::render_text_to_pixmap(
            font_system,
            cache,
            buffer,
            &mut mask_pm,
            text_x,
            text_y,
            Color::WHITE,
        );

        // Apply gradient by modifying pixel colors based on position
        let angle_rad = self.angle.to_radians();
        let cos_a = angle_rad.cos();
        let sin_a = angle_rad.sin();
        let max_dist = (w as f32 * cos_a.abs() + h as f32 * sin_a.abs()).max(1.0);

        let sr = self.start_color.red();
        let sg = self.start_color.green();
        let sb = self.start_color.blue();
        let er = self.end_color.red();
        let eg = self.end_color.green();
        let eb = self.end_color.blue();

        let mask_data = mask_pm.data_mut();
        let cx = w as f32 / 2.0;
        let cy = h as f32 / 2.0;

        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize * 4;
                let alpha = mask_data[idx + 3];
                if alpha == 0 {
                    continue;
                }

                // Gradient position along angle
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let proj = dx * cos_a + dy * sin_a;
                let t = ((proj / max_dist) + 0.5).clamp(0.0, 1.0);

                let r = (sr + (er - sr) * t).clamp(0.0, 1.0);
                let g = (sg + (eg - sg) * t).clamp(0.0, 1.0);
                let b = (sb + (eb - sb) * t).clamp(0.0, 1.0);
                let a_f = alpha as f32 / 255.0;

                // Premultiplied alpha
                mask_data[idx] = (r * a_f * 255.0) as u8;
                mask_data[idx + 1] = (g * a_f * 255.0) as u8;
                mask_data[idx + 2] = (b * a_f * 255.0) as u8;
            }
        }

        pixmap.draw_pixmap(
            0, 0,
            mask_pm.as_ref(),
            &PixmapPaint {
                blend_mode: BlendMode::SourceOver,
                ..Default::default()
            },
            Transform::identity(),
            None,
        );

        Ok(())
    }
}
