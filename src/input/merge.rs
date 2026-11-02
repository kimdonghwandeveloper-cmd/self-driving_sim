//! 키보드·게임패드 프레임을 하나로 합치는 규칙.
//!
//! 장치는 `DriverInput`과 요청 메시지를 직접 쓰지 않고 [`DeviceFrames`]에만 기록한다.
//! 여기서 프레임마다 한 번만 합치므로 같은 프레임에 두 장치가 서로를 덮어쓰지 않는다.
//!
//! - 조향·가속: [`InputSource`]로 선택한 장치 하나의 값을 쓴다. 다른 장치를 조작해도 지금 장치의
//!   입력이 없어질 때까지는 선택을 유지하고, 지금 장치가 쉬는 동안 다른 장치를 조작하면 바꾼다.
//! - 제동: 어느 장치든 더 강한 값을 쓴다. 제동이 있으면 가속은 0이다.
//! - 단발 명령: 하나의 프레임으로 합쳐 `command_requests` 규칙을 그대로 따른다. 비상 정지는 어느
//!   장치든 우선하고, 시동 토글은 두 장치가 함께 눌러도 한 번이며, 기어가 다르면 동시 요청으로 거부한다.
//! - 연결 해제: 게임패드로 주행 중 연결이 끊기면 비상 정지를 요청한다.
//! - 새 게임패드: 입력이 중립이 된 뒤에 활성화한다. 그 전에는 비상 정지만 받는다.

use bevy::prelude::*;

use super::keyboard::{KeyFrame, command_requests};
use super::mapping::resolve_gear_selection;
use super::{ControlRequest, DriverInput};
use crate::calibration::ResetCourseRequest;
use crate::vehicle::{Gear, Vehicle, VehicleInput};

/// 장치 하나의 이번 프레임 입력.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceFrame {
    pub input: VehicleInput,
    /// 비상 정지 입력이 이번 프레임에 새로 눌림.
    pub emergency_stop_pressed: bool,
    /// 비상 정지 입력이 눌려 있음(새로 눌린 경우 포함).
    pub emergency_stop_held: bool,
    pub emergency_release_pressed: bool,
    pub engine_toggle_pressed: bool,
    pub gears_just_pressed: Vec<Gear>,
    pub gears_held: Vec<Gear>,
    pub reset_pressed: bool,
}

impl DeviceFrame {
    /// 비상 정지 입력만 남긴 프레임을 반환한다.
    pub fn emergency_only(&self) -> Self {
        Self {
            emergency_stop_pressed: self.emergency_stop_pressed,
            emergency_stop_held: self.emergency_stop_held,
            ..Self::default()
        }
    }
}

/// 장치별 입력 시스템이 기록하고 [`combine_inputs`]가 읽는 이번 프레임 입력.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct DeviceFrames {
    pub keyboard: DeviceFrame,
    pub gamepad: DeviceFrame,
    /// 조작에 쓰던 게임패드가 이번 프레임에 사라짐.
    pub gamepad_lost: bool,
}

/// 조향·가속에 쓰는 입력 장치.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputSource {
    #[default]
    Keyboard,
    Gamepad,
}

impl InputSource {
    /// HUD에 표시할 장치 이름을 반환한다.
    pub fn label(self) -> &'static str {
        match self {
            InputSource::Keyboard => "Keyboard",
            InputSource::Gamepad => "Gamepad",
        }
    }
}

/// 조작에 쓰는 게임패드의 식별 정보.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectedGamepad {
    pub entity: Entity,
    pub name: String,
    /// 선택된 뒤 입력이 중립이 된 적이 있어 조작에 쓸 수 있음.
    pub ready: bool,
}

/// 입력 장치 연결·선택 상태. HUD에 표시한다.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct InputStatus {
    pub gamepad: Option<ConnectedGamepad>,
    pub source: InputSource,
}

/// 조향·가속·제동 중 하나라도 입력이 있는지 확인한다.
fn has_input(input: VehicleInput) -> bool {
    input.throttle > 0.0 || input.brake > 0.0 || input.steering != 0.0
}

/// 입력 유무로 이번 프레임의 장치를 고른다. 둘 다 입력이 있거나 없으면 지금 장치를 유지한다.
pub fn select_source(
    current: InputSource,
    keyboard: VehicleInput,
    gamepad: VehicleInput,
) -> InputSource {
    match (has_input(keyboard), has_input(gamepad)) {
        (true, false) => InputSource::Keyboard,
        (false, true) => InputSource::Gamepad,
        _ => current,
    }
}

