use bevy::prelude::*;
use bevy_infinite_grid::{InfiniteGrid, InfiniteGridSettings};

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.11)))
            .insert_resource(GlobalAmbientLight {
                color: Color::WHITE,
                brightness: 150.0,
                ..default()
            })
            .init_resource::<WorldAxesState>()
            .add_systems(Startup, setup_scene)
            .add_systems(Update, draw_world_axes);
    }
}

/// Whether to render the world-space coordinate gizmo on top of the
/// scene. Default on — the user asked for it as a navigation aid
/// ("which way is +X? is my model actually at the origin?").
#[derive(Resource, Debug, Clone, Copy)]
pub struct WorldAxesState {
    pub visible: bool,
}

impl Default for WorldAxesState {
    fn default() -> Self {
        Self { visible: true }
    }
}

/// Length (world units / cells) each axis extends from the origin.
/// 10 covers the common 16×16 canvas out to its rough edge without
/// dominating the frame.
const AXIS_LENGTH: f32 = 10.0;
/// How far each minor tick sits from the previous one — one per cell
/// so the user can read rough distances by eye.
const AXIS_TICK_STEP: f32 = 1.0;
/// Size of each minor tick, perpendicular to the axis.
const AXIS_TICK_SIZE: f32 = 0.12;

fn draw_world_axes(state: Res<WorldAxesState>, mut gizmos: Gizmos) {
    if !state.visible {
        return;
    }

    // +X (red), +Y (green), +Z (blue). Bevy's gizmo lines are
    // immediate-mode and render from every active camera, so they
    // show up in the main preview *and* in each PIP — the point of
    // adding them.
    //
    // Alpha is deliberately low (~0.55) because gizmos render on top
    // of the mesh without a depth test; at full opacity they bleed
    // through solid geometry and make the toon surface read as
    // "transparent with neon wires on top". A translucent axis set is
    // still easy to parse as a navigation cue while staying visually
    // secondary to the model.
    let red = Color::srgba(0.95, 0.30, 0.30, 0.55);
    let green = Color::srgba(0.40, 0.9, 0.40, 0.55);
    let blue = Color::srgba(0.35, 0.65, 1.0, 0.55);

    gizmos.line(Vec3::ZERO, Vec3::X * AXIS_LENGTH, red);
    gizmos.line(Vec3::ZERO, Vec3::Y * AXIS_LENGTH, green);
    gizmos.line(Vec3::ZERO, Vec3::Z * AXIS_LENGTH, blue);

    // Ticks every cell so the user can count units without a ruler.
    // Each tick is a short perpendicular line centred on the axis.
    let mut t = AXIS_TICK_STEP;
    while t <= AXIS_LENGTH + 1e-3 {
        gizmos.line(
            Vec3::new(t, 0.0, -AXIS_TICK_SIZE),
            Vec3::new(t, 0.0, AXIS_TICK_SIZE),
            red,
        );
        gizmos.line(
            Vec3::new(-AXIS_TICK_SIZE, t, 0.0),
            Vec3::new(AXIS_TICK_SIZE, t, 0.0),
            green,
        );
        gizmos.line(
            Vec3::new(-AXIS_TICK_SIZE, 0.0, t),
            Vec3::new(AXIS_TICK_SIZE, 0.0, t),
            blue,
        );
        t += AXIS_TICK_STEP;
    }
}

fn setup_scene(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 10.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        InfiniteGrid,
        InfiniteGridSettings {
            major_line_color: Color::srgba(0.55, 0.55, 0.55, 0.5),
            minor_line_color: Color::srgba(0.35, 0.35, 0.35, 0.2),
            x_axis_color: Color::srgba(0.9, 0.3, 0.3, 0.7),
            z_axis_color: Color::srgba(0.3, 0.6, 0.9, 0.7),
            ..default()
        },
    ));
}
