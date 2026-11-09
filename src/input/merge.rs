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

//!
//! 합치기 규칙(장치 선택·제동·단발 명령·연결 해제 판정)은 엔진 무관 `sim_core::input::merge`에 있고,
//! 여기에는 Bevy 리소스와 시스템만 둔다.

use bevy::prelude::*;

use super::keyboard::command_requests;
use super::{ControlRequest, DriverInput};
use crate::calibration::ResetCourseRequest;
use crate::vehicle::Vehicle;

pub use sim_core::input::merge::{
    DeviceFrame, InputSource, merge_key_frames, merge_vehicle_input, needs_safe_stop, select_source,
};

/// 장치별 입력 시스템이 기록하고 [`combine_inputs`]가 읽는 이번 프레임 입력.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct DeviceFrames {
    pub keyboard: DeviceFrame,
    pub gamepad: DeviceFrame,
    /// 조작에 쓰던 게임패드가 이번 프레임에 사라짐.
    pub gamepad_lost: bool,
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
