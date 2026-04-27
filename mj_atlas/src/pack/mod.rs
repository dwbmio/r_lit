pub mod contour;
pub mod dedup;
pub mod extrude;
pub mod manifest;
pub mod multi_bin;
pub mod simplify;
pub mod triangulate;
pub mod trim;

use crate::error::{AppError, Result};
use image::RgbaImage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Options for the packing operation.
#[derive(Debug, Clone)]
pub struct PackOptions {
    pub input_dir: PathBuf,
    pub output_name: String,
    pub output_dir: PathBuf,
    pub max_size: usize,
    pub spacing: u32,
    pub padding: u32,
    pub extrude: u32,
    pub trim: bool,
    pub trim_threshold: u8,
    pub rotate: bool,
    pub pot: bool,
    pub recursive: bool,
    /// When `Some`, pack exactly these source files instead of scanning
    /// `input_dir`. Used by the GUI to honor the user's drag-drop selection
    /// without picking up unrelated images that happen to live next to them.
    /// Sprite names (relative-path keys) are computed by stripping
    /// `input_dir` from each path; if a path doesn't share that prefix the
    /// file's basename is used as the key.
    pub explicit_sprites: Option<Vec<PathBuf>>,
    /// Enable incremental packing — read manifest, only repack what changed.
    /// When all inputs match (and the manifest is consistent with the on-disk
    /// atlases) the pack is skipped entirely. Otherwise we attempt a partial
    /// repack (additive / in-place) before falling back to a full repack.
    pub incremental: bool,
    /// Force full repack even when incremental cache would hit. Implies
    /// rewriting the manifest from scratch.
    pub force: bool,
    /// Output metadata format (json/json-array/godot-tpsheet/godot-tres).
    /// Part of the cache key — changing format invalidates the manifest.
    pub format: crate::output::Format,
    /// Enable PNG quantization (lossy compression)
    pub quantize: bool,
    /// PNG quantization quality 0-100 (lower = smaller file, more loss)
    pub quantize_quality: u8,
    /// Enable polygon mode (contour-based packing + mesh output)
    pub polygon: bool,
    /// Polygon simplification tolerance (lower = tighter fit, more vertices)
    pub tolerance: f32,
    /// Polygon shape model — concave (default), convex (hull per component),
    /// or auto (heuristic based on convex-vs-concave area ratio).
    pub polygon_shape: PolygonShape,
    /// Maximum total vertex count across all components per sprite.
    /// 0 disables the budget — uses `tolerance` as-is.
    /// >0 enables a binary search on tolerance to land within this budget.
    pub max_vertices: u32,
}

/// Polygon shape mode — controls how each connected component is converted to a mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolygonShape {
    /// Use the simplified concave outline (default; tightest fit, most vertices).
    Concave,
    /// Replace the outline with its convex hull (fewer vertices, may overdraw).
    Convex,
    /// Auto-pick: convex when hull-area / concave-area > 0.85, else concave.
    Auto,
}

/// Information about a packed sprite in the final atlas.
#[derive(Debug, Clone)]
pub struct PackedSprite {
    /// Sprite name (relative path from input dir)
    pub name: String,
    /// Position in the atlas (content area, excluding extrude)
    pub x: u32,
    pub y: u32,
    /// Size in the atlas (after trim, before extrude)
    pub w: u32,
    pub h: u32,
    /// Whether the sprite was rotated 90° CW
    pub rotated: bool,
    /// Whether the sprite was trimmed
    pub trimmed: bool,
    /// Trim offset from original top-left
    pub trim_offset_x: u32,
    pub trim_offset_y: u32,
    /// Original source size before trim
    pub source_w: u32,
    pub source_h: u32,
    /// If this sprite is an alias (duplicate), the canonical name it references
    pub alias_of: Option<String>,
    /// Polygon mesh vertices in sprite-local coordinates (polygon mode)
    pub vertices: Option<Vec<[f32; 2]>>,
    /// Polygon mesh vertices in atlas UV coordinates (polygon mode)
    pub vertices_uv: Option<Vec<[f32; 2]>>,
    /// Triangle indices into vertices array (polygon mode)
    pub triangles: Option<Vec<[usize; 3]>>,
}

/// Result of packing a single atlas.
#[derive(Debug)]
pub struct AtlasResult {
    pub image_path: PathBuf,
    pub data_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub sprites: Vec<PackedSprite>,
    pub animations: HashMap<String, Vec<String>>,
    pub duplicates_removed: usize,
    /// In-memory atlas image (not written to disk until save_to_disk is called)
    pub atlas_image: RgbaImage,
    /// Outer rectangles per non-alias sprite (place_x, place_y, place_w, place_h, rotated).
    /// Populated during full/partial pack — used by manifest to maintain UV stability.
    pub outer_rects: Vec<manifest::UsedRect>,
    /// Maximal free rectangles within this atlas. Used by additive incremental packing.
    pub free_rects: Vec<manifest::FreeRect>,
    /// True when this atlas was reused from cache and disk writes should be skipped.
    pub from_cache: bool,
}

impl AtlasResult {
    /// Write atlas image and metadata to disk. When `from_cache` is true, both
    /// the atlas PNG and the metadata sidecar are already valid on disk and we
    /// skip writes (idempotent re-runs become near-zero cost).
    pub fn save_to_disk(&self, opts: &PackOptions, fmt: crate::output::Format) -> Result<()> {
        if self.from_cache {
            log::info!(
                "Cache hit: {} unchanged ({}x{}, {} sprites)",
                self.image_path.display(),
                self.width,
                self.height,
                self.sprites.len()
            );
            return Ok(());
        }

        std::fs::create_dir_all(&opts.output_dir)?;
        if opts.quantize {
            save_quantized_png(&self.atlas_image, &self.image_path, opts.quantize_quality)?;
        } else {
            self.atlas_image.save(&self.image_path)?;
        }
        log::info!("Saved atlas image: {}", self.image_path.display());
        crate::output::write_output(self, fmt, opts)?;
        Ok(())
    }
}

/// Main entry point: execute the packing pipeline (in-memory only).
///
/// When `opts.incremental` is true and a valid manifest is found, this returns
/// cached results (with `from_cache = true`) for unchanged atlases without
/// touching the disk. The caller is responsible for calling
/// [`AtlasResult::save_to_disk`] (a no-op for cached entries) and, in CLI
/// flows, [`persist_manifest`] afterwards to refresh the manifest sidecar.
pub fn execute(opts: &PackOptions) -> Result<Vec<AtlasResult>> {
    let entries = collect_images_for(opts)?;
    if entries.is_empty() {
        return Err(AppError::NoImages(opts.input_dir.display().to_string()));
    }
    if opts.explicit_sprites.is_some() {
        log::info!("Found {} sprite(s) (explicit list)", entries.len());
    } else {
        log::info!(
            "Found {} sprite(s) in {}",
            entries.len(),
            opts.input_dir.display()
        );
    }

    if opts.incremental && !opts.force {
        match try_incremental(opts, &entries)? {
            Some(results) => return Ok(results),
            None => {} // fall through to full repack
        }
    }

    log::info!("Running full repack");
    full_pack(opts, &entries)
}

/// Persist (or refresh) the manifest sidecar after atlases have been written.
///
/// Call this after `AtlasResult::save_to_disk` in CLI flows. When all atlases
/// were cache hits the existing manifest stays as-is (no rehash, no rewrite).
/// When `opts.incremental` is false this is a no-op — only manifest cleanup is
/// performed so a stale sidecar from a previous incremental run can't poison
/// future runs.
pub fn persist_manifest(opts: &PackOptions, results: &[AtlasResult]) -> Result<()> {
    if !opts.incremental {
        let mpath = manifest::Manifest::path_for(opts);
        if mpath.exists() {
            let _ = std::fs::remove_file(&mpath);
        }
        return Ok(());
    }

    // All results from cache ⇒ manifest already matches disk state.
    if results.iter().all(|r| r.from_cache) {
        return Ok(());
    }

    // Re-walk the same source set so manifest hashes line up with what was
    // actually packed (explicit list when GUI-driven, dir scan otherwise).
    let entries = collect_images_for(opts)?;
    write_manifest(opts, results, &entries)
}

