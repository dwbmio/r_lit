use std::collections::HashMap;
use cosmic_text::{Buffer, FontSystem, SwashCache};
use tiny_skia::{Color, Pixmap};

use crate::effect::{Effect, EffectPhase, param_or};
use crate::error::Result;
use crate::render::text;

/// Film/print grain — per-pixel multiplicative noise restricted to the glyph area.
///
/// Gives the text a gritty / retro printed texture (think old arcade title art).
/// Applied in the Post phase so it modulates whatever fill (solid / gradient) is
/// already on the glyph. The text alpha mask keeps grain off the transparent bg.
pub struct Grain {
    /// Noise intensity 0..1 (0 = none, higher = grittier). Default 0.25.
    amount: f32,
    /// Deterministic seed so the same spec reproduces the same grain.
    seed: u32,
    /// Grain cell size in px (1 = per-pixel fine grain, >1 = chunkier specks).
    scale: u32,
}

impl Grain {
    pub fn from_params(params: &HashMap<String, String>) -> Result<Self> {
        let amount: f32 = param_or(params, "amount", 0.25)?;
        let seed: u32 = param_or(params, "seed", 0)?;
        let scale: u32 = param_or(params, "scale", 1)?;
        Ok(Self {
            amount: amount.clamp(0.0, 1.0),
            seed,
            scale: scale.max(1),
        })
    }
}

/// Cheap integer hash → [0,1). Deterministic, no allocation.
fn hash01(mut a: u32) -> f32 {
    a ^= a >> 17;
    a = a.wrapping_mul(0xed5a_d4bb);
    a ^= a >> 11;
    a = a.wrapping_mul(0xac4c_1b51);
    a ^= a >> 15;
    a = a.wrapping_mul(0x3184_8bab);
    a ^= a >> 14;
    (a as f32) / (u32::MAX as f32)
}

impl Effect for Grain {
    fn name(&self) -> &str {
        "grain"
    }
    fn phase(&self) -> EffectPhase {
        EffectPhase::Post
    }

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

        // Glyph mask: only modulate pixels that belong to the text.
        let mut mask_pm = Pixmap::new(w, h)
            .ok_or_else(|| crate::error::AppError::Render("failed to create grain mask".into()))?;
        text::render_text_to_pixmap(
            font_system,
            cache,
            buffer,
            &mut mask_pm,
            text_x,
            text_y,
            Color::WHITE,
        );
        let mask = mask_pm.data().to_vec();

        let data = pixmap.data_mut();
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize * 4;
                if mask[idx + 3] == 0 {
                    continue;
                }
                let cx = x / self.scale;
                let cy = y / self.scale;
                let key = cx
                    .wrapping_mul(73_856_093)
                    ^ cy.wrapping_mul(19_349_663)
                    ^ self.seed.wrapping_mul(83_492_791);
                // noise in [-1, 1]
                let n = hash01(key) * 2.0 - 1.0;
                let factor = (1.0 + n * self.amount).max(0.0);
                // pixmap is premultiplied alpha: keep rgb <= alpha to stay valid.
                let alpha = data[idx + 3] as f32;
                for c in 0..3 {
                    let v = data[idx + c] as f32 * factor;
                    data[idx + c] = v.min(alpha).clamp(0.0, 255.0) as u8;
                }
            }
        }

        Ok(())
    }
}
