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
use maquette::mesher::{build_color_buckets, MeshBuilder};

use crate::toon::ToonMaterial;

const OUTLINE_WIDTH_PX: f32 = 3.0;

#[derive(Component)]
pub struct CellMesh;

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
) {
    if !grid.dirty {
        return;
    }

    for e in &existing {
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
