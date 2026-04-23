//! GUI binary. Owns the windowed Bevy app and the UI plugins.
//!
//! All the headless logic (grid data, meshing, project I/O, export)
//! lives in the `maquette` lib and is shared with `maquette-cli`.
//! See `docs/handoff/COST_AWARENESS.md` §The Headless Invariant for
//! why this split exists.

mod camera;
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
use bevy::window::WindowResolution;
use bevy_egui::EguiPlugin;
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
                        ..default()
                    }),
                    ..default()
                })
                .set(ImagePlugin::default_nearest()),
        )
        .add_plugins(EguiPlugin::default())
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
        .add_plugins(multiview::MultiViewPlugin)
        .add_plugins(float_window::FloatWindowPlugin)
        .add_plugins(notify::NotifyPlugin)
        .add_plugins(ui::UiPlugin)
        .run();
}
