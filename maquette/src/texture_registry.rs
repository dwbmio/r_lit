//! GUI-side cache_key → `Handle<Image>` registry for the toon
//! shader's optional `base_color_texture`.
//!
//! Pipeline:
//!
//! 1. `slot_texgen::SlotTexgenPlugin` writes a freshly-generated
//!    PNG to `~/.cache/maquette/textures/<cache_key>.png` and stamps
//!    the slot's `PaletteSlotMeta::texture` with the cache key.
//! 2. `preview_mesh::rebuild_cell_mesh` walks the palette while
//!    rebuilding the 3-D cube mesh; for each palette colour, it asks
//!    [`TextureRegistry::handle_for`] for a `Handle<Image>` keyed
//!    by `cache_key`.
//! 3. The registry deduplicates: identical `cache_key`s share one
//!    `Image` asset (and one wgpu texture) regardless of how many
//!    palette slots reference them. Repeated palette-rebuilds (every
//!    edit while in Textured view) only decode each PNG once.
//! 4. On decode failure the failure is *sticky*: subsequent calls
//!    return `None` immediately rather than re-decoding a corrupt
//!    file every frame, and the failure is logged once.
//!
//! The whole module is bin-only — the headless lib has no
//! `wgpu::Image` concept and the export pipeline (Export Golden
//! Rule) deliberately ships flat colour, not toon-shaded textured
//! geometry.

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use maquette::texgen::default_cache_dir;

/// Per-cache-key load state. We track failures as a separate
/// variant rather than a plain `Option<Handle<Image>>` so the
/// registry can be sticky — see module-level docs.
#[derive(Debug, Clone)]
enum LoadState {
    /// PNG decoded and uploaded to wgpu via the asset server.
    Loaded(Handle<Image>),
    /// Decode / IO failed; sticky-cached so we don't retry every
    /// frame. The string is purely diagnostic — the registry
    /// surfaces it through `last_errors` for the GUI.
    Failed(String),
}

/// Bevy resource owning the cache_key → image-handle map.
///
/// Inserted by [`TextureRegistryPlugin::build`] at startup with an
/// empty map. Mutated lazily by `handle_for` — there's no
/// background loader thread; PNG decode runs on the main thread on
/// first request. Each PNG is small (typically 256² ≈ 50–80 KB
/// after Fal's PNG encoder) so the synchronous decode lands
/// well under one frame's budget.
///
/// If decode latency ever becomes a problem (e.g. a project
/// references hundreds of textures and hits Textured mode all at
/// once), promoting this to an `AsyncComputeTaskPool::spawn`
/// pipeline is a contained change: keep `LoadState::Loading`
/// variant, hand the `Handle<Image>` early via
/// `Assets::reserve_handle`, fill the bytes asynchronously.
#[derive(Resource, Default)]
pub struct TextureRegistry {
    entries: HashMap<String, LoadState>,
}

impl TextureRegistry {
    /// Resolve `cache_key` to a wgpu image handle, decoding and
    /// uploading the PNG on first request. Subsequent calls with
    /// the same key reuse the same handle.
    ///
    /// `None` is returned when:
    /// * the texgen disk-cache directory isn't resolvable (unset
    ///   `HOME` on bare CI, or pre-`default_cache_dir()` failure),
    /// * the `<cache_key>.png` doesn't exist on disk,
    /// * the PNG is malformed / has an unsupported color type.
    ///
    /// All three are sticky — the failure is cached so we don't
    /// retry every frame the user is editing.
    pub fn handle_for(
        &mut self,
        cache_key: &str,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        if let Some(state) = self.entries.get(cache_key) {
            return match state {
                LoadState::Loaded(h) => Some(h.clone()),
                LoadState::Failed(_) => None,
            };
        }

        // Cold path: locate, read, decode, upload.
        match resolve_and_load(cache_key, images) {
            Ok(handle) => {
                self.entries
                    .insert(cache_key.to_string(), LoadState::Loaded(handle.clone()));
                Some(handle)
            }
            Err(reason) => {
                log::warn!(
                    "texture_registry: failed to load cache_key={cache_key}: {reason}"
                );
                self.entries
                    .insert(cache_key.to_string(), LoadState::Failed(reason));
                None
            }
        }
    }

