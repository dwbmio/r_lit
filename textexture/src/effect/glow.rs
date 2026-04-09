use std::collections::HashMap;
use cosmic_text::{Buffer, FontSystem, SwashCache};
use tiny_skia::{Color, Pixmap, PixmapPaint, BlendMode, Transform};

use crate::color;
use crate::effect::{Effect, EffectPhase, param_or};
use crate::error::Result;
use crate::render::{blur, text};

pub struct Glow {
    color: Color,
    radius: u32,
}

impl Glow {
    pub fn from_params(params: &HashMap<String, String>) -> Result<Self> {
        let color = match params.get("color") {
            Some(c) => color::parse_skia_color(c)?,
            None => Color::from_rgba8(0, 255, 255, 255),
        };
        let radius: u32 = param_or(params, "radius", 15)?;
        Ok(Self { color, radius })
    }
}

impl Effect for Glow {
    fn name(&self) -> &str { "glow" }
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

        let mut glow_pm = Pixmap::new(w, h)
            .ok_or_else(|| crate::error::AppError::Render("failed to create glow layer".into()))?;

        // Render text in glow color
        text::render_text_to_pixmap(
            font_system, cache, buffer, &mut glow_pm,
            text_x, text_y, self.color,
        );

        // Blur to create glow
        blur::gaussian_blur(&mut glow_pm, self.radius);

        // Composite with Screen blend for additive glow
        pixmap.draw_pixmap(
            0, 0,
            glow_pm.as_ref(),
            &PixmapPaint {
                blend_mode: BlendMode::Screen,
                ..Default::default()
            },
            Transform::identity(),
            None,
        );

        Ok(())
    }
}
