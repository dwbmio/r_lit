pub mod contour;
pub mod dedup;
pub mod extrude;
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
    #[allow(dead_code)]
    pub incremental: bool,
    /// Enable PNG quantization (lossy compression)
    pub quantize: bool,
    /// PNG quantization quality 0-100 (lower = smaller file, more loss)
    pub quantize_quality: u8,
    /// Enable polygon mode (contour-based packing + mesh output)
    pub polygon: bool,
    /// Polygon simplification tolerance (lower = tighter fit, more vertices)
    pub tolerance: f32,
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
}

impl AtlasResult {
    /// Write atlas image and metadata to disk. Call this only on explicit export.
    pub fn save_to_disk(&self, opts: &PackOptions, fmt: crate::output::Format) -> Result<()> {
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

/// Main entry point: execute the packing pipeline.
pub fn execute(opts: &PackOptions) -> Result<Vec<AtlasResult>> {
    // 1. Collect input images
    let entries = collect_images(&opts.input_dir, opts.recursive)?;
    if entries.is_empty() {
        return Err(AppError::NoImages(opts.input_dir.display().to_string()));
    }
    log::info!(
        "Found {} sprite(s) in {}",
        entries.len(),
        opts.input_dir.display()
    );

    // 2. Load all images in parallel
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
                let raw_contour = contour::extract_contour(&trimmed_img, opts.trim_threshold);
                let simplified = simplify::simplify_polygon(&raw_contour, opts.tolerance);
                let triangles = triangulate::triangulate(&simplified);
                let hull = contour::convex_hull(&simplified);
                let obb = contour::min_area_obb(&hull);
                Some(PolygonData {
                    contour: simplified,
                    triangles,
                    obb,
                })
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

        results.push(AtlasResult {
            image_path,
            data_path,
            width: bin_w as u32,
            height: bin_h as u32,
            sprites: atlas_sprites,
            animations: animations.clone(),
            duplicates_removed: dup_count,
            atlas_image: atlas_img,
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

/// Polygon mesh data for a sprite.
#[derive(Clone)]
struct PolygonData {
    /// Simplified contour vertices in sprite-local coords
    contour: Vec<(f32, f32)>,
    /// Triangle indices into contour
    triangles: Vec<[usize; 3]>,
    /// OBB: (center_x, center_y, half_w, half_h, angle)
    #[allow(dead_code)]
    obb: (f32, f32, f32, f32, f32),
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

/// Collect all image files from the input directory.
fn collect_images(dir: &Path, recursive: bool) -> Result<Vec<(String, PathBuf)>> {
    let walker = if recursive {
        WalkDir::new(dir).follow_links(true)
    } else {
        WalkDir::new(dir).max_depth(1).follow_links(true)
    };

    let image_exts = ["png", "jpg", "jpeg", "bmp", "gif", "tga", "webp"];

    let mut entries: Vec<(String, PathBuf)> = Vec::new();
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
            .strip_prefix(dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        entries.push((rel, path.to_path_buf()));
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
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
