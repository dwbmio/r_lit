//! Spine 萌宠：Mesh2D + 内置 Spine 材质，紧凑 HUD 选播 mascot.json 六序列。

use bevy::camera::{ClearColorConfig, Viewport};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::egui;
use bevy_spine::{
    SkeletonData, SkeletonDataHandle, Spine, SpineDrawer, SpineEvent, SpineMesh, SpineMeshState,
    SpineReadyEvent, SpineSettings,
};
use bevy_spine::rusty_spine::Physics;

use crate::app::{Pet, PetCamera, Settings};
pub use crate::spine_layout::{
    GEAR, HUD_PANEL, LAYOUT_VERSION, NOTIFY, PET_COL_W as SPINE_PET_W, PET_VIEWPORT,
    WIN_H as SPINE_WIN_H, WIN_W as SPINE_WIN_W, HUD_COL_W as SPINE_HUD_W,
};

pub(crate) const SPINE_UI_SCALE: f32 = 0.78;
pub(crate) const SPINE_PET_Y_BIAS: f32 = 2.0;
/// 缩放留白：动画（跳跃/举手）需要比 setup pose 略小的 scale。
const SPINE_FIT_MARGIN: f32 = 0.84;

pub(crate) fn spine_pet_viewport_top() -> f32 {
    PET_VIEWPORT.y
}

pub(crate) fn spine_notify_rect() -> crate::spine_layout::UiRect {
    NOTIFY
}

pub(crate) fn spine_gear_rect() -> crate::spine_layout::UiRect {
    GEAR
}

pub(crate) fn spine_pet_draw_h() -> f32 {
    PET_VIEWPORT.h
}

pub(crate) fn spine_win_w(_hud_open: bool) -> f32 {
    SPINE_WIN_W
}

pub(crate) fn spine_pet_center() -> Vec2 {
    let top = spine_pet_viewport_top();
    Vec2::new(SPINE_PET_W * 0.5, top + spine_pet_draw_h() * 0.5)
}

pub(crate) fn spine_hit_r() -> f32 {
    SPINE_PET_W * 0.44
}

pub(crate) const SPINE_JSON: &str = "spine/out/mascot.json";
pub(crate) const SPINE_ATLAS: &str = "spine/out/mascot.atlas";

pub(crate) const ANIM_SEQUENCES: &[(&str, &str)] = &[
    ("idle", "待机"),
    ("think", "思考"),
    ("sad", "沮丧"),
    ("letter", "信件送达"),
    ("happy", "开心"),
    ("ask", "询问"),
];

#[derive(Component)]
pub(crate) struct SpinePet;

#[derive(Resource, Default)]
pub(crate) struct SpineHud {
    pub selected: usize,
    pub playing: Option<String>,
    pub play_request: bool,
    pub auto_play: bool,
    pub auto_index: usize,
    pub ready: bool,
    pub status: String,
}

pub(crate) fn spine_enabled(assets_dir: &std::path::Path) -> bool {
    if !spine_assets_exist(assets_dir) {
        return false;
    }
    std::env::var("DESKPET_SPINE")
        .ok()
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
}

pub(crate) fn spine_assets_exist(assets_dir: &std::path::Path) -> bool {
    assets_dir.join(SPINE_JSON).is_file() && assets_dir.join(SPINE_ATLAS).is_file()
}

/// setup pose 下所有 region attachment 的世界坐标 AABB（比 skeleton 元数据 512² 更准）。
fn setup_pose_bounds(skeleton: &mut bevy_spine::rusty_spine::Skeleton) -> Option<(f32, f32, f32, f32)> {
    skeleton.set_to_setup_pose();
    skeleton.update_world_transform(Physics::Pose);

    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut any = false;
    let mut verts = [0.0f32; 8];

    for slot in skeleton.slots() {
        let Some(attachment) = slot.attachment() else {
            continue;
        };
        let Some(region) = attachment.as_region() else {
            continue;
        };
        // SAFETY: `slot` 与 `region` 来自同一 skeleton 迭代。
        unsafe {
            region.compute_world_vertices(&slot, &mut verts, 0, 2);
        }
        for i in 0..4 {
            let x = verts[i * 2];
            let y = verts[i * 2 + 1];
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            any = true;
        }
    }

    any.then_some((min_x, min_y, max_x, max_y))
}

