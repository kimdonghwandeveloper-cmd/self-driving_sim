//! Xbox 유형 게임패드 매핑과 축 변환의 엔진 무관 부분(Phase GD-2).
//!
//! 버튼 이름은 Xbox 표기(`A`, `B`, `X`, `Y`, `LB`, `RB`, `Back`, `Start` 등)를 사용한다.
//! 이름을 실제 버튼 값으로 바꾸는 일은 각 엔진이 [`GamepadMapping::resolve_with`]에 넘기는 함수가 한다.
//! 버튼 배치와 데드존은 실제 장비 규칙이 아니라 **시뮬레이터 임시 매핑**이다.

use serde::Deserialize;

use super::mapping::{KeyAction, MappingError};
use crate::vehicle::{Gear, VehicleInput};

/// 설정 파일에 쓸 수 있는 버튼 이름 전체. LT/RT는 아날로그 축이라 버튼 동작에 쓸 수 없다.
pub const SUPPORTED_BUTTON_NAMES: [&str; 14] = [
    "A",
    "B",
    "X",
    "Y",
    "LB",
    "RB",
    "Back",
    "Start",
    "LS",
    "RS",
    "DPadUp",
    "DPadDown",
    "DPadLeft",
    "DPadRight",
];

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

    /// 데드존 범위와 버튼 이름·중복 할당을 검사해 `parse`로 엔진 버튼 값을 쓰는 매핑을 만든다.
    pub fn resolve_with<B: Copy + PartialEq>(
        &self,
        parse: impl Fn(&str) -> Option<B>,
    ) -> Result<GamepadBindings<B>, String> {
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
        let mut buttons: Vec<ButtonBinding<B>> = Vec::new();
        for (action, name) in self.buttons() {
            let button = parse(name).ok_or_else(|| {
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
struct ButtonBinding<B> {
    action: KeyAction,
    button: B,
    name: String,
}

/// 검증을 마친 게임패드 매핑. `B`는 엔진의 버튼 값(Bevy `GamepadButton`, Godot `JoyButton`)이다.
#[derive(Debug, Clone, PartialEq)]
pub struct GamepadBindings<B> {
    layout: DriveLayout,
    stick_deadzone: f32,
    trigger_deadzone: f32,
    buttons: Vec<ButtonBinding<B>>,
}

impl<B: Copy> GamepadBindings<B> {
    /// 동작에 연결된 버튼 항목을 찾는다. 버튼 동작이 아니면 패닉한다.
    fn binding(&self, action: KeyAction) -> &ButtonBinding<B> {
        self.buttons
            .iter()
            .find(|binding| binding.action == action)
            .expect("every button action is bound after validation")
    }

    /// 동작에 연결된 버튼을 반환한다. 버튼 동작(기어·시동·비상 정지·해제)이 아니면 패닉한다.
    pub fn button(&self, action: KeyAction) -> B {
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
pub fn gamepad_vehicle_input<B>(axes: PadAxes, bindings: &GamepadBindings<B>) -> VehicleInput {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 지원 버튼 이름을 목록 순번으로 바꾸는 시험용 변환.
    fn index_of(name: &str) -> Option<usize> {
        SUPPORTED_BUTTON_NAMES
            .iter()
            .position(|known| *known == name)
    }

    /// 지정한 배치의 기본 매핑을 만든다.
    fn bindings(layout: DriveLayout) -> GamepadBindings<usize> {
        GamepadMapping {
            layout,
            ..GamepadMapping::default()
        }
        .resolve_with(index_of)
        .unwrap()
    }

    /// 지원하지 않는 버튼 이름과 아날로그 축으로 쓰는 LT/RT 할당을 거부하는지 확인한다.
    #[test]
    fn unknown_or_analog_button_is_rejected() {
        for name in ["Z", "RT", ""] {
            let mapping = GamepadMapping {
                emergency_stop: name.to_owned(),
                ..GamepadMapping::default()
            };
            assert!(
                mapping
                    .resolve_with(index_of)
                    .unwrap_err()
                    .contains("emergency_stop")
            );
        }
    }

    /// 한 버튼을 두 동작에 할당하면 거부하는지 확인한다.
    #[test]
    fn duplicate_button_is_rejected() {
        let mapping = GamepadMapping {
            emergency_release: "RB".to_owned(),
            ..GamepadMapping::default()
        };
        let error = mapping.resolve_with(index_of).unwrap_err();
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
            assert!(mapping.resolve_with(index_of).is_err(), "{deadzone}");
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
}
