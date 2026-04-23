//! Pure-CPU triangle rasterizer for headless preview PNGs.
//!
//! Used by `maquette-cli render` and available as a library function
//! for anyone (CI, a docs generator, a thumbnail pipeline) who needs a
//! picture of a `.maq` project without booting Bevy or a GPU.
//!
//! Design constraints:
//!
//! * **No GPU**. We do software scanline rasterization. This keeps the
//!   CLI runnable on headless CI nodes where `wgpu` can't find an
//!   adapter.
//! * **Deterministic**. Same input → same bytes (within IEEE-754
//!   fuzz). We avoid HashMap iteration over geometry; the greedy
//!   mesher already returns a deterministic order.
//! * **Matches the export, not the preview**. We render the same
//!   greedy mesh the exporter ships. The `ToonMaterial` cel shader
//!   and `bevy_mod_outline` inverted-hull live only in the preview;
//!   the CLI render reflects what game engines actually receive.
//!
//! ## What the picture shows
//!
//! Orthographic isometric projection: yaw = -45°, pitch =
//! asin(1/√3) ≈ 35.264°. Model is centered and fit into the frame
//! with a configurable margin. Each triangle is flat-shaded with a
//! Lambert term against a fixed camera-space light direction plus an
//! ambient floor so back-facing-toward-light surfaces stay readable.
//!
//! No outline is rasterized. Users who want a cartoonish silhouette
//! can still see the result of the exported inverted-hull by loading
//! the `.glb` into an engine; the CLI render is a shape sanity-check,
//! not a marketing beauty shot.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use bevy::prelude::Color;

use crate::grid::{Grid, Palette};
use crate::mesher::build_color_buckets;

/// Default output dimensions. Matches "small icon, still legible" for
/// CI golden images.
pub const DEFAULT_SIZE: u32 = 512;