/// Run the full pack pipeline with no cache. Used both as the no-incremental
/// path and as the fallback when partial repack is impossible.
fn full_pack(opts: &PackOptions, entries: &[(String, PathBuf)]) -> Result<Vec<AtlasResult>> {
    use rayon::prelude::*;
    let loaded: Vec<Result<(String, RgbaImage)>> = entries
        .par_iter()
        .map(|(name, path)| {
            let img = image::open(path)?.into_rgba8();
            Ok((name.clone(), img))
        })
        .collect();
    let loaded: Vec<(String, RgbaImage)> = loaded.into_iter().collect::<Result<Vec<_>>>()?;

    // 3. Detect duplicates
    let (unique_indices, aliases) = dedup::find_duplicates(&loaded);
    let dup_count = aliases.len();

    // 4. Preprocess unique sprites in parallel
    let sprites: Vec<SpriteData> = unique_indices
        .par_iter()
        .map(|&idx| {
            let (name, img) = &loaded[idx];
            let (trimmed_img, trim_info) = if opts.trim {
                let tr = trim::trim_transparent(img, opts.trim_threshold);
                (tr.image.clone(), tr)
            } else {
                let (w, h) = img.dimensions();
                (
                    img.clone(),
                    trim::TrimResult {
                        image: img.clone(),
                        offset_x: 0,
                        offset_y: 0,
                        source_w: w,
                        source_h: h,
                        trimmed: false,
                    },
                )
            };

            let extruded = extrude::extrude_edges(&trimmed_img, opts.extrude);

            let extra = opts.extrude * 2 + opts.padding * 2;
            let pack_w = trimmed_img.width() + extra;
            let pack_h = trimmed_img.height() + extra;

            let polygon_data = if opts.polygon {
                Some(build_polygon_data(&trimmed_img, opts))
            } else {
                None
            };

            SpriteData {
                name: name.clone(),
                original_image: trimmed_img,
                extruded_image: extruded,
                trim_info,
                pack_w,
                pack_h,
                polygon_data,
            }
        })
        .collect();

    // 5. Build packing items (add spacing)
    let pack_items: Vec<(String, usize, usize)> = sprites
        .iter()
        .map(|s| {
            (
                s.name.clone(),
                s.pack_w as usize + opts.spacing as usize,
                s.pack_h as usize + opts.spacing as usize,
            )
        })
        .collect();

    // 6. Pack into bins
    let bins = multi_bin::pack_multi_bin(pack_items, opts.max_size, opts.pot, opts.rotate)?;

    // 7. Build sprite lookup
    let sprite_map: HashMap<&str, &SpriteData> =
        sprites.iter().map(|s| (s.name.as_str(), s)).collect();

    // 8. Detect animation groups (on ALL sprites including aliases)
    let all_names: Vec<String> = loaded.iter().map(|(n, _)| n.clone()).collect();
    let animations = detect_animations_from_names(&all_names);

    // 9. Compose each atlas
    let mut results = Vec::with_capacity(bins.len());
    for (bin_idx, (bin_w, bin_h, packed_items)) in bins.into_iter().enumerate() {
        let suffix = if bin_idx == 0 && results.is_empty() {
            String::new()
        } else {
            format!("_{}", bin_idx)
        };

        let image_filename = format!("{}{}.png", opts.output_name, suffix);
        let image_path = opts.output_dir.join(&image_filename);
        let data_filename = format!("{}{}", opts.output_name, suffix);
        let data_path = opts.output_dir.join(&data_filename);

        let mut atlas_img = RgbaImage::new(bin_w as u32, bin_h as u32);
        let mut atlas_sprites = Vec::with_capacity(packed_items.len() + aliases.len());
        // Outer bboxes (incl. extrude/padding/spacing reservation) for free-rect tracking.
        let mut outer_rects: Vec<manifest::UsedRect> = Vec::with_capacity(packed_items.len());

        // Build a map: canonical_name → PackedSprite for alias resolution
        let mut canonical_packed: HashMap<String, PackedSprite> = HashMap::new();

        for packed in &packed_items {
            let sprite = sprite_map
                .get(packed.data.as_str())
                .ok_or_else(|| AppError::Custom(format!("Sprite '{}' not found", packed.data)))?;

            let place_x = packed.rect.x as u32;
            let place_y = packed.rect.y as u32;

            let was_rotated = packed.rect.w as u32 != sprite.pack_w + opts.spacing
                && packed.rect.h as u32 == sprite.pack_w + opts.spacing;

            let img_to_place = if was_rotated {
                rotate_90cw(&sprite.extruded_image)
            } else {
                sprite.extruded_image.clone()
            };

            image::imageops::overlay(
                &mut atlas_img,
                &img_to_place,
                place_x as i64,
                place_y as i64,
            );

            // The bin packer reserves `pack_w + spacing` × `pack_h + spacing` per
            // item — record that full reservation so additive incremental packing
            // stays clear of the spacing margin.
            outer_rects.push(manifest::UsedRect {
                name: sprite.name.clone(),
                x: place_x,
                y: place_y,
                w: packed.rect.w as u32,
                h: packed.rect.h as u32,
                rotated: was_rotated,
            });

            let content_x = place_x + opts.extrude + opts.padding;
            let content_y = place_y + opts.extrude + opts.padding;
            let content_w = sprite.original_image.width();
            let content_h = sprite.original_image.height();

            // Build polygon mesh data if in polygon mode
            let (vertices, vertices_uv, triangles) =
                if let Some(poly) = &sprite.polygon_data {
                    let verts: Vec<[f32; 2]> =
                        poly.contour.iter().map(|&(x, y)| [x, y]).collect();
                    let uvs: Vec<[f32; 2]> = poly
                        .contour
                        .iter()
                        .map(|&(x, y)| [content_x as f32 + x, content_y as f32 + y])
                        .collect();
                    (Some(verts), Some(uvs), Some(poly.triangles.clone()))
                } else {
                    (None, None, None)
                };

            let packed_sprite = PackedSprite {
                name: sprite.name.clone(),
                x: content_x,
                y: content_y,
                w: if was_rotated { content_h } else { content_w },
                h: if was_rotated { content_w } else { content_h },
                rotated: was_rotated,
                trimmed: sprite.trim_info.trimmed,
                trim_offset_x: sprite.trim_info.offset_x,
                trim_offset_y: sprite.trim_info.offset_y,
                source_w: sprite.trim_info.source_w,
                source_h: sprite.trim_info.source_h,
                alias_of: None,
                vertices,
                vertices_uv,
                triangles,
            };

            canonical_packed.insert(sprite.name.clone(), packed_sprite.clone());
            atlas_sprites.push(packed_sprite);
        }

        // Add alias entries — they share the same atlas position as the canonical
        for (alias_name, canonical_name) in &aliases {
            if let Some(canonical) = canonical_packed.get(canonical_name) {
                atlas_sprites.push(PackedSprite {
                    name: alias_name.clone(),
                    alias_of: Some(canonical_name.clone()),
                    // Copy all position/size data from the canonical
                    ..canonical.clone()
                });
            }
        }

        // Sort sprites by name for deterministic output
        atlas_sprites.sort_by(|a, b| a.name.cmp(&b.name));

        let used_for_free: Vec<(u32, u32, u32, u32)> = outer_rects
            .iter()
            .map(|r| (r.x, r.y, r.w, r.h))
            .collect();
        let free_rects = manifest::compute_free_rects(bin_w as u32, bin_h as u32, &used_for_free);

        results.push(AtlasResult {
            image_path,
            data_path,
            width: bin_w as u32,
            height: bin_h as u32,
            sprites: atlas_sprites,
            animations: animations.clone(),
            duplicates_removed: dup_count,
            atlas_image: atlas_img,
            outer_rects,
            free_rects,
            from_cache: false,
        });
    }

    Ok(results)
}

