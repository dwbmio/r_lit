//! 萌宠三点布光 + 材质提亮（StandardMaterial，避免自定义 shader 导致模型不可见）。

use bevy::prelude::*;

use crate::app::Pet;

/// 三点布光：主光 + 补光 + 轮廓光（萌宠在 world 原点附近）。
pub fn setup_studio_lighting(commands: &mut Commands) {
    let target = Vec3::new(0.0, 0.9, 0.0);

    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.98, 0.95),
            illuminance: 22_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(2.2, 4.5, 3.2).looking_at(target, Vec3::Y),
    ));

    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.82, 0.9, 1.0),
            illuminance: 10_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(-2.8, 2.0, 3.5).looking_at(target, Vec3::Y),
    ));

    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.92, 0.88),
            illuminance: 6_500.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, 2.5, -3.5).looking_at(target, Vec3::Y),
    ));
}

#[derive(Component)]
pub(crate) struct BrightMaterialApplied;

fn tune_material(mut m: StandardMaterial, toon: bool) -> StandardMaterial {
    m.metallic = m.metallic * 0.35;
    m.perceptual_roughness = m.perceptual_roughness.clamp(0.65, 0.95);
    let emissive_scale = if toon { 0.22 } else { 0.14 };
    m.emissive = (LinearRgba::from(m.base_color) * emissive_scale).into();
    if toon {
        // 无自定义 shader 时，用更高环境反射近似卡通亮面。
        m.base_color = Color::from(m.base_color).with_saturation(0.92);
    }
    m
}

/// 将萌宠 GLB 子网格材质提亮（StandardMaterial，只处理一次）。
pub fn apply_bright_materials(
    mut commands: Commands,
    settings: Res<crate::Settings>,
    pet: Query<Entity, With<Pet>>,
    children: Query<&Children>,
    pending: Query<
        (Entity, &MeshMaterial3d<StandardMaterial>),
        (Without<BrightMaterialApplied>,),
    >,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
) {
    let Ok(pet_e) = pet.single() else {
        return;
    };

    let mut stack = vec![pet_e];
    while let Some(e) = stack.pop() {
        if let Ok(kids) = children.get(e) {
            stack.extend(kids.iter());
        }
        let Ok((entity, mm)) = pending.get(e) else {
            continue;
        };
        let Some(base) = std_mats.get(&mm.0).cloned() else {
            continue;
        };
        let handle = std_mats.add(tune_material(base, settings.toon_shading));
        commands
            .entity(entity)
            .insert((MeshMaterial3d(handle), BrightMaterialApplied));
    }
}

/// HUD 切换 Toon 时更新已创建材质参数。
pub fn sync_material_tuning(
    settings: Res<crate::Settings>,
    applied: Query<&MeshMaterial3d<StandardMaterial>, With<BrightMaterialApplied>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
    mut was_toon: Local<bool>,
) {
    if settings.toon_shading == *was_toon && !settings.is_changed() {
        return;
    }
    *was_toon = settings.toon_shading;
    for mm in &applied {
        if let Some(mat) = std_mats.get_mut(&mm.0) {
            *mat = tune_material(mat.clone(), settings.toon_shading);
        }
    }
}
