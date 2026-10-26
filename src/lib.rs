//! 자율주행 셔틀 안전관리자 조작 훈련 시뮬레이터의 도메인 로직과 Bevy 플러그인.

pub mod config;
pub mod input;
pub mod scene;
pub mod ui;
pub mod vehicle;

use bevy::app::{RunFixedMainLoop, RunFixedMainLoopSystems};
use bevy::prelude::*;

/// 프레임 안의 처리 순서. 입력 → 차량 제어 → (고정 주기 운동 계산) → 화면 표시 순으로 실행한다.
///
/// `Input`·`Control`은 고정 주기 루프 직전(`RunFixedMainLoop`의 `BeforeFixedMainLoop`)에 돌아서,
/// 이번 프레임의 가속·제동 입력과 비상 정지 등 명령이 같은 프레임의 운동 계산에 바로 반영된다.
/// `Display`는 운동 계산이 끝난 뒤 `Update`에서 돈다.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimSet {
    Input,
    Control,
    Display,
}

/// [`SimSet`] 실행 위치와 순서를 앱에 설정한다.
pub fn configure_sim_sets(app: &mut App) {
    app.configure_sets(
        RunFixedMainLoop,
        (SimSet::Input, SimSet::Control)
            .chain()
            .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
    );
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::vehicle::{Gear, Vehicle, VehicleCommand, VehiclePlugin};

    /// 키보드 입력과 차량 시스템만 있는 창 없는 앱을 만든다. 한 번 갱신할 때마다 고정 주기 1단계가 돈다.
    fn headless_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 64.0,
            )));
        configure_sim_sets(&mut app);
        app.add_plugins((input::InputPlugin, VehiclePlugin));
        app
    }

    /// 지정 키를 누른 상태로 앱을 한 번 갱신한다(다른 키는 뗀다).
    fn update_with_keys(app: &mut App, keys: &[KeyCode]) {
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.release_all();
        for key in keys {
            input.press(*key);
        }
        app.update();
    }

    /// 제동·비상 정지 키를 누른 그 프레임의 운동 계산부터 가속이 멈추는지 확인한다.
    #[test]
    fn brake_and_emergency_stop_apply_in_same_frame() {
        for stop_key in [KeyCode::KeyS, KeyCode::Space] {
            let mut app = headless_app();
            {
                let state = &mut app.world_mut().resource_mut::<Vehicle>().0;
                state.apply(VehicleCommand::ToggleEngine).unwrap();
                state
                    .apply(VehicleCommand::RequestGear(Gear::Drive))
                    .unwrap();
            }
            for _ in 0..64 {
                update_with_keys(&mut app, &[KeyCode::KeyW]);
            }
            let before = app.world().resource::<Vehicle>().0.speed_mps();
            assert!(before > 0.5, "vehicle should be moving, got {before}");

            update_with_keys(&mut app, &[stop_key]);
            let after = app.world().resource::<Vehicle>().0.speed_mps();
            assert!(
                after < before,
                "{stop_key:?}: speed rose from {before} to {after} in the stop frame"
            );
        }
    }
}
