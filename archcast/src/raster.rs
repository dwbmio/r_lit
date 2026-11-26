//! Static PNG rasterization (`resvg` + `tiny-skia` SIMD) and layered minimal-bounds animated GIF export.
//!
//! Never re-encodes full-screen frames:
//! - `GifMode::Overlay` (default): exports ONLY the minimal dynamic bounding box as a transparent
//!   animated overlay layer (ideal for compositing over static HTML / SVG / PNG).
//! - `GifMode::Delta`: exports one static base frame followed by minimal dirty-rectangle transparent
//!   delta sub-frames (`DisposalMethod::Keep`) covering only changed pixels inside the dynamic region.
//!
//! When compiled with `--features gpu` and a WebGPU adapter is available, frame glow compositing runs
//! on a WGSL compute shader (`wgpu`) strictly over the minimal dynamic region; otherwise it runs in
//! parallel across frames on CPU via Rayon.

use crate::error::{ArchcastError, Result};
use crate::layout::LaidOutDiagram;
use crate::theme::ThemeConfig;
use clap::ValueEnum;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GifMode {
    /// Transparent minimal dynamic-bounds overlay layer (never full-screen; default) / 纯透明最小动态包围盒叠加层（默认，绝不全屏）
    #[default]
    Overlay,
    /// Full static base (frame 0) + minimal dirty-rect delta sub-frames / 静态底图 + 最小脏区增量帧
    Delta,
}

#[derive(Debug, Clone, Copy)]
pub struct GifOptions {
    pub mode: GifMode,
    pub scale: f32,
    pub frames: usize,
    pub fps: u16,
}