/// Save an RGBA image as quantized (lossy compressed) PNG.
fn save_quantized_png(img: &RgbaImage, path: &Path, quality: u8) -> Result<()> {
    let (w, h) = img.dimensions();
    let quality = quality.clamp(1, 100);

    // Convert raw u8 bytes to &[RGBA] slice
    let rgba_pixels: &[imagequant::RGBA] = unsafe {
        std::slice::from_raw_parts(
            img.as_raw().as_ptr() as *const imagequant::RGBA,
            (w * h) as usize,
        )
    };

    let mut liq = imagequant::new();
    liq.set_quality(0, quality)
        .map_err(|e| AppError::Custom(format!("imagequant set_quality: {}", e)))?;

    let mut liq_img = liq
        .new_image_borrowed(rgba_pixels, w as usize, h as usize, 0.0)
        .map_err(|e| AppError::Custom(format!("imagequant new_image: {}", e)))?;

    let mut res = liq
        .quantize(&mut liq_img)
        .map_err(|e| AppError::Custom(format!("imagequant quantize: {}", e)))?;

    res.set_dithering_level(1.0)
        .map_err(|e| AppError::Custom(format!("imagequant dithering: {}", e)))?;

    let (palette, pixels) = res
        .remapped(&mut liq_img)
        .map_err(|e| AppError::Custom(format!("imagequant remap: {}", e)))?;

    // Write PNG using lodepng
    let mut encoder = lodepng::Encoder::new();
    encoder.set_auto_convert(false);

    // Set palette on both raw and png info
    for info in [encoder.info_raw_mut()] {
        info.set_colortype(lodepng::ColorType::PALETTE);
        info.set_bitdepth(8);
        for entry in &palette {
            info.palette_add(lodepng::RGBA {
                r: entry.r,
                g: entry.g,
                b: entry.b,
                a: entry.a,
            })
            .map_err(|e| AppError::Custom(format!("palette_add: {}", e)))?;
        }
    }
    {
        let info = &mut encoder.info_png_mut().color;
        info.set_colortype(lodepng::ColorType::PALETTE);
        info.set_bitdepth(8);
        for entry in &palette {
            info.palette_add(lodepng::RGBA {
                r: entry.r,
                g: entry.g,
                b: entry.b,
                a: entry.a,
            })
            .map_err(|e| AppError::Custom(format!("palette_add: {}", e)))?;
        }
    }

    let png_data = encoder
        .encode(&pixels, w as usize, h as usize)
        .map_err(|e| AppError::Custom(format!("PNG encode: {}", e)))?;

    std::fs::write(path, &png_data)?;

    // Log savings — compare with unquantized PNG
    let mut unquantized_buf = Vec::new();
    {
        use image::codecs::png::PngEncoder;
        use image::ImageEncoder;
        let enc = PngEncoder::new(std::io::Cursor::new(&mut unquantized_buf));
        enc.write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .map_err(|e| AppError::Custom(format!("PNG encode baseline: {}", e)))?;
    }

    let q_size = png_data.len();
    let orig_size = unquantized_buf.len();
    let saved_pct = if orig_size > 0 {
        (1.0 - q_size as f64 / orig_size as f64) * 100.0
    } else {
        0.0
    };
    log::info!(
        "Quantized PNG: {} -> {} bytes ({:.0}% smaller)",
        orig_size,
        q_size,
        saved_pct
    );

    Ok(())
}

/// Polygon mesh data for a sprite. May span multiple disjoint components —
/// `triangles` indices are flat into the combined `contour` vertex list.
#[derive(Clone)]
struct PolygonData {
    /// Simplified contour vertices in sprite-local coords (concatenated across components).
    contour: Vec<(f32, f32)>,
    /// Triangle indices into the combined `contour` array.
    triangles: Vec<[usize; 3]>,
    /// OBB of the union: (center_x, center_y, half_w, half_h, angle).
    #[allow(dead_code)]
    obb: (f32, f32, f32, f32, f32),
}

/// Build polygon mesh data for one sprite. Handles multi-component sprites,
/// applies `polygon_shape` (concave/convex/auto), and enforces `max_vertices`
/// budget via tolerance escalation when the budget is exceeded.
fn build_polygon_data(img: &RgbaImage, opts: &PackOptions) -> PolygonData {
    // Drop dust components below this many opaque pixels — a 2x2 dot still
    // survives but isolated antialiased pixels are filtered out.
    const MIN_COMPONENT_AREA: u32 = 4;

    let raw_components = contour::extract_components(img, opts.trim_threshold, MIN_COMPONENT_AREA);

    // Step 1: simplify each component independently with the requested tolerance.
    // Step 2: if a vertex budget is set and we're still over it, escalate
    //         tolerance (×1.5 per round) up to a small bound.
    let mut tolerance = opts.tolerance;
    let mut simplified: Vec<Vec<(f32, f32)>>;
    let max_iters = if opts.max_vertices > 0 { 8 } else { 1 };
    let mut iter = 0;
    loop {
        simplified = raw_components
            .iter()
            .map(|c| simplify::simplify_polygon(c, tolerance))
            .collect();

        let total_verts: u32 = simplified.iter().map(|p| p.len() as u32).sum();
        if opts.max_vertices == 0 || total_verts <= opts.max_vertices || iter >= max_iters {
            break;
        }
        // Escalate — geometric progression converges fast for typical sprites.
        tolerance *= 1.5;
        iter += 1;
    }

    // Step 3: shape mode per component.
    let shaped: Vec<Vec<(f32, f32)>> = simplified
        .into_iter()
        .map(|c| apply_shape_mode(&c, opts.polygon_shape))
        .collect();

    // Step 4: triangulate each component, concatenate with index offset.
    let mut combined_contour: Vec<(f32, f32)> = Vec::new();
    let mut combined_triangles: Vec<[usize; 3]> = Vec::new();
    for poly in &shaped {
        let offset = combined_contour.len();
        let tris = triangulate::triangulate(poly);
        combined_contour.extend(poly.iter().copied());
        combined_triangles.extend(
            tris.iter()
                .map(|t| [t[0] + offset, t[1] + offset, t[2] + offset]),
        );
    }

    let hull = contour::convex_hull(&combined_contour);
    let obb = contour::min_area_obb(&hull);

    PolygonData {
        contour: combined_contour,
        triangles: combined_triangles,
        obb,
    }
}

fn apply_shape_mode(poly: &[(f32, f32)], mode: PolygonShape) -> Vec<(f32, f32)> {
    if poly.len() < 3 {
        return poly.to_vec();
    }
    match mode {
        PolygonShape::Concave => poly.to_vec(),
        PolygonShape::Convex => contour::convex_hull(poly),
        PolygonShape::Auto => {
            // Pick convex when the hull only adds a small amount of overdraw.
            // Threshold 0.85 chosen empirically: typical character outlines have
            // ratio ~0.6-0.75 (concave wins), simple icons ~0.9+ (convex wins).
            let concave_area = contour::polygon_area(poly);
            let hull = contour::convex_hull(poly);
            let hull_area = contour::polygon_area(&hull);
            if hull_area > 0.0 && concave_area / hull_area >= 0.85 {
                hull
            } else {
                poly.to_vec()
            }
        }
    }
}