/// 在固定宠物视口（SPINE_PET_W × draw_h）内居中，不随 HUD 展收改变。
pub(crate) fn fit_spine_transform(spine: &mut Spine, transform: &mut Transform) -> f32 {
    let (cx, cy, w, h) = setup_pose_bounds(&mut spine.skeleton).map_or_else(
        || {
            let data = spine.skeleton.data();
            let w = data.width().max(1.0);
            let h = data.height().max(1.0);
            (
                data.x() + w * 0.5,
                data.y() + h * 0.5,
                w,
                h,
            )
        },
        |(min_x, min_y, max_x, max_y)| {
            let w = (max_x - min_x).max(1.0);
            let h = (max_y - min_y).max(1.0);
            ((min_x + max_x) * 0.5, (min_y + max_y) * 0.5, w, h)
        },
    );

    let draw_h = spine_pet_draw_h();
    let scale_h = (draw_h * SPINE_FIT_MARGIN) / h;
    let scale_w = (SPINE_PET_W * SPINE_FIT_MARGIN) / w;
    let scale = scale_h.min(scale_w);
    spine.skeleton.set_scale(Vec2::splat(scale));

    transform.scale = Vec3::ONE;
    // 相机视口中心在 (0,0)；略下移，与通知条带留白对齐。
    transform.translation = Vec3::new(-cx * scale, -cy * scale - SPINE_PET_Y_BIAS, 0.0);
    scale
}

pub(crate) fn spawn_spine_pet(
    commands: &mut Commands,
    asset_server: &AssetServer,
    skeletons: &mut Assets<SkeletonData>,
) {
    let skeleton = SkeletonData::new_from_json(
        asset_server.load(SPINE_JSON),
        asset_server.load(SPINE_ATLAS),
    );
    let handle = skeletons.add(skeleton);
    commands.spawn((
        Pet::default(),
        SpinePet,
        Visibility::Visible,
        SkeletonDataHandle(handle),
        Transform::default(),
        SpineSettings {
            update_meshes_when_invisible: true,
            drawer: SpineDrawer::Separated,
            ..Default::default()
        },
    ));
}

pub(crate) fn spawn_spine_camera(commands: &mut Commands) {
    commands.spawn((
        PetCamera,
        Camera2d,
        Camera {
            // 仅清左侧宠物视口；None 避免灰底，与窗口透明合成一致。
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Msaa::Off,
    ));
}

fn play_sequence(spine: &mut Spine, index: usize, auto_preview: bool) -> Option<&'static str> {
    let (name, label) = ANIM_SEQUENCES.get(index)?;
    let looped = if auto_preview {
        false
    } else {
        *name == "idle"
    };
    spine.animation_state.clear_tracks();
    spine.skeleton.set_to_setup_pose();
    if spine
        .animation_state
        .set_animation_by_name(0, name, looped)
        .is_err()
    {
        return None;
    }
    Some(label)
}

pub(crate) fn on_spine_ready(
    mut hud: ResMut<SpineHud>,
    mut events: MessageReader<SpineReadyEvent>,
    mut q: Query<(&mut Spine, &mut Transform), With<SpinePet>>,
) {
    for event in events.read() {
        let Ok((mut spine, mut transform)) = q.get_mut(event.entity) else {
            continue;
        };
        let scale = fit_spine_transform(&mut spine, &mut transform);
        hud.ready = true;
        hud.status.clear();
        let idx = if hud.auto_play {
            hud.auto_index
        } else {
            hud.selected
        };
        if let Some(label) = play_sequence(&mut spine, idx, hud.auto_play) {
            hud.playing = Some(format!("{label} ({})", ANIM_SEQUENCES[idx].0));
        }
        log::info!(
            "deskpet spine: 就绪 mesh2d scale={:.4} pos=({:.1},{:.1})",
            scale,
            transform.translation.x,
            transform.translation.y,
        );
    }
}

pub(crate) fn log_spine_draw_ready(mut logged: Local<bool>, meshes: Query<&SpineMesh>) {
    if *logged {
        return;
    }
    let mut slots = 0usize;
    for mesh in &meshes {
        if matches!(mesh.state, SpineMeshState::Renderable { .. }) {
            slots += 1;
        }
    }
    if slots > 0 {
        *logged = true;
        log::info!("deskpet spine: {slots} 个 slot mesh 已进入 Renderable");
    }
}

