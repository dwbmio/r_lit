//! Block metadata + artifact-backed block library.
//!
//! Maquette's "block" was, until v0.10 C-2, a fixed `Cube | Sphere`
//! shape enum hanging off [`crate::grid::Cell::shape`]. That captures
//! geometry but nothing else — no name, no description, no
//! provider-specific texture hint, no provenance. As soon as the AI
//! texture pipeline (v0.10 B-bis / C-1) landed it became obvious we
//! need a richer "what *is* this block?" abstraction so downstream
//! systems (texgen prompt building, GUI block library, future glTF
//! material baking) all read from the same source.
//!
//! ## Design
//!
//! A [`BlockMeta`] is a piece of plain data with a stable `id` (e.g.
//! `"grass"`, `"oak_planks"`), a human-facing name + description, a
//! preferred [`crate::grid::ShapeKind`], a fallback color, and a free-form
//! `texture_hint` that the texgen pipeline feeds straight into the
//! worker's `prompt` field. Every `BlockMeta` records a
//! [`BlockMetaSource`] — either `Local` (built into the binary) or
//! `Hfrog { … }` (pulled from the artifact server) — so the GUI can
//! visually distinguish what came from where, and so a project file
//! that references a block can tell whether it's locally available
//! or needs a sync.
//!
//! The artifact server side is the [`hfrog`][hfrog] backend the user
//! already runs (typically `https://starlink.youxi123.com/hfrog`).
//! Maquette is a *consumer*: list, find, optionally download a
//! pre-rendered PNG payload. We never upload — publishing new blocks
//! happens through hfrog's own ops tooling.
//!
//! [hfrog]: /Users/admin/data0/public_work/hfrog/
//!
//! ## Provider trait
//!
//! [`BlockMetaProvider`] is sync on purpose: same reasoning as
//! [`crate::texgen::TextureProvider`] — keep `lib` tokio-free, let
//! Bevy systems offload to `AsyncComputeTaskPool` when they need
//! to. CLI calls flow straight-line.
//!
//! ## Disk cache
//!
//! Local blocks have nothing to cache — they're `const`. Hfrog blocks
//! are cached at `~/.cache/maquette/blocks/hfrog/<runtime>/<id>.json`,
//! and pre-rendered PNG payloads sit next to them as `<id>.png`. Cache
//! reads happen straight from disk, no JSON-deserialize-on-every-call
//! penalty; cache writes are atomic via `<id>.json.tmp` → rename.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use bevy::prelude::Color;
use serde::{Deserialize, Serialize};

use crate::grid::ShapeKind;

// ---------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------

/// Hfrog `runtime` field for Maquette blocks. Single namespace at v1;
/// when the schema changes (e.g. `texture_hint` becomes structured),
/// bump to `maquette-block/v2` so v1 clients can ignore the new
/// records.
pub const HFROG_RUNTIME: &str = "maquette-block/v1";

/// Default hfrog server. Overridable via `MAQUETTE_HFROG_BASE_URL`.
pub const DEFAULT_HFROG_BASE_URL: &str = "https://starlink.youxi123.com/hfrog";

/// HTTP timeout (seconds) for any single hfrog request. Hfrog's
/// `/list` is paged and bounded; if it takes more than this we'd
/// rather show an error than a stuck spinner.
pub const HFROG_REQUEST_TIMEOUT_SECS: u64 = 15;

// ---------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------

/// Where a `BlockMeta` came from. Persisted on the project file so
/// that closing and reopening a project preserves the provenance —
/// the GUI uses this to badge entries in the Block Library and to
/// decide whether `Sync` should re-fetch them.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BlockMetaSource {
    /// Bundled with the Maquette binary. Cannot be edited from the
    /// GUI; if a user wants a custom one they publish it to hfrog.
    Local,
    /// User-authored draft sitting in the local block-draft cache
    /// (`~/.cache/maquette/blocks/local-drafts/<id>.json` plus a
    /// sibling `.png`). Drafts come out of the Block Composer
    /// window's "Save Local Draft" path. They show up in the main
    /// Block Library exactly like local + hfrog blocks, but with a
    /// `draft` badge so the user can tell which ones are still
    /// theirs to publish.
    LocalDraft {
        /// UNIX epoch second when the draft was first saved.
        #[serde(default)]
        created_at: i64,
    },
    /// Pulled from a hfrog artifact server. The triplet
    /// `(name, ver, runtime)` is the logical identifier on the
    /// server side; `pid` is the row id (useful for the
    /// `/get_object_presigned_url?id=` endpoint), `md5` is the
    /// content checksum so we can detect stale caches, and
    /// `fetched_at` is the UNIX epoch second when we last pulled
    /// it (used by future TTL eviction).
    Hfrog {
        pid: i32,
        name: String,
        ver: String,
        md5: String,
        #[serde(default)]
        fetched_at: i64,
    },
}

impl BlockMetaSource {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::LocalDraft { .. } => "draft",
            Self::Hfrog { .. } => "hfrog",
        }
    }
}

/// Compact, sRGB color (0..1 floats). Stored on disk as four floats
/// rather than `bevy::prelude::Color` because `Color` is not
/// guaranteed wire-stable across Bevy major versions, and
/// `BlockMeta` records will live in hfrog's database long after a
/// given Bevy version is gone.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct RgbaColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    #[serde(default = "RgbaColor::default_alpha")]
    pub a: f32,
}

impl RgbaColor {
    /// Construct an opaque sRGB color (alpha = 1). `const fn` so
    /// the [`LocalProvider`] table can keep it in `&[…]` form
    /// without runtime work, and a const so callers building
    /// fixture data in tests don't pay heap cost.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }
    fn default_alpha() -> f32 {
        1.0
    }
    pub fn to_color(self) -> Color {
        Color::srgba(self.r, self.g, self.b, self.a)
    }
    pub fn from_color(c: Color) -> Self {
        let s = c.to_srgba();
        Self {
            r: s.red,
            g: s.green,
            b: s.blue,
            a: s.alpha,
        }
    }
}

impl Eq for RgbaColor {}

/// A block definition. `id` is the stable, slug-style identifier
/// (`grass` / `oak_planks` / `iron_block`). Don't use spaces, dots, or
/// slashes — anything that would be awkward as a filename or query
/// parameter, since `id` is both.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockMeta {
    /// Stable identifier. Lowercase + underscores, [a-z0-9_]. Used
    /// as cache filename and as the foreign key from a palette slot
    /// (`PaletteSlotMeta::block_id`).
    pub id: String,
    /// Human-facing label. ASCII or any Unicode is fine — the GUI
    /// just renders it.
    pub name: String,
    /// One-or-two-sentence description shown in the Block Library
    /// hover card. Doubles as a fallback prompt seed when the user
    /// hasn't set a `model_description` and there's no
    /// `texture_hint`.
    pub description: String,
    /// Preferred shape for cells using this block. The user can still
    /// right-click cycle to anything else; this is just the *default*
    /// shape new cells of this slot get when first painted.
    #[serde(default)]
    pub shape_hint: ShapeKind,
    /// Fallback color used when no texture is bound. Kept distinct
    /// from the eventual rendered texture so "Flat" view in
    /// `TexturePrefs` always has a sensible color to show.
    pub default_color: RgbaColor,
    /// Free-form prompt fragment fed to the texgen worker. Should
    /// describe the surface visually — "patchy grass over compact
    /// dirt, top-down lighting" — *not* the abstract role
    /// ("ground"). Stays empty when the block is generic enough
    /// that `description` is sufficient.
    #[serde(default)]
    pub texture_hint: String,
    /// Human-readable tags for filtering ("nature", "metal",
    /// "translucent", …). The Block Library groups by these.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Where this record came from. See [`BlockMetaSource`].
    pub source: BlockMetaSource,
    /// Optional: hfrog `s3_key` of a pre-rendered 128×128 PNG
    /// preview. When present, the GUI shows it as the block's
    /// thumbnail instead of a flat color swatch. Only meaningful
    /// for `BlockMetaSource::Hfrog`; local blocks bake-in
    /// thumbnails through the binary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_s3_key: Option<String>,
}

impl BlockMeta {
    /// Convenience constructor for tests: a local block from string
    /// slices. (`String::from` isn't `const`, so it can't be const
    /// fn; production code paths use the iterator builder in
    /// `LocalProvider::blocks` directly.)
    pub fn new_local(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        shape: ShapeKind,
        color: RgbaColor,
        texture_hint: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            shape_hint: shape,
            default_color: color,
            texture_hint: texture_hint.into(),
            tags: Vec::new(),
            source: BlockMetaSource::Local,
            preview_s3_key: None,
        }
    }

    pub fn label(&self) -> &str {
        if !self.name.is_empty() {
            &self.name
        } else {
            &self.id
        }
    }
}