/// 선택한 장치의 조향·가속과 두 장치 중 더 강한 제동으로 차량 입력을 만든다.
pub fn merge_vehicle_input(
    source: InputSource,
    keyboard: VehicleInput,
    gamepad: VehicleInput,
) -> VehicleInput {
    let selected = match source {
        InputSource::Keyboard => keyboard,
        InputSource::Gamepad => gamepad,
    };
    let brake = keyboard.brake.max(gamepad.brake);
    VehicleInput {
        throttle: if brake > 0.0 { 0.0 } else { selected.throttle },
        brake,
        steering: selected.steering,
    }
}

/// 두 장치의 단발 입력을 하나의 프레임으로 합친다. 기어는 합친 목록으로 동시 요청을 판정한다.
pub fn merge_key_frames(keyboard: &DeviceFrame, gamepad: &DeviceFrame) -> KeyFrame {
    let just_pressed = [
        keyboard.gears_just_pressed.as_slice(),
        gamepad.gears_just_pressed.as_slice(),
    ]
    .concat();
    let held = [
        keyboard.gears_held.as_slice(),
        gamepad.gears_held.as_slice(),
    ]
    .concat();
    KeyFrame {
        emergency_stop_pressed: keyboard.emergency_stop_pressed || gamepad.emergency_stop_pressed,
        emergency_stop_held: keyboard.emergency_stop_held || gamepad.emergency_stop_held,
        emergency_release_pressed: keyboard.emergency_release_pressed
            || gamepad.emergency_release_pressed,
        engine_toggle_pressed: keyboard.engine_toggle_pressed || gamepad.engine_toggle_pressed,
        gear: resolve_gear_selection(&just_pressed, &held),
    }
}

/// 게임패드 연결 해제에 비상 정지가 필요한지 판정한다.
///
/// 게임패드로 조작 중이었고 차량이 정지 상태가 아닐 때만 필요하다. 속도가 잘못된 값이면 정지로
/// 보지 않으므로 안전한 쪽인 비상 정지를 요청한다.
pub fn needs_safe_stop(gamepad_lost: bool, source: InputSource, stopped: bool) -> bool {
    gamepad_lost && source == InputSource::Gamepad && !stopped
}