/// Knobs for [`render_to_rgba`] / [`write_png`]. Defaults reproduce
/// the shipping `maquette-cli render` look.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub width: u32,
    pub height: u32,
    /// sRGB background color. Applied wherever no triangle wins the
    /// depth test. Transparent output is deliberately not supported —
    /// game-engine preview thumbs tend to be opaque.
    pub background: [u8; 3],
    /// Fraction of the shorter screen axis left empty around the
    /// model. `0.08` = 8% padding each side.
    pub margin: f32,
    /// Base luminance applied even when the surface faces away from
    /// the light. Keeps deep shadows from going pure black.
    pub ambient: f32,
    /// Light direction **in camera space**, un-normalised. The
    /// rasterizer normalises it before use. "Upper-right-front" is
    /// the usual choice for iso voxel art.
    pub light_dir: [f32; 3],
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            width: DEFAULT_SIZE,
            height: DEFAULT_SIZE,
            background: [24, 26, 30],
            margin: 0.08,
            ambient: 0.35,
            light_dir: [0.3, 0.55, 0.78],
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum RenderError {
    #[error("invalid render size: {w}×{h} (both dimensions must be > 0)")]
    InvalidSize { w: u32, h: u32 },
    #[error("png encode failed: {0}")]
    Png(#[from] png::EncodingError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Rasterize `grid` / `palette` into an RGBA8 byte buffer of length
/// `opts.width * opts.height * 4`. Pixels are in top-left origin
/// order; the PNG encoder consumes this layout directly.
pub fn render_to_rgba(
    grid: &Grid,
    palette: &Palette,
    opts: &RenderOptions,
) -> Result<Vec<u8>, RenderError> {
    if opts.width == 0 || opts.height == 0 {
        return Err(RenderError::InvalidSize {
            w: opts.width,
            h: opts.height,
        });
    }
    let w = opts.width as usize;
    let h = opts.height as usize;

    let bg = [
        opts.background[0],
        opts.background[1],
        opts.background[2],
        255u8,
    ];
    let mut fb = vec![bg; w * h];

    let buckets = build_color_buckets(grid);
    if buckets.is_empty() {
        return Ok(flatten(fb));
    }

    // Pre-rotate every vertex / normal once per bucket; triangle
    // indexing then points into the rotated arrays.
    let mut tris: Vec<Tri> = Vec::new();
    let mut bounds = Bounds2::empty();

    for (ci, builder) in &buckets {
        let color = palette.get(*ci).unwrap_or(Color::WHITE);
        let lin = color.to_linear();
        let rgb = [lin.red, lin.green, lin.blue];

        let rotated_pos: Vec<[f32; 3]> =
            builder.positions.iter().map(|p| rotate_iso(*p)).collect();
        let rotated_nrm: Vec<[f32; 3]> =
            builder.normals.iter().map(|n| rotate_iso(*n)).collect();

        for p in &rotated_pos {
            bounds.include(p[0], p[1]);
        }

        for chunk in builder.indices.chunks_exact(3) {
            let i0 = chunk[0] as usize;
            let i1 = chunk[1] as usize;
            let i2 = chunk[2] as usize;
            tris.push(Tri {
                v: [rotated_pos[i0], rotated_pos[i1], rotated_pos[i2]],
                // The mesher guarantees every vertex in a triangle shares
                // a normal (one normal per quad). Using the first vertex's
                // is safe and avoids an averaging step.
                n: rotated_nrm[i0],
                color: rgb,
            });
        }
    }

    if tris.is_empty() {
        return Ok(flatten(fb));
    }

    let model_w = (bounds.max_x - bounds.min_x).max(1e-3);
    let model_h = (bounds.max_y - bounds.min_y).max(1e-3);
    let margin_x = opts.width as f32 * opts.margin;
    let margin_y = opts.height as f32 * opts.margin;
    let avail_w = (opts.width as f32 - 2.0 * margin_x).max(1.0);
    let avail_h = (opts.height as f32 - 2.0 * margin_y).max(1.0);
    let scale = (avail_w / model_w).min(avail_h / model_h);
    let cx_model = (bounds.min_x + bounds.max_x) * 0.5;
    let cy_model = (bounds.min_y + bounds.max_y) * 0.5;
    let cx_screen = opts.width as f32 * 0.5;
    let cy_screen = opts.height as f32 * 0.5;

    let mut depth = vec![f32::NEG_INFINITY; w * h];
    let light = normalize3(opts.light_dir);

    for t in &tris {
        let p0 = project(&t.v[0], cx_model, cy_model, scale, cx_screen, cy_screen);
        let p1 = project(&t.v[1], cx_model, cy_model, scale, cx_screen, cy_screen);
        let p2 = project(&t.v[2], cx_model, cy_model, scale, cx_screen, cy_screen);

        let n = normalize3(t.n);
        let nl = (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).max(0.0);
        let shade = opts.ambient + (1.0 - opts.ambient) * nl;

        let lin = [
            (t.color[0] * shade).clamp(0.0, 1.0),
            (t.color[1] * shade).clamp(0.0, 1.0),
            (t.color[2] * shade).clamp(0.0, 1.0),
        ];
        let pixel = [
            f_to_u8(linear_to_srgb(lin[0])),
            f_to_u8(linear_to_srgb(lin[1])),
            f_to_u8(linear_to_srgb(lin[2])),
            255u8,
        ];

        rasterize(&p0, &p1, &p2, pixel, w, h, &mut fb, &mut depth);
    }

    Ok(flatten(fb))
}

/// Convenience wrapper around [`render_to_rgba`] that writes the
/// image as sRGB PNG at `out`.
pub fn write_png(
    grid: &Grid,
    palette: &Palette,
    opts: &RenderOptions,
    out: &Path,
) -> Result<(), RenderError> {
    let rgba = render_to_rgba(grid, palette, opts)?;
    let file = File::create(out)?;
    let buf = BufWriter::new(file);
    let mut encoder = png::Encoder::new(buf, opts.width, opts.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&rgba)?;
    Ok(())
}

// --------------------------------------------------------------------
// Internals
// --------------------------------------------------------------------

struct Tri {
    v: [[f32; 3]; 3],
    n: [f32; 3],
    color: [f32; 3],
}

struct Bounds2 {
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
}

impl Bounds2 {
    fn empty() -> Self {
        Self {
            min_x: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            min_y: f32::INFINITY,
            max_y: f32::NEG_INFINITY,
        }
    }
    fn include(&mut self, x: f32, y: f32) {
        if x < self.min_x {
            self.min_x = x;
        }
        if x > self.max_x {
            self.max_x = x;
        }
        if y < self.min_y {
            self.min_y = y;
        }
        if y > self.max_y {
            self.max_y = y;
        }
    }
}

#[inline]
fn project(
    p: &[f32; 3],
    cx_model: f32,
    cy_model: f32,
    scale: f32,
    cx_screen: f32,
    cy_screen: f32,
) -> [f32; 3] {
    let sx = (p[0] - cx_model) * scale + cx_screen;
    // Screen Y grows downward; model Y grows up.
    let sy = cy_screen - (p[1] - cy_model) * scale;
    // Depth: after `rotate_iso`, the world's (+X,+Y,+Z) corner lands
    // at the largest +Z. We keep "greater z = closer" so the z-test
    // is `z > zbuf[idx]`.
    let sz = p[2];
    [sx, sy, sz]
}

#[allow(clippy::too_many_arguments)] // triangle + target framebuffer; splitting into a struct buys nothing
fn rasterize(
    p0: &[f32; 3],
    p1: &[f32; 3],
    p2: &[f32; 3],
    color: [u8; 4],
    w: usize,
    h: usize,
    fb: &mut [[u8; 4]],
    zbuf: &mut [f32],
) {
    let min_x = p0[0].min(p1[0]).min(p2[0]).floor().max(0.0) as i32;
    let max_x = p0[0]
        .max(p1[0])
        .max(p2[0])
        .ceil()
        .min((w as i32 - 1) as f32) as i32;
    let min_y = p0[1].min(p1[1]).min(p2[1]).floor().max(0.0) as i32;
    let max_y = p0[1]
        .max(p1[1])
        .max(p2[1])
        .ceil()
        .min((h as i32 - 1) as f32) as i32;
    if max_x < min_x || max_y < min_y {
        return;
    }

    let area = edge(p0[0], p0[1], p1[0], p1[1], p2[0], p2[1]);
    if area.abs() < 1e-7 {
        return;
    }
    let inv_area = 1.0 / area;
    let ccw = area > 0.0;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = edge(p1[0], p1[1], p2[0], p2[1], px, py);
            let w1 = edge(p2[0], p2[1], p0[0], p0[1], px, py);
            let w2 = edge(p0[0], p0[1], p1[0], p1[1], px, py);
            let inside = if ccw {
                w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
            } else {
                w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
            };
            if !inside {
                continue;
            }
            let b0 = w0 * inv_area;
            let b1 = w1 * inv_area;
            let b2 = w2 * inv_area;
            let z = b0 * p0[2] + b1 * p1[2] + b2 * p2[2];
            let idx = y as usize * w + x as usize;
            if z > zbuf[idx] {
                zbuf[idx] = z;
                fb[idx] = color;
            }
        }
    }
}

