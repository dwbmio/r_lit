use std::collections::HashMap;
use cosmic_text::{Buffer, FontSystem, SwashCache};
use tiny_skia::{Color, Pixmap, PixmapPaint, BlendMode, Transform};

use crate::color;
use crate::effect::{Effect, EffectPhase, param_or};
use crate::error::Result;
use crate::render::text;

pub struct Outline {
    color: Color,
    width: f32,
}

impl Outline {
    pub fn from_params(params: &HashMap<String, String>) -> Result<Self> {
        let color = match params.get("color") {
            Some(c) => color::parse_skia_color(c)?,
            None => Color::from_rgba8(255, 255, 255, 255),
        };
        let width: f32 = param_or(params, "width", 2.0)?;
        Ok(Self { color, width })
    }
}

impl Effect for Outline {
    fn name(&self) -> &str { "outline" }
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

        // Simulate outline by rendering text at offsets in the outline color
        let offsets = generate_outline_offsets(self.width);

        let mut outline_pm = Pixmap::new(w, h)
            .ok_or_else(|| crate::error::AppError::Render("failed to create outline layer".into()))?;

        for (dx, dy) in &offsets {
            text::render_text_to_pixmap(
                font_system,
                cache,
                buffer,
                &mut outline_pm,
                text_x + dx,
                text_y + dy,
                self.color,
            );
        }

        pixmap.draw_pixmap(
            0, 0,
            outline_pm.as_ref(),
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

/// Generate offset points around the origin for outline rendering.
fn generate_outline_offsets(width: f32) -> Vec<(f32, f32)> {
    let mut offsets = Vec::new();
    let steps = (width.ceil() as i32).max(1) * 4;

    for i in 0..steps {
        let angle = (i as f32 / steps as f32) * std::f32::consts::TAU;
        offsets.push((angle.cos() * width, angle.sin() * width));
    }

    offsets
}
