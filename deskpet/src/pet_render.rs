//! 萌宠 NPR Toon：Unlit 纹理原色 + 运行时描边（蒙皮 mesh 等比放大 + Cull Front）。
//!
//! GLB 加载后资产级 + 实例级双重 patch，确保不走 PBR 光照。

use std::collections::HashSet;

use bevy::asset::{AssetEvent, AssetId, Assets};
use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::gltf::Gltf;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::render::render_resource::Face;

use crate::app::{Pet, PetCamera};

/// 三点布光（仅 HUD 关闭 Toon、回退 PBR 时使用）。
#[derive(Component)]
pub(crate) struct StudioLight;

/// Blender 导出的描边 mesh（名称含 outline/ink/_cel）。
#[derive(Component)]
pub(crate) struct AnimeOutline;

/// 运行时 inverted-hull 描边实体（勿再处理）。
#[derive(Component)]
pub(crate) struct OutlineHull;

#[derive(Component)]
pub(crate) struct BrightMaterialApplied;

#[derive(Resource)]
pub(crate) struct OutlineMaterial(pub Handle<StandardMaterial>);

#[derive(Resource, Default)]
pub(crate) struct PatchedGltfs(HashSet<AssetId<Gltf>>);

const OUTLINE_SCALE: f32 = 1.052;
const PBR_EXPOSURE: f32 = 12.5;
const PBR_AMBIENT: f32 = 2_200.0;

pub fn init_render_resources(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.init_resource::<PatchedGltfs>();
    let handle = materials.add(StandardMaterial {
        base_color: Color::srgb(0.012, 0.012, 0.02),
        unlit: true,
        cull_mode: Some(Face::Front),
        ..default()
    });
    commands.insert_resource(OutlineMaterial(handle));
}

pub fn setup_studio_lighting(commands: &mut Commands) {
    let target = Vec3::new(0.0, 0.9, 0.0);
    for (pos, color, lux) in [
        (Vec3::new(2.2, 4.5, 3.2), Color::srgb(1.0, 0.98, 0.95), 22_000.0),
        (Vec3::new(-2.8, 2.0, 3.5), Color::srgb(0.82, 0.9, 1.0), 10_000.0),
        (Vec3::new(0.0, 2.5, -3.5), Color::srgb(1.0, 0.92, 0.88), 6_500.0),
    ] {
        commands.spawn((
            StudioLight,
            Visibility::Hidden,
            DirectionalLight {
                color,
                illuminance: lux,
                shadows_enabled: false,
                ..default()
            },
            Transform::from_translation(pos).looking_at(target, Vec3::Y),
        ));
    }
}

fn is_outline_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("outline") || n.contains("ink") || n.ends_with("_cel")
}

/// Toon：Unlit + base_color=WHITE（纹理原色）；Outline：纯黑 Unlit。
fn tune_material(mut m: StandardMaterial, toon: bool, outline: bool) -> StandardMaterial {
    if outline {
        return StandardMaterial {
            base_color: Color::srgb(0.012, 0.012, 0.02),
            unlit: true,
            alpha_mode: m.alpha_mode,
            cull_mode: Some(Face::Back),
            ..default()
        };
    }

    if toon {
        let has_tex = m.base_color_texture.is_some();
        m.unlit = true;
        m.metallic = 0.0;
        m.perceptual_roughness = 1.0;
        m.emissive = LinearRgba::BLACK.into();
        m.normal_map_texture = None;
        m.metallic_roughness_texture = None;
        m.occlusion_texture = None;
        m.emissive_texture = None;
        m.reflectance = 0.0;
        m.cull_mode = Some(Face::Back);
        if has_tex {
            m.base_color = Color::WHITE;
        }
        return m;
    }

    m.unlit = false;
    m.metallic = m.metallic * 0.35;
    m.perceptual_roughness = m.perceptual_roughness.clamp(0.65, 0.95);
    m.emissive = (LinearRgba::from(m.base_color) * 0.14).into();
    m
}

fn patch_gltf_asset(
    gltf: &Gltf,
    settings: &crate::Settings,
    materials: &mut Assets<StandardMaterial>,
) {
    for handle in &gltf.materials {
        if let Some(mat) = materials.get_mut(handle) {
            *mat = tune_material(mat.clone(), settings.toon_shading, false);
        }
    }
}

fn try_patch_gltf(
    id: AssetId<Gltf>,
    gltfs: &Assets<Gltf>,
    settings: &crate::Settings,
    materials: &mut Assets<StandardMaterial>,
    patched: &mut PatchedGltfs,
) {
    if patched.0.contains(&id) {
        return;
    }
    let Some(gltf) = gltfs.get(id) else {
        return;
    };
    patch_gltf_asset(gltf, settings, materials);
    patched.0.insert(id);
    log::info!(
        "deskpet: GLB 材质 → {} ({} material)",
        if settings.toon_shading {
            "Toon Unlit"
        } else {
            "PBR"
        },
        gltf.materials.len()
    );
}

