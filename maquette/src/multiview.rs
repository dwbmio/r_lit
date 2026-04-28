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

use crate::camera::PreviewViewportRect;

/// Whether the three ortho PIPs are rendered and how big they are.
/// Resource kept in `UiState` siblinghood, tweakable via the
/// menu. Default = `enabled = true` so new installs see the feature
/// the first time they launch; users who want the v0.7 look can
/// disable it.
///
/// ### Sizing model
///
/// Panel size is **proportional to window width**, not a fixed
/// pixel value. Early versions used a hard-coded 180 px per panel,
/// which read as comfortable on a 1024 px window (~53 % of width)
/// but microscopic on a 4K monitor and oppressively large on a
/// 13" laptop half-screen. We now lock the three-PIP strip to a
/// consistent fraction of window width, with a sensible per-panel
/// clamp so the view doesn't become either unreadable on tiny
/// windows or wastefully gigantic on ultrawide monitors. See
/// [`compute_panel_size`] for the math.
#[derive(Resource, Debug, Clone, Copy)]
pub struct MultiViewState {
    pub enabled: bool,
    /// Fraction of the primary window's logical width that the
    /// entire three-PIP strip (including the two inter-panel gaps)
    /// is allowed to occupy. 0.38 ≈ three mid-sized panels on a
    /// typical 1280 px editor window.
    pub strip_fraction: f32,
    /// Gap between PIPs and between the strip and the window edge.
    pub gap: u32,
    /// How much space to leave above the bottom window edge — roughly
    /// the status-bar height so the PIPs never sit behind egui.
    pub bottom_reserved: u32,
    /// Hard floor on per-panel size. Below this, labels and axis
    /// ticks become illegible; when a window is very narrow we'd
    /// rather let the strip spill past `strip_fraction` than shrink
    /// into pixel soup.
    pub min_panel_size: u32,
    /// Hard ceiling on per-panel size. Above this, the PIPs start
    /// eating into the main perspective preview and feel bloated;
    /// on wide monitors we stop growing and let the "free" space
    /// go to the main view.
    pub max_panel_size: u32,
}

impl Default for MultiViewState {
    fn default() -> Self {
        Self {
            enabled: true,
            strip_fraction: 0.38,
            gap: 8,
            bottom_reserved: 32,
            min_panel_size: 100,
            max_panel_size: 160,
        }
    }
}

/// Derive the current per-panel logical side length from the
/// window width and the strip-fraction policy in the state. The
/// strip contains three panels and two gaps; we solve for the
/// panel side and clamp into the `min / max` band.
///
/// Pure function so `sync_viewports` (physical-pixel viewport
/// math) and `pip_logical_rects` (logical-pixel label placement)
/// always agree on what size the PIPs are — any divergence here
/// produced misaligned label boxes in v0.8.
pub fn compute_panel_size(state: &MultiViewState, window_width_logical: f32) -> f32 {
    let strip_width = window_width_logical * state.strip_fraction;
    let raw = (strip_width - 2.0 * state.gap as f32) / 3.0;
    raw.clamp(state.min_panel_size as f32, state.max_panel_size as f32)
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
            .add_message::<JumpToOrthoView>()
            .add_systems(Startup, spawn_ortho_cameras)
            .add_systems(Update, (apply_enabled, sync_viewports).chain());
    }
}

/// Sent when the user clicks one of the Top / Front / Side PIPs (or
/// uses the corresponding keyboard shortcut). The camera module
/// listens and snaps the main `PanOrbitCamera` to that canonical
/// angle so the user can inspect the model from a pure axis view
/// without giving up the ability to rotate afterwards.
///
/// Intentionally does *not* swap the main camera's projection to
/// orthographic — the PIPs on the right already cover that use
/// case. This message is about "rotate the perspective preview to
/// look from this direction", which is the natural way to read the
/// gesture "I clicked the Top thumbnail".
#[derive(Message, Clone, Copy, Debug)]
pub struct JumpToOrthoView {
    pub kind: OrthoKind,
}

/// Fixed ortho viewport height in world units. 14 covers a full
/// MAX_GRID (128) canvas? No — but it covers the typical 16×16 case
/// with a comfortable margin. Users with larger canvases will see the
/// sides clip; that's acceptable for a glance-only reference view
/// and will be revisited if real users complain.
const ORTHO_VIEWPORT: f32 = 14.0;

