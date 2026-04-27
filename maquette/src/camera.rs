//! Preview camera (`PanOrbitCamera`) and `View → Reset Preview` plumbing.
//!
//! The user never sees the word "camera" — we call it the *preview view*.
//! This module owns the default view parameters and exposes a
//! [`ResetPreviewView`] message the UI can fire to snap back to them.

use std::f32::consts::FRAC_PI_2;

use bevy::camera::{ClearColorConfig, Viewport};
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, RequestRedraw};
use bevy_egui::PrimaryEguiContext;
use bevy_panorbit_camera::PanOrbitCamera;
use maquette::grid::{Grid, CELL_SIZE};

use crate::multiview::{JumpToOrthoView, OrthoKind};

/// Default orbit angles/radius for the preview. Chosen to give a 3/4 view
/// that reads well for a typical 16×16 canvas at 1 world-unit per cell.
pub const DEFAULT_YAW: f32 = 0.54; // ~31° — looks down onto the build plane
pub const DEFAULT_PITCH: f32 = -0.47; // ~-27°
pub const DEFAULT_RADIUS: f32 = 13.0;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ResetPreviewView>()
            .add_message::<FitPreviewToModel>()
            .add_message::<ZoomPreview>()
            .init_resource::<PreviewViewportRect>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (
                    handle_reset_view,
                    handle_fit_to_model,
                    handle_jump_to_ortho,
                    handle_zoom_preview,
                    sync_main_viewport,
                    request_redraw_while_animating,
                ),
            );
    }
}

/// Min/max allowed radius for the main preview orbit camera.
/// Aligned with the Fit-to-Model clamp so buttons, scroll, and
/// `Fit` all bottom out / top out at the same distances. Below
/// `MIN_RADIUS` the camera pokes through geometry; above
/// `MAX_RADIUS` a typical model becomes a speck.
pub const MIN_RADIUS: f32 = 3.0;
pub const MAX_RADIUS: f32 = 120.0;

/// Multiplicative zoom step for a single Zoom button click or
/// key press. Round factor that the user can apply repeatedly —
/// five clicks roughly doubles or halves the view distance.
pub const ZOOM_STEP: f32 = 1.15;

/// Logical-pixel rectangle where the main 3D preview should render —
/// i.e. egui's "central" region after all panels (menu bar, side
/// panel, status bar, etc.) have claimed their space. Written by
/// `ui::ui_system` at the end of the egui frame, read by
/// [`sync_main_viewport`] to scissor the main camera.
///
/// `None` = no UI pass has run yet this frame, or the region is
/// degenerate (0-sized); fall back to the full window.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct PreviewViewportRect {
    pub rect: Option<egui_rect::Rect>,
}

/// Tiny stand-in for an `egui::Rect` so `camera.rs` doesn't take an
/// egui dependency just for a 4-tuple. Same semantics: logical
/// pixels, origin top-left.
pub mod egui_rect {
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Rect {
        pub min_x: f32,
        pub min_y: f32,
        pub max_x: f32,
        pub max_y: f32,
    }
}

/// Fire this to send the preview view back to its default angle and zoom.
#[derive(Message, Default, Clone, Copy)]
pub struct ResetPreviewView;

/// Fire this to frame the painted geometry in the preview. Keeps the
/// current orbit angle, only adjusts focus + radius. When the canvas
/// is empty this falls back to the default view so the user doesn't
/// end up looking at a point 1e-9 units across.
#[derive(Message, Default, Clone, Copy)]
pub struct FitPreviewToModel;

/// Fire this to multiplicatively adjust the preview view's zoom
/// distance. `factor < 1.0` zooms in (smaller radius = closer);
/// `factor > 1.0` zooms out. Result is clamped into
/// `[MIN_RADIUS, MAX_RADIUS]` so spamming the button can't
/// break out of the usable view range.
///
/// Deliberately separate from `ResetPreviewView` + `FitPreviewToModel`
/// because those two re-frame the model; zoom only changes distance
/// along the current orbit ray, preserving yaw/pitch/focus the same
/// way the scroll wheel does.
#[derive(Message, Clone, Copy, Debug)]
pub struct ZoomPreview {
    pub factor: f32,
}