impl Default for GifOptions {
    fn default() -> Self {
        Self {
            mode: GifMode::Overlay,
            scale: 0.65,
            frames: 20,
            fps: 12,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GifLayerMetadata {
    pub mode: GifMode,
    /// Pixel offset inside the scaled diagram canvas (0,0 for delta; minimal crop origin for overlay).
    pub offset_x: u32,
    pub offset_y: u32,
    pub width: u32,
    pub height: u32,
    pub canvas_width: u32,
    pub canvas_height: u32,
    /// Percentage placement relative to full SVG canvas (ready for CSS `left/top/width/height`).
    pub left_pct: f64,
    pub top_pct: f64,
    pub width_pct: f64,
    pub height_pct: f64,
    pub frames: usize,
}

pub struct EncodedGif {
    pub bytes: Vec<u8>,
    pub meta: GifLayerMetadata,
}

#[cfg(feature = "raster")]
mod imp {
    use super::*;
    use crate::schema::Tone;
    use gif::DisposalMethod;
    use rayon::prelude::*;
    use resvg::{tiny_skia, usvg};
    use std::borrow::Cow;
    use std::sync::{Arc, OnceLock};

    static FONTDB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();

    fn shared_fontdb() -> Arc<usvg::fontdb::Database> {
        FONTDB
            .get_or_init(|| {
                let mut db = usvg::fontdb::Database::new();
                db.load_system_fonts();
                Arc::new(db)
            })
            .clone()
    }

    pub struct RasterRegion {
        pub width: u32,
        pub height: u32,
        /// Straight (non-premultiplied) RGBA bytes.
        pub rgba: Vec<u8>,
    }

    #[derive(Debug, Clone, Copy)]
    #[cfg_attr(feature = "gpu", repr(C))]
    #[cfg_attr(feature = "gpu", derive(bytemuck::Pod, bytemuck::Zeroable))]
    pub struct PulseParticle {
        pub x: f32,
        pub y: f32,
        pub radius: f32,
        pub intensity: f32,
        pub r: f32,
        pub g: f32,
        pub b: f32,
        pub _pad: f32,
    }

    fn rasterize_pixmap(svg_xml: &str, scale: f32, bg_rgba: [u8; 4]) -> Result<tiny_skia::Pixmap> {
        let scale = scale.clamp(0.2, 3.0);
        let mut opt = usvg::Options {
            fontdb: shared_fontdb(),
            ..usvg::Options::default()
        };
        opt.font_family = "PingFang SC".to_string();

        let tree = usvg::Tree::from_str(svg_xml, &opt)
            .map_err(|e| ArchcastError::Raster(format!("usvg parse failed: {e}")))?;

        let size = tree.size();
        let w = ((size.width() * scale).ceil() as u32).max(1);
        let h = ((size.height() * scale).ceil() as u32).max(1);

        let mut pixmap = tiny_skia::Pixmap::new(w, h)
            .ok_or_else(|| ArchcastError::Raster(format!("invalid pixmap size {w}x{h}")))?;
        pixmap.fill(tiny_skia::Color::from_rgba8(
            bg_rgba[0], bg_rgba[1], bg_rgba[2], bg_rgba[3],
        ));

        let transform = tiny_skia::Transform::from_scale(scale, scale);
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        Ok(pixmap)
    }

    fn pixmap_to_straight_rgba(pixmap: &tiny_skia::Pixmap) -> RasterRegion {
        let mut rgba = pixmap.data().to_vec();
        for px in rgba.chunks_exact_mut(4) {
            let a = px[3];
            if a > 0 && a < 255 {
                let af = a as f32 / 255.0;
                px[0] = ((px[0] as f32 / af).min(255.0)) as u8;
                px[1] = ((px[1] as f32 / af).min(255.0)) as u8;
                px[2] = ((px[2] as f32 / af).min(255.0)) as u8;
            }
        }
        RasterRegion {
            width: pixmap.width(),
            height: pixmap.height(),
            rgba,
        }
    }

    fn crop_rgba(src: &RasterRegion, rx: u32, ry: u32, rw: u32, rh: u32) -> RasterRegion {
        let mut out = vec![0u8; (rw as usize) * (rh as usize) * 4];
        let src_stride = (src.width as usize) * 4;
        let dst_stride = (rw as usize) * 4;
        for row in 0..rh as usize {
            let sy = (ry as usize) + row;
            let src_off = sy * src_stride + (rx as usize) * 4;
            let dst_off = row * dst_stride;
            out[dst_off..dst_off + dst_stride]
                .copy_from_slice(&src.rgba[src_off..src_off + dst_stride]);
        }
        RasterRegion {
            width: rw,
            height: rh,
            rgba: out,
        }
    }

    pub fn encode_png(svg_xml: &str, scale: f32, theme: &ThemeConfig) -> Result<Vec<u8>> {
        let pixmap = rasterize_pixmap(svg_xml, scale, theme.svg_bg_rgba())?;
        pixmap
            .encode_png()
            .map_err(|e| ArchcastError::Raster(format!("PNG encode failed: {e}")))
    }

    pub fn encode_animated_gif(
        diagram: &LaidOutDiagram,
        svg_xml: &str,
        opts: GifOptions,
        use_gpu: bool,
        theme: &ThemeConfig,
    ) -> Result<EncodedGif> {
        let frames_count = opts.frames.clamp(4, 60);
        let fps = opts.fps.clamp(2, 30);
        let scale = opts.scale.clamp(0.2, 2.0);
        let delay_cs = ((100.0 / fps as f32).round() as u16).max(2);

        let full_w = ((diagram.width * scale as f64).ceil() as u32).max(1);
        let full_h = ((diagram.height * scale as f64).ceil() as u32).max(1);

        // Compute strictly minimal dynamic bounding box in scaled pixel space.
        let dyn_svg = diagram.dynamic_bounds(20.0).unwrap_or(crate::layout::DynamicBounds {
            x: 0.0,
            y: 0.0,
            w: diagram.width,
            h: diagram.height,
        });
        let dyn_x = ((dyn_svg.x * scale as f64).floor() as u32).min(full_w.saturating_sub(1));
        let dyn_y = ((dyn_svg.y * scale as f64).floor() as u32).min(full_h.saturating_sub(1));
        let dyn_w = ((dyn_svg.w * scale as f64).ceil() as u32)
            .max(1)
            .min(full_w - dyn_x);
        let dyn_h = ((dyn_svg.h * scale as f64).ceil() as u32)
            .max(1)
            .min(full_h - dyn_y);

        // Build tracks & beacons shifted into minimal dynamic box coordinates!
        let tracks = collect_polyline_tracks(diagram, scale as f64, dyn_x as f64, dyn_y as f64, theme);
        let beacons = collect_badge_beacons(diagram, scale as f64, dyn_x as f64, dyn_y as f64, theme);

        let per_frame_particles: Vec<Vec<PulseParticle>> = (0..frames_count)
            .map(|fi| {
                let phase = fi as f64 / frames_count as f64;
                sample_frame_particles(&tracks, &beacons, phase, scale)
            })
            .collect();

        match opts.mode {
            GifMode::Overlay => {
                // Pure transparent minimal-region animation layer — ZERO full-screen background!
                let transparent_base = RasterRegion {
                    width: dyn_w,
                    height: dyn_h,
                    rgba: vec![0u8; (dyn_w as usize) * (dyn_h as usize) * 4],
                };
                let region_frames = composite_region_frames(
                    &transparent_base,
                    &per_frame_particles,
                    true,
                    use_gpu,
                );

                let w_u16 = u16::try_from(dyn_w).map_err(|_| {
                    ArchcastError::Raster("overlay GIF width exceeds u16::MAX".to_string())
                })?;
                let h_u16 = u16::try_from(dyn_h).map_err(|_| {
                    ArchcastError::Raster("overlay GIF height exceeds u16::MAX".to_string())
                })?;

                // For each overlay frame, tightly crop to active non-transparent pixels!
                let gif_frames: Vec<gif::Frame<'static>> = region_frames
                    .into_par_iter()
                    .map(|rgba| {
                        let mut f = build_transparent_subframe(
                            &rgba,
                            dyn_w,
                            dyn_h,
                            0,
                            0,
                            delay_cs,
                            DisposalMethod::Background,
                        );
                        f.dispose = DisposalMethod::Background;
                        f
                    })
                    .collect();

                let bytes = write_gif_stream(w_u16, h_u16, &gif_frames)?;
                let meta = GifLayerMetadata {
                    mode: GifMode::Overlay,
                    offset_x: dyn_x,
                    offset_y: dyn_y,
                    width: dyn_w,
                    height: dyn_h,
                    canvas_width: full_w,
                    canvas_height: full_h,
                    left_pct: (dyn_svg.x / diagram.width) * 100.0,
                    top_pct: (dyn_svg.y / diagram.height) * 100.0,
                    width_pct: (dyn_svg.w / diagram.width) * 100.0,
                    height_pct: (dyn_svg.h / diagram.height) * 100.0,
                    frames: frames_count,
                };
                Ok(EncodedGif { bytes, meta })
            }
            GifMode::Delta => {
                // Full static base once + minimal dirty-rect transparent delta sub-frames inside dynamic bounds!
                let pixmap = rasterize_pixmap(svg_xml, scale, theme.svg_bg_rgba())?;
                let full_base = pixmap_to_straight_rgba(&pixmap);
                let actual_w = full_base.width;
                let actual_h = full_base.height;
                let clamped_x = dyn_x.min(actual_w.saturating_sub(1));
                let clamped_y = dyn_y.min(actual_h.saturating_sub(1));
                let clamped_w = dyn_w.min(actual_w - clamped_x).max(1);
                let clamped_h = dyn_h.min(actual_h - clamped_y).max(1);

                // Only composite pulses inside the minimal dynamic patch!
                let dyn_base = crop_rgba(&full_base, clamped_x, clamped_y, clamped_w, clamped_h);
                let region_frames =
                    composite_region_frames(&dyn_base, &per_frame_particles, false, use_gpu);

                let w_u16 = u16::try_from(actual_w).map_err(|_| {
                    ArchcastError::Raster("delta GIF width exceeds u16::MAX".to_string())
                })?;
                let h_u16 = u16::try_from(actual_h).map_err(|_| {
                    ArchcastError::Raster("delta GIF height exceeds u16::MAX".to_string())
                })?;

                // Frame 0: full static canvas with frame-0 dynamic patch blitted in (clean loop reset).
                let mut frame0_rgba = full_base.rgba.clone();
                blit_patch(
                    &mut frame0_rgba,
                    actual_w,
                    &region_frames[0],
                    clamped_x,
                    clamped_y,
                    clamped_w,
                    clamped_h,
                );
                let mut frame0 = gif::Frame::from_rgba_speed(w_u16, h_u16, &mut frame0_rgba, 15);
                frame0.delay = delay_cs;
                frame0.dispose = DisposalMethod::Keep;

                // Frames 1..N-1: compute exact changed pixels vs previous frame inside dynamic patch,
                // crop to minimal dirty rect, and encode with transparent unchanged pixels!
                let delta_frames: Vec<gif::Frame<'static>> = (1..frames_count)
                    .into_par_iter()
                    .map(|fi| {
                        build_delta_subframe(
                            &region_frames[fi - 1],
                            &region_frames[fi],
                            clamped_w,
                            clamped_h,
                            clamped_x,
                            clamped_y,
                            delay_cs,
                        )
                    })
                    .collect();

                let mut all_frames = Vec::with_capacity(frames_count);
                all_frames.push(frame0);
                all_frames.extend(delta_frames);

                let bytes = write_gif_stream(w_u16, h_u16, &all_frames)?;
                let meta = GifLayerMetadata {
                    mode: GifMode::Delta,
                    offset_x: clamped_x,
                    offset_y: clamped_y,
                    width: clamped_w,
                    height: clamped_h,
                    canvas_width: actual_w,
                    canvas_height: actual_h,
                    left_pct: (dyn_svg.x / diagram.width) * 100.0,
                    top_pct: (dyn_svg.y / diagram.height) * 100.0,
                    width_pct: (dyn_svg.w / diagram.width) * 100.0,
                    height_pct: (dyn_svg.h / diagram.height) * 100.0,
                    frames: frames_count,
                };
                Ok(EncodedGif { bytes, meta })
            }
        }
    }

    fn write_gif_stream(w: u16, h: u16, frames: &[gif::Frame<'static>]) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(65_536);
        {
            let mut encoder = gif::Encoder::new(&mut buf, w, h, &[])
                .map_err(|e| ArchcastError::Raster(format!("GIF encoder init failed: {e}")))?;
            encoder
                .set_repeat(gif::Repeat::Infinite)
                .map_err(|e| ArchcastError::Raster(format!("GIF repeat failed: {e}")))?;
            for frame in frames {
                encoder
                    .write_frame(frame)
                    .map_err(|e| ArchcastError::Raster(format!("GIF frame write failed: {e}")))?;
            }
        }
        Ok(buf)
    }

    fn blit_patch(
        dst: &mut [u8],
        dst_w: u32,
        patch: &[u8],
        px: u32,
        py: u32,
        pw: u32,
        ph: u32,
    ) {
        let dst_stride = (dst_w as usize) * 4;
        let patch_stride = (pw as usize) * 4;
        for row in 0..ph as usize {
            let d_off = ((py as usize) + row) * dst_stride + (px as usize) * 4;
            let p_off = row * patch_stride;
            dst[d_off..d_off + patch_stride].copy_from_slice(&patch[p_off..p_off + patch_stride]);
        }
    }

    /// Build a minimal dirty-rectangle transparent delta frame comparing `prev` vs `curr`.
    fn build_delta_subframe(
        prev: &[u8],
        curr: &[u8],
        region_w: u32,
        region_h: u32,
        origin_x: u32,
        origin_y: u32,
        delay_cs: u16,
    ) -> gif::Frame<'static> {
        let w = region_w as usize;
        let h = region_h as usize;

        let mut min_x = w;
        let mut min_y = h;
        let mut max_x = 0usize;
        let mut max_y = 0usize;
        let mut any = false;

        for y in 0..h {
            let row_off = y * w * 4;
            for x in 0..w {
                let i = row_off + x * 4;
                // Ignore sub-2-level quantization noise for maximum dirty-rect compression.
                let dr = prev[i].abs_diff(curr[i]);
                let dg = prev[i + 1].abs_diff(curr[i + 1]);
                let db = prev[i + 2].abs_diff(curr[i + 2]);
                if dr > 2 || dg > 2 || db > 2 {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                    any = true;
                }
            }
        }

        if !any {
            return tiny_noop_frame(origin_x, origin_y, delay_cs, DisposalMethod::Keep);
        }

        let sw = max_x - min_x + 1;
        let sh = max_y - min_y + 1;
        let mut sub_rgba = vec![0u8; sw * sh * 4];

        for y in 0..sh {
            let sy = min_y + y;
            let src_row = sy * w * 4;
            let dst_row = y * sw * 4;
            for x in 0..sw {
                let sx = min_x + x;
                let si = src_row + sx * 4;
                let di = dst_row + x * 4;
                let dr = prev[si].abs_diff(curr[si]);
                let dg = prev[si + 1].abs_diff(curr[si + 1]);
                let db = prev[si + 2].abs_diff(curr[si + 2]);
                if dr > 2 || dg > 2 || db > 2 {
                    sub_rgba[di] = curr[si];
                    sub_rgba[di + 1] = curr[si + 1];
                    sub_rgba[di + 2] = curr[si + 2];
                    sub_rgba[di + 3] = 255;
                }
            }
        }

        quantize_transparent_frame(
            &sub_rgba,
            (origin_x as usize + min_x) as u16,
            (origin_y as usize + min_y) as u16,
            sw as u16,
            sh as u16,
            delay_cs,
            DisposalMethod::Keep,
        )
    }

    /// Crop an overlay RGBA buffer tightly to its non-transparent pixels (`alpha >= 48`) and quantize.
    fn build_transparent_subframe(
        rgba: &[u8],
        region_w: u32,
        region_h: u32,
        origin_x: u32,
        origin_y: u32,
        delay_cs: u16,
        dispose: DisposalMethod,
    ) -> gif::Frame<'static> {
        let w = region_w as usize;
        let h = region_h as usize;

        let mut min_x = w;
        let mut min_y = h;
        let mut max_x = 0usize;
        let mut max_y = 0usize;
        let mut any = false;

        for y in 0..h {
            let row_off = y * w * 4;
            for x in 0..w {
                if rgba[row_off + x * 4 + 3] >= 48 {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                    any = true;
                }
            }
        }

        if !any {
            return tiny_noop_frame(origin_x, origin_y, delay_cs, dispose);
        }

        let sw = max_x - min_x + 1;
        let sh = max_y - min_y + 1;
        let mut sub_rgba = vec![0u8; sw * sh * 4];
        for y in 0..sh {
            let src_row = (min_y + y) * w * 4;
            let dst_row = y * sw * 4;
            for x in 0..sw {
                let si = src_row + (min_x + x) * 4;
                let di = dst_row + x * 4;
                let a = rgba[si + 3];
                if a >= 48 {
                    sub_rgba[di] = rgba[si];
                    sub_rgba[di + 1] = rgba[si + 1];
                    sub_rgba[di + 2] = rgba[si + 2];
                    sub_rgba[di + 3] = 255;
                }
            }
        }

        quantize_transparent_frame(
            &sub_rgba,
            (origin_x as usize + min_x) as u16,
            (origin_y as usize + min_y) as u16,
            sw as u16,
            sh as u16,
            delay_cs,
            dispose,
        )
    }

    fn tiny_noop_frame(
        left: u32,
        top: u32,
        delay_cs: u16,
        dispose: DisposalMethod,
    ) -> gif::Frame<'static> {
        gif::Frame {
            delay: delay_cs,
            dispose,
            transparent: Some(0),
            left: left as u16,
            top: top as u16,
            width: 1,
            height: 1,
            palette: Some(vec![0, 0, 0, 255, 255, 255]),
            buffer: Cow::Owned(vec![0]),
            ..gif::Frame::default()
        }
    }

    /// Collision-free transparent GIF quantization:
    /// index `0` is ALWAYS reserved for transparency; opaque pixels map to `1..=255`.
    fn quantize_transparent_frame(
        sub_rgba: &[u8],
        left: u16,
        top: u16,
        width: u16,
        height: u16,
        delay_cs: u16,
        dispose: DisposalMethod,
    ) -> gif::Frame<'static> {
        let mut opaque_rgba = Vec::new();
        for px in sub_rgba.chunks_exact(4) {
            if px[3] > 0 {
                opaque_rgba.extend_from_slice(px);
            }
        }
        if opaque_rgba.is_empty() {
            return tiny_noop_frame(left as u32, top as u32, delay_cs, dispose);
        }

        let nq = color_quant::NeuQuant::new(15, 255, &opaque_rgba);
        let cmap = nq.color_map_rgb();
        let mut palette = Vec::with_capacity(3 + cmap.len());
        palette.extend_from_slice(&[0, 0, 0]); // Index 0 = dedicated transparent slot.
        palette.extend_from_slice(&cmap);

        let mut indices = Vec::with_capacity((width as usize) * (height as usize));
        for px in sub_rgba.chunks_exact(4) {
            if px[3] == 0 {
                indices.push(0);
            } else {
                let idx = (nq.index_of(px) as u8).min(254);
                indices.push(idx + 1);
            }
        }

        gif::Frame {
            delay: delay_cs,
            dispose,
            transparent: Some(0),
            left,
            top,
            width,
            height,
            palette: Some(palette),
            buffer: Cow::Owned(indices),
            ..gif::Frame::default()
        }
    }

