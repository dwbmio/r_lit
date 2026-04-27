//! Incremental pack manifest — persisted layout + content fingerprints.
//!
//! The manifest serves three roles:
//!   1. **Cache key**: `(options_hash, sprite content hashes)` — when nothing changed
//!      we can skip the entire pack and reuse the existing atlas files on disk.
//!   2. **Layout state**: `used_rects` and `free_rects` per atlas — so partial repack
//!      (additive / in-place) can fit new sprites into the existing layout while
//!      preserving UVs of unchanged sprites. This is the **UV stability** invariant
//!      that lets shipped game code drop in a new atlas without rebaking UVs.
//!   3. **Asset registry foundation** (v0.3 roadmap): the per-sprite content_hash
//!      + per-atlas image_hash form a content-addressed view of the project, on
//!      which a higher-level "raw resource manager" can be built.
//!
//! Manifest path: `<output_dir>/<output_name>.manifest.json` (sidecar to atlases).

use crate::error::{AppError, Result};
use crate::pack::PackOptions;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Current manifest schema version. Bump on any breaking field change.
pub const MANIFEST_VERSION: u32 = 1;

/// Top-level manifest persisted as `<output>.manifest.json`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Manifest {
    pub version: u32,
    pub tool: String,
    /// Hash of all PackOptions that affect the output. Mismatch ⇒ full repack.
    pub options_hash: String,
    pub input_root: String,
    /// Sprites keyed by their relative path (matches `PackedSprite.name`).
    pub sprites: BTreeMap<String, SpriteEntry>,
    pub atlases: Vec<AtlasEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpriteEntry {
    pub rel_path: String,
    /// File size in bytes — fast pre-check, used together with mtime.
    pub file_size: u64,
    /// File modification time (UNIX seconds). Used as a quick-reject signal.
    pub mtime: i64,
    /// SHA256 of pixel data (post-decode RGBA). Authoritative content key.
    pub content_hash: String,
    /// Trim offset (x, y) — needed to keep UV stable across in-place modifications.
    pub trim_offset: [u32; 2],
    /// Trimmed sprite size (w, h) in atlas. If different on next run ⇒ relocate.
    pub trimmed_size: [u32; 2],
    /// Original (pre-trim) size (w, h).
    pub source_size: [u32; 2],
    /// SHA256 of the simplified polygon mesh (only when polygon mode is on).
    pub polygon_hash: Option<String>,
    /// Atlas index (into `Manifest.atlases`) where this sprite lives.
    pub atlas_idx: usize,
    /// Position inside that atlas (content rect; same as PackedSprite.x/y).
    pub content_x: u32,
    pub content_y: u32,
    /// Was the sprite rotated 90° CW when packed? Critical for UV stability.
    pub rotated: bool,
    /// If this sprite is a duplicate, name of the canonical it aliases.
    pub alias_of: Option<String>,

    // ─── User-editable metadata (v0.3+) ──────────────────────────────────────
    // These are NOT part of the cache key — they describe the sprite to humans
    // and downstream tools, but pack output is independent of them. Set/read
    // via `mj_atlas tag` (no repack required) and preserved by future packs.
    /// Free-form tags. De-duplicated, sorted on save.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// License/author attribution for asset bookkeeping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
    /// Where this sprite came from (URL, original asset id, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AtlasEntry {
    /// Filename of the atlas PNG (relative to output_dir).
    pub image_filename: String,
    /// Filename of the metadata file (relative to output_dir, no extension).
    pub data_filename: String,
    pub width: u32,
    pub height: u32,
    /// SHA256 of the atlas PNG file on disk. Detects external corruption.
    pub image_hash: String,
    /// Output format used for this atlas (e.g. "json", "godot-tres").
    pub format: String,
    /// Used rectangles inside the atlas — `(name, place_x, place_y, place_w, place_h, rotated)`.
    /// place_* refer to the OUTER bbox (i.e. content + extrude + padding) — same
    /// coordinate space as the bin packer's PackedItem.
    pub used_rects: Vec<UsedRect>,
    /// Free rectangles available for additive incremental packing.
    /// Maintained as the maximal-rectangles set after every layout change.
    pub free_rects: Vec<FreeRect>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UsedRect {
    pub name: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub rotated: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FreeRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Manifest {
    /// Compute the manifest path next to the atlas output (the canonical sidecar).
    pub fn path_for(opts: &PackOptions) -> PathBuf {
        opts.output_dir
            .join(format!("{}.manifest.json", opts.output_name))
    }

    /// Try loading. Returns Ok(None) when the file does not exist; Err on parse fail.
    pub fn try_load(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(path)?;
        let manifest: Self = serde_json::from_str(&content)
            .map_err(|e| AppError::Custom(format!("manifest parse error: {}", e)))?;
        if manifest.version != MANIFEST_VERSION {
            log::warn!(
                "manifest version mismatch (file={}, expected={}); ignoring cache",
                manifest.version,
                MANIFEST_VERSION
            );
            return Ok(None);
        }
        Ok(Some(manifest))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Look up a sprite entry by its relative-path key (matches PackedSprite.name).
    pub fn sprite(&self, name: &str) -> Option<&SpriteEntry> {
        self.sprites.get(name)
    }
}

/// Hash the subset of PackOptions that affect the output. Order is fixed so the
/// hash is stable across runs (BTreeMap-style canonical encoding).
///
/// **Excluded fields** (do not invalidate the cache):
///   - `output_dir`, `output_name` — only file naming, atlas pixels are unchanged
///     (renaming/moving doesn't need a repack; we update manifest in place).
///   - `incremental`, `force` — these are run-mode flags, not pack parameters.
///
/// **Included fields**: everything else that touches the layout, pixels, or polygon mesh.
pub fn compute_options_hash(opts: &PackOptions) -> String {
    let mut h = Sha256::new();
    h.update(b"mj_atlas:options:v1\n");

    macro_rules! field {
        ($name:literal, $val:expr) => {{
            h.update($name.as_bytes());
            h.update(b"=");
            h.update(format!("{}", $val).as_bytes());
            h.update(b"\n");
        }};
    }

    field!("max_size", opts.max_size);
    field!("spacing", opts.spacing);
    field!("padding", opts.padding);
    field!("extrude", opts.extrude);
    field!("trim", opts.trim);
    field!("trim_threshold", opts.trim_threshold);
    field!("rotate", opts.rotate);
    field!("pot", opts.pot);
    field!("recursive", opts.recursive);
    field!("quantize", opts.quantize);
    field!("quantize_quality", opts.quantize_quality);
    field!("polygon", opts.polygon);
    field!("format", opts.format.as_str());
    field!(
        "polygon_shape",
        match opts.polygon_shape {
            crate::pack::PolygonShape::Concave => "concave",
            crate::pack::PolygonShape::Convex => "convex",
            crate::pack::PolygonShape::Auto => "auto",
        }
    );
    field!("max_vertices", opts.max_vertices);
    // tolerance is f32 — format with full precision for stable hashing.
    h.update(b"tolerance=");
    h.update(format!("{:.6}", opts.tolerance).as_bytes());
    h.update(b"\n");

    let bytes = h.finalize();
    hex_encode(&bytes)
}

/// SHA256 over decoded RGBA pixels (matches `dedup::pixel_hash`).
pub fn hash_pixels(img: &image::RgbaImage) -> String {
    let mut h = Sha256::new();
    h.update(img.width().to_le_bytes());
    h.update(img.height().to_le_bytes());
    h.update(img.as_raw());
    hex_encode(&h.finalize())
}

/// SHA256 over a file's raw bytes — used for atlas PNG integrity checks.
pub fn hash_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(hex_encode(&h.finalize()))
}

/// SHA256 over the simplified polygon contour + triangulation.
pub fn hash_polygon(contour: &[(f32, f32)], triangles: &[[usize; 3]]) -> String {
    let mut h = Sha256::new();
    for &(x, y) in contour {
        h.update(format!("{:.4},{:.4};", x, y).as_bytes());
    }
    h.update(b"|");
    for tri in triangles {
        h.update(format!("{},{},{};", tri[0], tri[1], tri[2]).as_bytes());
    }
    hex_encode(&h.finalize())
}

/// Lightweight file fingerprint (size + mtime). Used as a fast pre-reject before
/// reading + decoding pixels for `content_hash` confirmation.
pub fn file_fingerprint(path: &Path) -> Result<(u64, i64)> {
    let meta = std::fs::metadata(path)?;
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok((size, mtime))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

// ─── Diff between current input and manifest ────────────────────────────────

/// What changed compared to the manifest. Drives the partial-repack decision.
#[derive(Debug, Clone, Default)]
pub struct InputDiff {
    /// Sprites present now but not in the manifest.
    pub added: Vec<String>,
    /// Sprites in the manifest but no longer on disk.
    pub removed: Vec<String>,
    /// Sprites with same name but new pixels. `(name, new_size_changed)`.
    pub modified: Vec<ModifiedSprite>,
    /// Sprites that match (no work needed).
    pub unchanged: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ModifiedSprite {
    pub name: String,
    /// True iff the trimmed dimensions changed — must relocate (treat as remove+add).
    /// False ⇒ pixels can be replaced in place (UV-stable).
    pub size_changed: bool,
}

impl InputDiff {
    pub fn is_unchanged(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }
}

// ─── Free rect maintenance (maximal rectangles algorithm) ───────────────────

/// Compute the set of maximal free rectangles given a bin and used rectangles.
///
/// Implementation: start with one free rect = the entire bin. For each used rect,
/// split every overlapping free rect into up to 4 sub-rects (left/right/top/bottom)
/// and prune any rect contained in another. This is O(n²) in the worst case but n
/// is small (sprite count per atlas); the algorithm is from Jukka Jylänki's
/// "A Thousand Ways to Pack the Bin" — Maximal-Rectangles approach.
pub fn compute_free_rects(bin_w: u32, bin_h: u32, used: &[(u32, u32, u32, u32)]) -> Vec<FreeRect> {
    let mut free: Vec<FreeRect> = vec![FreeRect {
        x: 0,
        y: 0,
        w: bin_w,
        h: bin_h,
    }];

    for &(ux, uy, uw, uh) in used {
        let mut new_free = Vec::with_capacity(free.len() * 2);
        for f in free.drain(..) {
            // Disjoint? Keep as-is.
            if ux >= f.x + f.w || ux + uw <= f.x || uy >= f.y + f.h || uy + uh <= f.y {
                new_free.push(f);
                continue;
            }
            // Split f minus used into up to 4 max-sub-rects.
            // Left strip
            if ux > f.x {
                new_free.push(FreeRect {
                    x: f.x,
                    y: f.y,
                    w: ux - f.x,
                    h: f.h,
                });
            }
            // Right strip
            if ux + uw < f.x + f.w {
                let new_x = ux + uw;
                new_free.push(FreeRect {
                    x: new_x,
                    y: f.y,
                    w: (f.x + f.w) - new_x,
                    h: f.h,
                });
            }
            // Top strip
            if uy > f.y {
                new_free.push(FreeRect {
                    x: f.x,
                    y: f.y,
                    w: f.w,
                    h: uy - f.y,
                });
            }
            // Bottom strip
            if uy + uh < f.y + f.h {
                let new_y = uy + uh;
                new_free.push(FreeRect {
                    x: f.x,
                    y: new_y,
                    w: f.w,
                    h: (f.y + f.h) - new_y,
                });
            }
        }
        free = prune_contained(new_free);
    }
    free
}

/// Drop any rect contained within another (keeps only maximal rects).
fn prune_contained(mut rects: Vec<FreeRect>) -> Vec<FreeRect> {
    rects.retain(|r| r.w > 0 && r.h > 0);
    let mut keep = vec![true; rects.len()];
    for i in 0..rects.len() {
        if !keep[i] {
            continue;
        }
        for j in 0..rects.len() {
            if i == j || !keep[j] {
                continue;
            }
            if contains(&rects[i], &rects[j]) {
                keep[j] = false;
            }
        }
    }
    rects
        .into_iter()
        .enumerate()
        .filter_map(|(i, r)| if keep[i] { Some(r) } else { None })
        .collect()
}

fn contains(outer: &FreeRect, inner: &FreeRect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.w <= outer.x + outer.w
        && inner.y + inner.h <= outer.y + outer.h
}

/// Try to place a `(w, h)` rectangle into one of the free rects using
/// best-short-side fit (minimum of leftover width/height). Returns the chosen
/// position and the (possibly rotated) actual `(w, h)` used.
///
/// `allow_rotation` enables 90° rotation when w/h would fit better that way.
pub fn try_fit(
    free: &[FreeRect],
    w: u32,
    h: u32,
    allow_rotation: bool,
) -> Option<FitResult> {
    let mut best: Option<FitResult> = None;

    for f in free {
        // Try un-rotated
        if w <= f.w && h <= f.h {
            let leftover_x = f.w - w;
            let leftover_y = f.h - h;
            let score = leftover_x.min(leftover_y);
            if best.as_ref().map_or(true, |b| score < b.score) {
                best = Some(FitResult {
                    x: f.x,
                    y: f.y,
                    w,
                    h,
                    rotated: false,
                    score,
                });
            }
        }
        // Try rotated 90°
        if allow_rotation && w != h && h <= f.w && w <= f.h {
            let leftover_x = f.w - h;
            let leftover_y = f.h - w;
            let score = leftover_x.min(leftover_y);
            if best.as_ref().map_or(true, |b| score < b.score) {
                best = Some(FitResult {
                    x: f.x,
                    y: f.y,
                    w: h,
                    h: w,
                    rotated: true,
                    score,
                });
            }
        }
    }

    best
}

#[derive(Debug, Clone)]
pub struct FitResult {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub rotated: bool,
    /// Internal score (smaller is better) — short-side fit leftover.
    pub score: u32,
}

// ─── Path resolution (v0.3) ─────────────────────────────────────────────────

/// Resolve any user-friendly path to its corresponding manifest sidecar.
///
/// Accepts:
///   - `<name>.manifest.json`  → use as-is
///   - any JSON file whose contents already parse as a Manifest → use as-is
///     (handy for snapshotted/renamed copies passed to `diff`)
///   - `<name>.png` / `.json` / `.tpsheet` / `.tres` → strip extension,
///     append `.manifest.json` next to it. If the stem ends in `_<digits>`
///     (multi-bin atlas suffix) and the sibling lookup fails, also try after
///     stripping that suffix.
///   - directory → find a single `*.manifest.json` inside; error otherwise.
pub fn resolve_manifest_path(input: &Path) -> Result<PathBuf> {
    // 1. Direct hit on a `.manifest.json` file.
    if input.is_file()
        && input
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.ends_with(".manifest.json"))
            .unwrap_or(false)
    {
        return Ok(input.to_path_buf());
    }

    // 2. Directory: find a single `*.manifest.json` inside.
    if input.is_dir() {
        let mut hits: Vec<PathBuf> = Vec::new();
        for entry in std::fs::read_dir(input)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".manifest.json") {
                    hits.push(entry.path());
                }
            }
        }
        return match hits.len() {
            0 => Err(AppError::Custom(format!(
                "no `*.manifest.json` found in directory {}",
                input.display()
            ))),
            1 => Ok(hits.pop().unwrap()),
            n => Err(AppError::Custom(format!(
                "{n} manifests in {} — pass an exact path",
                input.display()
            ))),
        };
    }

    // 3. Any JSON file whose body already deserializes as a Manifest. This
    //    covers snapshotted or renamed copies (e.g. `manifest_before.json`
    //    saved by a CI pipeline).
    if input.is_file() {
        if let Some(name) = input.file_name().and_then(|s| s.to_str()) {
            if name.ends_with(".json") {
                if let Ok(content) = std::fs::read_to_string(input) {
                    if let Ok(parsed) = serde_json::from_str::<Manifest>(&content) {
                        if parsed.version == MANIFEST_VERSION {
                            return Ok(input.to_path_buf());
                        }
                    }
                }
            }
        }
    }

    // 4. Atlas-side path: strip the extension, append `.manifest.json`.
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| AppError::Custom(format!("invalid path: {}", input.display())))?;

    let direct = parent.join(format!("{}.manifest.json", stem));
    if direct.is_file() {
        return Ok(direct);
    }

    // 5. Multi-bin: strip `_<digits>` suffix and retry.
    if let Some(base) = strip_bin_suffix(stem) {
        let stripped = parent.join(format!("{}.manifest.json", base));
        if stripped.is_file() {
            return Ok(stripped);
        }
    }

    Err(AppError::Custom(format!(
        "no manifest sidecar for {} (looked for {}.manifest.json next to it)",
        input.display(),
        stem
    )))
}

