//! glTF 2.0 / GLB exporter + inverted-hull outline baker.
//!
//! ## What ships in the file
//!
//! Per the **Export Golden Rule** in `docs/handoff/COST_AWARENESS.md`,
//! exports contain only what every engine understands without help:
//!
//! * **Body** — one glTF mesh with one primitive per palette color that
//!   the user actually painted with. Each primitive has `POSITION` +
//!   `NORMAL` attributes and references a dedicated unlit material with
//!   that color as `baseColorFactor`.
//! * **Outline** (optional, enabled by default) — one extra glTF mesh,
//!   inverted-hull silhouette: vertex-extruded along normals by a
//!   user-chosen percentage of the model's bounding diagonal, with
//!   reversed triangle winding so the engine's default backface culling
//!   renders the back half as a black cage. No shader, no post-effect,
//!   "just works" in Godot / Unity / Blender.
//!
//! The toon shader used in the preview is **never** referenced from the
//! export. Preview ≠ export is an invariant; see `about` dialog.
//!
//! ## Why we hand-roll the glTF JSON types
//!
//! Depending on `gltf-json` would pin us to a specific spec revision
//! and compile an entire dep graph for ≈ 40 LoC of structs. We emit a
//! small, well-defined subset of glTF 2.0 and every field we write is
//! covered by the canonical spec; every target engine will ignore
//! fields we don't write. A hand-rolled document is easier to audit
//! when a user reports "Blender wouldn't load the file".
//!
//! ## Two output modes
//!
//! * `.glb` — binary-packaged single file (JSON chunk + BIN chunk),
//!   ideal for sharing and dragging into engines.
//! * `.gltf` — JSON file with a sibling `.bin`, easier to diff and to
//!   inspect by hand, friendlier to version control.
//!
//! Both modes go through the same builder; only the final serializer
//! differs.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::Serialize;

use crate::grid::{Grid, Palette, CELL_SIZE};
use crate::mesher::{build_color_buckets, MeshBuilder};

/// Sent by the UI when the user confirms the Export dialog.
#[derive(Message, Clone)]
pub struct ExportRequest(pub ExportOptions);

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub path: PathBuf,
    pub format: ExportFormat,
    pub outline: OutlineConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// Single-file binary. The model and the binary buffer travel
    /// together; the right default for "I want to drop this into my
    /// engine".
    Glb,
    /// Text JSON + external `.bin`. Use when the user wants to commit
    /// the model to git, diff it, or inspect it.
    Gltf,
}

/// Outline configuration as seen by the **export path**, not the
/// preview. Preview outlines live in `bevy_mod_outline`; these
/// settings bake a geometry-only silhouette that target engines can
/// render with no shader support.
#[derive(Debug, Clone)]
pub struct OutlineConfig {
    pub enabled: bool,
    /// Extrusion amount, expressed as a percentage of the model's
    /// bounding diagonal. Locked range `0.0..=10.0` per
    /// `docs/handoff/NEXT.md` §Locked decisions.
    pub width_pct: f32,
    pub color: Color,
}

impl Default for OutlineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            width_pct: 3.0,
            color: Color::BLACK,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("the project is empty — paint at least one cell before exporting")]
    Empty,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct ExportPlugin;

impl Plugin for ExportPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ExportRequest>()
            .add_message::<ExportOutcome>()
            .add_systems(Update, handle_export_request);
    }
}

/// Emitted by the GUI's export plugin after every `ExportRequest`.
/// The GUI's notification layer subscribes to turn success / failure
/// into user-visible toasts. The CLI never observes this message
/// (it calls `export::write` directly and handles the `Result`), so
/// it stays behind the Headless Invariant.
#[derive(Message, Clone, Debug)]
pub enum ExportOutcome {
    Success { path: std::path::PathBuf },
    Failure { message: String },
}

fn handle_export_request(
    mut events: MessageReader<ExportRequest>,
    grid: Res<Grid>,
    palette: Res<Palette>,
    mut outcomes: MessageWriter<ExportOutcome>,
) {
    for ExportRequest(opts) in events.read() {
        match write(&grid, &palette, opts) {
            Ok(()) => {
                log::info!("exported to {}", opts.path.display());
                outcomes.write(ExportOutcome::Success {
                    path: opts.path.clone(),
                });
            }
            Err(e) => {
                log::error!("export failed: {e}");
                outcomes.write(ExportOutcome::Failure {
                    message: format!("{e}"),
                });
            }
        }
    }
}