    fn composite_region_frames(
        base: &RasterRegion,
        per_frame_particles: &[Vec<PulseParticle>],
        transparent_overlay: bool,
        use_gpu: bool,
    ) -> Vec<Vec<u8>> {
        #[cfg(feature = "gpu")]
        {
            if use_gpu
                && let Some(gpu_frames) = gpu_compute::render_frames_wgpu(
                    base,
                    per_frame_particles,
                    transparent_overlay,
                )
            {
                return gpu_frames;
            }
        }
        #[cfg(not(feature = "gpu"))]
        let _ = use_gpu;

        render_frames_cpu_rayon(base, per_frame_particles, transparent_overlay)
    }

    struct Track {
        pts: Vec<(f64, f64)>,
        cum_len: Vec<f64>,
        total_len: f64,
        phase_offset: f64,
        color: (f32, f32, f32),
    }

    struct Beacon {
        x: f32,
        y: f32,
        color: (f32, f32, f32),
        phase_offset: f64,
    }

    fn collect_polyline_tracks(
        diagram: &LaidOutDiagram,
        scale: f64,
        offset_x: f64,
        offset_y: f64,
        theme: &ThemeConfig,
    ) -> Vec<Track> {
        let mut tracks = Vec::new();
        let palette = theme.pulse_rgb();

        for (ei, edge) in diagram.edges.iter().enumerate() {
            let p = palette[ei % palette.len()];
            let color = (p[0], p[1], p[2]);
            for (pi, raw_pts) in edge.polylines.iter().enumerate() {
                if raw_pts.len() < 2 {
                    continue;
                }
                let pts: Vec<(f64, f64)> = raw_pts
                    .iter()
                    .map(|&(x, y)| (x * scale - offset_x, y * scale - offset_y))
                    .collect();
                let mut cum_len = Vec::with_capacity(pts.len());
                cum_len.push(0.0);
                let mut acc = 0.0;
                for w in pts.windows(2) {
                    let dx = w[1].0 - w[0].0;
                    let dy = w[1].1 - w[0].1;
                    acc += (dx * dx + dy * dy).sqrt();
                    cum_len.push(acc);
                }
                if acc > 4.0 {
                    tracks.push(Track {
                        pts,
                        cum_len,
                        total_len: acc,
                        phase_offset: ((ei * 7 + pi * 3) % 10) as f64 / 10.0,
                        color,
                    });
                }
            }
        }
        tracks
    }

