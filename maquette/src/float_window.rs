//! Float/dock toggle for the 3D preview.
//!
//! When the user pops the preview out, we spawn a second OS window
//! plus a dedicated `Camera3d` + `PanOrbitCamera` that renders into
//! it. The main in-editor preview keeps rendering as-is — the user
//! now sees *both* views simultaneously, which is the point (paint
//! on the 2D canvas while an undocked window sits on a second
//! monitor showing the model).
//!
//! When docking (toggle off, or the user clicks the OS close button
//! on the floating window), we copy the orbit state back into the
//! main camera so the in-editor preview picks up wherever the
//! floating one left off.
//!
//! No MCP / no asset pipeline changes — this module is entirely
//! optional UI candy. It lives in the GUI binary only, per the
//! Headless Invariant.

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowClosed, WindowRef, WindowResolution};
use bevy_panorbit_camera::PanOrbitCamera;

use crate::camera::MainPreviewCamera;

/// GUI-only preference: should the preview live in a second window?
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct FloatPreviewState {
    pub floating: bool,
}

/// Marker on the secondary OS window entity.
#[derive(Component)]
pub struct FloatPreviewWindow;

/// Marker on the secondary camera rendering into that window.
#[derive(Component)]
pub struct FloatPreviewCamera;

pub struct FloatWindowPlugin;

impl Plugin for FloatWindowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FloatPreviewState>().add_systems(
            Update,
            (apply_float_state, handle_float_window_closed).chain(),
        );
    }
}

/// React to changes in `FloatPreviewState`. Spawns or despawns the
/// floating window + its camera accordingly. Only runs when the
/// resource actually changes, so the hot path is a single
/// `.is_changed()` check per frame.
#[allow(clippy::too_many_arguments)]
fn apply_float_state(
    state: Res<FloatPreviewState>,
    mut commands: Commands,
    float_windows: Query<Entity, With<FloatPreviewWindow>>,
    float_cameras: Query<(Entity, &PanOrbitCamera), With<FloatPreviewCamera>>,
    mut main_cameras: Query<&mut PanOrbitCamera, (With<MainPreviewCamera>, Without<FloatPreviewCamera>)>,
) {
    if !state.is_changed() {
        return;
    }

    if state.floating {
        // Idempotent: if a float window already exists (hot-reload,
        // double-toggle, ...), don't spawn a second one.
        if !float_windows.is_empty() {
            return;
        }

        // Snapshot the main camera's pose so the floating preview
        // opens onto whatever the user was looking at, not a
        // default reset.
        let pose = main_cameras
            .iter()
            .next()
            .map(snapshot_pose)
            .unwrap_or_default();

        let window = commands
            .spawn((
                Window {
                    title: "Maquette Preview".into(),
                    resolution: WindowResolution::new(720, 720),
                    ..default()
                },
                FloatPreviewWindow,
            ))
            .id();

        commands.spawn((
            Name::new("Float Preview Camera"),
            Camera3d::default(),
            Camera::default(),
            RenderTarget::Window(WindowRef::Entity(window)),
            Transform::default(),
            PanOrbitCamera {
                focus: pose.focus,
                target_focus: pose.focus,
                yaw: Some(pose.yaw),
                pitch: Some(pose.pitch),
                radius: Some(pose.radius),
                target_yaw: pose.yaw,
                target_pitch: pose.pitch,
                target_radius: pose.radius,
                ..default()
            },
            FloatPreviewCamera,
        ));
    } else {
        // Dock: copy pose back into main, then tear down.
        if let Some((_, fc_pan)) = float_cameras.iter().next() {
            if let Some(mut main_pan) = main_cameras.iter_mut().next() {
                main_pan.target_yaw = fc_pan.target_yaw;
                main_pan.target_pitch = fc_pan.target_pitch;
                main_pan.target_radius = fc_pan.target_radius;
                main_pan.target_focus = fc_pan.target_focus;
            }
        }

        for (e, _) in &float_cameras {
            commands.entity(e).despawn();
        }
        for e in &float_windows {
            commands.entity(e).despawn();
        }
    }
}

/// Sync state when the user closes the floating window via the OS
/// title-bar close button. Without this, the "Float" toggle stays
/// stuck `true` even though no window exists, and the next toggle
/// would attempt to re-dock nothing.
///
/// We also guard against the primary window closing: if the user
/// closes the main window the app should exit normally (Bevy's
/// default behavior). We only react to our own floating window.
fn handle_float_window_closed(
    mut events: MessageReader<WindowClosed>,
    float_windows: Query<(), With<FloatPreviewWindow>>,
    primary: Query<Entity, With<PrimaryWindow>>,
    mut state: ResMut<FloatPreviewState>,
) {
    let primary_entity = primary.iter().next();
    for ev in events.read() {
        if Some(ev.window) == primary_entity {
            continue;
        }
        if float_windows.contains(ev.window) {
            state.floating = false;
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Pose {
    yaw: f32,
    pitch: f32,
    radius: f32,
    focus: Vec3,
}

fn snapshot_pose(cam: &PanOrbitCamera) -> Pose {
    Pose {
        yaw: cam.target_yaw,
        pitch: cam.target_pitch,
        radius: cam.target_radius,
        focus: cam.target_focus,
    }
}