// =====================================================================
// Writer entry point
// =====================================================================

pub fn write(grid: &Grid, palette: &Palette, opts: &ExportOptions) -> Result<(), ExportError> {
    let mut buckets = build_color_buckets(grid);
    if buckets.is_empty() {
        return Err(ExportError::Empty);
    }

    // Match the preview's centring: X/Z centred on origin, Y anchored
    // at ground (the column grows upward from y=0). This is what every
    // engine will assume when the user drops the asset at a node
    // origin.
    let ox = -(grid.w as f32) * CELL_SIZE * 0.5;
    let oz = -(grid.h as f32) * CELL_SIZE * 0.5;
    for (_, b) in &mut buckets {
        b.translate(ox, 0.0, oz);
    }

    let bounds = compute_bounds(&buckets);
    let bounds_diag = bounds.diagonal();

    let outline = if opts.outline.enabled && opts.outline.width_pct > 0.0 && bounds_diag > 0.0 {
        let extrude = opts.outline.width_pct * 0.01 * bounds_diag;
        Some(build_outline_hull(&buckets, extrude))
    } else {
        None
    };

    let (doc, blob, buffer_uri) = build_gltf(
        &buckets,
        outline.as_ref(),
        palette,
        &opts.outline,
        opts.format,
        &opts.path,
    );

    match opts.format {
        ExportFormat::Glb => write_glb(&opts.path, &doc, &blob),
        ExportFormat::Gltf => write_gltf_pair(&opts.path, &doc, &blob, buffer_uri.as_deref()),
    }
}

// =====================================================================
// Geometry helpers
// =====================================================================

#[derive(Debug, Clone, Copy)]
struct Bounds {
    min: [f32; 3],
    max: [f32; 3],
}

