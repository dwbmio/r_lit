use crate::error::{AppError, Result};

/// Parse a CSS color string into RGBA u8 components.
pub fn parse_color(s: &str) -> Result<(u8, u8, u8, u8)> {
    let c = csscolorparser::parse(s)
        .map_err(|e| AppError::ColorParse(format!("{}: {}", s, e)))?;
    Ok((
        (c.r * 255.0) as u8,
        (c.g * 255.0) as u8,
        (c.b * 255.0) as u8,
        (c.a * 255.0) as u8,
    ))
}

/// Parse a CSS color into a tiny_skia::Color.
pub fn parse_skia_color(s: &str) -> Result<tiny_skia::Color> {
    let c = csscolorparser::parse(s)
        .map_err(|e| AppError::ColorParse(format!("{}: {}", s, e)))?;
    tiny_skia::Color::from_rgba(c.r as f32, c.g as f32, c.b as f32, c.a as f32)
        .ok_or_else(|| AppError::ColorParse(format!("invalid RGBA values: {}", s)))
}