pub(crate) fn apply_spine_hud_requests(
    mut hud: ResMut<SpineHud>,
    mut q: Query<&mut Spine, With<SpinePet>>,
) {
    if !hud.play_request {
        return;
    }
    hud.play_request = false;
    if !hud.ready {
        hud.status = "Spine 仍在加载…".into();
        hud.play_request = true;
        return;
    }
    let Ok(mut spine) = q.single_mut() else {
        hud.status = "Spine 实体未找到".into();
        return;
    };
    let idx = if hud.auto_play {
        hud.auto_index
    } else {
        hud.selected
    };
    if let Some(label) = play_sequence(&mut spine, idx, hud.auto_play) {
        hud.playing = Some(format!("{label} ({})", ANIM_SEQUENCES[idx].0));
    } else {
        hud.status = format!("未找到序列 {}", ANIM_SEQUENCES[idx].0);
    }
}

pub(crate) fn advance_spine_auto_play(
    mut hud: ResMut<SpineHud>,
    mut events: MessageReader<SpineEvent>,
    mut q: Query<&mut Spine, With<SpinePet>>,
) {
    if !hud.auto_play {
        return;
    }
    for event in events.read() {
        let SpineEvent::Complete { entity, .. } = event else {
            continue;
        };
        hud.auto_index += 1;
        if hud.auto_index >= ANIM_SEQUENCES.len() {
            hud.auto_play = false;
            hud.playing = Some("预览完成 → 待机".into());
            if let Ok(mut spine) = q.get_mut(*entity) {
                let _ = play_sequence(&mut spine, 0, false);
            }
            return;
        }
        hud.selected = hud.auto_index;
        if let Ok(mut spine) = q.get_mut(*entity) {
            if let Some(label) = play_sequence(&mut spine, hud.auto_index, true) {
                hud.playing = Some(format!(
                    "[{}/{}] {label}",
                    hud.auto_index + 1,
                    ANIM_SEQUENCES.len()
                ));
            }
        }
    }
}