#[inline]
fn edge(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

/// World-to-iso-camera rotation. Yaw first (−45° about Y), then pitch
/// (≈ 35.264° about X). The constants are pre-baked so the hot loop
/// never hits `asin`.
///
/// Sign convention (verified by trace):
/// * world (+X, +Y, +Z) ↦ rotated (+0, +0, +√3). Biggest +Z =
///   closest to the viewer.
/// * world (−X, −Y, −Z) ↦ rotated (0, 0, −√3). Furthest.
///
/// The screen axes are then the rotated x and y; `rotated_z` is the
/// depth value the z-buffer sorts on.
#[inline]
fn rotate_iso(p: [f32; 3]) -> [f32; 3] {
    const YAW: f32 = -std::f32::consts::FRAC_PI_4; // −45°
    // asin(1/√3) — the canonical isometric tilt. Precomputed to avoid
    // stdlib math in the inner loop.
    const PITCH: f32 = 0.615_479_7;
    let ycos = YAW.cos();
    let ysin = YAW.sin();
    let pcos = PITCH.cos();
    let psin = PITCH.sin();

    let x1 = ycos * p[0] + ysin * p[2];
    let y1 = p[1];
    let z1 = -ysin * p[0] + ycos * p[2];

    let x2 = x1;
    let y2 = pcos * y1 - psin * z1;
    let z2 = psin * y1 + pcos * z1;

    [x2, y2, z2]
}

#[inline]
fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if len2 < 1e-12 {
        // Defensive default — geometry with zero normals shouldn't reach
        // us, but if it did, point "up" so shading doesn't go NaN.
        return [0.0, 1.0, 0.0];
    }
    let inv = 1.0 / len2.sqrt();
    [v[0] * inv, v[1] * inv, v[2] * inv]
}