    fn collect_badge_beacons(
        diagram: &LaidOutDiagram,
        scale: f64,
        offset_x: f64,
        offset_y: f64,
        theme: &ThemeConfig,
    ) -> Vec<Beacon> {
        let mut beacons = Vec::new();
        let mut idx = 0usize;
        for col in &diagram.columns {
            for card in &col.cards {
                if let Some(b) = &card.badge
                    && matches!(b.spec.tone, Tone::Red | Tone::Emerald | Tone::Green)
                {
                    let rgb = match b.spec.tone {
                        Tone::Red => theme.beacon_red_rgb(),
                        _ => theme.beacon_green_rgb(),
                    };
                    let color = (rgb[0], rgb[1], rgb[2]);
                    beacons.push(Beacon {
                        x: (b.rect.x * scale - offset_x) as f32,
                        y: (b.rect.cy() * scale - offset_y) as f32,
                        color,
                        phase_offset: (idx % 6) as f64 / 6.0,
                    });
                    idx += 1;
                }
            }
        }
        beacons
    }

    fn sample_polyline(track: &Track, u: f64) -> (f32, f32) {
        let target = u.clamp(0.0, 1.0) * track.total_len;
        for i in 0..track.pts.len() - 1 {
            let l0 = track.cum_len[i];
            let l1 = track.cum_len[i + 1];
            if target <= l1 || i + 2 == track.pts.len() {
                let seg = (l1 - l0).max(1e-6);
                let t = ((target - l0) / seg).clamp(0.0, 1.0);
                let x = track.pts[i].0 + (track.pts[i + 1].0 - track.pts[i].0) * t;
                let y = track.pts[i].1 + (track.pts[i + 1].1 - track.pts[i].1) * t;
                return (x as f32, y as f32);
            }
        }
        let last = track.pts[track.pts.len() - 1];
        (last.0 as f32, last.1 as f32)
    }

