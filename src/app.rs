use std::path::Path;

use bevy::{app::AppExit, prelude::*};
use self_driving_sim::{
    SimSet, config,
    input::InputPlugin,
    ui::HudPlugin,
    vehicle::{Vehicle, VehiclePlugin, VehicleState},
};

/// 앱을 구성하고 실행한다. `smoke_test`이면 약 5초 후 자동 종료한다.
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

    // 로그 플러그인이 등록된 뒤에 설정을 읽어 로드 결과가 로그에 남도록 한다.
    let vehicle_config = config::load_vehicle_config(Path::new(config::VEHICLE_CONFIG_PATH));
    let key_bindings = config::load_key_bindings(Path::new(config::INPUT_CONFIG_PATH));
    app.configure_sets(
        Update,
        (SimSet::Input, SimSet::Control, SimSet::Display).chain(),
    )
    .insert_resource(Vehicle(VehicleState::new(
        vehicle_config.state_machine.stop_threshold_mps,
    )))
    .insert_resource(key_bindings)
    .add_plugins((InputPlugin, VehiclePlugin, HudPlugin));

    if smoke_test {
        app.add_systems(Update, finish_smoke_test);
    }

    app.run()
}

/// 개발 확인용 바닥, 도형, 조명, 카메라를 배치한다.
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

/// 자동 검증 모드에서 5초가 지나면 앱을 정상 종료한다.
fn finish_smoke_test(time: Res<Time>, mut exit: MessageWriter<AppExit>) {
    if time.elapsed_secs() >= 5.0 {
        info!("Window/render smoke test completed.");
        exit.write(AppExit::Success);
    }
}
