pub mod shadow;
pub mod outline;
pub mod gradient;
pub mod glow;
pub mod neon;

use std::collections::HashMap;
use cosmic_text::{Buffer, FontSystem, SwashCache};
use serde::Serialize;
use tiny_skia::{Color, Pixmap};

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectPhase {
    Pre,  // Before text (shadow, 3D)
    Fill, // Replaces default text fill (gradient, texture)
    Post, // After text (outline, glow, neon)
}

pub trait Effect: Send + Sync {
    fn name(&self) -> &str;
    fn phase(&self) -> EffectPhase;
    fn apply(
        &self,
        pixmap: &mut Pixmap,
        font_system: &mut FontSystem,
        cache: &mut SwashCache,
        buffer: &Buffer,
        text_x: f32,
        text_y: f32,
        text_color: Color,
    ) -> Result<()>;
}

#[derive(Debug, Serialize)]
pub struct EffectInfo {
    pub name: String,
    pub description: String,
    pub params: Vec<String>,
}

/// List all available effects and their parameters.
pub fn list_effects() -> Vec<EffectInfo> {
    vec![
        EffectInfo {
            name: "shadow".into(),
            description: "Drop shadow / 阴影".into(),
            params: vec!["color".into(), "ox".into(), "oy".into(), "blur".into()],
        },
        EffectInfo {
            name: "outline".into(),
            description: "Text outline/stroke / 描边".into(),
            params: vec!["color".into(), "width".into()],
        },
        EffectInfo {
            name: "gradient".into(),
            description: "Gradient fill / 渐变填充".into(),
            params: vec!["start".into(), "end".into(), "angle".into()],
        },
        EffectInfo {
            name: "glow".into(),
            description: "Outer glow / 外发光".into(),
            params: vec!["color".into(), "radius".into()],
        },
        EffectInfo {
            name: "neon".into(),
            description: "Neon glow effect / 霓虹效果".into(),
            params: vec!["color".into(), "radius".into()],
        },
    ]
}

/// Parse "name:key=val,key=val" effect specs into Effect trait objects.
pub fn parse_effect_specs(specs: &[String]) -> Result<Vec<Box<dyn Effect>>> {
    let mut effects: Vec<Box<dyn Effect>> = Vec::new();

    for spec in specs {
        let (name, params) = parse_one_spec(spec)?;
        let effect: Box<dyn Effect> = match name.as_str() {
            "shadow" => Box::new(shadow::Shadow::from_params(&params)?),
            "outline" => Box::new(outline::Outline::from_params(&params)?),
            "gradient" => Box::new(gradient::Gradient::from_params(&params)?),
            "glow" => Box::new(glow::Glow::from_params(&params)?),
            "neon" => Box::new(neon::Neon::from_params(&params)?),
            _ => return Err(AppError::UnknownEffect(name)),
        };
        effects.push(effect);
    }

    Ok(effects)
}

/// Parse "name:key=val,key=val" into (name, map).
fn parse_one_spec(spec: &str) -> Result<(String, HashMap<String, String>)> {
    let mut params = HashMap::new();

    let (name, rest) = if let Some(idx) = spec.find(':') {
        (&spec[..idx], Some(&spec[idx + 1..]))
    } else {
        (spec, None)
    };

    if let Some(rest) = rest {
        for pair in rest.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            if let Some(eq_idx) = pair.find('=') {
                let key = pair[..eq_idx].trim().to_string();
                let val = pair[eq_idx + 1..].trim().to_string();
                params.insert(key, val);
            } else {
                return Err(AppError::InvalidEffectParam(format!(
                    "expected key=value, got '{}'",
                    pair
                )));
            }
        }
    }

    Ok((name.to_string(), params))
}

/// Helper: get a param or use default.
pub fn param_or<T: std::str::FromStr>(
    params: &HashMap<String, String>,
    key: &str,
    default: T,
) -> Result<T> {
    match params.get(key) {
        Some(v) => v.parse::<T>().map_err(|_| {
            AppError::InvalidEffectParam(format!("invalid value for '{}': {}", key, v))
        }),
        None => Ok(default),
    }
}