impl Bounds {
    fn diagonal(&self) -> f32 {
        let dx = self.max[0] - self.min[0];
        let dy = self.max[1] - self.min[1];
        let dz = self.max[2] - self.min[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

fn compute_bounds(buckets: &[(u8, MeshBuilder)]) -> Bounds {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for (_, b) in buckets {
        for p in &b.positions {
            for i in 0..3 {
                if p[i] < min[i] {
                    min[i] = p[i];
                }
                if p[i] > max[i] {
                    max[i] = p[i];
                }
            }
        }
    }
    // Guard against empty buckets (caller ensures non-empty, but be
    // defensive so accessor min/max never ship ±INF).
    if !min[0].is_finite() {
        min = [0.0; 3];
        max = [0.0; 3];
    }
    Bounds { min, max }
}

/// Build the inverted-hull outline as a single bucket.
///
/// Algorithm:
/// 1. Copy every vertex from every color bucket, displaced outward by
///    `extrude` along its vertex normal.
/// 2. Flip the normal so any engine that does Gouraud shading on the
///    outline (we don't ask it to, but it might) shades it sensibly.
/// 3. Reverse triangle winding on the indices so the engine's default
///    backface culling keeps only the side we want — the silhouette.
///
/// No vertex sharing across buckets, so seams between originally
/// different colors widen slightly — cheap and visually indistinguishable.
fn build_outline_hull(buckets: &[(u8, MeshBuilder)], extrude: f32) -> MeshBuilder {
    let mut out = MeshBuilder::default();
    for (_, b) in buckets {
        let base = out.positions.len() as u32;
        for (i, pos) in b.positions.iter().enumerate() {
            let n = b.normals[i];
            out.positions.push([
                pos[0] + n[0] * extrude,
                pos[1] + n[1] * extrude,
                pos[2] + n[2] * extrude,
            ]);
            out.normals.push([-n[0], -n[1], -n[2]]);
            out.uvs.push([0.0, 0.0]);
        }
        // Reverse winding: (a, b, c) → (a, c, b). This swaps which side
        // of the triangle faces outward, so the default
        // cull-back-facing renderer ends up displaying only the inverted
        // hull's back surface — the silhouette.
        for tri in b.indices.chunks_exact(3) {
            out.indices.push(base + tri[0]);
            out.indices.push(base + tri[2]);
            out.indices.push(base + tri[1]);
        }
    }
    out
}

// =====================================================================
// glTF document builders
// =====================================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GltfDoc {
    asset: GltfAsset,
    scene: u32,
    scenes: Vec<GltfScene>,
    nodes: Vec<GltfNode>,
    meshes: Vec<GltfMesh>,
    materials: Vec<GltfMaterial>,
    accessors: Vec<GltfAccessor>,
    buffer_views: Vec<GltfBufferView>,
    buffers: Vec<GltfBuffer>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    extensions_used: Vec<String>,
}

#[derive(Serialize)]
struct GltfAsset {
    version: String,
    generator: String,
}

#[derive(Serialize)]
struct GltfScene {
    nodes: Vec<u32>,
}

#[derive(Serialize)]
struct GltfNode {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<u32>,
}

#[derive(Serialize)]
struct GltfMesh {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    primitives: Vec<GltfPrimitive>,
}

#[derive(Serialize)]
struct GltfPrimitive {
    attributes: GltfPrimitiveAttrs,
    indices: u32,
    material: u32,
}

#[derive(Serialize)]
struct GltfPrimitiveAttrs {
    #[serde(rename = "POSITION")]
    position: u32,
    #[serde(rename = "NORMAL")]
    normal: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GltfMaterial {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    pbr_metallic_roughness: GltfPbr,
    double_sided: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    extensions: BTreeMap<String, serde_json::Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GltfPbr {
    base_color_factor: [f32; 4],
    metallic_factor: f32,
    roughness_factor: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GltfAccessor {
    buffer_view: u32,
    component_type: u32,
    count: u32,
    #[serde(rename = "type")]
    ty: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<Vec<f32>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GltfBufferView {
    buffer: u32,
    byte_offset: u32,
    byte_length: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GltfBuffer {
    #[serde(skip_serializing_if = "Option::is_none")]
    uri: Option<String>,
    byte_length: u32,
}

// glTF component types
const COMP_UNSIGNED_INT: u32 = 5125;
const COMP_FLOAT: u32 = 5126;

// glTF bufferView targets
const TARGET_ARRAY_BUFFER: u32 = 34962;
const TARGET_ELEMENT_ARRAY_BUFFER: u32 = 34963;

/// Build the glTF JSON document AND the packed binary blob that its
/// buffer views index into.
///
/// Returns `(doc, blob, external_bin_uri)`. For `Glb` output the URI is
/// `None` (the BIN chunk lives in the same file); for `Gltf` it is the
/// filename of the sibling `.bin` file, which the caller writes.
#[allow(clippy::too_many_arguments)]
fn build_gltf(
    buckets: &[(u8, MeshBuilder)],
    outline: Option<&MeshBuilder>,
    palette: &Palette,
    outline_cfg: &OutlineConfig,
    format: ExportFormat,
    out_path: &Path,
) -> (GltfDoc, Vec<u8>, Option<String>) {
    let mut blob = Vec::<u8>::new();
    let mut accessors = Vec::new();
    let mut buffer_views = Vec::new();
    let mut materials = Vec::new();

    // --- Body primitives: one per color bucket ---
    let mut body_primitives = Vec::with_capacity(buckets.len());
    for (ci, b) in buckets {
        let pos_a = push_positions(&mut blob, &mut accessors, &mut buffer_views, &b.positions);
        let nor_a = push_normals(&mut blob, &mut accessors, &mut buffer_views, &b.normals);
        let idx_a = push_indices(&mut blob, &mut accessors, &mut buffer_views, &b.indices);
        // v0.6: palette is sparse. A filled cell should never
        // reference a deleted slot (the delete paths remap/erase
        // first), but we still degrade gracefully to white if it
        // somehow does — better than panicking during export.
        let color = palette.get(*ci).unwrap_or(Color::WHITE);
        materials.push(unlit_material(
            Some(format!("Color{ci}")),
            srgba_array(color),
            false,
        ));
        body_primitives.push(GltfPrimitive {
            attributes: GltfPrimitiveAttrs {
                position: pos_a,
                normal: nor_a,
            },
            indices: idx_a,
            material: (materials.len() - 1) as u32,
        });
    }

    // --- Optional outline mesh ---
    let mut meshes = vec![GltfMesh {
        name: Some("MaquetteBody".into()),
        primitives: body_primitives,
    }];
    let mut nodes = vec![
        GltfNode {
            name: Some("Maquette".into()),
            mesh: None,
            children: vec![1],
        },
        GltfNode {
            name: Some("Body".into()),
            mesh: Some(0),
            children: vec![],
        },
    ];

    if let Some(o) = outline {
        let pos_a = push_positions(&mut blob, &mut accessors, &mut buffer_views, &o.positions);
        let nor_a = push_normals(&mut blob, &mut accessors, &mut buffer_views, &o.normals);
        let idx_a = push_indices(&mut blob, &mut accessors, &mut buffer_views, &o.indices);
        materials.push(unlit_material(
            Some("Outline".into()),
            srgba_array(outline_cfg.color),
            false,
        ));
        meshes.push(GltfMesh {
            name: Some("MaquetteOutline".into()),
            primitives: vec![GltfPrimitive {
                attributes: GltfPrimitiveAttrs {
                    position: pos_a,
                    normal: nor_a,
                },
                indices: idx_a,
                material: (materials.len() - 1) as u32,
            }],
        });
        nodes[0].children.push(2);
        nodes.push(GltfNode {
            name: Some("Outline".into()),
            mesh: Some(1),
            children: vec![],
        });
    }

    // GLB has no external URI; .gltf gets a sibling .bin.
    let (buffer_uri, external_bin_uri) = match format {
        ExportFormat::Glb => (None, None),
        ExportFormat::Gltf => {
            let bin_name = out_path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|stem| format!("{stem}.bin"))
                .unwrap_or_else(|| "model.bin".to_string());
            (Some(bin_name.clone()), Some(bin_name))
        }
    };

    let doc = GltfDoc {
        asset: GltfAsset {
            version: "2.0".into(),
            generator: format!("Maquette {}", env!("CARGO_PKG_VERSION")),
        },
        scene: 0,
        scenes: vec![GltfScene { nodes: vec![0] }],
        nodes,
        meshes,
        materials,
        accessors,
        buffer_views,
        buffers: vec![GltfBuffer {
            uri: buffer_uri,
            byte_length: blob.len() as u32,
        }],
        extensions_used: vec!["KHR_materials_unlit".into()],
    };

    (doc, blob, external_bin_uri)
}

fn unlit_material(
    name: Option<String>,
    base_color: [f32; 4],
    double_sided: bool,
) -> GltfMaterial {
    let mut extensions = BTreeMap::new();
    extensions.insert(
        "KHR_materials_unlit".to_string(),
        serde_json::json!({}),
    );
    GltfMaterial {
        name,
        pbr_metallic_roughness: GltfPbr {
            base_color_factor: base_color,
            metallic_factor: 0.0,
            // Roughness=1 so engines that ignore the unlit extension
            // fall back to matte, not mirror.
            roughness_factor: 1.0,
        },
        double_sided,
        extensions,
    }
}

fn srgba_array(c: Color) -> [f32; 4] {
    let s = c.to_srgba();
    [s.red, s.green, s.blue, s.alpha]
}

fn push_positions(
    blob: &mut Vec<u8>,
    accessors: &mut Vec<GltfAccessor>,
    buffer_views: &mut Vec<GltfBufferView>,
    positions: &[[f32; 3]],
) -> u32 {
    align4(blob);
    let byte_offset = blob.len() as u32;
    for p in positions {
        for v in p {
            blob.extend_from_slice(&v.to_le_bytes());
        }
    }
    let byte_length = blob.len() as u32 - byte_offset;

    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        for i in 0..3 {
            if p[i] < min[i] {
                min[i] = p[i];
            }
            if p[i] > max[i] {
                max[i] = p[i];
            }
        }
    }

    buffer_views.push(GltfBufferView {
        buffer: 0,
        byte_offset,
        byte_length,
        target: Some(TARGET_ARRAY_BUFFER),
    });
    accessors.push(GltfAccessor {
        buffer_view: (buffer_views.len() - 1) as u32,
        component_type: COMP_FLOAT,
        count: positions.len() as u32,
        ty: "VEC3",
        // glTF spec requires POSITION accessors to carry min/max;
        // downstream tools (Godot, Blender) use it for bounds culling.
        min: Some(min.to_vec()),
        max: Some(max.to_vec()),
    });
    (accessors.len() - 1) as u32
}

fn push_normals(
    blob: &mut Vec<u8>,
    accessors: &mut Vec<GltfAccessor>,
    buffer_views: &mut Vec<GltfBufferView>,
    normals: &[[f32; 3]],
) -> u32 {
    align4(blob);
    let byte_offset = blob.len() as u32;
    for n in normals {
        for v in n {
            blob.extend_from_slice(&v.to_le_bytes());
        }
    }
    let byte_length = blob.len() as u32 - byte_offset;

    buffer_views.push(GltfBufferView {
        buffer: 0,
        byte_offset,
        byte_length,
        target: Some(TARGET_ARRAY_BUFFER),
    });
    accessors.push(GltfAccessor {
        buffer_view: (buffer_views.len() - 1) as u32,
        component_type: COMP_FLOAT,
        count: normals.len() as u32,
        ty: "VEC3",
        min: None,
        max: None,
    });
    (accessors.len() - 1) as u32
}

fn push_indices(
    blob: &mut Vec<u8>,
    accessors: &mut Vec<GltfAccessor>,
    buffer_views: &mut Vec<GltfBufferView>,
    indices: &[u32],
) -> u32 {
    align4(blob);
    let byte_offset = blob.len() as u32;
    for i in indices {
        blob.extend_from_slice(&i.to_le_bytes());
    }
    let byte_length = blob.len() as u32 - byte_offset;

    buffer_views.push(GltfBufferView {
        buffer: 0,
        byte_offset,
        byte_length,
        target: Some(TARGET_ELEMENT_ARRAY_BUFFER),
    });
    accessors.push(GltfAccessor {
        buffer_view: (buffer_views.len() - 1) as u32,
        component_type: COMP_UNSIGNED_INT,
        count: indices.len() as u32,
        ty: "SCALAR",
        min: None,
        max: None,
    });
    (accessors.len() - 1) as u32
}

fn align4(blob: &mut Vec<u8>) {
    while !blob.len().is_multiple_of(4) {
        blob.push(0);
    }
}

// =====================================================================
// Serializers: .gltf (+.bin) and .glb
// =====================================================================

fn write_gltf_pair(
    path: &Path,
    doc: &GltfDoc,
    blob: &[u8],
    bin_uri: Option<&str>,
) -> Result<(), ExportError> {
    let json = serde_json::to_string_pretty(doc)?;
    fs::write(path, json)?;
    let bin_path = match bin_uri {
        Some(name) => path
            .parent()
            .map(|p| p.join(name))
            .unwrap_or_else(|| PathBuf::from(name)),
        None => path.with_extension("bin"),
    };
    fs::write(bin_path, blob)?;
    Ok(())
}

/// GLB = 12-byte header + JSON chunk (padded with spaces to /4) +
/// optional BIN chunk (padded with zeros to /4). Spec: glTF 2.0 §4.4.
fn write_glb(path: &Path, doc: &GltfDoc, blob: &[u8]) -> Result<(), ExportError> {
    const MAGIC: u32 = 0x4654_6C67; // "glTF"
    const VERSION: u32 = 2;
    const CHUNK_JSON: u32 = 0x4E4F_534A; // "JSON"
    const CHUNK_BIN: u32 = 0x004E_4942; // "BIN\0"

    let json_bytes = serde_json::to_vec(doc)?;
    let json_padded = pad_to_4(&json_bytes, b' ');
    let bin_padded = pad_to_4(blob, 0x00);

    let json_chunk_len = json_padded.len() as u32;
    let bin_chunk_len = bin_padded.len() as u32;

    // 12 header + (8 + json) + (8 + bin) (skip bin section if empty).
    let mut total = 12 + 8 + json_chunk_len;
    if !bin_padded.is_empty() {
        total += 8 + bin_chunk_len;
    }

    let mut f = fs::File::create(path)?;
    f.write_all(&MAGIC.to_le_bytes())?;
    f.write_all(&VERSION.to_le_bytes())?;
    f.write_all(&total.to_le_bytes())?;

    f.write_all(&json_chunk_len.to_le_bytes())?;
    f.write_all(&CHUNK_JSON.to_le_bytes())?;
    f.write_all(&json_padded)?;

    if !bin_padded.is_empty() {
        f.write_all(&bin_chunk_len.to_le_bytes())?;
        f.write_all(&CHUNK_BIN.to_le_bytes())?;
        f.write_all(&bin_padded)?;
    }
    f.flush()?;
    Ok(())
}

fn pad_to_4(data: &[u8], pad: u8) -> Vec<u8> {
    let mut out = data.to_vec();
    while !out.len().is_multiple_of(4) {
        out.push(pad);
    }
    out
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Grid;

    fn tiny_project() -> (Grid, Palette) {
        let mut g = Grid::with_size(4, 4);
        g.paint(0, 0, 0, 1);
        g.paint(1, 0, 2, 2);
        (g, Palette::default())
    }

    #[test]
    fn glb_round_trip_has_expected_magic_and_version() {
        let (g, p) = tiny_project();
        let tmp = std::env::temp_dir().join("maquette_test.glb");
        let opts = ExportOptions {
            path: tmp.clone(),
            format: ExportFormat::Glb,
            outline: OutlineConfig::default(),
        };
        write(&g, &p, &opts).unwrap();
        let bytes = fs::read(&tmp).unwrap();
        // Header: "glTF" magic
        assert_eq!(&bytes[0..4], b"glTF");
        // Version = 2
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 2);
        // Total length = file size
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
            bytes.len()
        );
    }

    #[test]
    fn gltf_text_mode_writes_sibling_bin() {
        let (g, p) = tiny_project();
        let dir = std::env::temp_dir().join("maquette_gltf_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("scene.gltf");
        let opts = ExportOptions {
            path: path.clone(),
            format: ExportFormat::Gltf,
            outline: OutlineConfig::default(),
        };
        write(&g, &p, &opts).unwrap();
        assert!(path.exists());
        assert!(dir.join("scene.bin").exists());
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(json["asset"]["version"], "2.0");
        assert_eq!(json["buffers"][0]["uri"], "scene.bin");
    }

    #[test]
    fn empty_project_returns_error() {
        let g = Grid::with_size(4, 4);
        let p = Palette::default();
        let opts = ExportOptions {
            path: std::env::temp_dir().join("never.glb"),
            format: ExportFormat::Glb,
            outline: OutlineConfig::default(),
        };
        let err = write(&g, &p, &opts).unwrap_err();
        assert!(matches!(err, ExportError::Empty));
    }

    #[test]
    fn outline_off_produces_one_mesh_only() {
        let (g, p) = tiny_project();
        let opts = ExportOptions {
            path: std::env::temp_dir().join("maquette_nooutline.glb"),
            format: ExportFormat::Glb,
            outline: OutlineConfig {
                enabled: false,
                width_pct: 0.0,
                color: Color::BLACK,
            },
        };
        write(&g, &p, &opts).unwrap();
        // Shallow check: inspect JSON chunk to confirm a single mesh.
        let bytes = fs::read(&opts.path).unwrap();
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let json_start = 20;
        let json_bytes = &bytes[json_start..json_start + json_len];
        let s = std::str::from_utf8(json_bytes).unwrap().trim_end_matches(' ');
        let v: serde_json::Value = serde_json::from_str(s).unwrap();
        assert_eq!(v["meshes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn glb_round_trips_through_canonical_parser() {
        let (g, p) = tiny_project();
        let tmp = std::env::temp_dir().join("maquette_parser_roundtrip.glb");
        write(
            &g,
            &p,
            &ExportOptions {
                path: tmp.clone(),
                format: ExportFormat::Glb,
                outline: OutlineConfig::default(),
            },
        )
        .unwrap();

        let (doc, buffers, _images) = ::gltf::import(&tmp).expect("canonical gltf parser");
        // One scene, one root node, two mesh nodes beneath it (body + outline).
        assert_eq!(doc.scenes().count(), 1);
        let root = doc.scenes().next().unwrap().nodes().next().unwrap();
        assert_eq!(root.children().count(), 2);

        // Body must have one primitive per painted color (we painted two).
        let body_mesh = doc
            .meshes()
            .find(|m| m.name() == Some("MaquetteBody"))
            .unwrap();
        assert_eq!(body_mesh.primitives().count(), 2);

        // Every primitive's positions accessor must be readable without
        // errors — this is what catches buffer-view/accessor bugs.
        for mesh in doc.meshes() {
            for prim in mesh.primitives() {
                let reader = prim.reader(|buf| Some(&buffers[buf.index()]));
                let positions: Vec<_> = reader
                    .read_positions()
                    .expect("positions")
                    .collect();
                assert!(!positions.is_empty());
                assert!(reader.read_normals().is_some());
                assert!(reader.read_indices().is_some());
            }
        }
    }

    #[test]
    fn outline_hull_has_same_triangle_count_reversed() {
        let (g, _p) = tiny_project();
        let mut buckets = build_color_buckets(&g);
        for (_, b) in &mut buckets {
            b.translate(0.0, 0.0, 0.0);
        }
        let original_tris: usize = buckets.iter().map(|(_, b)| b.indices.len()).sum();
        let outline = build_outline_hull(&buckets, 0.05);
        assert_eq!(outline.indices.len(), original_tris);
        assert_eq!(
            outline.positions.len(),
            buckets.iter().map(|(_, b)| b.positions.len()).sum::<usize>()
        );
    }
}
