//! Xbox 유형 게임패드 입력.
//!
//! 버튼 이름은 Xbox 표기(`A`, `B`, `X`, `Y`, `LB`, `RB`, `Back`, `Start` 등)를 사용한다.
//! 버튼 배치와 데드존은 실제 장비 규칙이 아니라 **시뮬레이터 임시 매핑**이다.

use bevy::prelude::*;
use serde::Deserialize;

use super::mapping::{KeyAction, MappingError, gears};
use super::merge::{ConnectedGamepad, DeviceFrame, DeviceFrames, InputStatus};
use crate::vehicle::{Gear, VehicleInput};

/// 데드존 설정의 허용 상한. 축 입력이 거의 사라지는 잘못된 값을 막는다.
const MAX_DEADZONE: f32 = 0.9;

/// 가속·제동·조향을 어느 축에 둘지 정하는 배치.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DriveLayout {
    /// 왼쪽 스틱 좌우=조향, RT=가속, LT=제동.
    Triggers,
    /// 왼쪽 스틱 위=가속, 아래=제동, 오른쪽 스틱 좌우=조향(기본값, PLAN.md 0-1절).
    #[default]
    Sticks,
}

/// `config/input.toml`의 `[gamepad]` 구역.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GamepadMapping {
    pub layout: DriveLayout,
    pub stick_deadzone: f32,
    pub trigger_deadzone: f32,
    pub gear_park: String,
    pub gear_reverse: String,
    pub gear_neutral: String,
    pub gear_drive: String,
    pub engine_toggle: String,
    pub emergency_stop: String,
    pub emergency_release: String,
}

impl Default for GamepadMapping {
    /// 기본 게임패드 매핑(임시)을 만든다.
    fn default() -> Self {
        Self {
            layout: DriveLayout::default(),
            stick_deadzone: 0.12,
            trigger_deadzone: 0.05,
            gear_park: "Y".to_owned(),
            gear_reverse: "B".to_owned(),
            gear_neutral: "A".to_owned(),
            gear_drive: "X".to_owned(),
            engine_toggle: "Start".to_owned(),
            emergency_stop: "RB".to_owned(),
            emergency_release: "Back".to_owned(),
        }
    }
}

impl GamepadMapping {
    /// 동작과 설정된 버튼 이름의 전체 목록을 반환한다.
    fn buttons(&self) -> [(KeyAction, &str); 7] {
        [
            (KeyAction::Gear(Gear::Park), &self.gear_park),
            (KeyAction::Gear(Gear::Reverse), &self.gear_reverse),
            (KeyAction::Gear(Gear::Neutral), &self.gear_neutral),
            (KeyAction::Gear(Gear::Drive), &self.gear_drive),
            (KeyAction::EngineToggle, &self.engine_toggle),
            (KeyAction::EmergencyStop, &self.emergency_stop),
            (KeyAction::EmergencyRelease, &self.emergency_release),
        ]
    }

    /// 데드존 범위와 버튼 이름·중복 할당을 검사해 입력 시스템용 매핑을 만든다.
    pub fn resolve(&self) -> Result<GamepadBindings, String> {
        for (field, value) in [
            ("stick_deadzone", self.stick_deadzone),
            ("trigger_deadzone", self.trigger_deadzone),
        ] {
            if !(0.0..=MAX_DEADZONE).contains(&value) {
                return Err(format!(
                    "gamepad.{field} must be within 0..={MAX_DEADZONE}, got {value}"
                ));
            }
        }
        let mut buttons: Vec<ButtonBinding> = Vec::new();
        for (action, name) in self.buttons() {
            let button = parse_button_name(name).ok_or_else(|| {
                MappingError::UnknownKey {
                    action,
                    name: name.to_owned(),
                }
                .to_string()
            })?;
            if let Some(first) = buttons.iter().find(|binding| binding.button == button) {
                return Err(MappingError::DuplicateKey {
                    name: name.to_owned(),
                    first: first.action,
                    second: action,
                }
                .to_string());
            }
            buttons.push(ButtonBinding {
                action,
                button,
                name: name.to_owned(),
            });
        }
        Ok(GamepadBindings {
            layout: self.layout,
            stick_deadzone: self.stick_deadzone,
            trigger_deadzone: self.trigger_deadzone,
            buttons,
        })
    }
}