// ---------------------------------------------------------------------
// Provider trait + errors
// ---------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum BlockMetaError {
    #[error("block id `{0}` not found")]
    NotFound(String),
    #[error("network or remote-API failure: {0}")]
    Remote(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed payload: {0}")]
    Decode(String),
}

/// Sync data source for [`BlockMeta`] records. See module docs.
pub trait BlockMetaProvider: Send + Sync {
    /// Stable identifier surfaced in logs / `--source` flags.
    /// Must be lowercase, kebab-case-ish: `local`, `hfrog`.
    fn name(&self) -> &'static str;

    /// All blocks the provider knows about. Order is not specified
    /// — callers that care about display order should sort.
    fn list(&self) -> Result<Vec<BlockMeta>, BlockMetaError>;

    /// Look up a single block by its `id`. Returns
    /// [`BlockMetaError::NotFound`] if the provider doesn't have it.
    fn get(&self, id: &str) -> Result<BlockMeta, BlockMetaError>;
}

// ---------------------------------------------------------------------
// LocalProvider — built-in 12-block set
// ---------------------------------------------------------------------

/// Bundle of blocks that ship inside the Maquette binary. Twelve of
/// them, deliberately matching the default palette (red / orange /
/// yellow / green / sky / blue / purple / sand / brown / slate / bone /
/// moss) so a fresh project can bind block to slot one-to-one
/// without thinking. Order matches the default `Palette::default()`
/// slot order.
pub struct LocalProvider;

impl LocalProvider {
    pub const fn new() -> Self {
        Self
    }

    /// All built-in blocks. Cloned out per-call rather than handed
    /// out as `&'static [BlockMeta]` because `BlockMeta`'s
    /// `String` fields aren't `const`; the cost is twelve heap
    /// allocations on a list call, which we measure in microseconds.
    pub fn blocks() -> Vec<BlockMeta> {
        // Tuples: (id, name, description, shape, color, texture_hint, tags)
        // Colors mirror Palette::default() one-to-one to make
        // "auto-bind block N to slot N" a sensible default.
        type LocalSpec = (
            &'static str,
            &'static str,
            &'static str,
            ShapeKind,
            RgbaColor,
            &'static str,
            &'static [&'static str],
        );
        let raw: &[LocalSpec] = &[
            (
                "brick",
                "红砖块 / Red Brick",
                "经典红砖墙的一面：横向砌缝、轻微斑驳。可贴在墙体或烟囱上。",
                ShapeKind::Cube,
                RgbaColor::rgb(0.90, 0.30, 0.35),
                "weathered red brick wall, horizontal mortar lines, low-poly",
                &["building", "wall", "warm"],
            ),
            (
                "lava",
                "岩浆块 / Lava",
                "炽热流动的岩浆，深红橙色调。点缀点亮深色场景。",
                ShapeKind::Cube,
                RgbaColor::rgb(0.95, 0.60, 0.25),
                "glowing molten lava, soft cracks, low-poly emissive",
                &["liquid", "warm", "emissive"],
            ),
            (
                "sand",
                "沙地块 / Sand",
                "沙漠里那种细沙颗粒感的地面，浅金黄色。",
                ShapeKind::Cube,
                RgbaColor::rgb(0.95, 0.85, 0.35),
                "fine grain desert sand, top-down soft shadows, low-poly",
                &["ground", "natural", "warm"],
            ),
            (
                "grass",
                "草地块 / Grass",
                "草地顶面：杂乱草丛、零星深浅。最经典的 Minecraft 风格地表。",
                ShapeKind::Cube,
                RgbaColor::rgb(0.45, 0.80, 0.40),
                "patchy minecraft-style grass top, slight color variance, low-poly",
                &["ground", "natural"],
            ),
            (
                "ice",
                "冰块 / Ice",
                "半透明的浅蓝冰，有微小裂纹。寒冷场景常用。",
                ShapeKind::Cube,
                RgbaColor::rgb(0.35, 0.70, 0.90),
                "translucent pale blue ice, fine cracks, low-poly stylised",
                &["natural", "cool", "translucent"],
            ),
            (
                "water",
                "水面块 / Water",
                "深蓝海面：细微波纹、轻微高光。河湖与海洋通用。",
                ShapeKind::Cube,
                RgbaColor::rgb(0.30, 0.45, 0.85),
                "calm deep blue water surface with gentle ripples, low-poly",
                &["liquid", "natural", "cool"],
            ),
            (
                "amethyst",
                "紫晶块 / Amethyst",
                "紫水晶簇：朝外的尖锐多面体晶体丛。装饰性极强。",
                ShapeKind::Cube,
                RgbaColor::rgb(0.65, 0.40, 0.85),
                "amethyst crystal cluster, faceted purple gems, low-poly",
                &["crystal", "decoration", "magic"],
            ),
            (
                "wood",
                "木板块 / Oak Planks",
                "橡木地板：纹理顺直、暖色调。建筑常用。",
                ShapeKind::Cube,
                RgbaColor::rgb(0.90, 0.75, 0.65),
                "warm oak wood planks, straight grain, low-poly stylised",
                &["building", "natural", "warm"],
            ),
            (
                "dirt",
                "泥土块 / Dirt",
                "深棕色泥土，少量小石子和根须穿插。配合 grass 顶面构成草地块体。",
                ShapeKind::Cube,
                RgbaColor::rgb(0.50, 0.35, 0.25),
                "rich brown dirt with scattered pebbles and small roots, low-poly",
                &["ground", "natural"],
            ),
            (
                "stone",
                "石块 / Stone",
                "灰色岩石，自然风化的纹理。山体洞穴墙体百搭。",
                ShapeKind::Cube,
                RgbaColor::rgb(0.25, 0.25, 0.30),
                "weathered grey stone, organic texture, low-poly",
                &["building", "natural", "cool"],
            ),
            (
                "bone",
                "骨块 / Bone Block",
                "象牙白：略偏冷的骨质表面，沟壑细密。沙漠遗迹常用。",
                ShapeKind::Cube,
                RgbaColor::rgb(0.85, 0.85, 0.90),
                "ivory bone block surface with thin channels, low-poly",
                &["building", "decoration", "decay"],
            ),
            (
                "moss",
                "苔藓块 / Moss",
                "湿润苔藓覆盖的石块表面：浅绿+绿+暗绿斑驳。",
                ShapeKind::Cube,
                RgbaColor::rgb(0.55, 0.75, 0.55),
                "damp moss-covered surface, layered greens, low-poly stylised",
                &["natural", "decay", "cool"],
            ),
        ];

        raw.iter()
            .map(|(id, name, desc, shape, color, hint, tags)| BlockMeta {
                id: (*id).to_string(),
                name: (*name).to_string(),
                description: (*desc).to_string(),
                shape_hint: *shape,
                default_color: *color,
                texture_hint: (*hint).to_string(),
                tags: tags.iter().map(|s| (*s).to_string()).collect(),
                source: BlockMetaSource::Local,
                preview_s3_key: None,
            })
            .collect()
    }
}

impl Default for LocalProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockMetaProvider for LocalProvider {
    fn name(&self) -> &'static str {
        "local"
    }

    fn list(&self) -> Result<Vec<BlockMeta>, BlockMetaError> {
        Ok(Self::blocks())
    }

    fn get(&self, id: &str) -> Result<BlockMeta, BlockMetaError> {
        Self::blocks()
            .into_iter()
            .find(|b| b.id == id)
            .ok_or_else(|| BlockMetaError::NotFound(id.to_string()))
    }
}

// ---------------------------------------------------------------------
// Disk cache (used by HfrogProvider — local has nothing to cache)
// ---------------------------------------------------------------------

/// Where we cache hfrog-pulled blocks. Honours `XDG_CACHE_HOME` first,
/// then `$HOME/.cache`. Returns `None` only if neither is set, which
/// on a normal desktop is unusual; CLI honours `--no-cache` and tests
/// inject explicit dirs.
pub fn default_cache_dir() -> Option<PathBuf> {
    let base = if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            PathBuf::from(xdg)
        } else {
            return cache_dir_from_home();
        }
    } else {
        return cache_dir_from_home();
    };
    Some(base.join("maquette").join("blocks"))
}

fn cache_dir_from_home() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    if home.is_empty() {
        return None;
    }
    Some(
        PathBuf::from(home)
            .join(".cache")
            .join("maquette")
            .join("blocks"),
    )
}

fn cache_path_for(cache_dir: &Path, source: &str, runtime: &str, id: &str, ext: &str) -> PathBuf {
    cache_dir
        .join(source)
        .join(runtime)
        .join(format!("{id}.{ext}"))
}