#[inline]
fn linear_to_srgb(x: f32) -> f32 {
    if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

#[inline]
fn f_to_u8(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn flatten(fb: Vec<[u8; 4]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(fb.len() * 4);
    for px in fb {
        out.extend_from_slice(&px);
    }
    out
}

// --------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Grid;

    fn sample_grid() -> Grid {
        let mut g = Grid::with_size(4, 4);
        g.paint(0, 0, 0, 2);
        g.paint(1, 0, 0, 2);
        g.paint(2, 0, 3, 1);
        g.paint(2, 1, 3, 1);
        g
    }

    #[test]
    fn empty_grid_produces_background_only() {
        let grid = Grid::with_size(4, 4);
        let palette = Palette::default();
        let opts = RenderOptions {
            width: 32,
            height: 32,
            ..Default::default()
        };
        let rgba = render_to_rgba(&grid, &palette, &opts).unwrap();
        assert_eq!(rgba.len(), 32 * 32 * 4);
        let bg = &opts.background;
        for chunk in rgba.chunks_exact(4) {
            assert_eq!(&chunk[..3], bg, "empty grid should be pure background");
            assert_eq!(chunk[3], 255);
        }
    }

    #[test]
    fn painted_grid_writes_non_background_pixels() {
        let grid = sample_grid();
        let palette = Palette::default();
        let opts = RenderOptions {
            width: 64,
            height: 64,
            ..Default::default()
        };
        let rgba = render_to_rgba(&grid, &palette, &opts).unwrap();
        let bg = opts.background;
        let non_bg = rgba
            .chunks_exact(4)
            .filter(|c| c[0] != bg[0] || c[1] != bg[1] || c[2] != bg[2])
            .count();
        assert!(
            non_bg > 0,
            "painted grid should draw *some* non-background pixels"
        );
        // Sanity: at least ~1% of pixels should be shape; iso of a
        // 3-cell blob covers a lot more than that even at 64px.
        assert!(
            non_bg >= 32,
            "painted grid should cover more than a few stray pixels: {non_bg}"
        );
    }

    #[test]
    fn buffer_length_matches_dimensions() {
        let grid = sample_grid();
        let palette = Palette::default();
        let opts = RenderOptions {
            width: 17,
            height: 23,
            ..Default::default()
        };
        let rgba = render_to_rgba(&grid, &palette, &opts).unwrap();
        assert_eq!(rgba.len(), 17 * 23 * 4);
    }

    #[test]
    fn zero_dimensions_rejected() {
        let grid = sample_grid();
        let palette = Palette::default();
        let opts = RenderOptions {
            width: 0,
            height: 16,
            ..Default::default()
        };
        assert!(matches!(
            render_to_rgba(&grid, &palette, &opts),
            Err(RenderError::InvalidSize { w: 0, h: 16 })
        ));
    }

    #[test]
    fn render_is_deterministic() {
        let grid = sample_grid();
        let palette = Palette::default();
        let opts = RenderOptions {
            width: 48,
            height: 48,
            ..Default::default()
        };
        let a = render_to_rgba(&grid, &palette, &opts).unwrap();
        let b = render_to_rgba(&grid, &palette, &opts).unwrap();
        assert_eq!(a, b, "same inputs must produce identical bytes");
    }

    #[test]
    fn writes_png_to_disk() {
        let grid = sample_grid();
        let palette = Palette::default();
        let opts = RenderOptions {
            width: 32,
            height: 32,
            ..Default::default()
        };
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("out.png");
        write_png(&grid, &palette, &opts, &path).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        // PNG magic: 89 50 4E 47 0D 0A 1A 0A
        assert_eq!(&bytes[..8], &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

        // Decode back and check dimensions.
        let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        let reader = decoder.read_info().unwrap();
        assert_eq!(reader.info().width, 32);
        assert_eq!(reader.info().height, 32);
    }

    #[test]
    fn top_face_is_lit_brighter_than_side() {
        // A tall single column: top face fully lit by our fixed
        // up-and-front light, the -X side face significantly less so.
        // This guards against regressions in the rotation/shading
        // math where "dark side" and "lit side" get swapped.
        let mut g = Grid::with_size(8, 8);
        g.paint(3, 3, 2, 4); // palette 2 = sky/blue in defaults
        let palette = Palette::default();
        let opts = RenderOptions {
            width: 128,
            height: 128,
            ..Default::default()
        };
        let rgba = render_to_rgba(&g, &palette, &opts).unwrap();

        // Sample a pixel near the top of the model and one on the
        // lower-left silhouette. The top sample should have higher
        // average luminance.
        let pixel_at = |x: usize, y: usize| -> [u8; 3] {
            let i = (y * 128 + x) * 4;
            [rgba[i], rgba[i + 1], rgba[i + 2]]
        };
        let top = pixel_at(64, 40);
        let side = pixel_at(40, 90);
        let lum = |c: [u8; 3]| c[0] as u32 + c[1] as u32 + c[2] as u32;
        assert!(
            lum(top) > lum(side),
            "expected top face brighter than side: top={top:?} side={side:?}"
        );
    }
}
