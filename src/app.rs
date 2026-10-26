use std::path::Path;

use bevy::{app::AppExit, prelude::*};
use self_driving_sim::{
    config, configure_sim_sets,
    input::InputPlugin,
    scene::ScenePlugin,
    ui::HudPlugin,
    vehicle::{Vehicle, VehicleDynamics, VehiclePlugin, VehicleState},
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
    .insert_resource(ClearColor(Color::srgb(0.08, 0.11, 0.16)));

    // 로그 플러그인이 등록된 뒤에 설정을 읽어 로드 결과가 로그에 남도록 한다.
    let vehicle_config = config::load_vehicle_config(Path::new(config::VEHICLE_CONFIG_PATH));
    let key_bindings = config::load_key_bindings(Path::new(config::INPUT_CONFIG_PATH));
    configure_sim_sets(&mut app);
    app.insert_resource(Vehicle(VehicleState::new(
        vehicle_config.state_machine.stop_threshold_mps,
    )))
    .insert_resource(VehicleDynamics(vehicle_config.dynamics))
    .insert_resource(key_bindings)
    .add_plugins((InputPlugin, VehiclePlugin, ScenePlugin, HudPlugin));

    if smoke_test {
        app.add_systems(Update, finish_smoke_test);
    }

    app.run()
}

/// 자동 검증 모드에서 5초가 지나면 앱을 정상 종료한다.
fn finish_smoke_test(time: Res<Time>, mut exit: MessageWriter<AppExit>) {
    if time.elapsed_secs() >= 5.0 {
        info!("Window/render smoke test completed.");
        exit.write(AppExit::Success);
    }
}