    fn sample_frame_particles(
        tracks: &[Track],
        beacons: &[Beacon],
        phase: f64,
        scale: f32,
    ) -> Vec<PulseParticle> {
        let mut out = Vec::with_capacity(tracks.len() * 3 + beacons.len());
        let head_r = (5.8 * scale).max(2.6);

        for tr in tracks {
            let u = (phase + tr.phase_offset).fract();
            for (lag, r_mul, alpha) in [
                (0.0_f64, 1.0_f32, 0.94_f32),
                (0.035, 0.72, 0.56),
                (0.070, 0.50, 0.30),
            ] {
                let tu = (u - lag).rem_euclid(1.0);
                let (x, y) = sample_polyline(tr, tu);
                out.push(PulseParticle {
                    x,
                    y,
                    radius: head_r * r_mul,
                    intensity: alpha,
                    r: tr.color.0,
                    g: tr.color.1,
                    b: tr.color.2,
                    _pad: 0.0,
                });
            }
        }

        for bc in beacons {
            let s = ((phase + bc.phase_offset) * std::f64::consts::TAU).sin() * 0.5 + 0.5;
            out.push(PulseParticle {
                x: bc.x,
                y: bc.y,
                radius: (6.5 * scale * (0.75 + 0.45 * s as f32)).max(2.5),
                intensity: 0.25 + 0.58 * s as f32,
                r: bc.color.0,
                g: bc.color.1,
                b: bc.color.2,
                _pad: 0.0,
            });
        }

        out
    }

