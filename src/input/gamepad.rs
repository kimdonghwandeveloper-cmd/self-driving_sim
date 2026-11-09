//! Xbox 유형 게임패드 입력(Bevy 장치 읽기).
//!
//! 매핑 설정·검증, 데드존, 축→차량 입력 변환은 Godot 판과 함께 쓰도록 `sim_core::input::gamepad`에 있다.
//! 여기에는 버튼 이름→`GamepadButton` 변환과 Bevy 시스템만 둔다.

use bevy::prelude::*;

use super::mapping::{KeyAction, gears};
use super::merge::{ConnectedGamepad, DeviceFrame, DeviceFrames, InputStatus};

pub use sim_core::input::gamepad::{
    DriveLayout, GamepadMapping, PadAxes, apply_deadzone, gamepad_vehicle_input,
};

/// 검증을 마친 Bevy 게임패드 매핑 리소스. 입력 시스템과 HUD가 사용한다.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct GamepadBindings(sim_core::input::gamepad::GamepadBindings<GamepadButton>);

impl Default for GamepadBindings {
    /// 기본 게임패드 매핑을 검증한 결과를 만든다.
    fn default() -> Self {
        GamepadMapping::default()
            .resolve()
            .expect("default gamepad mapping must be valid")
    }
}

impl GamepadBindings {
    /// 동작에 연결된 버튼을 반환한다.
    fn button(&self, action: KeyAction) -> GamepadButton {
        self.0.button(action)
    }

    /// 동작에 연결된 버튼 이름을 HUD 표시용으로 반환한다.
    pub fn label(&self, action: KeyAction) -> &str {
        self.0.label(action)
    }

    /// 현재 배치의 가속·제동·조향 축 이름을 HUD 표시용으로 반환한다.
    pub fn drive_labels(&self) -> (&'static str, &'static str, &'static str) {
        self.0.drive_labels()
    }
}

/// 게임패드 매핑 설정을 Bevy 매핑으로 검증·변환하는 확장.
pub trait ResolveGamepad {
    /// 데드존 범위와 버튼 이름·중복 할당을 검사해 입력 시스템용 매핑을 만든다.
    fn resolve(&self) -> Result<GamepadBindings, String>;
}

impl ResolveGamepad for GamepadMapping {
    /// [`parse_button_name`]으로 `sim_core` 공통 검증을 거친다.
    fn resolve(&self) -> Result<GamepadBindings, String> {
        self.resolve_with(parse_button_name).map(GamepadBindings)
    }
}

/// 설정 파일의 버튼 이름을 `GamepadButton`으로 변환한다. 지원하지 않는 이름은 `None`.
///
/// LT/RT는 아날로그 가속·제동 축으로 쓰므로 버튼 동작에는 할당할 수 없다.
pub fn parse_button_name(name: &str) -> Option<GamepadButton> {
    let button = match name {
        "A" => GamepadButton::South,
        "B" => GamepadButton::East,
        "X" => GamepadButton::West,
        "Y" => GamepadButton::North,
        "LB" => GamepadButton::LeftTrigger,
        "RB" => GamepadButton::RightTrigger,
        "Back" => GamepadButton::Select,
        "Start" => GamepadButton::Start,
        "LS" => GamepadButton::LeftThumb,
        "RS" => GamepadButton::RightThumb,
        "DPadUp" => GamepadButton::DPadUp,
        "DPadDown" => GamepadButton::DPadDown,
        "DPadLeft" => GamepadButton::DPadLeft,
        "DPadRight" => GamepadButton::DPadRight,
        _ => return None,
    };
    Some(button)
}

/// 게임패드 한 대의 현재 상태를 장치 프레임으로 바꾼다.
pub fn gamepad_frame(gamepad: &Gamepad, bindings: &GamepadBindings) -> DeviceFrame {
    let axis = |axis| gamepad.get(axis).unwrap_or(0.0);
    let trigger = |button| gamepad.get(button).unwrap_or(0.0);
    let axes = PadAxes {
        left_x: axis(GamepadAxis::LeftStickX),
        left_y: axis(GamepadAxis::LeftStickY),
        right_x: axis(GamepadAxis::RightStickX),
        left_trigger: trigger(GamepadButton::LeftTrigger2),
        right_trigger: trigger(GamepadButton::RightTrigger2),
    };
    let pressed = |action| gamepad.pressed(bindings.button(action));
    let just_pressed = |action| gamepad.just_pressed(bindings.button(action));
    DeviceFrame {
        input: gamepad_vehicle_input(axes, &bindings.0),
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
        reset_pressed: false,
    }
}