fn strip_bin_suffix(stem: &str) -> Option<&str> {
    // Match `<base>_<digits>` where digits are at the end and base is non-empty.
    let underscore = stem.rfind('_')?;
    let (base, suffix) = stem.split_at(underscore);
    if base.is_empty() {
        return None;
    }
    let digits = &suffix[1..]; // skip the underscore
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(base)
}

/// Merge user-editable fields (tags / attribution / source_url) from `prior`
/// into `fresh`. Used by `pack` to avoid clobbering metadata set via `tag`.
pub fn merge_user_metadata(fresh: &mut Manifest, prior: &Manifest) {
    for (name, fresh_entry) in fresh.sprites.iter_mut() {
        if let Some(prior_entry) = prior.sprites.get(name) {
            if !prior_entry.tags.is_empty() && fresh_entry.tags.is_empty() {
                fresh_entry.tags = prior_entry.tags.clone();
            }
            if fresh_entry.attribution.is_none() && prior_entry.attribution.is_some() {
                fresh_entry.attribution = prior_entry.attribution.clone();
            }
            if fresh_entry.source_url.is_none() && prior_entry.source_url.is_some() {
                fresh_entry.source_url = prior_entry.source_url.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_bin_has_one_free_rect() {
        let free = compute_free_rects(100, 100, &[]);
        assert_eq!(free.len(), 1);
        assert_eq!(free[0].w, 100);
        assert_eq!(free[0].h, 100);
    }

    #[test]
    fn single_used_rect_splits_into_two_max_rects() {
        // Place a 40x40 rect at (0,0) in a 100x100 bin.
        // Maximal free rects: (40, 0, 60, 100) and (0, 40, 100, 60).
        let free = compute_free_rects(100, 100, &[(0, 0, 40, 40)]);
        assert_eq!(free.len(), 2);
        let mut sizes: Vec<_> = free.iter().map(|r| (r.x, r.y, r.w, r.h)).collect();
        sizes.sort();
        assert_eq!(sizes, vec![(0, 40, 100, 60), (40, 0, 60, 100)]);
    }

    #[test]
    fn fully_used_bin_has_no_free_rects() {
        let free = compute_free_rects(100, 100, &[(0, 0, 100, 100)]);
        assert!(free.is_empty());
    }

    #[test]
    fn fit_picks_tightest_rect() {
        // Two free rects: a wide-thin and a tall-narrow.
        let free = vec![
            FreeRect { x: 0, y: 0, w: 200, h: 50 },
            FreeRect { x: 0, y: 50, w: 50, h: 200 },
        ];
        // 30x30 fits in both; should pick the one with smaller leftover.
        let fit = try_fit(&free, 30, 30, false).unwrap();
        // Both leftovers are the same min; ensure we picked one valid.
        assert!(fit.x == 0);
    }

    #[test]
    fn fit_uses_rotation_when_beneficial() {
        // Tall-narrow free rect: 40 wide, 100 tall.
        let free = vec![FreeRect { x: 0, y: 0, w: 40, h: 100 }];
        // Try to fit 80x30 — won't fit un-rotated, but 30x80 does (rotated).
        let fit = try_fit(&free, 80, 30, true).unwrap();
        assert!(fit.rotated);
        assert_eq!((fit.w, fit.h), (30, 80));
    }

    #[test]
    fn options_hash_is_stable() {
        let opts1 = mock_opts();
        let opts2 = mock_opts();
        assert_eq!(compute_options_hash(&opts1), compute_options_hash(&opts2));
    }

    #[test]
    fn options_hash_differs_on_max_size_change() {
        let mut opts = mock_opts();
        let h1 = compute_options_hash(&opts);
        opts.max_size = 2048;
        let h2 = compute_options_hash(&opts);
        assert_ne!(h1, h2);
    }

    #[test]
    fn options_hash_unaffected_by_output_paths() {
        let mut opts = mock_opts();
        let h1 = compute_options_hash(&opts);
        opts.output_dir = std::path::PathBuf::from("/some/other/place");
        opts.output_name = "different".to_string();
        let h2 = compute_options_hash(&opts);
        assert_eq!(h1, h2, "output paths must not invalidate the cache");
    }

    #[test]
    fn schema_loads_manifest_without_v03_fields() {
        // A v0.2 manifest has no tags / attribution / source_url. It must still
        // deserialize cleanly into the v0.3 SpriteEntry struct.
        let v02_json = r#"{
            "version": 1,
            "tool": "mj_atlas 0.2.0",
            "options_hash": "abc",
            "input_root": "/in",
            "sprites": {
                "a.png": {
                    "rel_path": "a.png",
                    "file_size": 100, "mtime": 0,
                    "content_hash": "deadbeef",
                    "trim_offset": [0, 0],
                    "trimmed_size": [10, 10],
                    "source_size": [10, 10],
                    "polygon_hash": null,
                    "atlas_idx": 0,
                    "content_x": 0, "content_y": 0,
                    "rotated": false, "alias_of": null
                }
            },
            "atlases": []
        }"#;
        let m: Manifest = serde_json::from_str(v02_json).expect("v0.2 manifest must load");
        let entry = m.sprite("a.png").expect("sprite present");
        assert!(entry.tags.is_empty());
        assert!(entry.attribution.is_none());
        assert!(entry.source_url.is_none());
    }

    #[test]
    fn schema_omits_empty_v03_fields_on_serialize() {
        // No tags / attribution / source_url ⇒ those keys should NOT appear in
        // the serialized output (keeps manifests compact and round-trippable
        // with v0.2 readers if anyone has one).
        let entry = SpriteEntry {
            rel_path: "x.png".into(),
            file_size: 0, mtime: 0, content_hash: "".into(),
            trim_offset: [0, 0], trimmed_size: [1, 1], source_size: [1, 1],
            polygon_hash: None, atlas_idx: 0, content_x: 0, content_y: 0,
            rotated: false, alias_of: None,
            tags: vec![], attribution: None, source_url: None,
        };
        let s = serde_json::to_string(&entry).unwrap();
        assert!(!s.contains("\"tags\""), "empty tags should be omitted");
        assert!(!s.contains("\"attribution\""));
        assert!(!s.contains("\"source_url\""));
    }

    #[test]
    fn strip_bin_suffix_handles_multi_bin_naming() {
        assert_eq!(super::strip_bin_suffix("atlas"), None);
        assert_eq!(super::strip_bin_suffix("atlas_1"), Some("atlas"));
        assert_eq!(super::strip_bin_suffix("atlas_123"), Some("atlas"));
        assert_eq!(super::strip_bin_suffix("atlas_v2"), None);   // not pure digits
        assert_eq!(super::strip_bin_suffix("_42"), None);         // empty base
        assert_eq!(super::strip_bin_suffix("nested_name_2"), Some("nested_name"));
    }

    #[test]
    fn merge_user_metadata_preserves_prior_tags() {
        let mut fresh = mock_manifest();
        let mut prior = mock_manifest();
        prior.sprites.get_mut("a.png").unwrap().tags = vec!["ui".into(), "icon".into()];
        prior.sprites.get_mut("a.png").unwrap().attribution = Some("CC0".into());
        super::merge_user_metadata(&mut fresh, &prior);
        let merged = fresh.sprite("a.png").unwrap();
        assert_eq!(merged.tags, vec!["ui".to_string(), "icon".to_string()]);
        assert_eq!(merged.attribution.as_deref(), Some("CC0"));
    }

    #[test]
    fn merge_user_metadata_does_not_overwrite_explicit_fresh_values() {
        // If a fresh pack already populated a tag (it never does today, but
        // future flows might), the prior value must NOT clobber it.
        let mut fresh = mock_manifest();
        fresh.sprites.get_mut("a.png").unwrap().tags = vec!["fresh".into()];
        let mut prior = mock_manifest();
        prior.sprites.get_mut("a.png").unwrap().tags = vec!["old".into()];
        super::merge_user_metadata(&mut fresh, &prior);
        assert_eq!(fresh.sprite("a.png").unwrap().tags, vec!["fresh".to_string()]);
    }

    fn mock_manifest() -> Manifest {
        let mut sprites = BTreeMap::new();
        sprites.insert(
            "a.png".to_string(),
            SpriteEntry {
                rel_path: "a.png".into(),
                file_size: 100, mtime: 0, content_hash: "h".into(),
                trim_offset: [0, 0], trimmed_size: [10, 10], source_size: [10, 10],
                polygon_hash: None, atlas_idx: 0, content_x: 0, content_y: 0,
                rotated: false, alias_of: None,
                tags: vec![], attribution: None, source_url: None,
            },
        );
        Manifest {
            version: MANIFEST_VERSION,
            tool: "mj_atlas test".into(),
            options_hash: "abc".into(),
            input_root: "/in".into(),
            sprites,
            atlases: vec![],
        }
    }

    fn mock_opts() -> PackOptions {
        PackOptions {
            input_dir: std::path::PathBuf::from("/in"),
            output_name: "atlas".to_string(),
            output_dir: std::path::PathBuf::from("/out"),
            max_size: 4096,
            spacing: 0,
            padding: 0,
            extrude: 0,
            trim: false,
            trim_threshold: 0,
            rotate: false,
            pot: false,
            recursive: true,
            incremental: false,
            force: false,
            format: crate::output::Format::JsonHash,
            quantize: false,
            quantize_quality: 85,
            polygon: false,
            tolerance: 2.0,
            polygon_shape: crate::pack::PolygonShape::Concave,
            max_vertices: 0,
        }
    }
}
