//! Multi-angle preview — three orthographic PIPs (Top / Front / Side)
//! docked in the bottom-right corner of the main window.
//!
//! Original product brief asked for a "T-shape, multi-face preview"
//! window that stays visible while you paint. We ship that as
//! picture-in-picture overlays on top of the main perspective camera:
//!
//! * The main `PanOrbitCamera` keeps rendering to the whole window
//!   exactly as it did in v0.7. Users who disable multi-view get the
//!   v0.7 preview unchanged.
//! * Three extra `Camera3d`s with orthographic projections and tiny
//!   physical viewports cover the bottom-right corner. They clear
//!   their own viewport to a slightly darker bg so the PIPs read as
//!   separate panels, and render the exact same entities (meshes +
//!   outlines) from three canonical angles.
//!
//! Viewports are synced to the live window size every frame — on a
//! resize the PIPs stay anchored to the bottom-right corner.
//!
//! Toggle: `View → Multi-view Preview` (F2). Persisted only in
//! [`MultiViewState`], which is not serialised to the `.maq` file
//! — it's a viewport preference, not project data.

use bevy::camera::{ClearColorConfig, ScalingMode, Viewport};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// Whether the three ortho PIPs are rendered and how big they are.
/// Resource kept in `UiState` siblinghood, tweakable via the
/// menu. Default = `enabled = true` so new installs see the feature
/// the first time they launch; users who want the v0.7 look can
/// disable it.
#[derive(Resource, Debug, Clone, Copy)]
pub struct MultiViewState {
    pub enabled: bool,
    /// Side length of one PIP in logical pixels (before DPI scaling).
    pub panel_size: u32,
    /// Gap between PIPs and between the strip and the window edge.
    pub gap: u32,
    /// How much space to leave above the bottom window edge — roughly
    /// the status-bar height so the PIPs never sit behind egui.
    pub bottom_reserved: u32,
}

impl Default for MultiViewState {
    fn default() -> Self {
        Self {
            enabled: true,
            panel_size: 180,
            gap: 8,
            bottom_reserved: 32,
        }
    }
}

/// Attached to each PIP camera so we can identify and order them.
#[derive(Component, Clone, Copy)]
pub struct OrthoView {
    pub kind: OrthoKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrthoKind {
    Top,
    Front,
    Side,
}

impl OrthoKind {
    /// Column index, left-to-right. Drives which PIP lines up where
    /// in the bottom-right strip.
    pub fn column(self) -> u32 {
        match self {
            OrthoKind::Top => 0,
            OrthoKind::Front => 1,
            OrthoKind::Side => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            OrthoKind::Top => "Top",
            OrthoKind::Front => "Front",
            OrthoKind::Side => "Side",
        }
    }
}

pub struct MultiViewPlugin;

impl Plugin for MultiViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MultiViewState>()
            .add_systems(Startup, spawn_ortho_cameras)
            .add_systems(Update, (apply_enabled, sync_viewports).chain());
    }
}

/// Fixed ortho viewport height in world units. 14 covers a full
/// MAX_GRID (128) canvas? No — but it covers the typical 16×16 case
/// with a comfortable margin. Users with larger canvases will see the
/// sides clip; that's acceptable for a glance-only reference view
/// and will be revisited if real users complain.
const ORTHO_VIEWPORT: f32 = 14.0;

fn spawn_ortho_cameras(mut commands: Commands) {
    // Top: camera high above Y axis, looking straight down. "Up" on
    // screen = +Z so the canvas reads the same way as the 2D paint
    // panel (x right, z "up" toward the top of the screen).
    commands.spawn((
        Name::new("Ortho Top"),
        Camera3d::default(),
        Camera {
            order: 1,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::srgba(0.055, 0.065, 0.08, 1.0)),
            viewport: Some(placeholder_viewport()),
            ..default()
        },
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: ORTHO_VIEWPORT,
            },
            ..OrthographicProjection::default_3d()
        }),
        // Nudge slightly off-axis so `looking_at` has a defined frame
        // (a pure +Y / +up alignment is fine, but keeping a tiny +X
        // bias future-proofs us against floating-point edge cases).
        Transform::from_xyz(0.0001, 40.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z),
        OrthoView {
            kind: OrthoKind::Top,
        },
    ));

    // Front: camera at +Z looking toward -Z. "Up" = +Y. Y-centred on
    // 3 so the typical low-column shape sits near the middle of the
    // viewport instead of hugging the floor.
    commands.spawn((
        Name::new("Ortho Front"),
        Camera3d::default(),
        Camera {
            order: 2,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::srgba(0.055, 0.065, 0.08, 1.0)),
            viewport: Some(placeholder_viewport()),
            ..default()
        },
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: ORTHO_VIEWPORT,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, 3.0, 40.0).looking_at(Vec3::new(0.0, 3.0, 0.0), Vec3::Y),
        OrthoView {
            kind: OrthoKind::Front,
        },
    ));

    // Side: camera at +X looking toward -X.
    commands.spawn((
        Name::new("Ortho Side"),
        Camera3d::default(),
        Camera {
            order: 3,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::srgba(0.055, 0.065, 0.08, 1.0)),
            viewport: Some(placeholder_viewport()),
            ..default()
        },
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: ORTHO_VIEWPORT,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(40.0, 3.0, 0.0).looking_at(Vec3::new(0.0, 3.0, 0.0), Vec3::Y),
        OrthoView {
            kind: OrthoKind::Side,
        },
    ));
}