/// 조작에 쓸 게임패드를 고르고 이번 프레임의 게임패드 입력을 갱신한다.
///
/// 쓰던 게임패드가 있으면 유지하고, 없으면 연결된 것 중 하나를 고른다. 게임패드가 사라지면
/// `Gamepad` 컴포넌트가 제거되어 조회되지 않으므로 이전 입력은 남지 않는다.
///
/// 새로 고른 게임패드는 모든 입력이 중립이 될 때까지 비상 정지 외의 입력을 쓰지 않는다.
/// 따라서 대체 게임패드의 눌려 있던 트리거·스틱으로 차량이 갑자기 움직이지 않는다.
pub fn read_gamepad(
    gamepads: Query<(Entity, &Gamepad, Option<&Name>)>,
    bindings: Res<GamepadBindings>,
    mut status: ResMut<InputStatus>,
    mut frames: ResMut<DeviceFrames>,
) {
    let previous = status.gamepad.as_ref().map(|pad| pad.entity);
    let kept = previous.filter(|entity| gamepads.contains(*entity));
    let selected = kept.or_else(|| gamepads.iter().map(|(entity, ..)| entity).min());

    frames.gamepad_lost = previous.is_some() && kept.is_none();
    if selected != previous {
        status.gamepad =
            selected
                .and_then(|entity| gamepads.get(entity).ok())
                .map(|(entity, _, name)| ConnectedGamepad {
                    entity,
                    name: name
                        .map_or_else(|| "unnamed".to_owned(), |name| name.as_str().to_owned()),
                    ready: false,
                });
    }
    let frame = selected
        .and_then(|entity| gamepads.get(entity).ok())
        .map_or_else(DeviceFrame::default, |(_, gamepad, _)| {
            gamepad_frame(gamepad, &bindings)
        });
    let emergency_only = frame.emergency_only();
    let neutral = frame == emergency_only;
    let ready = status.gamepad.as_mut().is_some_and(|pad| {
        pad.ready |= neutral;
        pad.ready
    });
    frames.gamepad = if ready { frame } else { emergency_only };
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::vehicle::Gear;
    use sim_core::input::gamepad::SUPPORTED_BUTTON_NAMES;

    /// 공통 지원 버튼 이름을 Bevy가 모두 알아보는지 확인한다.
    #[test]
    fn every_supported_button_name_parses() {
        for name in SUPPORTED_BUTTON_NAMES {
            assert!(parse_button_name(name).is_some(), "{name}");
        }
    }

    /// 기본 매핑이 검증을 통과하고 계획된 버튼과 축을 쓰는지 확인한다.
    #[test]
    fn default_mapping_is_valid() {
        let bindings = GamepadBindings::default();
        assert_eq!(
            bindings.button(KeyAction::Gear(Gear::Drive)),
            GamepadButton::West
        );
        assert_eq!(bindings.label(KeyAction::EmergencyStop), "RB");
        assert_eq!(
            bindings.drive_labels(),
            ("left stick up", "left stick down", "right stick")
        );
    }

    /// 버튼 상태가 새로 눌림·누르고 있음으로 구분되어 장치 프레임에 들어가는지 확인한다.
    #[test]
    fn frame_separates_new_and_held_buttons() {
        let bindings = GamepadBindings::default();
        let mut gamepad = Gamepad::default();
        gamepad.digital_mut().press(GamepadButton::West);
        gamepad.digital_mut().press(GamepadButton::RightTrigger);
        gamepad.digital_mut().clear();
        gamepad.digital_mut().press(GamepadButton::Start);

        let frame = gamepad_frame(&gamepad, &bindings);
        assert!(frame.engine_toggle_pressed);
        assert!(frame.emergency_stop_held);
        assert!(!frame.emergency_stop_pressed);
        assert_eq!(frame.gears_held, [Gear::Drive]);
        assert!(frame.gears_just_pressed.is_empty());
    }
}
