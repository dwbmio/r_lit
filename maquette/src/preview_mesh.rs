//! GUI-side wiring: turn `maquette::mesher` buckets into `Mesh`
//! entities with the toon material + `OutlineVolume`.
//!
//! Bin-only; the lib's pure `MeshBuilder` holds the geometry and the
//! exporter consumes the same buckets without ever touching Bevy's
//! render pipeline.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy_mod_outline::{OutlineMode, OutlineVolume};
use maquette::grid::{Grid, Palette, CELL_SIZE};
use maquette::mesher::{build_color_buckets, build_sphere_instances, MeshBuilder};

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

fn rebuild_cell_mesh(
    mut commands: Commands,
    mut grid: ResMut<Grid>,
    palette: Res<Palette>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ToonMaterial>>,
    existing: Query<Entity, With<CellMesh>>,
    existing_spheres: Query<Entity, With<SphereCell>>,
) {
    if !grid.dirty {
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
        commands.spawn((
            CellMesh,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(ToonMaterial::with_color(color))),
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
        // Sharing a per-color material handle across every layer of
        // the same column lets Bevy batch draws — allocating one
        // material per layer would defeat the instancing win above.
        let material = materials.add(ToonMaterial::with_color(color));
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