    fn render_frames_cpu_rayon(
        base: &RasterRegion,
        per_frame_particles: &[Vec<PulseParticle>],
        transparent_overlay: bool,
    ) -> Vec<Vec<u8>> {
        let w = base.width as i32;
        let h = base.height as i32;

        per_frame_particles
            .par_iter()
            .map(|particles| {
                let mut frame = base.rgba.clone();
                for p in particles {
                    let reach = (p.radius * 2.0).ceil() as i32;
                    let x0 = (p.x.floor() as i32 - reach).max(0);
                    let y0 = (p.y.floor() as i32 - reach).max(0);
                    let x1 = (p.x.ceil() as i32 + reach).min(w - 1);
                    let y1 = (p.y.ceil() as i32 + reach).min(h - 1);
                    let inv_r2 = 1.0 / (p.radius * p.radius).max(0.5);

                    for py in y0..=y1 {
                        let dy = py as f32 + 0.5 - p.y;
                        let dy2 = dy * dy;
                        let row_off = (py as usize) * (base.width as usize) * 4;
                        for px in x0..=x1 {
                            let dx = px as f32 + 0.5 - p.x;
                            let norm = (dx * dx + dy2) * inv_r2;
                            if norm >= 4.0 {
                                continue;
                            }
                            let a = ((-norm * 1.35).exp() * p.intensity).clamp(0.0, 1.0);
                            let idx = row_off + (px as usize) * 4;
                            if transparent_overlay {
                                // Composite glow onto transparent layer (premultiplied-like max alpha).
                                let prev_a = frame[idx + 3] as f32 / 255.0;
                                let out_a = (prev_a + a * (1.0 - prev_a)).clamp(0.0, 1.0);
                                if out_a > 1e-4 {
                                    let w_prev = prev_a * (1.0 - a);
                                    frame[idx] = ((frame[idx] as f32 * w_prev + p.r * a) / out_a)
                                        .min(255.0) as u8;
                                    frame[idx + 1] = ((frame[idx + 1] as f32 * w_prev + p.g * a)
                                        / out_a)
                                        .min(255.0) as u8;
                                    frame[idx + 2] = ((frame[idx + 2] as f32 * w_prev + p.b * a)
                                        / out_a)
                                        .min(255.0) as u8;
                                    frame[idx + 3] = (out_a * 255.0).round() as u8;
                                }
                            } else {
                                let inv = 1.0 - a;
                                frame[idx] = (frame[idx] as f32 * inv + p.r * a).min(255.0) as u8;
                                frame[idx + 1] =
                                    (frame[idx + 1] as f32 * inv + p.g * a).min(255.0) as u8;
                                frame[idx + 2] =
                                    (frame[idx + 2] as f32 * inv + p.b * a).min(255.0) as u8;
                            }
                        }
                    }
                }
                frame
            })
            .collect()
    }

