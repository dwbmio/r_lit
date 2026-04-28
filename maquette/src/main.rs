//! GUI binary. Owns the windowed Bevy app and the UI plugins.
//!
//! All the headless logic (grid data, meshing, project I/O, export)
//! lives in the `maquette` lib and is shared with `maquette-cli`.
//! See `docs/handoff/COST_AWARENESS.md` §The Headless Invariant for
//! why this split exists.

mod autosave;
mod block_composer;
mod block_library;
mod camera;
mod export_dialog;
mod float_window;
mod history;
mod multiview;
mod notify;
mod preview_mesh;
mod scene;
mod session;
mod toon;
mod ui;

use bevy::prelude::*;
use bevy::window::{WindowResizeConstraints, WindowResolution};
use bevy::winit::WinitSettings;
use bevy_egui::{EguiGlobalSettings, EguiPlugin};
use bevy_infinite_grid::InfiniteGridPlugin;
use bevy_mod_outline::OutlinePlugin;
use bevy_panorbit_camera::PanOrbitCameraPlugin;
use maquette::export::ExportPlugin;
use maquette::grid::GridPlugin;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Maquette — Asset Forge".into(),
                        resolution: WindowResolution::new(1280, 800),
                        // Hard minimum window size. Below this, the
                        // layout starts overlapping: the PIP strip
                        // (bottom-right) eats into the left sidebar
                        // (min 360 px) and the main 3D preview is
                        // squeezed into nothing. Values picked so
                        //   * left sidebar at its own `min_width`
                        //     (360)
                        //   * + a ~300 px main preview slot
                        //   * + PIP strip at its `min_panel_size`
                        //     (3 × 100 + 2 gaps + edge margin ≈ 324)
                        //   * ≈ 1000 px total
                        // fit without collision. Height floor
                        // matches: menu bar + canvas heading +
                        // min-visible canvas + palette + status bar
                        // ≈ 640 px.
                        resize_constraints: WindowResizeConstraints {
                            min_width: 1000.0,
                            min_height: 640.0,
                            max_width: f32::INFINITY,
                            max_height: f32::INFINITY,
                        },
                        ..default()
                    }),
                    ..default()
                })
                .set(ImagePlugin::default_nearest()),
        )
        .add_plugins(EguiPlugin::default())
        // Pin the primary egui context to our main preview camera
        // (see camera::spawn_camera). Without this, bevy_egui would
        // auto-attach the primary context to whichever camera spawns
        // first, and on some frames the multi-view PIP cameras win
        // the Startup race — collapsing the entire UI into a 180×180
        // block in the bottom-right corner.
        .insert_resource(EguiGlobalSettings {
            auto_create_primary_context: false,
            ..default()
        })
        // Event-driven rendering — the same model Godot, Blender and
        // other DCC editors use. Bevy's default is `game()`: render
        // every frame at max rate, which keeps the GPU / CPU spinning
        // at ~60–144 fps even when the user is just staring at the
        // canvas. That's fine for a game, wasteful for an editor.
        //
        // `desktop_app()` means:
        //   * Focused window → `Reactive(5s)`: update only when a
        //     user / window / device event fires (mouse move, click,
        //     keypress, resize). The `5s` is a liveness heartbeat,
        //     not a render cap — if nothing happens for 5s we'll
        //     still pulse once to let long-idle systems tick.
        //   * Unfocused window → `ReactiveLowPower(60s)`: same, but
        //     also ignores device events (cursor motion outside the
        //     window, etc.) and idles for up to a minute. On Cmd+Tab
        //     away, Maquette's GPU / CPU usage drops to ~0.
        //
        // Smooth camera animations (Fit, Reset, PIP click) need a
        // steady stream of updates to interpolate on, so
        // `camera::request_redraw_while_animating` fires
        // `RequestRedraw` every frame while `PanOrbitCamera` is
        // still converging on its target. That wakes the reactive
        // loop immediately, so animation still plays at full fps
        // even though idle rendering is suspended.
        .insert_resource(WinitSettings::desktop_app())
        .add_plugins(PanOrbitCameraPlugin)
        .add_plugins(InfiniteGridPlugin)
        .add_plugins(OutlinePlugin)
        .add_plugins(scene::ScenePlugin)
        .add_plugins(camera::CameraPlugin)
        .add_plugins(toon::ToonPlugin)
        .add_plugins(GridPlugin)
        .add_plugins(preview_mesh::PreviewMeshPlugin)
        .add_plugins(history::HistoryPlugin)
        .add_plugins(session::ProjectPlugin)
        .add_plugins(ExportPlugin)
        .add_plugins(export_dialog::ExportDialogPlugin)
        .add_plugins(multiview::MultiViewPlugin)
        .add_plugins(float_window::FloatWindowPlugin)
        .add_plugins(notify::NotifyPlugin)
        .add_plugins(autosave::AutosavePlugin)
        .add_plugins(block_library::BlockLibraryPlugin)
        .add_plugins(block_composer::BlockComposerPlugin)
        .add_plugins(ui::UiPlugin)
        .run();
}