/// 장치 프레임을 합쳐 `DriverInput`을 갱신하고 명령·시작 위치 복귀 요청을 보낸다.
///
/// 비상 정지가 눌려 있는 동안에는 다른 명령처럼 복귀 요청도 보내지 않는다.
pub fn combine_inputs(
    frames: Res<DeviceFrames>,
    vehicle: Res<Vehicle>,
    mut status: ResMut<InputStatus>,
    mut driver: ResMut<DriverInput>,
    mut requests: MessageWriter<ControlRequest>,
    mut resets: MessageWriter<ResetCourseRequest>,
) {
    let safe_stop = needs_safe_stop(frames.gamepad_lost, status.source, vehicle.0.is_stopped());
    if frames.gamepad_lost {
        warn!("gamepad disconnected; driving input cleared (safe stop: {safe_stop})");
        status.source = InputSource::Keyboard;
    }
    let source = select_source(status.source, frames.keyboard.input, frames.gamepad.input);
    if status.source != source {
        status.source = source;
    }
    driver.0 = merge_vehicle_input(source, frames.keyboard.input, frames.gamepad.input);

    let mut frame = merge_key_frames(&frames.keyboard, &frames.gamepad);
    frame.emergency_stop_pressed |= safe_stop;
    let reset = frames.keyboard.reset_pressed && !frame.emergency_stop_held;
    requests.write_batch(command_requests(frame));
    if reset {
        resets.write(ResetCourseRequest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::mapping::GearSelection;
    use crate::vehicle::VehicleCommand;

    /// 가속·조향 입력을 만든다.
    fn driving(throttle: f32, steering: f32) -> VehicleInput {
        VehicleInput {
            throttle,
            brake: 0.0,
            steering,
        }
    }

    /// 지금 장치가 쉬는 동안 다른 장치를 조작하면 장치가 바뀌는지 확인한다.
    #[test]
    fn idle_device_yields_to_active_device() {
        assert_eq!(
            select_source(
                InputSource::Keyboard,
                VehicleInput::default(),
                driving(0.5, 0.0)
            ),
            InputSource::Gamepad
        );
        assert_eq!(
            select_source(
                InputSource::Gamepad,
                driving(1.0, 0.0),
                VehicleInput::default()
            ),
            InputSource::Keyboard
        );
    }

    /// 두 장치가 함께 조작되거나 함께 쉬면 지금 장치를 유지하는지 확인한다.
    #[test]
    fn current_device_is_kept_when_both_active_or_idle() {
        let idle = VehicleInput::default();
        for source in [InputSource::Keyboard, InputSource::Gamepad] {
            assert_eq!(
                select_source(source, driving(1.0, 1.0), driving(0.5, -1.0)),
                source
            );
            assert_eq!(select_source(source, idle, idle), source);
        }
    }

    /// 선택되지 않은 장치의 조향·가속이 섞이지 않는지 확인한다.
    #[test]
    fn unselected_device_does_not_overwrite_driving_input() {
        let merged =
            merge_vehicle_input(InputSource::Gamepad, driving(1.0, -1.0), driving(0.25, 0.5));
        assert_eq!(merged, driving(0.25, 0.5));
    }

    /// 선택되지 않은 장치의 제동도 적용되고, 제동이 있으면 가속이 0이 되는지 확인한다.
    #[test]
    fn brake_from_any_device_wins() {
        let braking = VehicleInput {
            brake: 0.5,
            ..VehicleInput::default()
        };
        let merged = merge_vehicle_input(InputSource::Gamepad, braking, driving(1.0, 0.25));
        assert_eq!(merged.brake, 0.5);
        assert_eq!(merged.throttle, 0.0);
        assert_eq!(merged.steering, 0.25);
    }

    /// 두 장치의 시동 입력이 겹치면 토글 요청이 한 번만 나가는지 확인한다.
    #[test]
    fn engine_toggle_from_both_devices_is_single_request() {
        let pressed = DeviceFrame {
            engine_toggle_pressed: true,
            ..DeviceFrame::default()
        };
        assert_eq!(
            command_requests(merge_key_frames(&pressed, &pressed)),
            [ControlRequest::Command(VehicleCommand::ToggleEngine)]
        );
    }

    /// 한 장치의 비상 정지가 다른 장치의 해제·시동·기어 요청을 막는지 양방향으로 확인한다.
    #[test]
    fn emergency_stop_on_one_device_suppresses_the_other() {
        let stop = DeviceFrame {
            emergency_stop_pressed: true,
            emergency_stop_held: true,
            ..DeviceFrame::default()
        };
        let other = DeviceFrame {
            emergency_release_pressed: true,
            engine_toggle_pressed: true,
            gears_just_pressed: vec![Gear::Drive],
            gears_held: vec![Gear::Drive],
            ..DeviceFrame::default()
        };
        let expected = [ControlRequest::Command(VehicleCommand::EngageEmergencyStop)];
        assert_eq!(command_requests(merge_key_frames(&stop, &other)), expected);
        assert_eq!(command_requests(merge_key_frames(&other, &stop)), expected);
    }

    /// 두 장치가 다른 기어를 요청하면 거부하고, 같은 기어면 한 번의 요청이 되는지 확인한다.
    #[test]
    fn gear_requests_across_devices_are_resolved_together() {
        let gear = |gear| DeviceFrame {
            gears_just_pressed: vec![gear],
            gears_held: vec![gear],
            ..DeviceFrame::default()
        };
        assert_eq!(
            merge_key_frames(&gear(Gear::Drive), &gear(Gear::Reverse)).gear,
            GearSelection::Conflict(vec![Gear::Reverse, Gear::Drive])
        );
        assert_eq!(
            merge_key_frames(&gear(Gear::Drive), &gear(Gear::Drive)).gear,
            GearSelection::Requested(Gear::Drive)
        );
    }

    /// 게임패드로 주행 중 연결이 끊긴 경우에만 비상 정지가 필요한지 확인한다.
    #[test]
    fn safe_stop_only_when_gamepad_was_driving_a_moving_vehicle() {
        assert!(needs_safe_stop(true, InputSource::Gamepad, false));
        assert!(!needs_safe_stop(true, InputSource::Gamepad, true));
        assert!(!needs_safe_stop(true, InputSource::Keyboard, false));
        assert!(!needs_safe_stop(false, InputSource::Gamepad, false));
    }
}