/// 하나의 동작에 연결된 버튼.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ButtonBinding {
    action: KeyAction,
    button: GamepadButton,
    name: String,
}

/// 검증을 마친 게임패드 매핑. 입력 시스템과 HUD가 사용한다.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct GamepadBindings {
    layout: DriveLayout,
    stick_deadzone: f32,
    trigger_deadzone: f32,
    buttons: Vec<ButtonBinding>,
}

impl Default for GamepadBindings {
    /// 기본 게임패드 매핑을 검증한 결과를 만든다.
    fn default() -> Self {
        GamepadMapping::default()
            .resolve()
            .expect("default gamepad mapping must be valid")
    }
}

impl GamepadBindings {
    /// 동작에 연결된 버튼 항목을 찾는다. 버튼 동작이 아니면 패닉한다.
    fn binding(&self, action: KeyAction) -> &ButtonBinding {
        self.buttons
            .iter()
            .find(|binding| binding.action == action)
            .expect("every button action is bound after validation")
    }

    /// 동작에 연결된 버튼을 반환한다.
    fn button(&self, action: KeyAction) -> GamepadButton {
        self.binding(action).button
    }

    /// 동작에 연결된 버튼 이름을 HUD 표시용으로 반환한다.
    pub fn label(&self, action: KeyAction) -> &str {
        &self.binding(action).name
    }

