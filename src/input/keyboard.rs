//! 개발 테스트용 키보드 폴백 입력.

use bevy::prelude::*;

use super::mapping::{GearSelection, KeyAction, KeyBindings, gears, resolve_gear_selection};
use super::{ControlRequest, DriverInput};
use crate::vehicle::{VehicleCommand, VehicleInput};

/// 키보드 상태를 읽어 연속 입력을 갱신하고 명령 요청을 보낸다.
///
/// 한 프레임에 여러 요청이 있으면 비상 정지 → 해제 → 시동 → 기어 순으로 보낸다.
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

    for (action, command) in [
        (
            KeyAction::EmergencyStop,
            VehicleCommand::EngageEmergencyStop,
        ),
        (
            KeyAction::EmergencyRelease,
            VehicleCommand::ReleaseEmergencyStop,
        ),
        (KeyAction::EngineToggle, VehicleCommand::ToggleEngine),
    ] {
        if just_pressed(action) {
            requests.write(ControlRequest::Command(command));
        }
    }

    let just: Vec<_> = gears()
        .into_iter()
        .filter(|gear| just_pressed(KeyAction::Gear(*gear)))
        .collect();
    let held: Vec<_> = gears()
        .into_iter()
        .filter(|gear| pressed(KeyAction::Gear(*gear)))
        .collect();
    match resolve_gear_selection(&just, &held) {
        GearSelection::None => {}
        GearSelection::Requested(gear) => {
            requests.write(ControlRequest::Command(VehicleCommand::RequestGear(gear)));
        }
        GearSelection::Conflict(gears) => {
            requests.write(ControlRequest::GearConflict(gears));
        }
    }
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

    #[test]
    fn brake_overrides_throttle() {
        let input = keyboard_vehicle_input(true, true, false, false);
        assert_eq!(input.throttle, 0.0);
        assert_eq!(input.brake, 1.0);
    }

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
}