    #[cfg(feature = "gpu")]
    mod gpu_compute {
        use super::{PulseParticle, RasterRegion};
        use wgpu::util::DeviceExt;

        const PULSE_WGSL: &str = r#"
struct Uniforms {
    width: u32,
    height: u32,
    particle_count: u32,
    transparent_mode: u32,
};

struct Particle {
    x: f32,
    y: f32,
    radius: f32,
    intensity: f32,
    r: f32,
    g: f32,
    b: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read> base_pixels: array<u32>;
@group(0) @binding(2) var<storage, read> particles: array<Particle>;
@group(0) @binding(3) var<storage, read_write> out_pixels: array<u32>;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= u.width || gid.y >= u.height) {
        return;
    }
    let idx = gid.y * u.width + gid.x;
    let packed = base_pixels[idx];
    var r = f32(packed & 255u);
    var g = f32((packed >> 8u) & 255u);
    var b = f32((packed >> 16u) & 255u);
    var a_out = f32((packed >> 24u) & 255u) / 255.0;

    let px = f32(gid.x) + 0.5;
    let py = f32(gid.y) + 0.5;

    for (var i: u32 = 0u; i < u.particle_count; i = i + 1u) {
        let p = particles[i];
        let dx = px - p.x;
        let dy = py - p.y;
        let r2 = max(p.radius * p.radius, 0.5);
        let norm = (dx * dx + dy * dy) / r2;
        if (norm < 4.0) {
            let a = clamp(exp(-norm * 1.35) * p.intensity, 0.0, 1.0);
            if (u.transparent_mode != 0u) {
                let next_a = clamp(a_out + a * (1.0 - a_out), 0.0, 1.0);
                if (next_a > 0.0001) {
                    let wp = a_out * (1.0 - a);
                    r = min((r * wp + p.r * a) / next_a, 255.0);
                    g = min((g * wp + p.g * a) / next_a, 255.0);
                    b = min((b * wp + p.b * a) / next_a, 255.0);
                    a_out = next_a;
                }
            } else {
                let inv = 1.0 - a;
                r = min(r * inv + p.r * a, 255.0);
                g = min(g * inv + p.g * a, 255.0);
                b = min(b * inv + p.b * a, 255.0);
            }
        }
    }