/// GLB 加载后立即 patch 资产材质（并扫描未 patch 的已加载 GLB）。
pub fn patch_gltf_materials(
    mascot: Res<crate::Mascot>,
    mut events: MessageReader<AssetEvent<Gltf>>,
    gltfs: Res<Assets<Gltf>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    settings: Res<crate::Settings>,
    mut patched: ResMut<PatchedGltfs>,
) {
    if mascot.use_spine {
        return;
    }
    for ev in events.read() {
        let id = match ev {
            AssetEvent::Added { id } | AssetEvent::LoadedWithDependencies { id } => *id,
            _ => continue,
        };
        try_patch_gltf(id, &gltfs, &settings, &mut materials, &mut patched);
    }
    for (id, _) in gltfs.iter() {
        try_patch_gltf(id, &gltfs, &settings, &mut materials, &mut patched);
    }
}

/// 实例级：替换 mesh 材质 handle；Toon 下追加蒙皮描边副本。
pub fn apply_bright_materials(
    mut commands: Commands,
    settings: Res<crate::Settings>,
    mascot: Res<crate::Mascot>,
    outline_mat: Option<Res<OutlineMaterial>>,
    pet: Query<Entity, With<Pet>>,
    children: Query<&Children>,
    child_of: Query<&ChildOf>,
    pending: Query<
        (
            Entity,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
            Option<&Name>,
            Option<&SkinnedMesh>,
        ),
        (
            Without<BrightMaterialApplied>,
            Without<OutlineHull>,
        ),
    >,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
) {
    if mascot.use_spine {
        return;
    }
    let Ok(pet_e) = pet.single() else {
        return;
    };

    let mut stack = vec![pet_e];
    while let Some(e) = stack.pop() {
        if let Ok(kids) = children.get(e) {
            stack.extend(kids.iter());
        }
        let Ok((entity, mesh, mm, name, skinned)) = pending.get(e) else {
            continue;
        };
        let Some(base) = std_mats.get(&mm.0).cloned() else {
            continue;
        };
        let named_outline = name
            .map(|n| is_outline_name(n.as_str()))
            .unwrap_or(false);
        let handle = std_mats.add(tune_material(
            base,
            settings.toon_shading,
            named_outline,
        ));
        commands
            .entity(entity)
            .insert((MeshMaterial3d(handle), BrightMaterialApplied));
        if named_outline {
            commands.entity(entity).insert(AnimeOutline);
            continue;
        }

        if !settings.toon_shading {
            continue;
        }
        let Some(outline_res) = outline_mat.as_ref() else {
            continue;
        };
        let parent = child_of
            .get(entity)
            .map(|c| c.parent())
            .unwrap_or(entity);
        let mut outline_entity = commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(outline_res.0.clone()),
            Transform::from_scale(Vec3::splat(OUTLINE_SCALE)),
            OutlineHull,
            BrightMaterialApplied,
        ));
        if let Some(sk) = skinned {
            outline_entity.insert(sk.clone());
        }
        let outline_id = outline_entity.id();
        commands.entity(parent).add_child(outline_id);
    }
}

/// Toon 下关闭 Tonemapping / 曝光 / 环境光，避免 Unlit 仍被后处理「打光」。
pub fn sync_toon_camera(
    settings: Res<crate::Settings>,
    mascot: Res<crate::Mascot>,
    mut cameras: Query<(&mut Tonemapping, &mut Exposure, &mut AmbientLight), With<PetCamera>>,
    mut was_toon: Local<Option<bool>>,
) {
    if mascot.use_spine {
        return;
    }
    let toon = settings.toon_shading;
    if *was_toon == Some(toon) && !settings.is_changed() {
        return;
    }
    *was_toon = Some(toon);

    for (mut tonemapping, mut exposure, mut ambient) in &mut cameras {
        if toon {
            *tonemapping = Tonemapping::None;
            exposure.ev100 = 0.0;
            ambient.brightness = 0.0;
        } else {
            *tonemapping = Tonemapping::AcesFitted;
            exposure.ev100 = PBR_EXPOSURE;
            ambient.brightness = PBR_AMBIENT;
        }
    }
}

pub fn sync_material_tuning(
    settings: Res<crate::Settings>,
    mascot: Res<crate::Mascot>,
    gltfs: Res<Assets<Gltf>>,
    patched: Res<PatchedGltfs>,
    applied: Query<
        (&MeshMaterial3d<StandardMaterial>, Option<&AnimeOutline>),
        (With<BrightMaterialApplied>, Without<OutlineHull>),
    >,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
    mut lights: Query<&mut Visibility, With<StudioLight>>,
    mut was_toon: Local<bool>,
) {
    if mascot.use_spine {
        return;
    }
    if settings.toon_shading == *was_toon && !settings.is_changed() {
        return;
    }
    *was_toon = settings.toon_shading;

    let light_vis = if settings.toon_shading {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };
    for mut vis in &mut lights {
        *vis = light_vis;
    }

    for id in &patched.0 {
        if let Some(gltf) = gltfs.get(*id) {
            patch_gltf_asset(gltf, &settings, &mut std_mats);
        }
    }

    for (mm, outline) in &applied {
        if let Some(mat) = std_mats.get_mut(&mm.0) {
            *mat = tune_material(
                mat.clone(),
                settings.toon_shading,
                outline.is_some(),
            );
        }
    }
}
