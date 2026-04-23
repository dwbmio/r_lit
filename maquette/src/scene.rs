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
            .add_systems(Startup, setup_scene);
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
