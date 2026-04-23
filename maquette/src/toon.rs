//! Toon / cel-shaded `Material` used by `CellCube` entities in the preview.
//!
//! The shader lives at `assets/shaders/toon.wgsl`. This module defines the
//! Rust-side uniform struct, registers the `MaterialPlugin`, and exposes a
//! helper to build a material from a palette `Color`.
//!
//! **This material is preview-only.** Per the Export Golden Rule
//! (`docs/handoff/COST_AWARENESS.md`), exports never reference this
//! material — they ship geometry + vertex color + a standard/unlit
//! material, and the toon look is either (a) reproduced by the target
//! engine's own toon shader, or (b) faked with a baked inverted-hull
//! outline mesh.

use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

pub const TOON_SHADER_PATH: &str = "shaders/toon.wgsl";

/// Preview-time default: light from upper-front-right, 3 bands, 35% ambient.
pub const DEFAULT_LIGHT_DIR: Vec3 = Vec3::new(-0.45, -1.0, -0.35);
pub const DEFAULT_BANDS: f32 = 3.0;
pub const DEFAULT_AMBIENT: f32 = 0.35;

/// Packed uniform passed to `toon.wgsl`.
///
/// Layout is hand-tuned for std140: all fields are `vec4`-sized or larger
/// so we never hit vec3/f32 alignment gotchas. Tests cover round-tripping.
#[derive(Debug, Clone, ShaderType)]
pub struct ToonParams {
    pub base_color: LinearRgba,
    /// xyz = light direction in world space (pointing *from* the light
    /// *toward* the scene); w = number of cel bands.
    pub light_dir_bands: Vec4,
    /// x = ambient floor (0..1). yzw reserved for future knobs.
    pub ambient_pad: Vec4,
}

impl Default for ToonParams {
    fn default() -> Self {
        Self {
            base_color: LinearRgba::WHITE,
            light_dir_bands: DEFAULT_LIGHT_DIR.extend(DEFAULT_BANDS),
            ambient_pad: Vec4::new(DEFAULT_AMBIENT, 0.0, 0.0, 0.0),
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct ToonMaterial {
    #[uniform(0)]
    pub params: ToonParams,
}

impl ToonMaterial {
    pub fn with_color(color: Color) -> Self {
        Self {
            params: ToonParams {
                base_color: color.into(),
                ..ToonParams::default()
            },
        }
    }
}

impl Material for ToonMaterial {
    fn fragment_shader() -> ShaderRef {
        TOON_SHADER_PATH.into()
    }
}

pub struct ToonPlugin;

impl Plugin for ToonPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<ToonMaterial>::default());
    }
}