pub(crate) fn sync_spine_visual(
    settings: Res<Settings>,
    mut q: Query<&mut Visibility, With<SpinePet>>,
) {
    let vis = if settings.show_animation {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut v in &mut q {
        *v = vis;
    }
}

/// 启动时校验布局并打日志（便于确认部署版本与区间不重叠）。
pub(crate) fn log_spine_layout_once(mut logged: Local<bool>) {
    if *logged {
        return;
    }
    *logged = true;
    match crate::spine_layout::validate_layout() {
        Ok(()) => log::info!("deskpet spine layout: {}", crate::spine_layout::layout_log_line()),
        Err(e) => log::error!("deskpet spine layout INVALID: {e}"),
    }
}

/// 一次性把窗口设为 Spine 固定尺寸（`init_screen` 也会设；此处防遗漏）。
pub(crate) fn sync_spine_window_layout(
    mascot: Res<crate::Mascot>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
    mut done: Local<bool>,
) {
    if !mascot.use_spine || *done {
        return;
    }
    if let Ok(mut window) = window.single_mut() {
        window.resolution.set(SPINE_WIN_W, SPINE_WIN_H);
        *done = true;
    }
}

/// 相机只渲染左侧宠物条；HUD 展收改变窗口宽时不影响 Spine 坐标系。
pub(crate) fn sync_spine_pet_camera_viewport(
    mascot: Res<crate::Mascot>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Camera, With<PetCamera>>,
    mut cached: Local<Option<(u32, u32, u32)>>,
) {
    if !mascot.use_spine {
        return;
    }
    let Ok(window) = window.single() else {
        return;
    };
    let scale = window.resolution.scale_factor();
    let w = (SPINE_PET_W * scale).round().max(1.0) as u32;
    let draw_h = spine_pet_draw_h();
    let band = (spine_pet_viewport_top() * scale).round().max(0.0) as u32;
    let h = (draw_h * scale).round().max(1.0) as u32;
    let key = (w, h, band);
    if *cached == Some(key) {
        return;
    }
    *cached = Some(key);
    for mut camera in &mut cameras {
        camera.viewport = Some(Viewport {
            physical_position: UVec2::new(0, band),
            physical_size: UVec2::new(w, h),
            depth: 0.0..1.0,
        });
    }
}

pub(crate) fn spine_point_in_notify(px: f32, py: f32) -> bool {
    NOTIFY.contains(px, py)
}

pub(crate) fn spine_point_in_gear(px: f32, py: f32) -> bool {
    GEAR.contains(px, py)
}

pub(crate) fn spine_point_in_hud_panel(px: f32, py: f32) -> bool {
    HUD_PANEL.contains(px, py)
}

pub(crate) fn spine_hud_panel(
    ctx: &egui::Context,
    settings: &mut Settings,
    hud: Option<&mut SpineHud>,
) {
    let glass = egui::Color32::from_rgba_unmultiplied(18, 20, 28, 220);
    let gear_glass = egui::Color32::from_rgba_unmultiplied(18, 20, 28, 205);
    let border = egui::Stroke::new(1.0, egui::Color32::from_rgb(72, 78, 96));

    if !settings.hud_open {
        egui::Area::new(egui::Id::new("spine_gear"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(GEAR.x, GEAR.y))
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_min_size(egui::vec2(GEAR.w, GEAR.h));
                egui::Frame::new()
                    .fill(gear_glass)
                    .stroke(border)
                    .corner_radius(egui::CornerRadius::same(4))
                    .inner_margin(2)
                    .show(ui, |ui| {
                        if ui.button(egui::RichText::new("⚙").size(12.0)).clicked() {
                            settings.hud_open = true;
                        }
                    });
            });
        return;
    }

    egui::Area::new(egui::Id::new("spine_hud"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(HUD_PANEL.x, HUD_PANEL.y))
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(HUD_PANEL.w, HUD_PANEL.h));
            ui.set_max_width(HUD_PANEL.w);
            egui::Frame::new()
                .fill(glass)
                .stroke(border)
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(4)
                .show(ui, |ui| {
                    ui.set_min_width(SPINE_HUD_W - 12.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("动画").size(11.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(egui::RichText::new("–").size(11.0))
                                .on_hover_text("收起")
                                .clicked()
                            {
                                settings.hud_open = false;
                            }
                        });
                    });
                    if let Some(hud) = hud {
                        spine_hud_ui(ui, hud);
                    }
                });
        });
}

pub(crate) fn spine_hud_ui(ui: &mut egui::Ui, hud: &mut SpineHud) {
    let border = egui::Stroke::new(1.0, egui::Color32::from_rgb(72, 78, 96));
    let border_on = egui::Stroke::new(1.5, egui::Color32::from_rgb(90, 150, 240));
    let bg = egui::Color32::from_rgba_unmultiplied(24, 28, 38, 210);
    let bg_on = egui::Color32::from_rgba_unmultiplied(36, 48, 72, 235);
    let bg_panel = egui::Color32::from_rgba_unmultiplied(12, 14, 20, 215);

    ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);

    if !hud.ready {
        ui.label(
            egui::RichText::new("加载…")
                .size(10.0)
                .color(egui::Color32::YELLOW),
        );
        return;
    }
    if !hud.status.is_empty() {
        ui.colored_label(egui::Color32::LIGHT_RED, &hud.status);
    }

    egui::Frame::new()
        .fill(bg_panel)
        .stroke(border)
        .corner_radius(4)
        .inner_margin(4)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("动画切换").size(10.0).strong());
            ui.add_space(2.0);
            for (i, (id, zh)) in ANIM_SEQUENCES.iter().enumerate() {
                let active = hud.selected == i;
                let item = egui::Frame::new()
                    .fill(if active { bg_on } else { bg })
                    .stroke(if active { border_on } else { border })
                    .corner_radius(3)
                    .inner_margin(egui::Margin::symmetric(4, 1));
                item.show(ui, |ui| {
                    let resp = ui
                        .selectable_label(active, egui::RichText::new(*zh).size(10.0))
                        .on_hover_text(*id);
                    if resp.clicked() {
                        hud.selected = i;
                        hud.auto_play = false;
                        hud.play_request = true;
                    }
                });
            }
        });

    if let Some(ref playing) = hud.playing {
        ui.label(
            egui::RichText::new(format!("▶ {playing}"))
                .size(9.0)
                .color(egui::Color32::from_rgb(160, 170, 190)),
        );
    }
}
