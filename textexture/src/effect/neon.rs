use std::collections::HashMap;
use cosmic_text::{Buffer, FontSystem, SwashCache};
use tiny_skia::{Color, Pixmap, PixmapPaint, BlendMode, Transform};

use crate::color;
use crate::effect::{Effect, EffectPhase, param_or};
use crate::error::Result;
use crate::render::{blur, text};

/// Neon: outer glow (large blur) + inner glow (small blur) + bright core.
pub struct Neon {
    color: Color,
    radius: u32,
}

impl Neon {
    pub fn from_params(params: &HashMap<String, String>) -> Result<Self> {
        let color = match params.get("color") {
            Some(c) => color::parse_skia_color(c)?,
            None => Color::from_rgba8(255, 0, 255, 255),
        };
        let radius: u32 = param_or(params, "radius", 20)?;
        Ok(Self { color, radius })
    }
}

impl Effect for Neon {
    fn name(&self) -> &str { "neon" }
    fn phase(&self) -> EffectPhase { EffectPhase::Post }

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

        // Layer 1: Large outer glow
        {
            let mut outer = Pixmap::new(w, h)
                .ok_or_else(|| crate::error::AppError::Render("neon outer layer failed".into()))?;
            text::render_text_to_pixmap(
                font_system, cache, buffer, &mut outer,
                text_x, text_y, self.color,
            );
            blur::gaussian_blur(&mut outer, self.radius);
            pixmap.draw_pixmap(
                0, 0, outer.as_ref(),
                &PixmapPaint { blend_mode: BlendMode::Screen, ..Default::default() },
                Transform::identity(), None,
            );
        }

        // Layer 2: Medium inner glow
        {
            let mut inner = Pixmap::new(w, h)
                .ok_or_else(|| crate::error::AppError::Render("neon inner layer failed".into()))?;
            text::render_text_to_pixmap(
                font_system, cache, buffer, &mut inner,
                text_x, text_y, self.color,
            );
            blur::gaussian_blur(&mut inner, self.radius / 3);
            pixmap.draw_pixmap(
                0, 0, inner.as_ref(),
                &PixmapPaint { blend_mode: BlendMode::Screen, ..Default::default() },
                Transform::identity(), None,
            );
        }

        // Layer 3: Bright white core
        {
            let bright = Color::from_rgba8(
                ((self.color.red() * 255.0) as u16).min(255).max(200) as u8,
                ((self.color.green() * 255.0) as u16).min(255).max(200) as u8,
                ((self.color.blue() * 255.0) as u16).min(255).max(200) as u8,
                255,
            );
            text::render_text_to_pixmap(
                font_system, cache, buffer, pixmap,
                text_x, text_y, bright,
            );
        }

        Ok(())
    }
}