/// Persist a `BlockMeta` JSON record to the cache. Atomic via tmp +
/// rename so a half-written file never poisons the cache.
pub fn cache_put_meta(
    cache_dir: &Path,
    runtime: &str,
    meta: &BlockMeta,
) -> std::io::Result<PathBuf> {
    let dir = cache_dir.join(meta.source.label()).join(runtime);
    fs::create_dir_all(&dir)?;
    let final_path = dir.join(format!("{}.json", meta.id));
    let tmp_path = dir.join(format!("{}.json.tmp", meta.id));
    {
        let mut f = fs::File::create(&tmp_path)?;
        let json = serde_json::to_vec_pretty(meta).map_err(io_err)?;
        f.write_all(&json)?;
        f.sync_all()?;
    }
    fs::rename(&tmp_path, &final_path)?;
    log::debug!("block_meta: cache put {}", final_path.display());
    Ok(final_path)
}

/// Read every cached block under `cache_dir/<source>/<runtime>/`.
/// Skips entries that fail to deserialize (logs a warning) so a
/// single bad file doesn't make the whole list call fail.
pub fn cache_list(
    cache_dir: &Path,
    source: &str,
    runtime: &str,
) -> std::io::Result<Vec<BlockMeta>> {
    let dir = cache_dir.join(source).join(runtime);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        match fs::read(&path).and_then(|b| serde_json::from_slice(&b).map_err(io_err)) {
            Ok(meta) => out.push(meta),
            Err(e) => log::warn!(
                "block_meta: skipping malformed cache entry {}: {e}",
                path.display()
            ),
        }
    }
    Ok(out)
}

/// Persist a block's pre-rendered PNG payload alongside its meta.
pub fn cache_put_png(
    cache_dir: &Path,
    source: &str,
    runtime: &str,
    id: &str,
    bytes: &[u8],
) -> std::io::Result<PathBuf> {
    let dir = cache_dir.join(source).join(runtime);
    fs::create_dir_all(&dir)?;
    let final_path = dir.join(format!("{id}.png"));
    let tmp_path = dir.join(format!("{id}.png.tmp"));
    {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp_path, &final_path)?;
    Ok(final_path)
}

/// Look up a cached PNG payload. Returns `Ok(None)` on miss.
pub fn cache_get_png(
    cache_dir: &Path,
    source: &str,
    runtime: &str,
    id: &str,
) -> std::io::Result<Option<Vec<u8>>> {
    let path = cache_path_for(cache_dir, source, runtime, id, "png");
    match fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn io_err<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
}