/// Turn the PIP cameras on/off when `MultiViewState.enabled` flips.
/// We don't despawn — flipping `is_active` is cheap and keeps camera
/// transforms stable across toggles.
fn apply_enabled(state: Res<MultiViewState>, mut cams: Query<&mut Camera, With<OrthoView>>) {
    if !state.is_changed() {
        return;
    }
    for mut cam in &mut cams {
        cam.is_active = state.enabled;
    }
}

/// Place the three PIPs in the bottom-right corner every frame. Cheap
/// (3 entities), and running unconditionally means the PIPs follow
/// window resizes without a `WindowResized` subscription.
fn sync_viewports(
    state: Res<MultiViewState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cams: Query<(&mut Camera, &OrthoView)>,
) {
    if !state.enabled {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let phys = window.physical_size();
    let scale = window.scale_factor();
    let panel_px = ((state.panel_size as f32) * scale) as u32;
    let gap_px = ((state.gap as f32) * scale) as u32;
    let bottom_px = ((state.bottom_reserved as f32) * scale) as u32;

    // Guard against freshly-created windows where physical size is 0.
    if phys.x < panel_px || phys.y < panel_px + bottom_px {
        return;
    }

    let y = phys.y.saturating_sub(bottom_px + panel_px);
    // Three panels anchored to the right edge.
    let right_edge = phys.x.saturating_sub(gap_px);

    for (mut cam, view) in &mut cams {
        // Columns go right-to-left: index 0 sits furthest from the
        // edge. That keeps the "Side" (index 2) panel pinned to the
        // window corner — which is the view the user glances at most
        // when painting on the 2D canvas on the left.
        let col = view.kind.column();
        let panels_from_edge = 3 - col;
        let x_right = right_edge.saturating_sub(panels_from_edge.saturating_sub(1) * (panel_px + gap_px));
        let x_left = x_right.saturating_sub(panel_px);

        if let Some(vp) = cam.viewport.as_mut() {
            vp.physical_position = UVec2::new(x_left, y);
            vp.physical_size = UVec2::new(panel_px, panel_px);
        }
    }
}

fn placeholder_viewport() -> Viewport {
    Viewport {
        physical_position: UVec2::ZERO,
        // 1×1 so Bevy doesn't reject a zero-area viewport before we
        // get to sync it in the first frame.
        physical_size: UVec2::new(1, 1),
        ..default()
    }
}

/// Returns the on-screen rectangle each PIP occupies in **logical**
/// (egui) pixels. Used by the UI to draw labels above each panel
/// without re-deriving the math from the physical-pixel viewport.
pub fn pip_logical_rects(window: &Window, state: &MultiViewState) -> [PipRect; 3] {
    let w = window.width();
    let h = window.height();
    let size = state.panel_size as f32;
    let gap = state.gap as f32;
    let bottom = state.bottom_reserved as f32;
    let y_top = h - bottom - size;

    let mut out = [PipRect {
        kind: OrthoKind::Top,
        x: 0.0,
        y: 0.0,
        size: 0.0,
    }; 3];
    for (i, kind) in [OrthoKind::Top, OrthoKind::Front, OrthoKind::Side]
        .into_iter()
        .enumerate()
    {
        let panels_from_edge = 3 - kind.column();
        let x_right = w - gap - (panels_from_edge.saturating_sub(1) as f32) * (size + gap);
        let x_left = x_right - size;
        out[i] = PipRect {
            kind,
            x: x_left,
            y: y_top,
            size,
        };
    }
    out
}

#[derive(Clone, Copy, Debug)]
pub struct PipRect {
    pub kind: OrthoKind,
    pub x: f32,
    pub y: f32,
    pub size: f32,
}