    let a_u8 = u32(clamp(round(a_out * 255.0), 0.0, 255.0)) & 255u;
    let out_packed = (u32(r) & 255u)
        | ((u32(g) & 255u) << 8u)
        | ((u32(b) & 255u) << 16u)
        | (a_u8 << 24u);
    out_pixels[idx] = out_packed;
}
"#;

        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Uniforms {
            width: u32,
            height: u32,
            particle_count: u32,
            transparent_mode: u32,
        }

        pub fn render_frames_wgpu(
            base: &RasterRegion,
            per_frame_particles: &[Vec<PulseParticle>],
            transparent_overlay: bool,
        ) -> Option<Vec<Vec<u8>>> {
            let instance = wgpu::Instance::default();
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                }))?;
            let (device, queue) = pollster::block_on(adapter.request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("archcast-pulse-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            ))
            .ok()?;

            let pixel_bytes = base.rgba.len() as u64;
            let base_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("base_pixels"),
                contents: &base.rgba,
                usage: wgpu::BufferUsages::STORAGE,
            });
            let out_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("out_pixels"),
                size: pixel_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("readback_pixels"),
                size: pixel_bytes,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            let max_particles = per_frame_particles
                .iter()
                .map(|p| p.len())
                .max()
                .unwrap_or(1)
                .max(1);
            let particle_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("particles"),
                size: (max_particles * std::mem::size_of::<PulseParticle>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("uniforms"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("pulse_wgsl"),
                source: wgpu::ShaderSource::Wgsl(PULSE_WGSL.into()),
            });

            let bind_group_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("pulse_bgl"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 3,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("pulse_pl"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("pulse_pipeline"),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            });

            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("pulse_bg"),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: base_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: particle_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: out_buf.as_entire_binding(),
                    },
                ],
            });

            let wg_x = base.width.div_ceil(16);
            let wg_y = base.height.div_ceil(16);
            let mut frames = Vec::with_capacity(per_frame_particles.len());

            for particles in per_frame_particles {
                let u = Uniforms {
                    width: base.width,
                    height: base.height,
                    particle_count: particles.len() as u32,
                    transparent_mode: u32::from(transparent_overlay),
                };
                queue.write_buffer(&uniform_buf, 0, bytemuck::bytes_of(&u));
                if !particles.is_empty() {
                    queue.write_buffer(&particle_buf, 0, bytemuck::cast_slice(particles));
                }

                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("pulse_encoder"),
                });
                {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("pulse_pass"),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &bind_group, &[]);
                    pass.dispatch_workgroups(wg_x, wg_y, 1);
                }
                encoder.copy_buffer_to_buffer(&out_buf, 0, &readback_buf, 0, pixel_bytes);
                queue.submit(Some(encoder.finish()));

                let slice = readback_buf.slice(..);
                let (tx, rx) = std::sync::mpsc::channel();
                slice.map_async(wgpu::MapMode::Read, move |res| {
                    let _ = tx.send(res);
                });
                device.poll(wgpu::Maintain::Wait);
                rx.recv().ok()?.ok()?;

                let data = slice.get_mapped_range();
                frames.push(data.to_vec());
                drop(data);
                readback_buf.unmap();
            }

            Some(frames)
        }
    }
}

pub fn encode_png(svg_xml: &str, scale: f32, theme: &ThemeConfig) -> Result<Vec<u8>> {
    #[cfg(feature = "raster")]
    {
        imp::encode_png(svg_xml, scale, theme)
    }
    #[cfg(not(feature = "raster"))]
    {
        let _ = (svg_xml, scale, theme);
        Err(ArchcastError::BackendUnavailable(
            "archcast was built without `--features raster`; rebuild with default features to export PNG".to_string(),
        ))
    }
}

pub fn encode_animated_gif(
    diagram: &LaidOutDiagram,
    svg_xml: &str,
    opts: GifOptions,
    use_gpu: bool,
    theme: &ThemeConfig,
) -> Result<EncodedGif> {
    #[cfg(feature = "raster")]
    {
        imp::encode_animated_gif(diagram, svg_xml, opts, use_gpu, theme)
    }
    #[cfg(not(feature = "raster"))]
    {
        let _ = (diagram, svg_xml, opts, use_gpu, theme);
        Err(ArchcastError::BackendUnavailable(
            "archcast was built without `--features raster`; rebuild with default features to export animated GIF".to_string(),
        ))
    }
}
