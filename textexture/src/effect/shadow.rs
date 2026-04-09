use std::collections::HashMap;
use cosmic_text::{Buffer, FontSystem, SwashCache};
use tiny_skia::{Color, Pixmap, PixmapPaint, BlendMode, Transform};

use crate::color;
use crate::effect::{Effect, EffectPhase, param_or};
use crate::error::Result;
use crate::render::{blur, text};

pub struct Shadow {
    color: Color,
    offset_x: f32,
    offset_y: f32,
    blur_radius: u32,
}

impl Shadow {
    pub fn from_params(params: &HashMap<String, String>) -> Result<Self> {
        let color = match params.get("color") {
            Some(c) => color::parse_skia_color(c)?,
            None => Color::from_rgba8(0, 0, 0, 128),
        };
        let offset_x: f32 = param_or(params, "ox", 4.0)?;
        let offset_y: f32 = param_or(params, "oy", 4.0)?;
        let blur_radius: u32 = param_or(params, "blur", 8)?;

        Ok(Self { color, offset_x, offset_y, blur_radius })
    }
}

impl Effect for Shadow {
    fn name(&self) -> &str { "shadow" }
    fn phase(&self) -> EffectPhase { EffectPhase::Pre }

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

        // Render text in shadow color on a temp pixmap
        let mut shadow_pm = Pixmap::new(w, h)
            .ok_or_else(|| crate::error::AppError::Render("failed to create shadow layer".into()))?;

        text::render_text_to_pixmap(
            font_system,
            cache,
            buffer,
            &mut shadow_pm,
            text_x + self.offset_x,
            text_y + self.offset_y,
            self.color,
        );

        // Blur
        blur::gaussian_blur(&mut shadow_pm, self.blur_radius);

        // Composite behind (draw shadow first, it's Pre phase)
        pixmap.draw_pixmap(
            0, 0,
            shadow_pm.as_ref(),
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