    /// 현재 배치의 가속·제동·조향 축 이름을 HUD 표시용으로 반환한다.
    pub fn drive_labels(&self) -> (&'static str, &'static str, &'static str) {
        match self.layout {
            DriveLayout::Triggers => ("RT", "LT", "left stick"),
            DriveLayout::Sticks => ("left stick up", "left stick down", "right stick"),
        }
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

/// 게임패드에서 읽은 축 값 묶음. 스틱은 -1..=1, 트리거는 0..=1이다.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PadAxes {
    pub left_x: f32,
    pub left_y: f32,
    pub right_x: f32,
    pub left_trigger: f32,
    pub right_trigger: f32,
}

/// 데드존 안의 값을 0으로 만들고, 바깥 값은 경계에서 0부터 다시 시작하도록 늘린다.
///
/// 경계를 넘는 순간 값이 튀지 않으며, 끝까지 밀면 ±1이 된다. NaN/무한대는 0으로 처리한다.
pub fn apply_deadzone(value: f32, deadzone: f32) -> f32 {
    if !value.is_finite() || value.abs() <= deadzone {
        return 0.0;
    }
    value.signum() * ((value.abs() - deadzone) / (1.0 - deadzone)).min(1.0)
}

/// 축 값을 차량 입력으로 변환한다. 가속과 제동이 함께 들어오면 키보드처럼 제동을 우선한다.
pub fn gamepad_vehicle_input(axes: PadAxes, bindings: &GamepadBindings) -> VehicleInput {
    let stick = |value| apply_deadzone(value, bindings.stick_deadzone);
    let trigger = |value| apply_deadzone(value, bindings.trigger_deadzone);
    let (throttle, brake, steering) = match bindings.layout {
        DriveLayout::Triggers => (
            trigger(axes.right_trigger).max(0.0),
            trigger(axes.left_trigger).max(0.0),
            stick(axes.left_x),
        ),
        DriveLayout::Sticks => {
            let vertical = stick(axes.left_y);
            (vertical.max(0.0), (-vertical).max(0.0), stick(axes.right_x))
        }
    };
    VehicleInput {
        throttle: if brake > 0.0 { 0.0 } else { throttle },
        brake,
        steering,
    }
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
        input: gamepad_vehicle_input(axes, bindings),
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

    /// 지정한 배치의 기본 매핑을 만든다.
    fn bindings(layout: DriveLayout) -> GamepadBindings {
        GamepadMapping {
            layout,
            ..GamepadMapping::default()
        }
        .resolve()
        .unwrap()
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

    /// 지원하지 않는 버튼 이름과 아날로그 축으로 쓰는 LT/RT 할당을 거부하는지 확인한다.
    #[test]
    fn unknown_or_analog_button_is_rejected() {
        for name in ["Z", "RT", ""] {
            let mapping = GamepadMapping {
                emergency_stop: name.to_owned(),
                ..GamepadMapping::default()
            };
            assert!(mapping.resolve().unwrap_err().contains("emergency_stop"));
        }
    }

    /// 한 버튼을 두 동작에 할당하면 거부하는지 확인한다.
    #[test]
    fn duplicate_button_is_rejected() {
        let mapping = GamepadMapping {
            emergency_release: "RB".to_owned(),
            ..GamepadMapping::default()
        };
        let error = mapping.resolve().unwrap_err();
        assert!(error.contains("emergency_stop") && error.contains("emergency_release"));
    }

    /// 범위를 벗어나거나 NaN인 데드존을 거부하는지 확인한다.
    #[test]
    fn invalid_deadzone_is_rejected() {
        for deadzone in [-0.1, 0.95, f32::NAN] {
            let mapping = GamepadMapping {
                stick_deadzone: deadzone,
                ..GamepadMapping::default()
            };
            assert!(mapping.resolve().is_err(), "{deadzone}");
        }
    }

    /// 데드존 안은 0, 경계 바로 밖은 0에 가깝고, 끝은 ±1이며 잘못된 값은 0인지 확인한다.
    #[test]
    fn deadzone_removes_center_noise_and_keeps_full_range() {
        assert_eq!(apply_deadzone(0.1, 0.12), 0.0);
        assert_eq!(apply_deadzone(-0.12, 0.12), 0.0);
        assert!(apply_deadzone(0.1201, 0.12) < 0.001);
        assert!((apply_deadzone(-0.56, 0.12) + 0.5).abs() < 1e-6);
        assert_eq!(apply_deadzone(1.0, 0.12), 1.0);
        assert_eq!(apply_deadzone(-1.0, 0.12), -1.0);
        assert_eq!(apply_deadzone(f32::NAN, 0.12), 0.0);
    }

    /// 기본 배치에서 왼쪽 스틱=조향, RT=가속, LT=제동이고 스틱 상하와 오른쪽 스틱은 무시하는지 확인한다.
    #[test]
    fn trigger_layout_maps_axes() {
        let input = gamepad_vehicle_input(
            PadAxes {
                left_x: -1.0,
                left_y: 1.0,
                right_x: 1.0,
                right_trigger: 1.0,
                left_trigger: 0.0,
            },
            &bindings(DriveLayout::Triggers),
        );
        assert_eq!(
            input,
            VehicleInput {
                throttle: 1.0,
                brake: 0.0,
                steering: -1.0
            }
        );
    }

    /// 스틱 배치에서 왼쪽 스틱 위=가속, 아래=제동, 오른쪽 스틱=조향이고 트리거는 무시하는지 확인한다.
    #[test]
    fn stick_layout_maps_axes() {
        let bindings = bindings(DriveLayout::Sticks);
        let up = gamepad_vehicle_input(
            PadAxes {
                left_y: 1.0,
                right_x: 1.0,
                left_trigger: 1.0,
                ..PadAxes::default()
            },
            &bindings,
        );
        assert_eq!(
            up,
            VehicleInput {
                throttle: 1.0,
                brake: 0.0,
                steering: 1.0
            }
        );
        let down = gamepad_vehicle_input(
            PadAxes {
                left_y: -1.0,
                ..PadAxes::default()
            },
            &bindings,
        );
        assert_eq!(down.throttle, 0.0);
        assert_eq!(down.brake, 1.0);
    }

    /// RT와 LT를 함께 밟으면 제동만 적용되는지 확인한다.
    #[test]
    fn brake_overrides_throttle() {
        let input = gamepad_vehicle_input(
            PadAxes {
                right_trigger: 1.0,
                left_trigger: 0.5,
                ..PadAxes::default()
            },
            &bindings(DriveLayout::Triggers),
        );
        assert_eq!(input.throttle, 0.0);
        assert!(input.brake > 0.0);
    }

    /// 스틱 중앙 떨림만 있으면 입력이 없는 상태로 처리되는지 확인한다.
    #[test]
    fn stick_drift_produces_no_input() {
        let input = gamepad_vehicle_input(
            PadAxes {
                left_x: 0.08,
                left_y: -0.09,
                right_x: 0.1,
                ..PadAxes::default()
            },
            &bindings(DriveLayout::Sticks),
        );
        assert_eq!(input, VehicleInput::default());
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