// ---------------------------------------------------------------------
// Tests (LocalProvider + cache; HfrogProvider tests are below in its
// own submodule because the type is bigger)
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn local_provider_lists_twelve_blocks() {
        let p = LocalProvider::new();
        let list = p.list().unwrap();
        assert_eq!(list.len(), 12, "default block library is 12 entries");
        for meta in &list {
            assert!(!meta.id.is_empty());
            assert!(!meta.name.is_empty());
            assert!(matches!(meta.source, BlockMetaSource::Local));
        }
    }

    #[test]
    fn local_provider_ids_are_unique_lowercase_underscore_only() {
        let mut seen = std::collections::HashSet::new();
        for meta in LocalProvider::blocks() {
            assert!(
                seen.insert(meta.id.clone()),
                "duplicate id: {}",
                meta.id
            );
            assert!(
                meta.id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "id `{}` contains illegal chars (must be [a-z0-9_])",
                meta.id
            );
        }
    }

    #[test]
    fn local_provider_get_returns_known_id_or_not_found() {
        let p = LocalProvider::new();
        let g = p.get("grass").unwrap();
        assert_eq!(g.id, "grass");
        assert!(matches!(p.get("does_not_exist"), Err(BlockMetaError::NotFound(_))));
    }

    #[test]
    fn block_meta_round_trips_through_serde() {
        let p = LocalProvider::new();
        let original = p.get("oak_planks").or_else(|_| p.get("wood")).unwrap();
        let s = serde_json::to_string(&original).unwrap();
        let back: BlockMeta = serde_json::from_str(&s).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn block_meta_source_serializes_with_kind_tag() {
        // `kind: "local"` shows up in the wire format. This is not
        // just visual — the GUI's library view filters by it.
        let json = serde_json::to_string(&BlockMetaSource::Local).unwrap();
        assert!(json.contains("\"kind\":\"local\""), "got {json}");

        let h = BlockMetaSource::Hfrog {
            pid: 42,
            name: "stone".into(),
            ver: "1.0.0".into(),
            md5: "abc".into(),
            fetched_at: 1_700_000_000,
        };
        let json = serde_json::to_string(&h).unwrap();
        assert!(json.contains("\"kind\":\"hfrog\""));
        assert!(json.contains("\"pid\":42"));
    }

    #[test]
    fn block_meta_old_payload_without_fetched_at_still_loads() {
        // Forward-compat: hfrog records written before
        // `fetched_at` was added must still deserialize. `serde
        // default = 0` covers it.
        let raw = r#"{"kind":"hfrog","pid":1,"name":"x","ver":"0.1","md5":""}"#;
        let parsed: BlockMetaSource = serde_json::from_str(raw).unwrap();
        match parsed {
            BlockMetaSource::Hfrog { fetched_at, .. } => assert_eq!(fetched_at, 0),
            _ => panic!("expected Hfrog variant"),
        }
    }

    #[test]
    fn rgba_color_round_trip_through_bevy_color() {
        let original = RgbaColor::rgb(0.45, 0.80, 0.40);
        let bevy = original.to_color();
        let back = RgbaColor::from_color(bevy);
        assert!((back.r - original.r).abs() < 1e-5);
        assert!((back.g - original.g).abs() < 1e-5);
        assert!((back.b - original.b).abs() < 1e-5);
        assert!((back.a - original.a).abs() < 1e-5);
    }

    #[test]
    fn cache_put_then_list_round_trip() {
        let dir = tempdir().unwrap();
        let blocks = LocalProvider::blocks();
        // Fake them as hfrog-sourced for cache test purposes (the
        // `local` source is tested above via direct LocalProvider
        // calls — the cache is *for* hfrog).
        for mut b in blocks.into_iter().take(3) {
            b.source = BlockMetaSource::Hfrog {
                pid: 1,
                name: b.id.clone(),
                ver: "0.1.0".into(),
                md5: "x".into(),
                fetched_at: 0,
            };
            cache_put_meta(dir.path(), HFROG_RUNTIME, &b).unwrap();
        }
        let listed = cache_list(dir.path(), "hfrog", HFROG_RUNTIME).unwrap();
        assert_eq!(listed.len(), 3);
    }

    #[test]
    fn cache_list_skips_malformed_files() {
        let dir = tempdir().unwrap();
        let bad_dir = dir.path().join("hfrog").join(HFROG_RUNTIME);
        fs::create_dir_all(&bad_dir).unwrap();
        fs::write(bad_dir.join("garbage.json"), b"{not json").unwrap();
        // Single good entry alongside.
        let mut good = LocalProvider::blocks().into_iter().next().unwrap();
        good.source = BlockMetaSource::Hfrog {
            pid: 1,
            name: good.id.clone(),
            ver: "0.1.0".into(),
            md5: "".into(),
            fetched_at: 0,
        };
        cache_put_meta(dir.path(), HFROG_RUNTIME, &good).unwrap();

        let listed = cache_list(dir.path(), "hfrog", HFROG_RUNTIME).unwrap();
        assert_eq!(listed.len(), 1, "only the good entry should survive");
    }

    #[test]
    fn cache_get_png_misses_and_hits() {
        let dir = tempdir().unwrap();
        assert!(cache_get_png(dir.path(), "hfrog", HFROG_RUNTIME, "x")
            .unwrap()
            .is_none());
        cache_put_png(dir.path(), "hfrog", HFROG_RUNTIME, "x", b"\x89PNG\r\n\x1a\n").unwrap();
        let got = cache_get_png(dir.path(), "hfrog", HFROG_RUNTIME, "x")
            .unwrap()
            .unwrap();
        assert_eq!(&got[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn new_local_helper_constructs_a_valid_block() {
        // `BlockMeta::new_local` is a convenience constructor used
        // by tests and external code that wants to register a
        // synthetic block (e.g. unit tests on consumers). Pin
        // its shape so the API doesn't accidentally regress.
        let m = BlockMeta::new_local(
            "x",
            "X",
            "x desc",
            ShapeKind::Cube,
            RgbaColor::rgb(0.0, 0.0, 0.0),
            "x hint",
        );
        assert_eq!(m.id, "x");
        assert_eq!(m.name, "X");
        assert_eq!(m.description, "x desc");
        assert_eq!(m.texture_hint, "x hint");
        assert!(matches!(m.source, BlockMetaSource::Local));
        assert!(matches!(m.shape_hint, ShapeKind::Cube));
    }
}

// =====================================================================
// HfrogProvider — HTTP client for the artifact server
// =====================================================================

/// HFrog HTTP client.
pub mod hfrog {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    use super::{
        cache_get_png, cache_list, cache_path_for, cache_put_meta, cache_put_png,
        BlockMeta, BlockMetaError, BlockMetaProvider, BlockMetaSource, RgbaColor,
        DEFAULT_HFROG_BASE_URL, HFROG_REQUEST_TIMEOUT_SECS, HFROG_RUNTIME,
    };
    use crate::grid::ShapeKind;

    // -----------------------------------------------------------------
    // Config
    // -----------------------------------------------------------------

    /// Configuration for [`HfrogProvider`]. Constructed via env vars
    /// for typical use, or directly for tests.
    ///
    /// | env var                     | default                                  |
    /// |-----------------------------|------------------------------------------|
    /// | `MAQUETTE_HFROG_BASE_URL`   | `https://starlink.youxi123.com/hfrog`     |
    /// | `MAQUETTE_HFROG_RUNTIME`    | `maquette-block/v1`                       |
    /// | `MAQUETTE_HFROG_TIMEOUT_SECS`| `15`                                      |
    #[derive(Debug, Clone)]
    pub struct HfrogConfig {
        /// Base URL **including** the `/hfrog` path component if the
        /// server is mounted under a sub-path. Sample values:
        /// `https://starlink.youxi123.com/hfrog`,
        /// `http://localhost:12121` (when run bare-host).
        pub base_url: String,
        /// hfrog `runtime` field — query namespace.
        pub runtime: String,
        /// HTTP request timeout for any single call.
        pub timeout_secs: u64,
    }

    impl Default for HfrogConfig {
        fn default() -> Self {
            Self {
                base_url: DEFAULT_HFROG_BASE_URL.to_string(),
                runtime: HFROG_RUNTIME.to_string(),
                timeout_secs: HFROG_REQUEST_TIMEOUT_SECS,
            }
        }
    }

    impl HfrogConfig {
        pub fn from_env() -> Self {
            let base_url = std::env::var("MAQUETTE_HFROG_BASE_URL")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| DEFAULT_HFROG_BASE_URL.to_string());
            let runtime = std::env::var("MAQUETTE_HFROG_RUNTIME")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| HFROG_RUNTIME.to_string());
            let timeout_secs = std::env::var("MAQUETTE_HFROG_TIMEOUT_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(HFROG_REQUEST_TIMEOUT_SECS);
            Self {
                base_url: base_url.trim_end_matches('/').to_string(),
                runtime,
                timeout_secs,
            }
        }
    }

    // -----------------------------------------------------------------
    // Provider
    // -----------------------------------------------------------------

    /// Talk to an [hfrog][crate::block_meta] artifact server for
    /// Maquette block records. Read-only — uploads happen through
    /// hfrog's own ops tooling.
    pub struct HfrogProvider {
        cfg: HfrogConfig,
        cache_dir: Option<PathBuf>,
    }

    impl HfrogProvider {
        pub fn new(cfg: HfrogConfig) -> Self {
            Self {
                cfg,
                cache_dir: super::default_cache_dir(),
            }
        }
        /// Override the cache directory (tests / ops tools).
        pub fn with_cache_dir(mut self, dir: Option<PathBuf>) -> Self {
            self.cache_dir = dir;
            self
        }

        pub fn config(&self) -> &HfrogConfig {
            &self.cfg
        }

        /// Re-fetch every Maquette block from the server, persist
        /// to disk cache, and return the freshly-pulled list.
        /// Network failures bubble up as
        /// [`BlockMetaError::Remote`].
        pub fn sync(&self) -> Result<Vec<BlockMeta>, BlockMetaError> {
            let blocks = self.list_remote()?;
            if let Some(dir) = &self.cache_dir {
                for b in &blocks {
                    if let Err(e) = cache_put_meta(dir, &self.cfg.runtime, b) {
                        log::warn!("block_meta: cache_put_meta failed for {}: {e}", b.id);
                    }
                }
            }
            log::info!("hfrog: synced {} blocks", blocks.len());
            Ok(blocks)
        }

        /// Hit `/list?runtime=…` against hfrog. Public only because
        /// the CLI's `block sync` exposes it as a verb; downstream
        /// callers usually want [`Self::sync`] which also caches.
        pub fn list_remote(&self) -> Result<Vec<BlockMeta>, BlockMetaError> {
            let url = format!(
                "{}/api/artifactory/list?runtime={}",
                self.cfg.base_url,
                urlencode(&self.cfg.runtime)
            );
            log::debug!("hfrog: GET {url}");
            let body = self.http_get(&url)?;
            let envelope: HfrogListEnvelope = serde_json::from_str(&body).map_err(|e| {
                BlockMetaError::Decode(format!("list response: {e} body={body}"))
            })?;
            envelope.into_blocks(&self.cfg.runtime)
        }

        fn http_get(&self, url: &str) -> Result<String, BlockMetaError> {
            let req = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(self.cfg.timeout_secs))
                .build()
                .get(url);
            req.call()
                .map_err(|e| BlockMetaError::Remote(format!("GET {url}: {e}")))?
                .into_string()
                .map_err(|e| BlockMetaError::Remote(format!("read body {url}: {e}")))
        }

        /// Resolve a presigned download URL for the artifact behind
        /// `meta`. Returns `None` if the meta has no `preview_s3_key`
        /// (i.e. the server doesn't ship a pre-rendered preview).
        pub fn resolve_preview_url(
            &self,
            meta: &BlockMeta,
        ) -> Result<Option<String>, BlockMetaError> {
            let pid = match &meta.source {
                BlockMetaSource::Hfrog { pid, .. } => *pid,
                BlockMetaSource::Local | BlockMetaSource::LocalDraft { .. } => {
                    // Drafts and bundled blocks live entirely
                    // client-side; there's nothing to resolve.
                    return Ok(None);
                }
            };
            if meta.preview_s3_key.is_none() {
                return Ok(None);
            }
            let url = format!(
                "{}/api/artifactory/get_object_presigned_url?id={pid}&action=Download",
                self.cfg.base_url
            );
            let body = self.http_get(&url)?;
            let v: Value = serde_json::from_str(&body).map_err(|e| {
                BlockMetaError::Decode(format!("presigned response: {e} body={body}"))
            })?;
            let pre = v
                .get("data")
                .and_then(|d| d.get("url"))
                .and_then(|u| u.as_str());
            Ok(pre.map(|s| s.to_string()))
        }

        /// Download the pre-rendered PNG bytes for `meta`, if any,
        /// caching the result on disk. Returns `Ok(None)` when no
        /// preview is registered for this block.
        pub fn fetch_preview_png(
            &self,
            meta: &BlockMeta,
        ) -> Result<Option<Vec<u8>>, BlockMetaError> {
            // Cache hit?
            if let Some(dir) = &self.cache_dir {
                if let Some(bytes) = cache_get_png(dir, "hfrog", &self.cfg.runtime, &meta.id)? {
                    return Ok(Some(bytes));
                }
            }
            let Some(url) = self.resolve_preview_url(meta)? else {
                return Ok(None);
            };
            let bytes = self.http_get_bytes(&url)?;
            if let Some(dir) = &self.cache_dir {
                if let Err(e) = cache_put_png(dir, "hfrog", &self.cfg.runtime, &meta.id, &bytes) {
                    log::warn!("hfrog: cache_put_png failed for {}: {e}", meta.id);
                }
            }
            Ok(Some(bytes))
        }

        fn http_get_bytes(&self, url: &str) -> Result<Vec<u8>, BlockMetaError> {
            let resp = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(self.cfg.timeout_secs))
                .build()
                .get(url)
                .call()
                .map_err(|e| BlockMetaError::Remote(format!("GET {url}: {e}")))?;
            let mut buf = Vec::new();
            resp.into_reader()
                .read_to_end(&mut buf)
                .map_err(BlockMetaError::Io)?;
            Ok(buf)
        }
    }

    impl BlockMetaProvider for HfrogProvider {
        fn name(&self) -> &'static str {
            "hfrog"
        }

        /// First tries the disk cache. Falls back to a network call
        /// if the cache directory is empty (or `MAQUETTE_NO_CACHE=1`
        /// — the same env var `texgen` honours). Falls back to an
        /// empty list (not an error) if the network is unreachable
        /// — callers that want strict failure should use
        /// [`HfrogProvider::sync`] / [`Self::list_remote`] directly.
        fn list(&self) -> Result<Vec<BlockMeta>, BlockMetaError> {
            let no_cache = std::env::var("MAQUETTE_NO_CACHE").as_deref() == Ok("1");
            if !no_cache {
                if let Some(dir) = &self.cache_dir {
                    let cached = cache_list(dir, "hfrog", &self.cfg.runtime).unwrap_or_default();
                    if !cached.is_empty() {
                        log::debug!("hfrog: list served from cache ({} entries)", cached.len());
                        return Ok(cached);
                    }
                }
            }
            self.list_remote()
        }

        fn get(&self, id: &str) -> Result<BlockMeta, BlockMetaError> {
            // Cache lookup first.
            if let Some(dir) = &self.cache_dir {
                let path = cache_path_for(dir, "hfrog", &self.cfg.runtime, id, "json");
                if path.exists() {
                    let bytes = std::fs::read(&path).map_err(BlockMetaError::Io)?;
                    let meta: BlockMeta = serde_json::from_slice(&bytes)
                        .map_err(|e| BlockMetaError::Decode(format!("{e} at {path:?}")))?;
                    return Ok(meta);
                }
            }
            // Fall back to /find?name=&runtime=. We search by `name`
            // matching `id` per convention (see HfrogListEnvelope's
            // mapping).
            let url = format!(
                "{}/api/artifactory/find?name={}&runtime={}",
                self.cfg.base_url,
                urlencode(id),
                urlencode(&self.cfg.runtime)
            );
            let body = self.http_get(&url)?;
            let envelope: HfrogFindEnvelope = serde_json::from_str(&body).map_err(|e| {
                BlockMetaError::Decode(format!("find response: {e} body={body}"))
            })?;
            envelope.into_block(id, &self.cfg.runtime)
        }
    }

    // -----------------------------------------------------------------
    // Wire types — the hfrog `{code, msg, data}` envelope and the
    // artifact row shape the server actually emits.
    // -----------------------------------------------------------------

    #[derive(Deserialize, Debug)]
    #[allow(dead_code)] // `msg` kept on the struct for debugging logs / Debug printing
    pub(crate) struct HfrogListEnvelope {
        pub code: i32,
        #[serde(default)]
        pub msg: String,
        #[serde(default)]
        pub data: Vec<HfrogArtifactRow>,
    }

    impl HfrogListEnvelope {
        pub(crate) fn into_blocks(
            self,
            _runtime: &str,
        ) -> Result<Vec<BlockMeta>, BlockMetaError> {
            if self.code != 0 {
                return Err(BlockMetaError::Remote(format!(
                    "hfrog list returned code {} ({})",
                    self.code, self.msg
                )));
            }
            let mut out = Vec::with_capacity(self.data.len());
            for row in self.data {
                match row.into_block() {
                    Ok(b) => out.push(b),
                    Err(e) => log::warn!("hfrog: skipping malformed row: {e}"),
                }
            }
            Ok(out)
        }
    }

    /// Hfrog `/find` returns either a single object in `data` or a
    /// `code != 0` not-found. We model it as Vec for convenience —
    /// real hfrog server returns either `data: [...]` or `data: {}`
    /// depending on the path; the `?find` endpoint returns a list.
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)] // `msg` kept on the struct for debugging logs / Debug printing
    pub(crate) struct HfrogFindEnvelope {
        pub code: i32,
        #[serde(default)]
        pub msg: String,
        #[serde(default)]
        pub data: Value,
    }

    impl HfrogFindEnvelope {
        pub(crate) fn into_block(
            self,
            id: &str,
            _runtime: &str,
        ) -> Result<BlockMeta, BlockMetaError> {
            if self.code != 0 {
                return Err(BlockMetaError::NotFound(id.to_string()));
            }
            // /find returns `data` as a JSON array (one or many rows)
            // when matching, or as nothing on miss.
            let row: Option<HfrogArtifactRow> = match self.data {
                Value::Array(arr) => arr
                    .into_iter()
                    .next()
                    .and_then(|v| serde_json::from_value(v).ok()),
                Value::Object(_) => serde_json::from_value(self.data).ok(),
                _ => None,
            };
            let row = row.ok_or_else(|| BlockMetaError::NotFound(id.to_string()))?;
            row.into_block()
        }
    }

    /// One row of hfrog's artifact table, *partially* deserialised.
    /// We pull `tag` (JSON, where the BlockMeta riches live) and the
    /// identifying fields. Anything else (cont_size / ci_info / …)
    /// is ignored.
    #[derive(Deserialize, Debug, Default)]
    #[allow(dead_code)] // `key_extension` kept for forward-compat with hfrog's typed downloads
    pub(crate) struct HfrogArtifactRow {
        #[serde(default)]
        pub pid: i32,
        #[serde(default)]
        pub name: String,
        #[serde(default)]
        pub ver: String,
        #[serde(default)]
        pub md5: String,
        #[serde(default)]
        pub descript: String,
        #[serde(default)]
        pub tag: Value,
        #[serde(default)]
        pub key_extension: Option<String>,
    }

    /// What we expect to find inside `artifact.tag` for a Maquette
    /// block record. **This is the wire schema for hfrog block
    /// records** — bumping fields here is a contract break and
    /// requires the runtime to bump too (`maquette-block/v1` →
    /// `v2`).
    #[derive(Deserialize, Serialize, Debug, Clone, Default)]
    pub struct HfrogBlockTag {
        /// Maquette block id (slug). Defaults to the artifact's
        /// `name` when missing.
        #[serde(default)]
        pub id: Option<String>,
        /// Human-facing label; defaults to `id`.
        #[serde(default)]
        pub display_name: Option<String>,
        /// Long description. Falls back to `artifact.descript`.
        #[serde(default)]
        pub description: Option<String>,
        /// `cube` / `sphere`. Defaults to `cube`.
        #[serde(default)]
        pub shape_hint: Option<String>,
        /// `[r, g, b]` 0..1 floats.
        #[serde(default)]
        pub default_color: Option<[f32; 3]>,
        /// Free-form prompt fragment.
        #[serde(default)]
        pub texture_hint: Option<String>,
        /// Tag list.
        #[serde(default)]
        pub tags: Vec<String>,
        /// S3 key of a pre-rendered PNG, if shipped.
        #[serde(default)]
        pub preview_s3_key: Option<String>,
    }

    impl HfrogArtifactRow {
        pub(crate) fn into_block(self) -> Result<BlockMeta, BlockMetaError> {
            // Best-effort decode of `tag` into the block-tag shape.
            // If `tag` is missing entirely (defensive: someone
            // manually inserted a row), we synthesise a minimal
            // BlockMeta from the artifact's bare fields.
            let tag: HfrogBlockTag = if self.tag.is_null() {
                HfrogBlockTag::default()
            } else {
                serde_json::from_value(self.tag.clone()).unwrap_or_default()
            };
            let id = tag
                .id
                .clone()
                .unwrap_or_else(|| self.name.clone());
            if id.is_empty() {
                return Err(BlockMetaError::Decode(
                    "artifact has neither tag.id nor name".into(),
                ));
            }
            let shape_hint = match tag.shape_hint.as_deref() {
                Some("sphere") => ShapeKind::Sphere,
                _ => ShapeKind::Cube,
            };
            let default_color = match tag.default_color {
                Some([r, g, b]) => RgbaColor { r, g, b, a: 1.0 },
                None => RgbaColor::rgb(0.6, 0.6, 0.6),
            };
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            Ok(BlockMeta {
                id,
                name: tag.display_name.unwrap_or_else(|| self.name.clone()),
                description: tag.description.unwrap_or_else(|| self.descript.clone()),
                shape_hint,
                default_color,
                texture_hint: tag.texture_hint.unwrap_or_default(),
                tags: tag.tags,
                source: BlockMetaSource::Hfrog {
                    pid: self.pid,
                    name: self.name,
                    ver: self.ver,
                    md5: self.md5,
                    fetched_at: now,
                },
                preview_s3_key: tag.preview_s3_key,
            })
        }
    }

    // -----------------------------------------------------------------
    // URL helpers
    // -----------------------------------------------------------------

    /// Minimal application/x-www-form-urlencoded encoder — just the
    /// characters we need to escape for query strings (`/` `+` `&` `=`
    /// `?` and Unicode). Avoids pulling in `urlencoding` for one
    /// callsite. Compatible with what `actix-web`'s `web::Query`
    /// expects to decode.
    fn urlencode(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for ch in s.chars() {
            match ch {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(ch),
                _ => {
                    let mut buf = [0u8; 4];
                    let bytes = ch.encode_utf8(&mut buf).as_bytes();
                    for b in bytes {
                        out.push_str(&format!("%{b:02X}"));
                    }
                }
            }
        }
        out
    }

    // -----------------------------------------------------------------
    // Tests — wire shape, decode logic, mock HTTP server.
    // -----------------------------------------------------------------

    #[cfg(test)]
    #[allow(clippy::single_char_pattern)]
    mod tests {
        use super::*;
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::mpsc;
        use std::thread;
        use tempfile::tempdir;

        #[test]
        fn config_defaults_match_constants() {
            let cfg = HfrogConfig::default();
            assert_eq!(cfg.base_url, DEFAULT_HFROG_BASE_URL);
            assert_eq!(cfg.runtime, HFROG_RUNTIME);
            assert_eq!(cfg.timeout_secs, HFROG_REQUEST_TIMEOUT_SECS);
        }

        #[test]
        fn urlencode_handles_basic_cases() {
            assert_eq!(urlencode("hello"), "hello");
            assert_eq!(urlencode("maquette-block/v1"), "maquette-block%2Fv1");
            assert_eq!(urlencode("a b"), "a%20b");
            // Unicode (Chinese chars used in our default block names)
            assert_eq!(urlencode("草"), "%E8%8D%89");
        }

        #[test]
        fn list_envelope_decodes_minimal_row() {
            let raw = r#"{"code":0,"msg":"","data":[{
                "pid": 7,
                "name": "grass",
                "ver": "0.1.0",
                "md5": "abc",
                "descript": "grass desc",
                "tag": {
                    "id": "grass",
                    "display_name": "草地",
                    "description": "草地块",
                    "shape_hint": "cube",
                    "default_color": [0.45, 0.80, 0.40],
                    "texture_hint": "patchy minecraft grass",
                    "tags": ["natural"]
                }
            }]}"#;
            let env: HfrogListEnvelope = serde_json::from_str(raw).unwrap();
            let blocks = env.into_blocks(HFROG_RUNTIME).unwrap();
            assert_eq!(blocks.len(), 1);
            let g = &blocks[0];
            assert_eq!(g.id, "grass");
            assert_eq!(g.name, "草地");
            assert_eq!(g.texture_hint, "patchy minecraft grass");
            assert!(matches!(g.source, BlockMetaSource::Hfrog { pid: 7, .. }));
        }

        #[test]
        fn list_envelope_with_missing_tag_synthesises_from_bare_fields() {
            let raw = r#"{"code":0,"data":[{
                "pid": 9,
                "name": "stone",
                "ver": "1",
                "md5": "",
                "descript": "stone desc"
            }]}"#;
            let env: HfrogListEnvelope = serde_json::from_str(raw).unwrap();
            let blocks = env.into_blocks(HFROG_RUNTIME).unwrap();
            assert_eq!(blocks.len(), 1);
            assert_eq!(blocks[0].id, "stone");
            assert_eq!(blocks[0].description, "stone desc");
        }

        #[test]
        fn list_envelope_skips_rows_without_id_or_name() {
            let raw = r#"{"code":0,"data":[
                { "pid": 1, "name": "" , "tag": {} },
                { "pid": 2, "name": "ok" }
            ]}"#;
            let env: HfrogListEnvelope = serde_json::from_str(raw).unwrap();
            let blocks = env.into_blocks(HFROG_RUNTIME).unwrap();
            assert_eq!(blocks.len(), 1, "the empty-name row must be dropped");
            assert_eq!(blocks[0].id, "ok");
        }

        #[test]
        fn list_envelope_with_nonzero_code_propagates_error() {
            let raw = r#"{"code": 1001, "msg": "internal", "data": []}"#;
            let env: HfrogListEnvelope = serde_json::from_str(raw).unwrap();
            assert!(matches!(
                env.into_blocks(HFROG_RUNTIME),
                Err(BlockMetaError::Remote(_))
            ));
        }

        #[test]
        fn find_envelope_handles_array_data() {
            let raw = r#"{"code":0,"data":[{"pid":1,"name":"grass"}]}"#;
            let env: HfrogFindEnvelope = serde_json::from_str(raw).unwrap();
            let b = env.into_block("grass", HFROG_RUNTIME).unwrap();
            assert_eq!(b.id, "grass");
        }

        #[test]
        fn find_envelope_with_nonzero_code_is_not_found() {
            let raw = r#"{"code":1010,"msg":"missing","data":null}"#;
            let env: HfrogFindEnvelope = serde_json::from_str(raw).unwrap();
            assert!(matches!(
                env.into_block("ghost", HFROG_RUNTIME),
                Err(BlockMetaError::NotFound(_))
            ));
        }

        // -----------------------------------------------------------------
        // Mock HTTP server — minimal in-process loopback so we can
        // round-trip ureq through real network bytes without
        // depending on `mockito` etc. Single-shot per test thread.
        // -----------------------------------------------------------------

        /// Run a one-request mock server on an ephemeral port,
        /// returning `(base_url, join_handle)`. The server serves
        /// exactly one HTTP request whose response body is `body`
        /// (with content-type application/json) and then exits.
        fn one_shot_mock_json(body: &'static str) -> (String, thread::JoinHandle<String>) {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let (tx, rx) = mpsc::channel();
            let handle = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 1024];
                let n = stream.read(&mut buf).unwrap();
                let req_text = String::from_utf8_lossy(&buf[..n]).to_string();
                tx.send(req_text.clone()).unwrap();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(resp.as_bytes()).unwrap();
                stream.flush().unwrap();
                drop(stream);
                rx.recv().unwrap()
            });
            (format!("http://127.0.0.1:{port}"), handle)
        }

        #[test]
        fn hfrog_list_remote_round_trips_through_real_socket() {
            let body = r#"{"code":0,"data":[{
                "pid":7,"name":"grass","ver":"0.1.0","md5":"x","descript":"d",
                "tag":{"id":"grass","texture_hint":"green grass"}
            }]}"#;
            let (base, handle) = one_shot_mock_json(body);
            let cfg = HfrogConfig {
                base_url: base,
                runtime: HFROG_RUNTIME.to_string(),
                timeout_secs: 5,
            };
            let provider = HfrogProvider::new(cfg).with_cache_dir(None);
            let blocks = provider.list_remote().unwrap();
            assert_eq!(blocks.len(), 1);
            assert_eq!(blocks[0].id, "grass");
            assert_eq!(blocks[0].texture_hint, "green grass");

            // Confirm we sent a sensible GET line with the runtime
            // query parameter properly encoded.
            let req = handle.join().unwrap();
            assert!(req.starts_with("GET /api/artifactory/list?runtime="));
            assert!(req.contains("maquette-block%2Fv1"));
        }

        #[test]
        fn hfrog_sync_caches_to_disk() {
            let body = r#"{"code":0,"data":[{
                "pid":42,"name":"stone","ver":"0.2.0","md5":"y","descript":"sd",
                "tag":{"id":"stone","texture_hint":"weathered grey"}
            }]}"#;
            let (base, _h) = one_shot_mock_json(body);
            let dir = tempdir().unwrap();
            let cfg = HfrogConfig {
                base_url: base,
                runtime: HFROG_RUNTIME.to_string(),
                timeout_secs: 5,
            };
            let provider = HfrogProvider::new(cfg).with_cache_dir(Some(dir.path().to_path_buf()));
            let blocks = provider.sync().unwrap();
            assert_eq!(blocks.len(), 1);
            // File on disk
            let cached_path = dir
                .path()
                .join("hfrog")
                .join(HFROG_RUNTIME)
                .join("stone.json");
            assert!(cached_path.exists(), "cache file not written");
            let cached_bytes = std::fs::read(&cached_path).unwrap();
            let cached: BlockMeta = serde_json::from_slice(&cached_bytes).unwrap();
            assert_eq!(cached.id, "stone");
        }
    }

    // =================================================================
    // HfrogPublisher — write-side client for "Publish to hfrog" path.
    // =================================================================

    /// Result of a successful `publish_block` call. The pid is the
    /// row id hfrog assigned; the upload URL (presigned) is what the
    /// publisher PUTs the PNG to. We surface both so the caller can
    /// log them — the GUI mostly just shows pid in a toast.
    #[derive(Debug, Clone)]
    pub struct PublishOutcome {
        pub pid: i32,
        /// Server-assigned `s3_key` — useful for verifying that the
        /// block actually reached durable storage.
        pub s3_key: String,
        /// MD5 we computed locally and sent in the JSON. Echoed back
        /// so the caller can pin it without recomputing.
        pub md5: String,
    }

    /// Talks to hfrog's write side. Construction is cheap; the
    /// publisher does no network until `publish_block` is called.
    pub struct HfrogPublisher {
        cfg: HfrogConfig,
    }

    impl HfrogPublisher {
        pub fn new(cfg: HfrogConfig) -> Self {
            Self { cfg }
        }

        /// Publish a `BlockMeta` together with its PNG bytes to
        /// hfrog. Two HTTP calls under the hood:
        ///
        /// 1. `PUT /api/artifactory/add` with a JSON
        ///    `ArtifactoryModel` describing the artefact. The
        ///    response carries `data.url`, an S3-presigned PUT URL.
        /// 2. `PUT <presigned URL>` with the PNG bytes as the
        ///    request body. The MinIO bucket-event hook flips
        ///    `is_artifactory_ready` to true, after which the
        ///    record shows up in `list?runtime=maquette-block/v1`.
        ///
        /// Yes, the multipart `/add_form_file` route exists on the
        /// server side too, but it caps at 5 MB *and* still
        /// double-trips through S3 — going through the presigned
        /// URL directly is simpler and lets us upload arbitrarily
        /// large pre-renders later if we ever want to. Same total
        /// number of HTTP round-trips.
        pub fn publish_block(
            &self,
            meta: &BlockMeta,
            png_bytes: &[u8],
        ) -> Result<PublishOutcome, BlockMetaError> {
            // 1. Compute an md5 of the PNG so the artefact record
            //    matches what we actually upload. The 32-char hex
            //    column is `NOT NULL` on the hfrog side so we have
            //    to send something.
            let md5 = md5_hex(png_bytes);

            // Pick a default ver. The user may extend this later
            // (a "ver" field on `ComposerDraft`) — for now,
            // a timestamped v0 keeps the artefact tuple unique.
            let ver = format!("0.0.{}", super::unix_seconds());
            let s3_key = format!("maquette-block/{}/{}.png", meta.id, ver);

            // 2. JSON body for `/add`. Field names mirror hfrog's
            //    `entity::artifactory::Model`. `is_artifactory_ready`
            //    starts `false`; the bucket-event hook flips it
            //    after the S3 PUT succeeds.
            let tag = HfrogBlockTag {
                id: Some(meta.id.clone()),
                display_name: Some(meta.name.clone()),
                description: Some(meta.description.clone()),
                shape_hint: Some(match meta.shape_hint {
                    ShapeKind::Cube => "cube".to_string(),
                    ShapeKind::Sphere => "sphere".to_string(),
                }),
                default_color: Some([
                    meta.default_color.r,
                    meta.default_color.g,
                    meta.default_color.b,
                ]),
                texture_hint: if meta.texture_hint.is_empty() {
                    None
                } else {
                    Some(meta.texture_hint.clone())
                },
                tags: meta.tags.clone(),
                preview_s3_key: Some(s3_key.clone()),
            };
            let body = serde_json::json!({
                "name": meta.id,
                "ver": ver,
                "runtime": self.cfg.runtime,
                "md5": md5,
                "descript": if meta.description.is_empty() {
                    meta.name.clone()
                } else {
                    meta.description.clone()
                },
                "tag": tag,
                "cont_size": png_bytes.len() as i64,
                "s3_key": s3_key,
                "s3_inc_id": std::env::var("MAQUETTE_HFROG_S3_INC_ID")
                    .ok()
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(0),
                "is_artifactory_ready": false,
                "is_raw": false,
                "key_extension": "png"
            });
            let url = format!("{}/api/artifactory/add", self.cfg.base_url);
            log::info!("hfrog: PUT {url}  body~={}B", body.to_string().len());
            let response = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(self.cfg.timeout_secs))
                .build()
                .put(&url)
                .set("content-type", "application/json")
                .send_string(&body.to_string())
                .map_err(|e| {
                    BlockMetaError::Remote(format!("PUT {url}: {e}"))
                })?;
            let response_body = response.into_string().map_err(|e| {
                BlockMetaError::Remote(format!("read response body: {e}"))
            })?;
            let v: Value = serde_json::from_str(&response_body).map_err(|e| {
                BlockMetaError::Decode(format!("add response: {e} body={response_body}"))
            })?;
            // hfrog wraps errors as `{code: !=0, msg, data: null}`.
            let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
            if code != 0 {
                let msg = v
                    .get("msg")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown");
                return Err(BlockMetaError::Remote(format!(
                    "hfrog refused publish (code={code}): {msg}"
                )));
            }
            let presigned_url = v
                .get("data")
                .and_then(|d| d.get("url"))
                .and_then(|u| u.as_str())
                .ok_or_else(|| {
                    BlockMetaError::Decode(format!(
                        "add response missing data.url: {response_body}"
                    ))
                })?
                .to_string();

            // 3. PUT the PNG bytes to the presigned URL.
            log::info!(
                "hfrog: PUT presigned (image/png, {}B)",
                png_bytes.len()
            );
            ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(
                    self.cfg.timeout_secs.max(60),
                ))
                .build()
                .put(&presigned_url)
                .set("content-type", "image/png")
                .send_bytes(png_bytes)
                .map_err(|e| {
                    BlockMetaError::Remote(format!(
                        "PUT presigned (S3): {e}"
                    ))
                })?;

            // The pid lives on a follow-up `/find` call; for the
            // toast we just need *something*. Re-query.
            let pid = self
                .lookup_pid(&meta.id, &ver)
                .unwrap_or(0);

            Ok(PublishOutcome {
                pid,
                s3_key,
                md5,
            })
        }

        fn lookup_pid(&self, id: &str, ver: &str) -> Option<i32> {
            let url = format!(
                "{}/api/artifactory/find?name={}&ver={}&runtime={}",
                self.cfg.base_url,
                urlencode(id),
                urlencode(ver),
                urlencode(&self.cfg.runtime),
            );
            let resp = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(self.cfg.timeout_secs))
                .build()
                .get(&url)
                .call()
                .ok()?;
            let body = resp.into_string().ok()?;
            let v: Value = serde_json::from_str(&body).ok()?;
            v.get("data")
                .and_then(|d| d.get(0).or(Some(d)))
                .and_then(|row| row.get("pid"))
                .and_then(|p| p.as_i64())
                .map(|p| p as i32)
        }
    }

    /// Tiny RFC 1321 MD5 — pure Rust, public-domain-style. Inlined
    /// because we only need it on the publish path; hauling in
    /// the `md-5` crate just for this is one more dep we'd rather
    /// not maintain. Constant-time it is not; we don't care
    /// (PNG bytes aren't a secret).
    fn md5_hex(input: &[u8]) -> String {
        let digest = md5_digest(input);
        let mut out = String::with_capacity(32);
        for byte in &digest {
            use std::fmt::Write;
            let _ = write!(&mut out, "{byte:02x}");
        }
        out
    }

    fn md5_digest(message: &[u8]) -> [u8; 16] {
        // Per-round shift + sine constants from RFC 1321.
        const S: [u32; 64] = [
            7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
            5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
            4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
            6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
        ];
        const K: [u32; 64] = [
            0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a,
            0xa8304613, 0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
            0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340,
            0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
            0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8,
            0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
            0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
            0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
            0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92,
            0xffeff47d, 0x85845dd1, 0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
            0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
        ];
        // Pre-processing: append 0x80, pad with zeros, append length.
        let original_bit_length = (message.len() as u64).wrapping_mul(8);
        let mut padded = message.to_vec();
        padded.push(0x80);
        while padded.len() % 64 != 56 {
            padded.push(0);
        }
        padded.extend_from_slice(&original_bit_length.to_le_bytes());

        let mut a0: u32 = 0x67452301;
        let mut b0: u32 = 0xefcdab89;
        let mut c0: u32 = 0x98badcfe;
        let mut d0: u32 = 0x10325476;

        for chunk in padded.chunks_exact(64) {
            let mut m = [0u32; 16];
            for (i, w) in m.iter_mut().enumerate() {
                let off = i * 4;
                *w = u32::from_le_bytes([
                    chunk[off],
                    chunk[off + 1],
                    chunk[off + 2],
                    chunk[off + 3],
                ]);
            }
            let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
            for i in 0..64 {
                let (f, g) = match i {
                    0..=15 => (((b & c) | (!b & d)), i),
                    16..=31 => (((d & b) | (!d & c)), (5 * i + 1) % 16),
                    32..=47 => ((b ^ c ^ d), (3 * i + 5) % 16),
                    _ => ((c ^ (b | !d)), (7 * i) % 16),
                };
                let temp = d;
                d = c;
                c = b;
                b = b.wrapping_add(
                    a.wrapping_add(f)
                        .wrapping_add(K[i])
                        .wrapping_add(m[g])
                        .rotate_left(S[i]),
                );
                a = temp;
            }
            a0 = a0.wrapping_add(a);
            b0 = b0.wrapping_add(b);
            c0 = c0.wrapping_add(c);
            d0 = d0.wrapping_add(d);
        }

        let mut out = [0u8; 16];
        out[..4].copy_from_slice(&a0.to_le_bytes());
        out[4..8].copy_from_slice(&b0.to_le_bytes());
        out[8..12].copy_from_slice(&c0.to_le_bytes());
        out[12..16].copy_from_slice(&d0.to_le_bytes());
        out
    }

    #[cfg(test)]
    mod publisher_tests {
        use super::*;

        #[test]
        fn md5_matches_known_vectors() {
            // RFC 1321 reference vectors.
            assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
            assert_eq!(md5_hex(b"a"), "0cc175b9c0f1b6a831c399e269772661");
            assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
            assert_eq!(
                md5_hex(b"message digest"),
                "f96b697d7cb7938d525a2f31aaf161d0"
            );
            assert_eq!(
                md5_hex(b"abcdefghijklmnopqrstuvwxyz"),
                "c3fcd3d76192e4007dfb496cca67e13b"
            );
        }
    }
}

