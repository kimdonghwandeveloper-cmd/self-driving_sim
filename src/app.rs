use bevy::{app::AppExit, prelude::*};

pub fn run(smoke_test: bool) -> AppExit {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Shuttle Training Simulator - Development Setup".into(),
            resolution: (1280, 720).into(),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.08, 0.11, 0.16)))
    .add_systems(Startup, setup);

    if smoke_test {
        app.add_systems(Update, finish_smoke_test);
    }

    app.run()
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(10.0, 10.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.25, 0.30, 0.30))),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.30, 0.65, 0.85))),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-3.0, 3.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    info!("Development scene ready. Close the window to exit.");
}

fn finish_smoke_test(time: Res<Time>, mut exit: MessageWriter<AppExit>) {
    if time.elapsed_secs() >= 5.0 {
        info!("Window/render smoke test completed.");
        exit.write(AppExit::Success);
    }
}
