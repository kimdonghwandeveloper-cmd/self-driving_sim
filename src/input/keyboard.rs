//! 개발 테스트용 키보드 폴백 입력.
//!
//! 키 상태를 명령 요청·연속 입력으로 바꾸는 규칙은 엔진 무관 `sim_core::input::commands`에 있다.

use bevy::prelude::*;

use super::mapping::{KeyAction, KeyBindings, gears};
use super::merge::{DeviceFrame, DeviceFrames};

pub use sim_core::input::commands::{KeyFrame, command_requests, keyboard_vehicle_input};

/// 키보드 상태를 읽어 이번 프레임의 키보드 입력을 기록한다.
pub fn read_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    mut frames: ResMut<DeviceFrames>,
) {
    let pressed = |action| bindings.keys(action).iter().any(|key| keys.pressed(*key));
    let just_pressed = |action| {
        bindings
            .keys(action)
            .iter()
            .any(|key| keys.just_pressed(*key))
    };

    frames.keyboard = DeviceFrame {
        input: keyboard_vehicle_input(
            pressed(KeyAction::Throttle),
            pressed(KeyAction::Brake),
            pressed(KeyAction::SteerLeft),
            pressed(KeyAction::SteerRight),
        ),
        emergency_stop_pressed: just_pressed(KeyAction::EmergencyStop),
        emergency_stop_held: pressed(KeyAction::EmergencyStop),
        emergency_release_pressed: just_pressed(KeyAction::EmergencyRelease),
        engine_toggle_pressed: just_pressed(KeyAction::EngineToggle),
        gears_just_pressed: gears()
            .into_iter()
            .filter(|gear| just_pressed(KeyAction::Gear(*gear)))
            .collect(),
        gears_held: gears()
            .into_iter()
            .filter(|gear| pressed(KeyAction::Gear(*gear)))
            .collect(),
        reset_pressed: just_pressed(KeyAction::ResetCourse),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::mapping::GearSelection;
    use crate::vehicle::controller::handle_request;
    use crate::vehicle::{EngineState, VehicleCommand, VehicleState};

    /// 아무 키도 누르지 않은 프레임을 만든다.
    fn idle_frame() -> KeyFrame {
        KeyFrame {
            emergency_stop_pressed: false,
            emergency_stop_held: false,
            emergency_release_pressed: false,
            engine_toggle_pressed: false,
            gear: GearSelection::None,
        }
    }

    /// 비상 정지·해제를 같은 프레임에 눌러도 상태 머신이 비상 정지에 머무는지 확인한다.
    #[test]
    fn simultaneous_stop_and_release_keeps_emergency_stop() {
        let mut state = VehicleState::default();
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        let frame = KeyFrame {
            emergency_stop_pressed: true,
            emergency_stop_held: true,
            emergency_release_pressed: true,
            ..idle_frame()
        };
        for request in command_requests(frame) {
            handle_request(&mut state, &request);
        }
        assert_eq!(state.engine(), EngineState::EmergencyStop);
    }
}
