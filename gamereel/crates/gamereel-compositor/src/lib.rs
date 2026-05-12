//! gamereel-compositor — wgpu-based scene compositor.
//!
//! Replaces the per-pixel CPU blends in `gamereel_core::ffmpeg_inc::image_effect`
//! with a Vulkan-backed render pipeline. The compositor takes a list of
//! transformed sprites (texture + position + scale + rotation + anchor +
//! opacity) and paints them onto a render target, then reads the result
//! back as RGBA bytes.
//!
//! Why readback (M4) instead of zero-copy interop (M4.5):
//!   * wgpu's external-memory API for sharing Vulkan textures with CUDA
//!     is unstable across wgpu versions; locking it down deserves a
//!     dedicated milestone with its own self-proof tests.
//!   * The readback path still wins on heavy compositing scenes
//!     (predicted multi-x on 50+ overlay synthetic scenes) and proves
//!     out the architecture without wading into the interop deep end.
//!
//! Deliberately out of scope for M4:
//!   * Text rendering — hs-mvp pre-rasterizes text into PNG sprites
//!     in the report module, so the engine doesn't need cosmic-text yet.
//!   * Atlas packing — small N of sprites in current scenes makes
//!     individual sampled-textures fine; revisit if N grows past ~32.

pub mod compositor;
pub mod scene_adapter;
pub mod sprite;

pub use compositor::{CompositorError, WgpuCompositor};
pub use scene_adapter::{compose_scene_frame, upload_scene_textures};
pub use sprite::SpriteDraw;
