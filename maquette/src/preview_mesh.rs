//! GUI-side wiring: turn `maquette::mesher` buckets into `Mesh`
//! entities with the toon material + `OutlineVolume`.
//!
//! Bin-only; the lib's pure `MeshBuilder` holds the geometry and the
//! exporter consumes the same buckets without ever touching Bevy's
//! render pipeline.
//!
//! v0.10 D-1.D added Textured-view support: when
//! `ProjectMeta::texture_prefs.view_mode == Textured` and a palette
//! slot's `texture: Some(TextureHandle)` resolves through
//! `TextureRegistry`, the per-bucket material gets the AI-generated
//! PNG bound as `base_color_texture`. Slots without a texture (or
//! whose PNG didn't decode) fall back to flat colour automatically
//! — the toon shader's optional sampler binding handles both lanes
//! through the same code path.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy_mod_outline::{OutlineMode, OutlineVolume};
use maquette::grid::{Grid, Palette, CELL_SIZE};
use maquette::mesher::{build_color_buckets, build_sphere_instances, MeshBuilder};
use maquette::project::ProjectMeta;
use maquette::texture_meta::PaletteViewMode;

use crate::texture_registry::TextureRegistry;
use crate::toon::ToonMaterial;

const OUTLINE_WIDTH_PX: f32 = 3.0;

#[derive(Component)]
pub struct CellMesh;

/// Marker for per-cell sphere entities (v0.9 shape toggle). Kept
/// separate from [`CellMesh`] so the sphere-sync system can tear
/// down its own entities without touching the cube mesh pipeline.
#[derive(Component)]
pub struct SphereCell;

pub struct PreviewMeshPlugin;

impl Plugin for PreviewMeshPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, rebuild_cell_mesh);
    }
}

