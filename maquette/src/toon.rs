//! Toon / cel-shaded `Material` used by `CellCube` entities in the preview.
//!
//! The shader lives at `assets/shaders/toon.wgsl`. This module defines the
//! Rust-side uniform struct, registers the `MaterialPlugin`, and exposes a
//! helper to build a material from a palette `Color`.
//!
//! v0.10 D-1.D added an **optional** `base_color_texture` so the
//! Material drawer's `View: Textured` mode can sample generated PNGs
//! on the 3-D preview. The same `Material` struct covers both modes:
//! * Flat → `base_color_texture: None`. Bevy's
//!   `AsBindGroup` impl fills the slot with a 1×1 white fallback so
//!   `textureSample(...) × base_color × shade` collapses to plain
//!   `base_color × shade`. Identical to the pre-D-1.D pixel output.
//! * Textured → `base_color_texture: Some(handle)` pointing at the
//!   `<cache_key>.png` image loaded by `texture_registry`. The
//!   shader multiplies that against `base_color` (which is white in
//!   this mode) so the texture is the dominant signal.
//!
//! **This material is preview-only.** Per the Export Golden Rule
//! (`docs/handoff/COST_AWARENESS.md`), exports never reference this
//! material — they ship geometry + vertex color + a standard/unlit
//! material, and the toon look is either (a) reproduced by the target
//! engine's own toon shader, or (b) faked with a baked inverted-hull
//! outline mesh.

use bevy::image::Image;
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
    /// Optional albedo texture sampled in `toon.wgsl`.
    ///
    /// `None` is the Flat-view default — the shader's bind group
    /// impl auto-fills a 1×1 white image (Bevy
    /// `AsBindGroup` Option semantics) so the same shader path
    /// renders both modes without branching.
    ///
    /// Texture is sampled in linear sRGB and multiplied by
    /// `params.base_color`, so the GUI can either:
    /// * tint a shared neutral texture by per-slot color (set
    ///   `base_color` to the palette colour, hand a greyscale tile
    ///   here), or
    /// * use a per-slot full-colour texture (set `base_color` to
    ///   white, hand the AI-generated PNG here) — this is what
    ///   v0.10 D-1.D's Textured mode actually does.
    #[texture(1)]
    #[sampler(2)]
    pub base_color_texture: Option<Handle<Image>>,
}

impl ToonMaterial {
    pub fn with_color(color: Color) -> Self {
        Self {
            params: ToonParams {
                base_color: color.into(),
                ..ToonParams::default()
            },
            base_color_texture: None,
        }
    }

    /// Build a textured variant: shader multiplies `texture × base_color`.
    /// Pass `Color::WHITE` for `tint` to let the texture dominate, or any
    /// non-white tint to colour-grade the texture per slot.
    pub fn with_color_and_texture(tint: Color, texture: Handle<Image>) -> Self {
        Self {
            params: ToonParams {
                base_color: tint.into(),
                ..ToonParams::default()
            },
            base_color_texture: Some(texture),
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