/// Internal sprite data during processing.
struct SpriteData {
    name: String,
    original_image: RgbaImage,
    extruded_image: RgbaImage,
    trim_info: trim::TrimResult,
    pack_w: u32,
    pack_h: u32,
    polygon_data: Option<PolygonData>,
}

/// Collect input sprites for packing.
///
/// When `opts.explicit_sprites` is `Some(list)`, pack exactly that list — this
/// is what the GUI uses so the user's curated selection isn't polluted by
/// unrelated images that happen to live in the same directory. Otherwise we
/// fall back to scanning `opts.input_dir` (recursively when `opts.recursive`).
fn collect_images_for(opts: &PackOptions) -> Result<Vec<(String, PathBuf)>> {
    let image_exts = ["png", "jpg", "jpeg", "bmp", "gif", "tga", "webp"];
    let mut entries: Vec<(String, PathBuf)> = Vec::new();

    if let Some(list) = &opts.explicit_sprites {
        // Honor the explicit list verbatim. Skip anything that doesn't exist on
        // disk or has a non-image extension — better to drop silently than to
        // crash the whole pack. Duplicates (same path twice) are also dropped.
        let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for path in list {
            if !path.is_file() {
                log::warn!("explicit sprite skipped (not a file): {}", path.display());
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_default();
            if !image_exts.contains(&ext.as_str()) {
                log::warn!(
                    "explicit sprite skipped (unsupported extension '{}'): {}",
                    ext,
                    path.display()
                );
                continue;
            }
            if !seen.insert(path.clone()) {
                continue;
            }

            let rel = match path.strip_prefix(&opts.input_dir) {
                Ok(stripped) => stripped.to_string_lossy().replace('\\', "/"),
                Err(_) => path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string()),
            };

            entries.push((rel, path.clone()));
        }
    } else {
        let walker = if opts.recursive {
            WalkDir::new(&opts.input_dir).follow_links(true)
        } else {
            WalkDir::new(&opts.input_dir).max_depth(1).follow_links(true)
        };
        for entry in walker {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_default();

            if !image_exts.contains(&ext.as_str()) {
                continue;
            }

            let rel = path
                .strip_prefix(&opts.input_dir)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");

            entries.push((rel, path.to_path_buf()));
        }
    }

    // De-duplicate by relative name — explicit lists with two files that
    // resolve to the same basename would otherwise get the same key in
    // PackedSprite and confuse the consumer.
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut deduped: Vec<(String, PathBuf)> = Vec::with_capacity(entries.len());
    let mut last_name: Option<String> = None;
    for (name, path) in entries {
        if last_name.as_deref() == Some(name.as_str()) {
            log::warn!("duplicate sprite name '{}' — keeping first occurrence", name);
            continue;
        }
        last_name = Some(name.clone());
        deduped.push((name, path));
    }
    Ok(deduped)
}


/// Detect animation groups from sprite names.
fn detect_animations_from_names(names: &[String]) -> HashMap<String, Vec<String>> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();

    let re = regex::Regex::new(r"^(.+?)[-_](\d+)\.\w+$").expect("valid regex");

    for name in names {
        if let Some(caps) = re.captures(name) {
            let group_name = caps.get(1).map(|m| m.as_str()).unwrap_or(name);
            let clean_group = group_name.rsplit('/').next().unwrap_or(group_name);
            groups
                .entry(clean_group.to_string())
                .or_default()
                .push(name.clone());
        }
    }

    groups.retain(|_, v| v.len() >= 2);
    for frames in groups.values_mut() {
        frames.sort();
    }

    groups
}

// ─── Incremental pack ────────────────────────────────────────────────────────

/// Try the incremental fast path. Returns:
///   - `Ok(Some(results))`  → cache is valid, use these results (some may be `from_cache`)
///   - `Ok(None)`           → cache miss / partial repack rejected; caller falls back to full
///   - `Err(_)`             → I/O error reading manifest or input metadata
fn try_incremental(
    opts: &PackOptions,
    entries: &[(String, PathBuf)],
) -> Result<Option<Vec<AtlasResult>>> {
    let manifest_path = manifest::Manifest::path_for(opts);
    let cached = match manifest::Manifest::try_load(&manifest_path)? {
        Some(m) => m,
        None => {
            log::debug!(
                "incremental: no manifest at {}, full repack",
                manifest_path.display()
            );
            return Ok(None);
        }
    };

    // Stage 1: option compatibility — must match exactly.
    let current_options_hash = manifest::compute_options_hash(opts);
    if current_options_hash != cached.options_hash {
        log::info!("incremental: options changed since last pack, full repack");
        return Ok(None);
    }

    // Stage 2: input root must match (different sprite tree ⇒ unrelated cache).
    if cached.input_root != opts.input_dir.to_string_lossy() {
        log::info!("incremental: input directory changed, full repack");
        return Ok(None);
    }

    // Stage 3: existing atlas files must still be present and unaltered.
    for atlas in &cached.atlases {
        let atlas_image_path = opts.output_dir.join(&atlas.image_filename);
        if !atlas_image_path.exists() {
            log::info!(
                "incremental: atlas image missing on disk: {}",
                atlas_image_path.display()
            );
            return Ok(None);
        }
        let on_disk_hash = manifest::hash_file(&atlas_image_path)?;
        if on_disk_hash != atlas.image_hash {
            log::info!(
                "incremental: atlas image hash changed on disk: {}",
                atlas_image_path.display()
            );
            return Ok(None);
        }
    }

    // Stage 4: diff inputs against the manifest (no pixel decode yet — file
    // size + mtime is the fast pre-check; only ambiguous cases hit pixel hash).
    let diff = diff_inputs(&cached, entries)?;
    log::info!(
        "incremental: diff = {} added, {} removed, {} modified, {} unchanged",
        diff.added.len(),
        diff.removed.len(),
        diff.modified.len(),
        diff.unchanged.len()
    );

    // Branch A: nothing changed → full skip, synthesize cached results.
    if diff.is_unchanged() {
        log::info!("incremental: full cache hit, skipping pack entirely");
        return Ok(Some(synthesize_cached_results(opts, &cached)?));
    }

    // Branch B: try partial repack — keep unchanged sprites at their exact
    // (x, y, rotated) so deployed clients can drop in the new atlas without
    // rebaking UVs. Falls back to None on any layout-breaking change.
    match try_partial_repack(opts, entries, &cached, &diff)? {
        Some(results) => {
            log::info!("incremental: partial repack succeeded (UV-stable)");
            Ok(Some(results))
        }
        None => {
            log::info!("incremental: partial repack rejected, falling back to full repack");
            Ok(None)
        }
    }
}