fn spawn_ortho_cameras(mut commands: Commands) {
    // Top: camera high above the Y axis, looking straight down.
    //
    // Coordinate alignment with the 2D paint canvas is critical here:
    // the canvas uses egui's y-down screen convention, and cell (x, y)
    // is emitted into world (x, 0, y) (see `preview_mesh.rs` origin).
    // For Top to *match* the 2D canvas pixel-for-pixel, we need:
    //   * world +X → right on screen
    //   * world +Z → down on screen (so canvas y-down = Top y-down)
    // Using `up = +Z` would give world +Z → up (inverted rows) AND,
    // via the right-hand rule, world +X → left (mirrored columns),
    // i.e. a 180° rotation vs. the painting surface. `NEG_Z` as up
    // flips both back into alignment.
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
        // Tiny +X bias keeps `looking_at` numerically well-defined
        // against a near-parallel up vector.
        Transform::from_xyz(0.0001, 40.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z),
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

/// Place the three PIPs in the bottom-right corner of the central
/// preview area every frame. Cheap (3 entities), and running
/// unconditionally means the PIPs follow window resizes without a
/// `WindowResized` subscription.
///
/// "Central preview area" is what's left after egui's panels (left
/// canvas, right Block Library, menu bar, status bar) have claimed
/// their slices — `PreviewViewportRect` is populated by
/// `ui::ui_system` from `ctx.available_rect()`. Without consulting
/// that resource the PIPs would overlap the right SidePanel, which
/// is the v0.10 C-2 regression that prompted this rewrite.
fn sync_viewports(
    state: Res<MultiViewState>,
    central: Res<PreviewViewportRect>,
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
    let panel_logical = compute_panel_size(&state, window.width());
    let panel_px = (panel_logical * scale) as u32;
    let gap_px = ((state.gap as f32) * scale) as u32;
    let bottom_px = ((state.bottom_reserved as f32) * scale) as u32;

    // Right / bottom edges of the central area, in physical pixels.
    // Falls back to the full window when the UI hasn't reported a
    // rect yet (first frame).
    let (central_right_px, central_bottom_px) = match central.rect {
        Some(r) if r.max_x > r.min_x && r.max_y > r.min_y => {
            let rx = (r.max_x * scale).round().max(1.0) as u32;
            let by = (r.max_y * scale).round().max(1.0) as u32;
            (rx.min(phys.x), by.min(phys.y))
        }
        _ => (phys.x, phys.y),
    };

    // Guard against freshly-created windows where physical size is 0
    // and the degenerate case where the central area is too narrow
    // to fit even one PIP. Both paths skip the update rather than
    // produce a 0-sized viewport that bevy rejects.
    if central_right_px < panel_px + gap_px
        || central_bottom_px < panel_px + bottom_px
    {
        return;
    }

    let y = central_bottom_px.saturating_sub(bottom_px + panel_px);
    let right_edge = central_right_px.saturating_sub(gap_px);

    for (mut cam, view) in &mut cams {
        // Columns go right-to-left: index 0 sits furthest from the
        // edge. That keeps the "Side" (index 2) panel pinned to the
        // central-area's bottom-right corner — which is the view
        // the user glances at most when painting on the 2D canvas
        // on the left.
        let col = view.kind.column();
        let panels_from_edge = 3 - col;
        let x_right = right_edge
            .saturating_sub(panels_from_edge.saturating_sub(1) * (panel_px + gap_px));
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
///
/// `central_right_logical` / `central_bottom_logical` describe
/// where the egui central region ends — i.e. the rightmost /
/// bottommost columns the PIPs are allowed to occupy without
/// running underneath a SidePanel or status bar. Pass
/// `window.width() / window.height()` when no central rect is
/// known yet (the result is then identical to the pre-v0.10 C-2
/// behaviour).
pub fn pip_logical_rects(
    window: &Window,
    state: &MultiViewState,
    central_right_logical: f32,
    central_bottom_logical: f32,
) -> [PipRect; 3] {
    let w = central_right_logical.min(window.width()).max(0.0);
    let h = central_bottom_logical.min(window.height()).max(0.0);
    let size = compute_panel_size(state, w);
    let gap = state.gap as f32;
    let bottom = state.bottom_reserved as f32;
    let y_top = (h - bottom - size).max(0.0);

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
        let x_left = (x_right - size).max(0.0);
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