#[allow(clippy::too_many_arguments)]
fn rebuild_cell_mesh(
    mut commands: Commands,
    mut grid: ResMut<Grid>,
    palette: Res<Palette>,
    meta: Res<ProjectMeta>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ToonMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut texture_registry: ResMut<TextureRegistry>,
    existing: Query<Entity, With<CellMesh>>,
    existing_spheres: Query<Entity, With<SphereCell>>,
) {
    // v0.10 D-1.D: rebuild on three triggers, not just `grid.dirty`.
    //   1. `grid.dirty`     — paint stroke / shape edit (the original).
    //   2. `palette` changed — colour edited, slot binding flipped, OR
    //                          `PaletteSlotMeta::texture` populated by
    //                          slot_texgen finishing a generate task.
    //   3. `meta` changed   — `texture_prefs.view_mode` toggle (Flat ↔
    //                         Textured), or `model_description`
    //                         re-saved (cosmetic, but a redraw doesn't
    //                         hurt).
    // Any single trigger forces a full rebuild — not the cheapest
    // possible diff, but it matches the existing whole-scene rebuild
    // model and avoids a fiddly per-bucket invalidation pass.
    let needs_rebuild = grid.dirty || palette.is_changed() || meta.is_changed();
    if !needs_rebuild {
        return;
    }

    for e in &existing {
        commands.entity(e).despawn();
    }
    for e in &existing_spheres {
        commands.entity(e).despawn();
    }

    let ox = -(grid.w as f32) * CELL_SIZE * 0.5;
    let oz = -(grid.h as f32) * CELL_SIZE * 0.5;
    let textured = meta.texture_prefs.view_mode == PaletteViewMode::Textured;

    for (ci, builder) in build_color_buckets(&grid) {
        // Palette is sparse since v0.6. A deleted slot referenced by
        // a filled cell "shouldn't" happen (delete paths erase /
        // remap first), but we fall back to white rather than panic
        // so a stale preview can't crash the editor.
        let color = palette.get(ci).unwrap_or(Color::WHITE);
        let mesh = bucket_to_mesh(builder.with_world_origin(ox, oz));
        if mesh.count_vertices() == 0 {
            continue;
        }
        let material = build_slot_material(
            &palette,
            ci,
            color,
            textured,
            &mut texture_registry,
            &mut images,
        );
        commands.spawn((
            CellMesh,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(material)),
            OutlineVolume {
                visible: true,
                width: OUTLINE_WIDTH_PX,
                colour: Color::BLACK,
            },
            OutlineMode::ExtrudeFlat,
            Transform::default(),
        ));
    }

    // Sphere-shape cells render as a vertical stack of unit spheres
    // — one per layer — so a height-3 sphere column reads as three
    // discrete balls rather than a stretched pill. Matches the
    // "voxel-per-layer" semantics cube columns already have: height
    // encodes stack count, not per-block scale.
    //
    // Placeholder path — the cube mesher is the only one the
    // exporter currently consumes, so spheres are preview-only
    // until the export pipeline grows shape support.
    //
    // Sharing the sphere `Mesh` handle across every sphere entity
    // lets Bevy instance the draw call under the hood, which keeps
    // performance bounded even for canvases heavy on spheres
    // (worst case: 128 × 128 × MAX_HEIGHT = ~131k instances, still
    // one-mesh-one-material).
    let sphere_mesh = meshes.add(Sphere::new(CELL_SIZE * 0.5));
    for inst in build_sphere_instances(&grid) {
        let color = palette.get(inst.color_idx).unwrap_or(Color::WHITE);
        let mat = build_slot_material(
            &palette,
            inst.color_idx,
            color,
            textured,
            &mut texture_registry,
            &mut images,
        );
        // Sharing a per-color material handle across every layer of
        // the same column lets Bevy batch draws — allocating one
        // material per layer would defeat the instancing win above.
        let material = materials.add(mat);
        let cx = ox + (inst.grid_x as f32 + 0.5) * CELL_SIZE;
        let cz = oz + (inst.grid_z as f32 + 0.5) * CELL_SIZE;
        let layers = inst.height.max(1);
        for layer in 0..layers {
            // Layer `i` sits between y=i and y=i+1, so its centre is
            // at y=i+0.5. Matches the cube mesher's voxel placement
            // exactly — top of a height-3 column ends at y=3 whether
            // the shape is cube or sphere.
            let cy = (layer as f32 + 0.5) * CELL_SIZE;
            commands.spawn((
                SphereCell,
                Mesh3d(sphere_mesh.clone()),
                MeshMaterial3d(material.clone()),
                OutlineVolume {
                    visible: true,
                    width: OUTLINE_WIDTH_PX,
                    colour: Color::BLACK,
                },
                OutlineMode::ExtrudeFlat,
                Transform::from_xyz(cx, cy, cz),
            ));
        }
    }

    grid.dirty = false;
}

/// Pick the right `ToonMaterial` for a palette slot given the
/// current view mode + texture availability.
///
/// Decision tree:
/// * Flat view → flat colour material, no texture handle.
/// * Textured view + slot has a `cache_key` that loads OK →
///   `base_color_texture: Some(handle)` and `base_color: WHITE`
///   so the texture dominates.
/// * Textured view + slot has no `cache_key`, OR the registry
///   couldn't decode the PNG → fall back to the flat colour
///   material so the user sees *something* sensible (not a
///   missing-texture magenta) for slots they haven't generated yet.
fn build_slot_material(
    palette: &Palette,
    slot_idx: u8,
    color: Color,
    textured: bool,
    registry: &mut TextureRegistry,
    images: &mut Assets<Image>,
) -> ToonMaterial {
    if !textured {
        return ToonMaterial::with_color(color);
    }
    let Some(slot_meta) = palette.meta(slot_idx) else {
        return ToonMaterial::with_color(color);
    };
    let Some(handle) = slot_meta.texture.as_ref() else {
        return ToonMaterial::with_color(color);
    };
    match registry.handle_for(&handle.cache_key, images) {
        Some(image_handle) => ToonMaterial::with_color_and_texture(Color::WHITE, image_handle),
        None => ToonMaterial::with_color(color),
    }
}

fn bucket_to_mesh(builder: MeshBuilder) -> Mesh {
    let MeshBuilder {
        positions,
        normals,
        uvs,
        indices,
    } = builder;
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
