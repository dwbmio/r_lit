//! Preview camera (`PanOrbitCamera`) and `View → Reset Preview` plumbing.
//!
//! The user never sees the word "camera" — we call it the *preview view*.
//! This module owns the default view parameters and exposes a
//! [`ResetPreviewView`] message the UI can fire to snap back to them.

use bevy::prelude::*;
use bevy_panorbit_camera::PanOrbitCamera;
use maquette::grid::{Grid, CELL_SIZE};

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
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, (handle_reset_view, handle_fit_to_model));
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

/// Marks the primary perspective camera — the one the UI drives.
/// The float-window plugin uses this to find the camera it needs
/// to redirect / copy pose from without having to care about the
/// PIP cameras next to it.
#[derive(Component)]
pub struct MainPreviewCamera;

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        MainPreviewCamera,
        Camera3d::default(),
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
        let radius = (half_diag * 2.4).clamp(3.0, 120.0);
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
