//! 개발 테스트용 키보드 폴백 입력.

use bevy::prelude::*;

use super::mapping::{GearSelection, KeyAction, KeyBindings, gears, resolve_gear_selection};
use super::{ControlRequest, DriverInput};
use crate::vehicle::{VehicleCommand, VehicleInput};

/// 키보드 상태를 읽어 연속 입력을 갱신하고 명령 요청을 보낸다.
pub fn read_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    mut driver: ResMut<DriverInput>,
    mut requests: MessageWriter<ControlRequest>,
) {
    let pressed = |action| bindings.keys(action).iter().any(|key| keys.pressed(*key));
    let just_pressed = |action| {
        bindings
            .keys(action)
            .iter()
            .any(|key| keys.just_pressed(*key))
    };

    driver.0 = keyboard_vehicle_input(
        pressed(KeyAction::Throttle),
        pressed(KeyAction::Brake),
        pressed(KeyAction::SteerLeft),
        pressed(KeyAction::SteerRight),
    );

    let just: Vec<_> = gears()
        .into_iter()
        .filter(|gear| just_pressed(KeyAction::Gear(*gear)))
        .collect();
    let held: Vec<_> = gears()
        .into_iter()
        .filter(|gear| pressed(KeyAction::Gear(*gear)))
        .collect();
    let frame = KeyFrame {
        emergency_stop_pressed: just_pressed(KeyAction::EmergencyStop),
        emergency_stop_held: pressed(KeyAction::EmergencyStop),
        emergency_release_pressed: just_pressed(KeyAction::EmergencyRelease),
        engine_toggle_pressed: just_pressed(KeyAction::EngineToggle),
        gear: resolve_gear_selection(&just, &held),
    };
    requests.write_batch(command_requests(frame));
}

/// 한 프레임의 단발성 키 입력 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyFrame {
    /// 비상 정지 키가 이번 프레임에 새로 눌림.
    pub emergency_stop_pressed: bool,
    /// 비상 정지 키가 눌려 있음(새로 눌린 경우 포함).
    pub emergency_stop_held: bool,
    pub emergency_release_pressed: bool,
    pub engine_toggle_pressed: bool,
    pub gear: GearSelection,
}

/// 키 입력 상태를 명령 요청 목록으로 바꾼다.
///
/// 비상 정지 키가 눌려 있는 동안에는 비상 정지 진입만 보내고 해제·시동·기어 요청은 버린다.
/// 따라서 해제는 비상 정지 키를 뗀 뒤 별도로 눌러야 한다.
/// 그 외에는 해제 → 시동 → 기어 순으로 보낸다.
pub fn command_requests(frame: KeyFrame) -> Vec<ControlRequest> {
    if frame.emergency_stop_held || frame.emergency_stop_pressed {
        return if frame.emergency_stop_pressed {
            vec![ControlRequest::Command(VehicleCommand::EngageEmergencyStop)]
        } else {
            Vec::new()
        };
    }
    let mut requests = Vec::new();
    if frame.emergency_release_pressed {
        requests.push(ControlRequest::Command(
            VehicleCommand::ReleaseEmergencyStop,
        ));
    }
    if frame.engine_toggle_pressed {
        requests.push(ControlRequest::Command(VehicleCommand::ToggleEngine));
    }
    match frame.gear {
        GearSelection::None => {}
        GearSelection::Requested(gear) => {
            requests.push(ControlRequest::Command(VehicleCommand::RequestGear(gear)));
        }
        GearSelection::Conflict(gears) => {
            requests.push(ControlRequest::GearConflict(gears));
        }
    }
    requests
}

/// 디지털 키 상태를 연속 입력으로 변환한다. 가속과 제동이 동시에 눌리면 제동을 우선한다.
pub fn keyboard_vehicle_input(
    throttle: bool,
    brake: bool,
    left: bool,
    right: bool,
) -> VehicleInput {
    /// 눌림 여부를 0 또는 1로 바꾼다.
    fn level(pressed: bool) -> f32 {
        if pressed { 1.0 } else { 0.0 }
    }
    VehicleInput {
        throttle: level(throttle && !brake),
        brake: level(brake),
        steering: level(right) - level(left),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::controller::handle_request;
    use crate::vehicle::{EngineState, Gear, VehicleState};

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

    /// 가속과 제동을 함께 누르면 제동만 적용되는지 확인한다.
    #[test]
    fn brake_overrides_throttle() {
        let input = keyboard_vehicle_input(true, true, false, false);
        assert_eq!(input.throttle, 0.0);
        assert_eq!(input.brake, 1.0);
    }

    /// 좌우 조향 키를 함께 누르면 상쇄되고, 하나만 누르면 -1/+1이 되는지 확인한다.
    #[test]
    fn opposite_steering_keys_cancel() {
        assert_eq!(
            keyboard_vehicle_input(false, false, true, true).steering,
            0.0
        );
        assert_eq!(
            keyboard_vehicle_input(false, false, true, false).steering,
            -1.0
        );
        assert_eq!(
            keyboard_vehicle_input(false, false, false, true).steering,
            1.0
        );
    }

    /// 비상 정지와 해제·시동·기어 키를 같은 프레임에 누르면 비상 정지만 보내는지 확인한다.
    #[test]
    fn emergency_stop_press_suppresses_other_requests() {
        let frame = KeyFrame {
            emergency_stop_pressed: true,
            emergency_stop_held: true,
            emergency_release_pressed: true,
            engine_toggle_pressed: true,
            gear: GearSelection::Requested(Gear::Drive),
        };
        assert_eq!(
            command_requests(frame),
            [ControlRequest::Command(VehicleCommand::EngageEmergencyStop)]
        );
    }

    /// 비상 정지 키를 누르고 있는 동안에는 해제 키를 새로 눌러도 요청이 없는지 확인한다.
    #[test]
    fn release_ignored_while_emergency_stop_held() {
        let frame = KeyFrame {
            emergency_stop_held: true,
            emergency_release_pressed: true,
            ..idle_frame()
        };
        assert!(command_requests(frame).is_empty());
    }

    /// 비상 정지 키를 뗀 뒤에는 해제·시동·기어 요청을 정해진 순서로 보내는지 확인한다.
    #[test]
    fn requests_follow_release_engine_gear_order() {
        let frame = KeyFrame {
            emergency_release_pressed: true,
            engine_toggle_pressed: true,
            gear: GearSelection::Requested(Gear::Drive),
            ..idle_frame()
        };
        assert_eq!(
            command_requests(frame),
            [
                ControlRequest::Command(VehicleCommand::ReleaseEmergencyStop),
                ControlRequest::Command(VehicleCommand::ToggleEngine),
                ControlRequest::Command(VehicleCommand::RequestGear(Gear::Drive)),
            ]
        );
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