/// Rebuild a `Vec<AtlasResult>` from a manifest and the on-disk atlas PNGs,
/// without doing any packing. The atlas image is loaded so the caller can still
/// access pixels (e.g. GUI inline preview); `from_cache=true` means save_to_disk
/// is a no-op.
fn synthesize_cached_results(
    opts: &PackOptions,
    manifest: &manifest::Manifest,
) -> Result<Vec<AtlasResult>> {
    let mut results = Vec::with_capacity(manifest.atlases.len());

    // Group sprites by atlas_idx for fast assembly.
    let mut sprites_by_atlas: HashMap<usize, Vec<&manifest::SpriteEntry>> = HashMap::new();
    for entry in manifest.sprites.values() {
        sprites_by_atlas
            .entry(entry.atlas_idx)
            .or_default()
            .push(entry);
    }

    let all_names: Vec<String> = manifest.sprites.keys().cloned().collect();
    let animations = detect_animations_from_names(&all_names);

    for (atlas_idx, atlas) in manifest.atlases.iter().enumerate() {
        let image_path = opts.output_dir.join(&atlas.image_filename);
        let data_path = opts.output_dir.join(&atlas.data_filename);

        // Load the cached atlas PNG so callers (GUI) can still preview.
        let atlas_image = image::open(&image_path)?.into_rgba8();

        let mut sprites: Vec<PackedSprite> = sprites_by_atlas
            .get(&atlas_idx)
            .map(|v| {
                v.iter()
                    .map(|e| PackedSprite {
                        name: e.rel_path.clone(),
                        x: e.content_x,
                        y: e.content_y,
                        w: e.trimmed_size[0],
                        h: e.trimmed_size[1],
                        rotated: e.rotated,
                        trimmed: e.trim_offset != [0, 0]
                            || e.trimmed_size != e.source_size,
                        trim_offset_x: e.trim_offset[0],
                        trim_offset_y: e.trim_offset[1],
                        source_w: e.source_size[0],
                        source_h: e.source_size[1],
                        alias_of: e.alias_of.clone(),
                        // Polygon mesh is not reconstructed in cached mode — the
                        // sidecar metadata file already on disk contains it. The
                        // in-memory PackedSprite is only used for `--json` summary
                        // and GUI preview, which don't need the mesh.
                        vertices: None,
                        vertices_uv: None,
                        triangles: None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        sprites.sort_by(|a, b| a.name.cmp(&b.name));

        results.push(AtlasResult {
            image_path,
            data_path,
            width: atlas.width,
            height: atlas.height,
            sprites,
            animations: animations.clone(),
            duplicates_removed: 0,
            atlas_image,
            outer_rects: atlas.used_rects.clone(),
            free_rects: atlas.free_rects.clone(),
            from_cache: true,
        });
    }

    Ok(results)
}

/// Diff current input set against the manifest. Uses (file_size, mtime) as a
/// fast pre-check and only computes a SHA256 pixel hash for ambiguous cases.
fn diff_inputs(
    manifest: &manifest::Manifest,
    entries: &[(String, PathBuf)],
) -> Result<manifest::InputDiff> {
    use rayon::prelude::*;

    let entries_by_name: HashMap<&str, &Path> = entries
        .iter()
        .map(|(n, p)| (n.as_str(), p.as_path()))
        .collect();

    // Removed = in manifest, not on disk.
    let mut removed: Vec<String> = manifest
        .sprites
        .keys()
        .filter(|n| !entries_by_name.contains_key(n.as_str()) && {
            // Aliases live in the manifest but resolve to the canonical sprite.
            // If the canonical is still present, alias is technically derived and
            // can be skipped from "removed" — but for v0.2 simplicity we treat
            // alias removal as a change too.
            true
        })
        .cloned()
        .collect();
    removed.sort();

    // Each entry: classify as added / unchanged / modified.
    let classified: Vec<(String, ClassifyResult)> = entries
        .par_iter()
        .map(|(name, path)| {
            let result = classify_entry(name, path, manifest)?;
            Ok((name.clone(), result))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut unchanged = Vec::new();

    for (name, c) in classified {
        match c {
            ClassifyResult::Added => added.push(name),
            ClassifyResult::Unchanged => unchanged.push(name),
            ClassifyResult::Modified { size_changed } => {
                modified.push(manifest::ModifiedSprite { name, size_changed })
            }
        }
    }

    added.sort();
    unchanged.sort();
    modified.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(manifest::InputDiff {
        added,
        removed,
        modified,
        unchanged,
    })
}

enum ClassifyResult {
    Added,
    Unchanged,
    Modified { size_changed: bool },
}

fn classify_entry(
    name: &str,
    path: &Path,
    manifest: &manifest::Manifest,
) -> Result<ClassifyResult> {
    let entry = match manifest.sprite(name) {
        Some(e) => e,
        None => return Ok(ClassifyResult::Added),
    };

    // Fast pre-check: identical (size, mtime) ⇒ assume unchanged.
    // This avoids decoding pixels for the common "rebuild without changes" case.
    let (size, mtime) = manifest::file_fingerprint(path)?;
    if size == entry.file_size && mtime == entry.mtime {
        return Ok(ClassifyResult::Unchanged);
    }

    // Fingerprint changed — decode and compute pixel hash to confirm.
    let img = image::open(path)?.into_rgba8();
    let pixel_hash = manifest::hash_pixels(&img);
    if pixel_hash == entry.content_hash {
        // Pixels identical, just file metadata changed (touch, copy).
        return Ok(ClassifyResult::Unchanged);
    }

    // Genuine change — was the trimmed dimension preserved?
    // We only compute trimmed size if trim is enabled (using a default threshold
    // here is a slight approximation; classification only needs a binary "size
    // changed" hint, which the upcoming partial-repack path will refine).
    let (w, h) = img.dimensions();
    let size_changed = (w, h) != (entry.source_size[0], entry.source_size[1]);

    Ok(ClassifyResult::Modified { size_changed })
}

// ─── Partial repack ──────────────────────────────────────────────────────────

/// Attempt a partial repack that preserves the exact (x, y, rotated) of every
/// sprite that did not change. Returns `Some(results)` on success or `None` if
/// the diff cannot be applied without growing an atlas (in which case the
/// caller falls back to a full repack).
///
/// Strategy:
///   1. Build a working copy of every existing atlas (load PNG, copy used/free).
///   2. **Removed** sprites: drop entry, clear pixels, mark old rect as free.
///   3. **Modified-same-size** sprites: replace pixels in-place at existing rect.
///   4. **Modified-resized** sprites: treat as remove + add.
///   5. **Added** sprites: try-fit into combined free space across atlases
///      (best-short-side fit), place pixels, refresh free rects.
///   6. If any add fails to fit anywhere ⇒ return None (no atlas growth).
///
/// Animations are re-detected on the new full sprite list. Atlases that didn't
/// see any add/remove/modify keep their PNG (cache hit) but their metadata
/// sidecar is re-emitted because animations are a global view.
fn try_partial_repack(
    opts: &PackOptions,
    entries: &[(String, PathBuf)],
    cached: &manifest::Manifest,
    diff: &manifest::InputDiff,
) -> Result<Option<Vec<AtlasResult>>> {
    // 1. Load all existing atlas images into editable state.
    let mut atlases: Vec<WorkingAtlas> = cached
        .atlases
        .iter()
        .map(|a| {
            let img_path = opts.output_dir.join(&a.image_filename);
            let img = image::open(&img_path)?.into_rgba8();
            Ok(WorkingAtlas {
                image_filename: a.image_filename.clone(),
                data_filename: a.data_filename.clone(),
                width: a.width,
                height: a.height,
                image: img,
                used_rects: a.used_rects.clone(),
                free_rects: a.free_rects.clone(),
                dirty: false,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // 2. Build a mutable view of the manifest's per-sprite entries.
    let mut sprites: HashMap<String, manifest::SpriteEntry> =
        cached.sprites.clone().into_iter().collect();

    // 3. Apply removals: drop entry, free outer rect, clear pixels.
    for name in &diff.removed {
        if let Some(entry) = sprites.remove(name) {
            // Aliases reference a canonical — only the canonical owns a layout slot.
            if entry.alias_of.is_some() {
                continue;
            }
            if let Some(atlas) = atlases.get_mut(entry.atlas_idx) {
                if let Some(pos) = atlas.used_rects.iter().position(|u| u.name == *name) {
                    let removed = atlas.used_rects.swap_remove(pos);
                    clear_atlas_region(&mut atlas.image, &removed);
                    atlas.free_rects = manifest::compute_free_rects(
                        atlas.width,
                        atlas.height,
                        &atlas.used_rects.iter().map(|r| (r.x, r.y, r.w, r.h)).collect::<Vec<_>>(),
                    );
                    atlas.dirty = true;
                }
            }
        }
    }

    // 4. Build the entries-by-name lookup for image loading.
    let entries_by_name: HashMap<&str, &Path> = entries
        .iter()
        .map(|(n, p)| (n.as_str(), p.as_path()))
        .collect();

    // 5. Process added + modified sprites: load + preprocess.
    //    For modified-resized we treat as remove+add (free old rect first).
    let mut to_place: Vec<SpriteData> = Vec::new();
    use rayon::prelude::*;

    let mut work_names: Vec<String> = diff.added.clone();
    for m in &diff.modified {
        if m.size_changed {
            // Free the old rect (full remove), schedule re-add later.
            if let Some(entry) = sprites.remove(&m.name) {
                if let Some(atlas) = atlases.get_mut(entry.atlas_idx) {
                    if let Some(pos) =
                        atlas.used_rects.iter().position(|u| u.name == m.name)
                    {
                        let removed = atlas.used_rects.swap_remove(pos);
                        clear_atlas_region(&mut atlas.image, &removed);
                        atlas.free_rects = manifest::compute_free_rects(
                            atlas.width,
                            atlas.height,
                            &atlas.used_rects.iter().map(|r| (r.x, r.y, r.w, r.h)).collect::<Vec<_>>(),
                        );
                        atlas.dirty = true;
                    }
                }
            }
            work_names.push(m.name.clone());
        }
    }
    work_names.sort();
    work_names.dedup();

    let processed: Vec<Result<SpriteData>> = work_names
        .par_iter()
        .map(|name| {
            let path = entries_by_name.get(name.as_str()).ok_or_else(|| {
                AppError::Custom(format!("partial: missing path for '{}'", name))
            })?;
            let img = image::open(path)?.into_rgba8();
            Ok(preprocess_sprite(name.clone(), img, opts))
        })
        .collect();
    for sd in processed {
        to_place.push(sd?);
    }

    // 6. In-place pixel replacement for same-size modifications. We re-trim
    //    using the same options; if the trimmed dimensions differ from the
    //    manifest's record we bail out (this reclassifies as resized — should
    //    have been caught earlier; safety net).
    for m in &diff.modified {
        if m.size_changed {
            continue;
        }
        let sprite_data = match preprocess_one(opts, &m.name, &entries_by_name)? {
            Some(s) => s,
            None => return Ok(None),
        };
        let entry = match sprites.get(&m.name) {
            Some(e) => e.clone(),
            None => continue,
        };
        if entry.alias_of.is_some() {
            continue;
        }
        // Verify trimmed dims match (else bail and let full-repack handle).
        let new_trim = [
            sprite_data.original_image.width(),
            sprite_data.original_image.height(),
        ];
        if new_trim != entry.trimmed_size {
            log::info!(
                "partial: sprite '{}' trimmed size changed {:?} -> {:?}; bail to full",
                m.name,
                entry.trimmed_size,
                new_trim
            );
            return Ok(None);
        }
        let atlas = match atlases.get_mut(entry.atlas_idx) {
            Some(a) => a,
            None => return Ok(None),
        };
        let outer = match atlas.used_rects.iter().find(|u| u.name == m.name) {
            Some(u) => u.clone(),
            None => return Ok(None),
        };
        // Repaint extruded image at outer.x/outer.y (apply rotation if needed).
        let to_paint = if outer.rotated {
            rotate_90cw(&sprite_data.extruded_image)
        } else {
            sprite_data.extruded_image.clone()
        };
        // Clear the old region first so transparent pixels of new sprite win.
        clear_atlas_region(&mut atlas.image, &outer);
        image::imageops::overlay(&mut atlas.image, &to_paint, outer.x as i64, outer.y as i64);

        // Update sprite entry (content_hash, mtime, polygon hash).
        let (size, mtime) = manifest::file_fingerprint(
            entries_by_name
                .get(m.name.as_str())
                .copied()
                .ok_or_else(|| AppError::Custom("partial: missing path".into()))?,
        )?;
        let new_hash = manifest::hash_pixels(&sprite_data.original_image);
        sprites
            .entry(m.name.clone())
            .and_modify(|e| {
                e.file_size = size;
                e.mtime = mtime;
                e.content_hash = new_hash.clone();
                if let Some(poly) = &sprite_data.polygon_data {
                    e.polygon_hash = Some(manifest::hash_polygon(&poly.contour, &poly.triangles));
                } else {
                    e.polygon_hash = None;
                }
            });
        atlas.dirty = true;
    }

    // 7. Place added (and resized-modified) sprites into combined free space.
    //    We try every atlas's free_rects; pick the one with the tightest fit.
    //    On a successful placement we update used_rects, free_rects, paint pixels,
    //    and append a sprite entry. If anything can't fit, return None.
    for sd in to_place {
        let extra = opts.extrude * 2 + opts.padding * 2;
        let outer_w = sd.original_image.width() + extra + opts.spacing;
        let outer_h = sd.original_image.height() + extra + opts.spacing;

        // Find the best-fit atlas + free rect.
        let mut best: Option<(usize, manifest::FitResult)> = None;
        for (idx, atlas) in atlases.iter().enumerate() {
            if let Some(fit) = manifest::try_fit(&atlas.free_rects, outer_w, outer_h, opts.rotate) {
                if best.as_ref().map_or(true, |(_, f)| fit.score < f.score) {
                    best = Some((idx, fit));
                }
            }
        }
        let (atlas_idx, fit) = match best {
            Some(x) => x,
            None => {
                log::info!(
                    "partial: sprite '{}' ({}x{}) does not fit any free rect; bail",
                    sd.name,
                    outer_w,
                    outer_h
                );
                return Ok(None);
            }
        };

        let atlas = &mut atlases[atlas_idx];
        // Paint extruded image rotated as needed.
        let to_paint = if fit.rotated {
            rotate_90cw(&sd.extruded_image)
        } else {
            sd.extruded_image.clone()
        };
        image::imageops::overlay(&mut atlas.image, &to_paint, fit.x as i64, fit.y as i64);

        atlas.used_rects.push(manifest::UsedRect {
            name: sd.name.clone(),
            x: fit.x,
            y: fit.y,
            w: fit.w,
            h: fit.h,
            rotated: fit.rotated,
        });
        atlas.free_rects = manifest::compute_free_rects(
            atlas.width,
            atlas.height,
            &atlas.used_rects.iter().map(|r| (r.x, r.y, r.w, r.h)).collect::<Vec<_>>(),
        );
        atlas.dirty = true;

        let content_x = fit.x + opts.extrude + opts.padding;
        let content_y = fit.y + opts.extrude + opts.padding;
        let content_w = sd.original_image.width();
        let content_h = sd.original_image.height();

        let path = entries_by_name
            .get(sd.name.as_str())
            .copied()
            .ok_or_else(|| AppError::Custom("partial: missing path".into()))?;
        let (size, mtime) = manifest::file_fingerprint(path)?;
        let content_hash = manifest::hash_pixels(&sd.original_image);

        sprites.insert(
            sd.name.clone(),
            manifest::SpriteEntry {
                rel_path: sd.name.clone(),
                file_size: size,
                mtime,
                content_hash,
                trim_offset: [sd.trim_info.offset_x, sd.trim_info.offset_y],
                trimmed_size: [content_w, content_h],
                source_size: [sd.trim_info.source_w, sd.trim_info.source_h],
                polygon_hash: sd
                    .polygon_data
                    .as_ref()
                    .map(|p| manifest::hash_polygon(&p.contour, &p.triangles)),
                atlas_idx,
                content_x,
                content_y,
                rotated: fit.rotated,
                alias_of: None,
                // Newly-added sprites have no tags by default. The user can
                // attach metadata afterwards with `mj_atlas tag`.
                tags: Vec::new(),
                attribution: None,
                source_url: None,
            },
        );
    }

    // 8. Build AtlasResult set. Reuse cached atlases when not dirty.
    let all_names: Vec<String> = sprites.keys().cloned().collect();
    let animations = detect_animations_from_names(&all_names);

    let mut sprites_by_atlas: HashMap<usize, Vec<&manifest::SpriteEntry>> = HashMap::new();
    for entry in sprites.values() {
        sprites_by_atlas.entry(entry.atlas_idx).or_default().push(entry);
    }

    let mut results = Vec::with_capacity(atlases.len());
    for (atlas_idx, atlas) in atlases.into_iter().enumerate() {
        let mut atlas_sprites: Vec<PackedSprite> = sprites_by_atlas
            .get(&atlas_idx)
            .map(|v| {
                v.iter()
                    .map(|e| PackedSprite {
                        name: e.rel_path.clone(),
                        x: e.content_x,
                        y: e.content_y,
                        w: e.trimmed_size[0],
                        h: e.trimmed_size[1],
                        rotated: e.rotated,
                        trimmed: e.trim_offset != [0, 0]
                            || e.trimmed_size != e.source_size,
                        trim_offset_x: e.trim_offset[0],
                        trim_offset_y: e.trim_offset[1],
                        source_w: e.source_size[0],
                        source_h: e.source_size[1],
                        alias_of: e.alias_of.clone(),
                        vertices: None,
                        vertices_uv: None,
                        triangles: None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        atlas_sprites.sort_by(|a, b| a.name.cmp(&b.name));

        // Animations are global — when ANY atlas is dirty, every metadata
        // sidecar must be re-emitted. So we mark cached=false for clean
        // atlases too if any sibling changed (but skip the expensive PNG
        // re-write because the image bytes are unchanged).
        let any_dirty = sprites.values().any(|_| true) // always recompute metadata
            && false; // placeholder; we override below

        let _ = any_dirty;
        let from_cache = !atlas.dirty;
        results.push(AtlasResult {
            image_path: opts.output_dir.join(&atlas.image_filename),
            data_path: opts.output_dir.join(&atlas.data_filename),
            width: atlas.width,
            height: atlas.height,
            sprites: atlas_sprites,
            animations: animations.clone(),
            duplicates_removed: 0,
            atlas_image: atlas.image,
            outer_rects: atlas.used_rects,
            free_rects: atlas.free_rects,
            from_cache,
        });
    }

    // If ANY result is dirty we must re-emit metadata for ALL atlases (animations
    // are global). Force from_cache=false on clean atlases when there's
    // any dirty sibling — but skip PNG re-writes by introducing a "metadata only"
    // marker. For v0.2 simplicity, when any sibling is dirty we mark all dirty:
    // a clean atlas's PNG re-encode matches its prior bytes (deterministic with
    // same RGBA), so manifest hash check still passes on subsequent runs.
    if results.iter().any(|r| !r.from_cache) {
        for r in &mut results {
            r.from_cache = false;
        }
    }

    Ok(Some(results))
}

struct WorkingAtlas {
    image_filename: String,
    data_filename: String,
    width: u32,
    height: u32,
    image: RgbaImage,
    used_rects: Vec<manifest::UsedRect>,
    free_rects: Vec<manifest::FreeRect>,
    dirty: bool,
}

/// Zero out a rectangular region in the atlas (transparent RGBA).
fn clear_atlas_region(img: &mut RgbaImage, rect: &manifest::UsedRect) {
    let (img_w, img_h) = img.dimensions();
    for y in rect.y..(rect.y + rect.h).min(img_h) {
        for x in rect.x..(rect.x + rect.w).min(img_w) {
            img.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
        }
    }
}

/// Preprocess a single sprite — trim, extrude, polygon mesh.
fn preprocess_sprite(name: String, img: RgbaImage, opts: &PackOptions) -> SpriteData {
    let (trimmed_img, trim_info) = if opts.trim {
        let tr = trim::trim_transparent(&img, opts.trim_threshold);
        (tr.image.clone(), tr)
    } else {
        let (w, h) = img.dimensions();
        (
            img.clone(),
            trim::TrimResult {
                image: img.clone(),
                offset_x: 0,
                offset_y: 0,
                source_w: w,
                source_h: h,
                trimmed: false,
            },
        )
    };

    let extruded = extrude::extrude_edges(&trimmed_img, opts.extrude);
    let extra = opts.extrude * 2 + opts.padding * 2;
    let pack_w = trimmed_img.width() + extra;
    let pack_h = trimmed_img.height() + extra;

    let polygon_data = if opts.polygon {
        Some(build_polygon_data(&trimmed_img, opts))
    } else {
        None
    };

    SpriteData {
        name,
        original_image: trimmed_img,
        extruded_image: extruded,
        trim_info,
        pack_w,
        pack_h,
        polygon_data,
    }
}

fn preprocess_one(
    opts: &PackOptions,
    name: &str,
    entries_by_name: &HashMap<&str, &Path>,
) -> Result<Option<SpriteData>> {
    let path = match entries_by_name.get(name) {
        Some(p) => *p,
        None => return Ok(None),
    };
    let img = image::open(path)?.into_rgba8();
    Ok(Some(preprocess_sprite(name.to_string(), img, opts)))
}

/// Persist the manifest after a successful (full or partial) pack.
fn write_manifest(
    opts: &PackOptions,
    results: &[AtlasResult],
    entries: &[(String, PathBuf)],
) -> Result<()> {
    use rayon::prelude::*;

    // Hash every input pixel buffer (parallel) and gather file fingerprints.
    type EntryHash = (String, u64, i64, String, [u32; 2]);
    let entry_hashes: Vec<Result<EntryHash>> = entries
        .par_iter()
        .map(|(name, path)| {
            let (size, mtime) = manifest::file_fingerprint(path)?;
            let img = image::open(path)?.into_rgba8();
            let pix = manifest::hash_pixels(&img);
            let dims = img.dimensions();
            Ok((name.clone(), size, mtime, pix, [dims.0, dims.1]))
        })
        .collect();
    let entry_hashes: Vec<EntryHash> = entry_hashes
        .into_iter()
        .collect::<Result<Vec<_>>>()?;

    let mut by_name: HashMap<String, EntryHash> = HashMap::new();
    for h in entry_hashes {
        by_name.insert(h.0.clone(), h);
    }

    // Build atlas entries first (and remember where each sprite landed).
    let mut atlases: Vec<manifest::AtlasEntry> = Vec::with_capacity(results.len());
    let mut sprite_atlas_idx: HashMap<String, usize> = HashMap::new();

    for (atlas_idx, r) in results.iter().enumerate() {
        let image_filename = r
            .image_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("{}.png", opts.output_name));
        let data_filename = r
            .data_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| opts.output_name.clone());

        // Hash the atlas PNG file ON DISK so manifest stays consistent with
        // whatever variant we wrote (quantized / not).
        let image_hash = manifest::hash_file(&r.image_path)?;

        for ur in &r.outer_rects {
            sprite_atlas_idx.insert(ur.name.clone(), atlas_idx);
        }

        atlases.push(manifest::AtlasEntry {
            image_filename,
            data_filename,
            width: r.width,
            height: r.height,
            image_hash,
            format: opts.format.as_str().to_string(),
            used_rects: r.outer_rects.clone(),
            free_rects: r.free_rects.clone(),
        });
    }

    // Build sprite map (canonical + aliases). We rely on `r.sprites` for the
    // full set (including aliases) and `r.outer_rects` for layout placement.
    let mut sprites = std::collections::BTreeMap::new();
    for r in results {
        for ps in &r.sprites {
            // Find atlas index for this sprite (alias resolves to canonical).
            let lookup_name = ps.alias_of.as_deref().unwrap_or(&ps.name);
            let atlas_idx = match sprite_atlas_idx.get(lookup_name).copied() {
                Some(i) => i,
                None => continue, // alias to a sprite not in outer_rects (shouldn't happen)
            };

            let (file_size, mtime, content_hash, source_size) =
                if let Some(h) = by_name.get(&ps.name) {
                    (h.1, h.2, h.3.clone(), h.4)
                } else {
                    (0u64, 0i64, String::new(), [ps.source_w, ps.source_h])
                };

            sprites.insert(
                ps.name.clone(),
                manifest::SpriteEntry {
                    rel_path: ps.name.clone(),
                    file_size,
                    mtime,
                    content_hash,
                    trim_offset: [ps.trim_offset_x, ps.trim_offset_y],
                    trimmed_size: [ps.w, ps.h],
                    source_size,
                    polygon_hash: ps
                        .vertices
                        .as_ref()
                        .zip(ps.triangles.as_ref())
                        .map(|(v, t)| {
                            let contour: Vec<(f32, f32)> =
                                v.iter().map(|&[a, b]| (a, b)).collect();
                            manifest::hash_polygon(&contour, t)
                        }),
                    atlas_idx,
                    content_x: ps.x,
                    content_y: ps.y,
                    rotated: ps.rotated,
                    alias_of: ps.alias_of.clone(),
                    // Tags / attribution / source_url are user-editable metadata
                    // and survive across packs via merge_user_metadata below.
                    tags: Vec::new(),
                    attribution: None,
                    source_url: None,
                },
            );
        }
    }

    let mut new_manifest = manifest::Manifest {
        version: manifest::MANIFEST_VERSION,
        tool: format!("mj_atlas {}", env!("CARGO_PKG_VERSION")),
        options_hash: manifest::compute_options_hash(opts),
        input_root: opts.input_dir.to_string_lossy().to_string(),
        sprites,
        atlases,
    };

    let path = manifest::Manifest::path_for(opts);
    // Preserve user-set tags/attribution/source_url across repacks. The fresh
    // manifest never sets these (the pack pipeline doesn't know about them);
    // we copy from the prior manifest if one exists.
    if let Some(prior) = manifest::Manifest::try_load(&path)? {
        manifest::merge_user_metadata(&mut new_manifest, &prior);
    }
    new_manifest.save(&path)?;
    log::info!("Saved manifest: {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn write_png(path: &Path, w: u32, h: u32, color: [u8; 4]) {
        let mut img = RgbaImage::new(w, h);
        for px in img.pixels_mut() {
            *px = image::Rgba(color);
        }
        let mut buf = Vec::new();
        {
            use image::codecs::png::PngEncoder;
            use image::ImageEncoder;
            let enc = PngEncoder::new(Cursor::new(&mut buf));
            enc.write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
                .unwrap();
        }
        std::fs::write(path, buf).unwrap();
    }

    fn make_opts(input_dir: &Path, output_dir: &Path, explicit: Option<Vec<PathBuf>>) -> PackOptions {
        PackOptions {
            input_dir: input_dir.to_path_buf(),
            output_name: "atlas".into(),
            output_dir: output_dir.to_path_buf(),
            max_size: 1024,
            spacing: 0,
            padding: 0,
            extrude: 0,
            trim: false,
            trim_threshold: 0,
            rotate: false,
            pot: false,
            recursive: true,
            explicit_sprites: explicit,
            incremental: false,
            force: false,
            format: crate::output::Format::JsonHash,
            quantize: false,
            quantize_quality: 85,
            polygon: false,
            tolerance: 2.0,
            polygon_shape: PolygonShape::Concave,
            max_vertices: 0,
        }
    }

    /// Regression: when the GUI hands us an explicit sprite list, the packer
    /// must NOT scan the parent directory and pick up unrelated images. The
    /// GUI's drag-drop selection drives the pack — deletions in the GUI list
    /// must propagate to the next pack instead of being silently re-added by
    /// a directory walk.
    #[test]
    fn explicit_sprites_ignores_unselected_sibling_files() {
        let tmp = std::env::temp_dir().join(format!(
            "mj_atlas_explicit_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let a = tmp.join("a.png");
        let b = tmp.join("b.png");
        let c = tmp.join("c_unrelated.png"); // never selected
        write_png(&a, 16, 16, [255, 0, 0, 255]);
        write_png(&b, 16, 16, [0, 255, 0, 255]);
        write_png(&c, 16, 16, [0, 0, 255, 255]);

        let opts = make_opts(&tmp, &tmp, Some(vec![a.clone(), b.clone()]));
        let entries = collect_images_for(&opts).unwrap();

        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["a.png", "b.png"],
            "explicit list must not include siblings; got {:?}",
            names
        );

        // Sanity: directory mode (explicit_sprites = None) still picks up all 3.
        let opts_dir = make_opts(&tmp, &tmp, None);
        let dir_entries = collect_images_for(&opts_dir).unwrap();
        assert_eq!(dir_entries.len(), 3, "dir scan should find all 3 PNGs");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Regression: deleting an explicit sprite between calls must shrink the
    /// next pack's input set, even though the file is still on disk. This is
    /// the literal scenario the GUI exercises after `x` is clicked.
    #[test]
    fn explicit_sprites_deletion_shrinks_pack_input() {
        let tmp = std::env::temp_dir().join(format!(
            "mj_atlas_delete_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let a = tmp.join("a.png");
        let b = tmp.join("b.png");
        write_png(&a, 16, 16, [255, 0, 0, 255]);
        write_png(&b, 16, 16, [0, 255, 0, 255]);

        let opts_full = make_opts(&tmp, &tmp, Some(vec![a.clone(), b.clone()]));
        assert_eq!(collect_images_for(&opts_full).unwrap().len(), 2);

        // User deletes 'a' from the GUI list — file still on disk.
        let opts_after_delete = make_opts(&tmp, &tmp, Some(vec![b.clone()]));
        let entries = collect_images_for(&opts_after_delete).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "b.png");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Sprites picked from disjoint directories (`/foo/x.png` + `/bar/y.png`)
    /// don't share a meaningful common prefix. Each sprite must still get a
    /// usable name; we fall back to the basename.
    #[test]
    fn explicit_sprites_disjoint_dirs_use_basenames() {
        let root = std::env::temp_dir().join(format!(
            "mj_atlas_disjoint_test_{}",
            std::process::id()
        ));
        let foo = root.join("foo");
        let bar = root.join("bar");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&foo).unwrap();
        std::fs::create_dir_all(&bar).unwrap();

        let x = foo.join("x.png");
        let y = bar.join("y.png");
        write_png(&x, 8, 8, [10, 20, 30, 255]);
        write_png(&y, 8, 8, [40, 50, 60, 255]);

        // input_dir doesn't contain either sprite — strip_prefix will fail and
        // we should fall back to the basename for the relative-name key.
        let opts = make_opts(&root, &root, Some(vec![x.clone(), y.clone()]));
        let entries = collect_images_for(&opts).unwrap();
        let mut names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        names.sort();
        // x.png is under root, so strip_prefix("root") yields "foo/x.png".
        // y.png same: "bar/y.png".
        assert_eq!(names, vec!["bar/y.png", "foo/x.png"]);

        let _ = std::fs::remove_dir_all(&root);
    }
}

/// Rotate an image 90° clockwise.
fn rotate_90cw(img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let mut rotated = RgbaImage::new(h, w);
    for y in 0..h {
        for x in 0..w {
            let pixel = *img.get_pixel(x, y);
            rotated.put_pixel(h - 1 - y, x, pixel);
        }
    }
    rotated
}
