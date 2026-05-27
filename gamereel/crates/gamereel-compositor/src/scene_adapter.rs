//! Bridge between gamereel-core's `Scene` and the wgpu compositor.
//!
//! Two helpers:
//!
//!   * [`upload_scene_textures`] — call once after `Scene::on_init`.
//!     Iterates the scene's required textures (background + every node's
//!     texture) and uploads each to the wgpu compositor's atlas keyed
//!     by texture id.
//!
//!   * [`compose_scene_frame`] — per-frame: ticks the scene's animation
//!     state via `Scene::do_action`, walks the children, builds a
//!     SpriteDraw for each *active* node (z-order = ascending node id,
//!     courtesy of `BTreeMap`), and asks the compositor to paint them
//!     onto the background.
//!
//! Background handling: the scene's `clear_tp_id` texture is drawn first
//! at top-left anchor covering the full canvas. Static children paint
//! over the background; non-static active children paint last.
//! Mirrors the layered semantics of `Scene::on_render` (`_clear_image`
//! → `_dynamic_beach_image` → final frame) but recomputes every frame
//! on the GPU rather than relying on the CPU dirty cache (GPU compose
//! is cheap; cache complexity isn't worth carrying over).

use crate::compositor::{CompositorError, WgpuCompositor};
use crate::sprite::SpriteDraw;
use gamereel_core::stage::scene::Scene;
use gamereel_core::RuntimeCtx;

/// Upload every texture the scene references. Idempotent — calling
/// twice with the same textures is a no-op overwrite.
pub fn upload_scene_textures(
    compositor: &mut WgpuCompositor,
    scene: &Scene,
    ctx: &RuntimeCtx,
) -> Result<(), CompositorError> {
    // Background.
    if !scene.tp_id.is_empty() {
        if let Some(arc) = ctx.get_texture(&scene.tp_id).dynamic_image.as_ref() {
            compositor.upload_texture(scene.tp_id.clone(), arc.as_ref())?;
        }
    }
    // Every node's texture (skips empty / unset).
    for (_, node) in &scene.children {
        if let Some(tp_id) = node.tp_id.as_ref() {
            if tp_id.is_empty() { continue; }
            if let Some(arc) = ctx.get_texture(tp_id).dynamic_image.as_ref() {
                compositor.upload_texture(tp_id.clone(), arc.as_ref())?;
            }
        }
    }
    Ok(())
}

/// Compose one frame of the scene. Returns tightly-packed RGBA8 bytes
/// the caller can hand to ffmpeg's RGBA→YUV path or the M3 cudarc
/// kernel without further re-conversion.
pub fn compose_scene_frame(
    compositor: &mut WgpuCompositor,
    scene: &mut Scene,
    _ctx: &mut RuntimeCtx,
    t: f32,
) -> Result<Vec<u8>, CompositorError> {
    // Tick scene animation state — this mutates child positions etc.
    let _is_dirty = scene
        .do_action(t)
        .map_err(|e| CompositorError::UnknownTexture(format!("scene.do_action: {e}")))?;

    let w = compositor.width() as f32;
    let h = compositor.height() as f32;

    let mut draws: Vec<SpriteDraw> = Vec::with_capacity(scene.children.len() + 1);

    // 1) Background covers full canvas.
    if !scene.tp_id.is_empty() {
        draws.push(SpriteDraw {
            texture_id: scene.tp_id.clone(),
            pos: [0.0, 0.0],
            size: Some([w, h]),
            scale: [1.0, 1.0],
            rotation_deg: 0.0,
            anchor: [0.0, 0.0],
            opacity: 255,
        });
    }

    // 2) Static layer (z = ascending id). These would be cached in
    //    `_dynamic_beach_image` on the CPU path; on GPU we just redraw.
    for (_, node) in &scene.children {
        if !node.is_static { continue; }
        let Some(tp_id) = node.tp_id.as_ref() else { continue };
        if tp_id.is_empty() { continue; }
        draws.push(node_to_draw(node, tp_id));
    }

    // 3) Dynamic active layer (last so it paints on top).
    for (_, node) in &scene.children {
        if node.is_static || !node.active { continue; }
        let Some(tp_id) = node.tp_id.as_ref() else { continue };
        if tp_id.is_empty() { continue; }
        draws.push(node_to_draw(node, tp_id));
    }

    compositor.compose_to_host(&draws)
}

fn node_to_draw(node: &gamereel_core::stage::node::NodeGraph, tp_id: &str) -> SpriteDraw {
    SpriteDraw {
        texture_id: tp_id.to_string(),
        pos: [node.pos[0], node.pos[1]],
        size: node.size,
        scale: node.scale.unwrap_or([1.0, 1.0]),
        rotation_deg: node.rotation.unwrap_or(0.0),
        anchor: node.anchor.unwrap_or([0.0, 0.0]),
        opacity: node.opacity.unwrap_or(255).min(255) as u8,
    }
}
