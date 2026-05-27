//! `SpriteDraw` — what `WgpuCompositor::compose` consumes per draw call.
//!
//! Mirrors the per-node parameters that `Scene::on_render` produces in
//! the CPU path (position, scale, rotation, anchor, opacity), plus an
//! identifier the compositor uses to look up the pre-uploaded texture
//! in its atlas.

use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SpriteDraw {
    /// Stable identifier; matches a key passed to
    /// `WgpuCompositor::upload_texture`. Lookup is HashMap<&str, _>
    /// so cheap to match on.
    pub texture_id: String,

    /// Top-left in scene-space pixels (matches Scene::blend_image
    /// semantics: `pos` is the anchor point).
    pub pos: [f32; 2],

    /// Optional explicit size; if None the texture's native size is used.
    pub size: Option<[f32; 2]>,

    /// Per-axis scale multiplier. Default 1.0.
    pub scale: [f32; 2],

    /// Degrees, rotates around the anchor.
    pub rotation_deg: f32,

    /// 0.0–1.0 anchor within the sprite (0,0 = top-left, 0.5,0.5 = center).
    pub anchor: [f32; 2],

    /// 0..255; multiplied with the sprite's per-pixel alpha.
    pub opacity: u8,
}

/// Internal — used by the compositor to refer to an uploaded texture.
#[derive(Debug, Clone)]
pub(crate) struct UploadedTexture {
    pub width: u32,
    pub height: u32,
    /// Owned by Arc so the compositor can hold it across frames cheaply
    /// even when multiple SpriteDraw instances reference the same id.
    pub view: Arc<wgpu::TextureView>,
    pub texture: Arc<wgpu::Texture>,
}