    /// Diagnostic accessor — surfaces every sticky failure for a
    /// future "show texture problems" UI panel. Currently unused;
    /// kept so the data is available without a re-fetch when the
    /// UI grows that affordance.
    #[allow(dead_code)]
    pub fn failures(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().filter_map(|(k, s)| match s {
            LoadState::Failed(e) => Some((k.as_str(), e.as_str())),
            LoadState::Loaded(_) => None,
        })
    }

    /// Drop every cached entry. Useful for the (planned) "clear
    /// texture cache" admin button — also makes the registry
    /// trivially testable without juggling lifetimes around the
    /// `Assets<Image>` collection.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Plugin that registers the resource. Trivial — there's no
/// startup work and no per-frame system. We keep it as a plugin
/// rather than a bare `init_resource` in `main.rs` so the
/// "owning module" pattern stays consistent across modules.
pub struct TextureRegistryPlugin;

impl Plugin for TextureRegistryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TextureRegistry>();
    }
}

fn resolve_and_load(
    cache_key: &str,
    images: &mut Assets<Image>,
) -> Result<Handle<Image>, String> {
    let dir = default_cache_dir().ok_or_else(|| "default_cache_dir() returned None".to_string())?;
    let path: PathBuf = dir.join(format!("{cache_key}.png"));
    if !path.exists() {
        return Err(format!("not on disk: {}", path.display()));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("io read {}: {e}", path.display()))?;
    let image = decode_png_to_image(&bytes)?;
    Ok(images.add(image))
}

/// Decode an in-memory PNG into a Bevy `Image` ready for wgpu
/// upload. Public so other GUI modules (e.g. `block_composer`'s
/// preview mesh) can share the same code path — historically each
/// site rolled its own decoder.
///
/// Output is always RGBA8 sRGB. RGB sources are widened with
/// alpha = 255 ; palette / 16-bit / grayscale images currently
/// hard-fail rather than silently mis-render.
pub fn decode_png_to_image(bytes: &[u8]) -> Result<Image, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("png header: {e}"))?;
    let buf_size = reader
        .output_buffer_size()
        .ok_or_else(|| "png: output_buffer_size overflowed".to_string())?;
    let mut buf = vec![0; buf_size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("png decode: {e}"))?;

    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(info.buffer_size() * 4 / 3);
            for c in buf.chunks(3) {
                out.extend_from_slice(c);
                out.push(255);
            }
            out
        }
        other => return Err(format!("unsupported color_type={other:?}")),
    };
    Ok(Image::new(
        Extent3d {
            width: info.width,
            height: info.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthesise a tiny 1×1 PNG for round-trip tests.
    fn one_pixel_png(r: u8, g: u8, b: u8) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[r, g, b, 255]).unwrap();
        drop(writer);
        out
    }

    #[test]
    fn decode_png_round_trips_rgba() {
        let png_bytes = one_pixel_png(255, 128, 64);
        let img = decode_png_to_image(&png_bytes).unwrap();
        assert_eq!(img.texture_descriptor.size.width, 1);
        assert_eq!(img.texture_descriptor.size.height, 1);
        let data = img.data.as_ref().expect("image data missing after decode");
        assert_eq!(&data[..3], &[255, 128, 64]);
    }

    #[test]
    fn decode_png_widens_rgb_to_rgba_with_full_alpha() {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, 1, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[10, 20, 30]).unwrap();
        drop(writer);
        let img = decode_png_to_image(&out).unwrap();
        let data = img.data.as_ref().unwrap();
        assert_eq!(data, &[10, 20, 30, 255]);
    }

    #[test]
    fn decode_png_rejects_garbage_bytes() {
        let err = decode_png_to_image(&[0, 1, 2, 3]).unwrap_err();
        assert!(err.contains("png header"), "unexpected error: {err}");
    }
}