/// Marks the primary perspective camera — the one the UI drives.
/// The float-window plugin uses this to find the camera it needs
/// to redirect / copy pose from without having to care about the
/// PIP cameras next to it.
#[derive(Component)]
pub struct MainPreviewCamera;

fn spawn_camera(mut commands: Commands) {
    // Dedicated "egui host" camera — covers the full window,
    // carries `PrimaryEguiContext`. Decoupling the egui screen-rect
    // from the 3D preview viewport is mandatory: bevy_egui 0.39
    // derives `screen_rect` from whichever camera owns the primary
    // context, so if we pinned the context to the main 3D camera
    // *and* shrunk that camera's viewport to the available central
    // region, egui would see a shrinking screen, panels would
    // re-grab a slice of that, the viewport would shrink again next
    // frame — a visible layout oscillation. Isolating the context
    // on a full-window anchor camera breaks that feedback loop.
    //
    // The camera must stay *active* (bevy_egui's render graph node
    // only executes for active cameras), but we set `ClearColorConfig::None`
    // so it doesn't overwrite what the main 3D camera rendered, and a
    // high `order` so it runs after the 3D + PIP cameras — this
    // effectively makes it "egui only, on top of everything".
    commands.spawn((
        Name::new("Egui Host"),
        Camera2d,
        Camera {
            order: 1000,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        PrimaryEguiContext,
    ));

    commands.spawn((
        MainPreviewCamera,
        Camera3d::default(),
        Camera {
            // Start with a 1×1 placeholder; `sync_main_viewport`
            // expands it to the egui-reported central rect once the
            // first UI frame runs. Without a viewport the camera
            // would render to the full window and `SidePanel::left`
            // would visually offset the focal centre toward the
            // left of the visible region.
            viewport: Some(Viewport {
                physical_position: UVec2::ZERO,
                physical_size: UVec2::new(1, 1),
                ..default()
            }),
            ..default()
        },
        Transform::default(),
        PanOrbitCamera {
            focus: Vec3::ZERO,
            target_focus: Vec3::ZERO,
            yaw: Some(DEFAULT_YAW),
            pitch: Some(DEFAULT_PITCH),
            radius: Some(DEFAULT_RADIUS),
            target_yaw: DEFAULT_YAW,
            target_pitch: DEFAULT_PITCH,
            target_radius: DEFAULT_RADIUS,
            ..default()
        },
    ));
}

fn handle_reset_view(
    mut events: MessageReader<ResetPreviewView>,
    mut cameras: Query<&mut PanOrbitCamera>,
) {
    if events.is_empty() {
        return;
    }
    // Drain regardless of count — we only care that at least one was fired.
    for _ in events.read() {}

    for mut cam in &mut cameras {
        cam.target_focus = Vec3::ZERO;
        cam.target_yaw = DEFAULT_YAW;
        cam.target_pitch = DEFAULT_PITCH;
        cam.target_radius = DEFAULT_RADIUS;
    }
}

/// Snap the main `PanOrbitCamera` to a Top / Front / Side axis-
/// aligned angle. The `PanOrbitCamera`'s smooth interpolation takes
/// care of animating the transition, so the user sees the model
/// rotate into place rather than teleport.
///
/// Pitch sign convention: this crate's `DEFAULT_PITCH = -0.47`
/// already documents that **negative pitch = camera above the
/// target, looking down** for `bevy_panorbit_camera`. So Top is
/// roughly `-π/2`. We clamp just below `π/2` to stay clear of the
/// gimbal-lock pole the plugin's math hits at exactly vertical.
///
/// Focus / radius are preserved so a user who ran Fit first and
/// then clicked Top keeps their carefully-framed model centred.
fn handle_jump_to_ortho(
    mut events: MessageReader<JumpToOrthoView>,
    mut cameras: Query<&mut PanOrbitCamera, With<MainPreviewCamera>>,
) {
    // Near-vertical; exactly FRAC_PI_2 trips gimbal lock in the
    // plugin's spherical-coord math. 0.001 rad ≈ 0.06°, visually
    // indistinguishable from straight-down.
    const TOP_PITCH: f32 = -(FRAC_PI_2 - 0.001);

    // Drain the queue but only act on the *last* request: if the
    // user mashes multiple PIPs in a single frame, honour the most
    // recent one.
    let mut last = None;
    for ev in events.read() {
        last = Some(ev.kind);
    }
    let Some(kind) = last else {
        return;
    };

    let (yaw, pitch) = match kind {
        OrthoKind::Top => (0.0, TOP_PITCH),
        // Front camera in `multiview.rs` sits at (0, ~3, +40) → +Z
        // looking toward -Z. `yaw = 0` in `bevy_panorbit_camera`
        // places the camera at +Z, matching exactly.
        OrthoKind::Front => (0.0, 0.0),
        // Side camera sits at (+40, ~3, 0). Rotating yaw by +π/2
        // around Y moves the orbit from +Z to +X, which is the
        // direction we want.
        OrthoKind::Side => (FRAC_PI_2, 0.0),
    };

    for mut cam in &mut cameras {
        cam.target_yaw = yaw;
        cam.target_pitch = pitch;
        // Deliberately leave `target_focus` and `target_radius`
        // untouched — "jump to this angle" should not overwrite a
        // framing the user just set with Fit / zoom.
    }
}

fn handle_fit_to_model(
    mut events: MessageReader<FitPreviewToModel>,
    grid: Res<Grid>,
    mut cameras: Query<&mut PanOrbitCamera>,
) {
    if events.is_empty() {
        return;
    }
    for _ in events.read() {}

    // World-space bbox of the painted geometry. `preview_mesh`
    // centres the grid on X–Z by subtracting half the grid dims, so
    // the bbox min/max computed here lines up with what the user
    // sees.
    let fit = painted_bbox(&grid).map(|(min, max)| {
        let centre = (min + max) * 0.5;
        let half_diag = ((max - min) * 0.5).length().max(0.5);
        // Radius chosen so the model fills ~70% of the viewport at
        // the default perspective FOV. 2.4× half-diag is a common
        // DCC-tool fit factor and reads comfortably for toon shapes.
        let radius = (half_diag * 2.4).clamp(MIN_RADIUS, MAX_RADIUS);
        (centre, radius)
    });

    let (focus, radius) = match fit {
        Some((c, r)) => (c, r),
        None => (Vec3::ZERO, DEFAULT_RADIUS),
    };

    for mut cam in &mut cameras {
        cam.target_focus = focus;
        cam.target_radius = radius;
        // Leave yaw/pitch alone — "fit" should not spin the model
        // on the user. Reset Preview still exists for that.
    }
}

/// Compose all pending `ZoomPreview` events into a single radius
/// update. Multiplying factors (instead of summing increments) means
/// repeated clicks scale smoothly — five "zoom in" clicks at
/// factor 1/1.15 ≈ ½ the distance, and are the same whether the
/// clicks land on one frame or five.
fn handle_zoom_preview(
    mut events: MessageReader<ZoomPreview>,
    mut cameras: Query<&mut PanOrbitCamera, With<MainPreviewCamera>>,
) {
    let mut factor = 1.0_f32;
    for ev in events.read() {
        factor *= ev.factor;
    }
    if (factor - 1.0).abs() < 1e-4 {
        return;
    }
    for mut cam in &mut cameras {
        cam.target_radius = (cam.target_radius * factor).clamp(MIN_RADIUS, MAX_RADIUS);
    }
}

/// Copy the UI-reported central rect into the main camera's physical
/// viewport. This keeps the 3D preview framed inside the "empty"
/// region between the left SidePanel, the menu/status bars, and the
/// right edge of the window — which is what users mean when they say
/// "the preview should be in the middle of the view".
///
/// Runs every frame. Cheap: single `Query::single_mut` + a few
/// multiplications.
fn sync_main_viewport(
    rect: Res<PreviewViewportRect>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cams: Query<&mut Camera, With<MainPreviewCamera>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(mut cam) = cams.single_mut() else {
        return;
    };

    let scale = window.scale_factor();
    let phys = window.physical_size();

    // Fallback: no UI rect reported yet (first frame), occupy the
    // whole window so the user sees *something* rather than the 1×1
    // placeholder we spawned with.
    let (pos, size) = match rect.rect {
        Some(r) if r.max_x > r.min_x && r.max_y > r.min_y => {
            let px = (r.min_x * scale).round().max(0.0) as u32;
            let py = (r.min_y * scale).round().max(0.0) as u32;
            let pw = ((r.max_x - r.min_x) * scale).round().max(1.0) as u32;
            let ph = ((r.max_y - r.min_y) * scale).round().max(1.0) as u32;
            let pw = pw.min(phys.x.saturating_sub(px)).max(1);
            let ph = ph.min(phys.y.saturating_sub(py)).max(1);
            (UVec2::new(px, py), UVec2::new(pw, ph))
        }
        _ => (UVec2::ZERO, phys.max(UVec2::new(1, 1))),
    };

    if let Some(vp) = cam.viewport.as_mut() {
        if vp.physical_position != pos || vp.physical_size != size {
            vp.physical_position = pos;
            vp.physical_size = size;
        }
    }
}

/// Keep the event loop awake while the `PanOrbitCamera` is still
/// interpolating toward its target pose. Without this, reactive
/// `WinitSettings::desktop_app` would sleep between input events,
/// and smooth animations (Fit, Reset, PIP click) would visibly
/// stutter — the user clicks Top, sees one frame of rotation, then
/// the camera freezes until the next mouse move.
///
/// Cheap: three `f32` comparisons + a `Vec3::length_squared`. No
/// allocations, no queries beyond the already-unique main cam.
fn request_redraw_while_animating(
    cams: Query<&PanOrbitCamera, With<MainPreviewCamera>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    // Squared epsilon — PanOrbit's default damping settles exponentially,
    // so we stop requesting redraws once the delta is below
    // ~0.003 rad / 0.003 world-units, which is visually indistinguishable
    // from the target.
    const EPS_SQ: f32 = 1e-5;

    for cam in &cams {
        let yaw = cam.yaw.unwrap_or(cam.target_yaw);
        let pitch = cam.pitch.unwrap_or(cam.target_pitch);
        let radius = cam.radius.unwrap_or(cam.target_radius);

        let dy = yaw - cam.target_yaw;
        let dp = pitch - cam.target_pitch;
        let dr = radius - cam.target_radius;
        let df = (cam.focus - cam.target_focus).length_squared();

        if dy * dy > EPS_SQ || dp * dp > EPS_SQ || dr * dr > EPS_SQ || df > EPS_SQ {
            redraw.write(RequestRedraw);
            return;
        }
    }
}

/// Axis-aligned bounding box of painted cells in world space, or
/// `None` if nothing is painted. Origin logic mirrors
/// `preview_mesh::rebuild_cell_mesh` so the camera frames exactly
/// what's on screen.
fn painted_bbox(grid: &Grid) -> Option<(Vec3, Vec3)> {
    let ox = -(grid.w as f32) * CELL_SIZE * 0.5;
    let oz = -(grid.h as f32) * CELL_SIZE * 0.5;

    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;

    for y in 0..grid.h {
        for x in 0..grid.w {
            let Some(cell) = grid.get(x, y) else { continue };
            if cell.color_idx.is_none() {
                continue;
            }
            any = true;
            let h = cell.height.max(1) as f32 * CELL_SIZE;
            let x0 = ox + x as f32 * CELL_SIZE;
            let z0 = oz + y as f32 * CELL_SIZE;
            min = min.min(Vec3::new(x0, 0.0, z0));
            max = max.max(Vec3::new(x0 + CELL_SIZE, h, z0 + CELL_SIZE));
        }
    }

    if any {
        Some((min, max))
    } else {
        None
    }
}
