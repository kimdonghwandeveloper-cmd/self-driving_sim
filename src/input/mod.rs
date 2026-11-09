//! 입력 어댑터. 장치 입력을 차량 조작 입력과 명령 요청으로 변환한다.
//!
//! 장치별 입력은 [`merge::DeviceFrames`]에 모은 뒤 프레임마다 한 번 합쳐 내보낸다.
//! 합치기·명령 변환 규칙은 Godot 판과 함께 쓰도록 `sim_core::input`에 있다.

pub mod gamepad;
pub mod keyboard;
pub mod mapping;
pub mod merge;

use bevy::prelude::*;

use bevy::app::RunFixedMainLoop;

use crate::SimSet;
use crate::calibration::ResetCourseRequest;
use crate::vehicle::VehicleInput;

pub use gamepad::{GamepadBindings, GamepadMapping};
pub use mapping::{KeyBindings, KeyboardMapping};
pub use merge::{InputSource, InputStatus};

/// 이번 프레임의 연속 조작 입력(가속·제동·조향).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct DriverInput(pub VehicleInput);

/// 입력 장치가 컨트롤러에 보내는 단발성 요청. 정의는 엔진 무관 `sim_core::input`에 있다.
pub use sim_core::input::ControlRequest;

/// 입력 리소스·메시지와 장치 입력·통합 시스템을 등록하는 플러그인.
pub struct InputPlugin;

impl Plugin for InputPlugin {
    /// 입력 관련 리소스, 메시지, 시스템을 앱에 등록한다. 입력은 고정 주기 루프 직전에 읽고 합친다.
    fn build(&self, app: &mut App) {
        app.init_resource::<DriverInput>()
            .init_resource::<KeyBindings>()
            .init_resource::<GamepadBindings>()
            .init_resource::<merge::DeviceFrames>()
            .init_resource::<InputStatus>()
            .add_message::<ControlRequest>()
            .add_message::<ResetCourseRequest>()
            .add_systems(
                RunFixedMainLoop,
                (
                    keyboard::read_keyboard,
                    gamepad::read_gamepad,
                    merge::combine_inputs,
                )
                    .chain()
                    .in_set(SimSet::Input),
            );
    }
}
