//! 입력 어댑터. 장치 입력을 차량 조작 입력과 명령 요청으로 변환한다.
//!
//! 현재는 개발용 키보드 입력만 제공한다. 게임패드는 장치 확보 후 같은 출력 형식으로 추가한다.

pub mod keyboard;
pub mod mapping;

use bevy::prelude::*;

use crate::SimSet;
use crate::vehicle::{Gear, VehicleCommand, VehicleInput};

pub use mapping::{KeyBindings, KeyboardMapping};

/// 이번 프레임의 연속 조작 입력(가속·제동·조향).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct DriverInput(pub VehicleInput);

/// 입력 장치가 컨트롤러에 보내는 단발성 요청.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub enum ControlRequest {
    Command(VehicleCommand),
    /// 서로 다른 기어 키가 동시에 눌려 거부된 요청.
    GearConflict(Vec<Gear>),
}

/// 입력 리소스·메시지와 키보드 입력 시스템을 등록하는 플러그인.
pub struct InputPlugin;

impl Plugin for InputPlugin {
    /// 입력 관련 리소스, 메시지, 시스템을 앱에 등록한다.
    fn build(&self, app: &mut App) {
        app.init_resource::<DriverInput>()
            .init_resource::<KeyBindings>()
            .add_message::<ControlRequest>()
            .add_systems(Update, keyboard::read_keyboard.in_set(SimSet::Input));
    }
}
