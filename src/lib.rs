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

    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::vehicle::{
        DynamicsParams, EngineState, Gear, Vehicle, VehicleCommand, VehiclePlugin,
    };

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

    /// 키 하나의 누름/뗌 메시지를 만든다. 실제 창 입력과 같은 경로(PreUpdate 키보드 시스템)를 탄다.
    fn key_message(key_code: KeyCode, state: ButtonState) -> KeyboardInput {
        KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    /// `keys`만 눌린 상태가 되도록 누름/뗌 메시지를 보내고 앱을 한 번 갱신한다.
    ///
    /// `ButtonInput`을 직접 바꾸면 PreUpdate에서 "새로 눌림"이 지워지므로 메시지로 전달한다.
    fn update_with_keys(app: &mut App, keys: &[KeyCode]) {
        let held: Vec<KeyCode> = app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .get_pressed()
            .copied()
            .collect();
        for key in held.iter().filter(|key| !keys.contains(key)) {
            app.world_mut()
                .write_message(key_message(*key, ButtonState::Released));
        }
        for key in keys.iter().filter(|key| !held.contains(key)) {
            app.world_mut()
                .write_message(key_message(*key, ButtonState::Pressed));
        }
        app.update();
    }

    /// 가속 키를 누른 채 제동·비상 정지 키를 누르면, 그 프레임의 운동 계산부터 해당 감속이 적용되는지 확인한다.
    ///
    /// 가속 키를 계속 누르고 있으므로 입력·명령이 한 프레임 늦으면 속도가 오히려 오른다.
    #[test]
    fn brake_and_emergency_stop_apply_in_same_frame() {
        let params = DynamicsParams::default();
        let dt = 1.0 / 64.0;
        for (stop_key, expected_decel) in [
            (KeyCode::KeyS, params.max_brake_decel_mps2),
            (KeyCode::Space, params.emergency_decel_mps2),
        ] {
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

            update_with_keys(&mut app, &[KeyCode::KeyW, stop_key]);
            let state = &app.world().resource::<Vehicle>().0;
            let drop = before - state.speed_mps();
            assert!(
                drop >= expected_decel * dt * 0.99,
                "{stop_key:?}: speed changed from {before} to {} in the stop frame",
                state.speed_mps()
            );
            if stop_key == KeyCode::Space {
                assert_eq!(state.engine(), EngineState::EmergencyStop);
            }
        }
    }
}
