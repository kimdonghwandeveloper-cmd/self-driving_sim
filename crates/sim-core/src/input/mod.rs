//! 장치 입력을 차량 명령 요청과 연속 입력으로 바꾸는 엔진 무관 규칙(Phase GD-1).
//!
//! 엔진이 바뀌어도 같은 규칙을 쓰도록 장치 읽기·키 매핑을 뺀 순수 함수만 둔다.
//! 장치 읽기는 각 앱이 [`merge::DeviceFrame`]을 채워 넘긴다.
//!
//! - 조향·가속: [`merge::InputSource`]로 선택한 장치 하나의 값을 쓴다. 다른 장치를 조작해도 지금 장치의
//!   입력이 없어질 때까지는 선택을 유지하고, 지금 장치가 쉬는 동안 다른 장치를 조작하면 바꾼다.
//! - 제동: 어느 장치든 더 강한 값을 쓴다. 제동이 있으면 가속은 0이다.
//! - 단발 명령: 하나의 프레임으로 합쳐 [`commands::command_requests`] 규칙을 따른다. 비상 정지는 어느
//!   장치든 우선하고, 시동 토글은 두 장치가 함께 눌러도 한 번이며, 기어가 다르면 동시 요청으로 거부한다.
//! - 연결 해제: 게임패드로 주행 중 연결이 끊기면 비상 정지를 요청한다.

pub mod commands;
pub mod gamepad;
pub mod gear;
pub mod mapping;
pub mod merge;

use crate::vehicle::{Gear, VehicleCommand};

pub use commands::{KeyFrame, command_requests, keyboard_vehicle_input};
pub use gear::{GearSelection, gears, resolve_gear_selection};
pub use merge::{
    DeviceFrame, InputSource, merge_key_frames, merge_vehicle_input, needs_safe_stop, select_source,
};

/// 입력 장치가 컨트롤러에 보내는 단발성 요청.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlRequest {
    Command(VehicleCommand),
    /// 서로 다른 기어 키가 동시에 눌려 거부된 요청.
    GearConflict(Vec<Gear>),
}