// =====================================================================
// LocalDraftProvider + on-disk drafts cache.
// =====================================================================

/// User-authored drafts produced by the Block Composer window. Drafts
/// live alongside the hfrog cache (under a `local-drafts` source
/// folder) so the same `cache_list` machinery can surface them to
/// the main Block Library; the GUI badges them differently via
/// `BlockMetaSource::LocalDraft`.
pub mod drafts {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{
        cache_list, cache_put_png, BlockMeta, BlockMetaError, BlockMetaProvider,
    };

    /// Folder name for drafts under the cache root. Distinct from
    /// the `hfrog` folder so the two never collide.
    pub const DRAFT_SOURCE_DIR: &str = "local-drafts";

    /// Logical "runtime" the draft files live under inside the
    /// cache directory tree. Only ever written by the composer; we
    /// keep it stable even though there's just one bucket.
    pub const DRAFT_RUNTIME: &str = "user/v1";

    /// Persist a draft (meta JSON + sibling PNG). Returns the JSON
    /// path on success.
    ///
    /// Drafts intentionally go under `local-drafts/<runtime>/`,
    /// **not** the `cache_put_meta`-derived path that uses
    /// `meta.source.label()` (which would be `"draft"`). Keeping the
    /// folder name explicit + decoupled from the enum label means
    /// renaming the source-label later doesn't strand on-disk
    /// drafts under a stale path.
    pub fn write_draft(
        cache_dir: &Path,
        meta: &BlockMeta,
        png_bytes: &[u8],
    ) -> std::io::Result<PathBuf> {
        // `cache_put_meta` would route by `meta.source.label()`; we
        // need to force the `local-drafts` folder. Easiest way: hand
        // the routing here ourselves with the same "tmp + rename"
        // semantics as the shared writer.
        use std::io::Write as _;
        let dir = cache_dir.join(DRAFT_SOURCE_DIR).join(DRAFT_RUNTIME);
        fs::create_dir_all(&dir)?;
        let final_path = dir.join(format!("{}.json", meta.id));
        let tmp_path = dir.join(format!("{}.json.tmp", meta.id));
        {
            let mut f = fs::File::create(&tmp_path)?;
            let json = serde_json::to_vec_pretty(meta).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
            })?;
            f.write_all(&json)?;
            f.sync_all()?;
        }
        fs::rename(&tmp_path, &final_path)?;
        cache_put_png(cache_dir, DRAFT_SOURCE_DIR, DRAFT_RUNTIME, &meta.id, png_bytes)?;
        Ok(final_path)
    }

    /// Drop a draft from disk (best-effort; missing entries are not
    /// errors). Used after a successful `Publish to Hfrog` so the
    /// draft doesn't sit around as a duplicate.
    pub fn remove_draft(cache_dir: &Path, id: &str) -> std::io::Result<()> {
        let dir = cache_dir.join(DRAFT_SOURCE_DIR).join(DRAFT_RUNTIME);
        for ext in ["json", "png"] {
            let path = dir.join(format!("{id}.{ext}"));
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Return the PNG bytes for a draft, if any.
    #[allow(dead_code)]
    pub fn read_draft_png(
        cache_dir: &Path,
        id: &str,
    ) -> std::io::Result<Option<Vec<u8>>> {
        let path = cache_dir
            .join(DRAFT_SOURCE_DIR)
            .join(DRAFT_RUNTIME)
            .join(format!("{id}.png"));
        match fs::read(&path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Provider that lists every JSON file under
    /// `<cache_dir>/local-drafts/user/v1/`. Reads off disk on
    /// every `list()` so a draft saved by the composer becomes
    /// visible the next frame without a cache invalidation step.
    pub struct LocalDraftProvider {
        cache_dir: Option<PathBuf>,
    }

    impl LocalDraftProvider {
        pub fn new() -> Self {
            Self {
                cache_dir: super::default_cache_dir(),
            }
        }
        #[allow(dead_code)]
        pub fn with_cache_dir(mut self, dir: Option<PathBuf>) -> Self {
            self.cache_dir = dir;
            self
        }

        /// Merge drafts into an existing block list. `existing`
        /// is typically the GUI's current `BlockLibraryState.blocks`.
        /// Drafts override on id collision (so editing a draft of
        /// `grass` shows up in the library instead of the bundled
        /// `grass`).
        pub fn merge_into_library(
            &self,
            existing: &[BlockMeta],
        ) -> Result<Vec<BlockMeta>, BlockMetaError> {
            let drafts = self.list()?;
            let draft_ids: std::collections::HashSet<&str> =
                drafts.iter().map(|b| b.id.as_str()).collect();
            let mut merged: Vec<BlockMeta> = existing
                .iter()
                .filter(|b| !draft_ids.contains(b.id.as_str()))
                .cloned()
                .collect();
            merged.extend(drafts);
            merged.sort_by(|a, b| a.id.cmp(&b.id));
            Ok(merged)
        }
    }

    impl Default for LocalDraftProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    impl BlockMetaProvider for LocalDraftProvider {
        fn name(&self) -> &'static str {
            "local-drafts"
        }
        fn list(&self) -> Result<Vec<BlockMeta>, BlockMetaError> {
            let Some(dir) = &self.cache_dir else {
                return Ok(Vec::new());
            };
            cache_list(dir, DRAFT_SOURCE_DIR, DRAFT_RUNTIME)
                .map_err(BlockMetaError::Io)
        }
        fn get(&self, id: &str) -> Result<BlockMeta, BlockMetaError> {
            self.list()?
                .into_iter()
                .find(|b| b.id == id)
                .ok_or_else(|| BlockMetaError::NotFound(id.to_string()))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::block_meta::{BlockMetaSource, RgbaColor};
        use crate::grid::ShapeKind;
        use tempfile::tempdir;

        fn fixture(id: &str) -> BlockMeta {
            BlockMeta {
                id: id.to_string(),
                name: format!("Draft {id}"),
                description: "draft description".into(),
                shape_hint: ShapeKind::Cube,
                default_color: RgbaColor::rgb(0.5, 0.5, 0.5),
                texture_hint: "draft hint".into(),
                tags: vec![],
                source: BlockMetaSource::LocalDraft { created_at: 1 },
                preview_s3_key: None,
            }
        }

        #[test]
        fn write_then_list_round_trip() {
            let dir = tempdir().unwrap();
            write_draft(dir.path(), &fixture("my_block"), b"\x89PNG\r\n\x1a\n").unwrap();
            let provider = LocalDraftProvider::new()
                .with_cache_dir(Some(dir.path().to_path_buf()));
            let listed = provider.list().unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].id, "my_block");
        }

        #[test]
        fn remove_draft_drops_both_files() {
            let dir = tempdir().unwrap();
            write_draft(dir.path(), &fixture("x"), b"\x89PNG\r\n\x1a\n").unwrap();
            remove_draft(dir.path(), "x").unwrap();
            let provider = LocalDraftProvider::new()
                .with_cache_dir(Some(dir.path().to_path_buf()));
            assert!(provider.list().unwrap().is_empty());
            assert!(read_draft_png(dir.path(), "x").unwrap().is_none());
        }

        #[test]
        fn remove_missing_draft_is_noop() {
            let dir = tempdir().unwrap();
            // No draft was ever written; remove must not error.
            remove_draft(dir.path(), "ghost").unwrap();
        }

        #[test]
        fn merge_drafts_override_collisions() {
            let dir = tempdir().unwrap();
            write_draft(dir.path(), &fixture("grass"), b"\x89PNG\r\n\x1a\n").unwrap();
            let bundled = vec![BlockMeta {
                id: "grass".into(),
                name: "Bundled grass".into(),
                description: "bundled".into(),
                shape_hint: ShapeKind::Cube,
                default_color: RgbaColor::rgb(0.4, 0.8, 0.4),
                texture_hint: "bundled hint".into(),
                tags: vec![],
                source: BlockMetaSource::Local,
                preview_s3_key: None,
            }];
            let provider = LocalDraftProvider::new()
                .with_cache_dir(Some(dir.path().to_path_buf()));
            let merged = provider.merge_into_library(&bundled).unwrap();
            assert_eq!(merged.len(), 1, "draft must override the bundled grass");
            assert_eq!(merged[0].name, "Draft grass");
            assert!(matches!(merged[0].source, BlockMetaSource::LocalDraft { .. }));
        }
    }
}

/// Internal helper exposed to the GUI binary.
pub(crate) fn unix_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
