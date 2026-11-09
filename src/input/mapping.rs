//! 키보드 키 이름 매핑. 기어 요청 판정은 `sim_core::input::gear`에 있다.
//!
//! 설정 파일의 키 이름은 Bevy `KeyCode` 변형 이름(예: `"ArrowUp"`, `"KeyW"`)을 사용한다.

use std::fmt;

use bevy::prelude::*;
use serde::Deserialize;

use crate::camera::ViewDirection;
use crate::vehicle::Gear;

pub use sim_core::input::gear::{GearSelection, gears, resolve_gear_selection};

/// 키에 할당할 수 있는 조작 동작.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Throttle,
    Brake,
    SteerLeft,
    SteerRight,
    Gear(Gear),
    EngineToggle,
    EmergencyStop,
    EmergencyRelease,
    /// 정지 상태에서 시작 위치로 복귀(보정 시험 반복용).
    ResetCourse,
    /// 입석 조작자 시선 방향 선택. 주행 명령과 별도로 처리한다.
    View(ViewDirection),
    /// 개발용 외부 디버그 카메라 켜기/끄기.
    DebugCamera,
}

impl KeyAction {
    /// 매핑 검증과 HUD 표시에 사용하는 전체 동작 목록.
    pub const ALL: [KeyAction; 17] = [
        KeyAction::Throttle,
        KeyAction::Brake,
        KeyAction::SteerLeft,
        KeyAction::SteerRight,
        KeyAction::Gear(Gear::Park),
        KeyAction::Gear(Gear::Reverse),
        KeyAction::Gear(Gear::Neutral),
        KeyAction::Gear(Gear::Drive),
        KeyAction::EngineToggle,
        KeyAction::EmergencyStop,
        KeyAction::EmergencyRelease,
        KeyAction::ResetCourse,
        KeyAction::View(ViewDirection::Front),
        KeyAction::View(ViewDirection::Rear),
        KeyAction::View(ViewDirection::Left),
        KeyAction::View(ViewDirection::Right),
        KeyAction::DebugCamera,
    ];

    /// 동작에 대응하는 설정 파일 필드 이름을 반환한다.
    pub fn config_name(self) -> &'static str {
        match self {
            KeyAction::Throttle => "throttle",
            KeyAction::Brake => "brake",
            KeyAction::SteerLeft => "steer_left",
            KeyAction::SteerRight => "steer_right",
            KeyAction::Gear(Gear::Park) => "gear_park",
            KeyAction::Gear(Gear::Reverse) => "gear_reverse",
            KeyAction::Gear(Gear::Neutral) => "gear_neutral",
            KeyAction::Gear(Gear::Drive) => "gear_drive",
            KeyAction::EngineToggle => "engine_toggle",
            KeyAction::EmergencyStop => "emergency_stop",
            KeyAction::EmergencyRelease => "emergency_release",
            KeyAction::ResetCourse => "reset_course",
            KeyAction::View(ViewDirection::Front) => "view_front",
            KeyAction::View(ViewDirection::Rear) => "view_rear",
            KeyAction::View(ViewDirection::Left) => "view_left",
            KeyAction::View(ViewDirection::Right) => "view_right",
            KeyAction::DebugCamera => "debug_camera",
        }
    }
}

/// `config/input.toml`의 `[keyboard]` 구역. 동작마다 키 이름 목록을 가진다.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KeyboardMapping {
    pub throttle: Vec<String>,
    pub brake: Vec<String>,
    pub steer_left: Vec<String>,
    pub steer_right: Vec<String>,
    pub gear_park: Vec<String>,
    pub gear_reverse: Vec<String>,
    pub gear_neutral: Vec<String>,
    pub gear_drive: Vec<String>,
    pub engine_toggle: Vec<String>,
    pub emergency_stop: Vec<String>,
    pub emergency_release: Vec<String>,
    pub reset_course: Vec<String>,
    pub view_front: Vec<String>,
    pub view_rear: Vec<String>,
    pub view_left: Vec<String>,
    pub view_right: Vec<String>,
    pub debug_camera: Vec<String>,
}

impl Default for KeyboardMapping {
    /// 개발용 기본 키 매핑(임시)을 만든다.
    fn default() -> Self {
        /// 문자열 슬라이스를 키 이름 목록으로 바꾼다.
        fn names(keys: &[&str]) -> Vec<String> {
            keys.iter().map(|key| (*key).to_owned()).collect()
        }
        Self {
            throttle: names(&["KeyW", "ArrowUp"]),
            brake: names(&["KeyS", "ArrowDown"]),
            steer_left: names(&["KeyA", "ArrowLeft"]),
            steer_right: names(&["KeyD", "ArrowRight"]),
            gear_park: names(&["KeyI"]),
            gear_reverse: names(&["KeyL"]),
            gear_neutral: names(&["KeyK"]),
            gear_drive: names(&["KeyJ"]),
            engine_toggle: names(&["Enter"]),
            emergency_stop: names(&["Space"]),
            emergency_release: names(&["Backspace"]),
            reset_course: names(&["KeyR"]),
            view_front: names(&["Digit1"]),
            view_rear: names(&["Digit2"]),
            view_left: names(&["Digit3"]),
            view_right: names(&["Digit4"]),
            debug_camera: names(&["KeyC"]),
        }
    }
}

impl KeyboardMapping {
    /// 동작에 설정된 키 이름 목록을 반환한다.
    fn names(&self, action: KeyAction) -> &[String] {
        match action {
            KeyAction::Throttle => &self.throttle,
            KeyAction::Brake => &self.brake,
            KeyAction::SteerLeft => &self.steer_left,
            KeyAction::SteerRight => &self.steer_right,
            KeyAction::Gear(Gear::Park) => &self.gear_park,
            KeyAction::Gear(Gear::Reverse) => &self.gear_reverse,
            KeyAction::Gear(Gear::Neutral) => &self.gear_neutral,
            KeyAction::Gear(Gear::Drive) => &self.gear_drive,
            KeyAction::EngineToggle => &self.engine_toggle,
            KeyAction::EmergencyStop => &self.emergency_stop,
            KeyAction::EmergencyRelease => &self.emergency_release,
            KeyAction::ResetCourse => &self.reset_course,
            KeyAction::View(ViewDirection::Front) => &self.view_front,
            KeyAction::View(ViewDirection::Rear) => &self.view_rear,
            KeyAction::View(ViewDirection::Left) => &self.view_left,
            KeyAction::View(ViewDirection::Right) => &self.view_right,
            KeyAction::DebugCamera => &self.debug_camera,
        }
    }

    /// 키 이름을 `KeyCode`로 변환하고, 미할당·알 수 없는 키·중복 할당을 검사한다.
    pub fn resolve(&self) -> Result<KeyBindings, MappingError> {
        let mut bindings = Vec::with_capacity(KeyAction::ALL.len());
        let mut used: Vec<(KeyCode, KeyAction)> = Vec::new();
        for action in KeyAction::ALL {
            let names = self.names(action);
            if names.is_empty() {
                return Err(MappingError::Unbound { action });
            }
            let mut keys = Vec::with_capacity(names.len());
            for name in names {
                let key = parse_key_name(name).ok_or_else(|| MappingError::UnknownKey {
                    action,
                    name: name.clone(),
                })?;
                if let Some(&(_, first)) = used.iter().find(|(used_key, _)| *used_key == key) {
                    return Err(MappingError::DuplicateKey {
                        name: name.clone(),
                        first,
                        second: action,
                    });
                }
                used.push((key, action));
                keys.push(key);
            }
            bindings.push(Binding {
                action,
                keys,
                names: names.to_vec(),
            });
        }
        Ok(KeyBindings { bindings })
    }
}

/// 키 매핑 검증 실패 사유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappingError {
    Unbound {
        action: KeyAction,
    },
    UnknownKey {
        action: KeyAction,
        name: String,
    },
    DuplicateKey {
        name: String,
        first: KeyAction,
        second: KeyAction,
    },
}

impl fmt::Display for MappingError {
    /// 검증 실패 사유를 설정 필드 이름과 함께 출력한다.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MappingError::Unbound { action } => {
                write!(f, "`{}` has no key assigned", action.config_name())
            }
            MappingError::UnknownKey { action, name } => {
                write!(f, "`{}`: unknown key name `{name}`", action.config_name())
            }
            MappingError::DuplicateKey {
                name,
                first,
                second,
            } => write!(
                f,
                "key `{name}` is assigned to both `{}` and `{}`",
                first.config_name(),
                second.config_name()
            ),
        }
    }
}

impl std::error::Error for MappingError {}

/// 하나의 동작에 연결된 키 목록.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Binding {
    action: KeyAction,
    keys: Vec<KeyCode>,
    names: Vec<String>,
}

/// 검증을 마친 키 매핑. 입력 시스템과 HUD가 사용한다.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct KeyBindings {
    bindings: Vec<Binding>,
}

impl Default for KeyBindings {
    /// 기본 키 매핑을 검증한 결과를 만든다.
    fn default() -> Self {
        KeyboardMapping::default()
            .resolve()
            .expect("default keyboard mapping must be valid")
    }
}

impl KeyBindings {
    /// 동작에 연결된 키 목록을 찾는다(검증 후에는 모든 동작이 존재한다).
    fn binding(&self, action: KeyAction) -> &Binding {
        self.bindings
            .iter()
            .find(|binding| binding.action == action)
            .expect("every action is bound after validation")
    }

    /// 동작에 연결된 `KeyCode` 목록을 반환한다.
    pub fn keys(&self, action: KeyAction) -> &[KeyCode] {
        &self.binding(action).keys
    }

    /// 동작에 연결된 키 이름을 `/`로 이어 HUD용 문자열로 반환한다.
    pub fn label(&self, action: KeyAction) -> String {
        self.binding(action).names.join("/")
    }
}

/// 설정 파일의 키 이름을 `KeyCode`로 변환한다. 지원하지 않는 이름은 `None`.
pub fn parse_key_name(name: &str) -> Option<KeyCode> {
    let key = match name {
        "KeyA" => KeyCode::KeyA,
        "KeyB" => KeyCode::KeyB,
        "KeyC" => KeyCode::KeyC,
        "KeyD" => KeyCode::KeyD,
        "KeyE" => KeyCode::KeyE,
        "KeyF" => KeyCode::KeyF,
        "KeyG" => KeyCode::KeyG,
        "KeyH" => KeyCode::KeyH,
        "KeyI" => KeyCode::KeyI,
        "KeyJ" => KeyCode::KeyJ,
        "KeyK" => KeyCode::KeyK,
        "KeyL" => KeyCode::KeyL,
        "KeyM" => KeyCode::KeyM,
        "KeyN" => KeyCode::KeyN,
        "KeyO" => KeyCode::KeyO,
        "KeyP" => KeyCode::KeyP,
        "KeyQ" => KeyCode::KeyQ,
        "KeyR" => KeyCode::KeyR,
        "KeyS" => KeyCode::KeyS,
        "KeyT" => KeyCode::KeyT,
        "KeyU" => KeyCode::KeyU,
        "KeyV" => KeyCode::KeyV,
        "KeyW" => KeyCode::KeyW,
        "KeyX" => KeyCode::KeyX,
        "KeyY" => KeyCode::KeyY,
        "KeyZ" => KeyCode::KeyZ,
        "Digit0" => KeyCode::Digit0,
        "Digit1" => KeyCode::Digit1,
        "Digit2" => KeyCode::Digit2,
        "Digit3" => KeyCode::Digit3,
        "Digit4" => KeyCode::Digit4,
        "Digit5" => KeyCode::Digit5,
        "Digit6" => KeyCode::Digit6,
        "Digit7" => KeyCode::Digit7,
        "Digit8" => KeyCode::Digit8,
        "Digit9" => KeyCode::Digit9,
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        "Space" => KeyCode::Space,
        "Enter" => KeyCode::Enter,
        "Backspace" => KeyCode::Backspace,
        "Escape" => KeyCode::Escape,
        "Tab" => KeyCode::Tab,
        "ShiftLeft" => KeyCode::ShiftLeft,
        "ShiftRight" => KeyCode::ShiftRight,
        "ControlLeft" => KeyCode::ControlLeft,
        "ControlRight" => KeyCode::ControlRight,
        "AltLeft" => KeyCode::AltLeft,
        "AltRight" => KeyCode::AltRight,
        _ => return None,
    };
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 기본 키 매핑이 검증을 통과하고 기대한 키·표시 이름을 갖는지 확인한다.
    #[test]
    fn default_mapping_is_valid() {
        let bindings = KeyboardMapping::default().resolve().unwrap();
        assert_eq!(
            bindings.keys(KeyAction::Throttle),
            [KeyCode::KeyW, KeyCode::ArrowUp]
        );
        assert_eq!(bindings.label(KeyAction::Gear(Gear::Drive)), "KeyJ");
        assert_eq!(bindings.keys(KeyAction::ResetCourse), [KeyCode::KeyR]);
        assert_eq!(
            bindings.keys(KeyAction::View(ViewDirection::Rear)),
            [KeyCode::Digit2]
        );
        assert_eq!(bindings.label(KeyAction::DebugCamera), "KeyC");
    }

    /// 시점 키가 주행·기어 키와 겹치면 거부하는지 확인한다.
    #[test]
    fn view_key_conflicting_with_drive_key_is_rejected() {
        let mapping = KeyboardMapping {
            view_left: vec!["KeyA".into()],
            ..KeyboardMapping::default()
        };
        assert_eq!(
            mapping.resolve(),
            Err(MappingError::DuplicateKey {
                name: "KeyA".into(),
                first: KeyAction::SteerLeft,
                second: KeyAction::View(ViewDirection::Left),
            })
        );
        let mapping = KeyboardMapping {
            view_front: vec!["KeyJ".into()],
            ..KeyboardMapping::default()
        };
        assert!(matches!(
            mapping.resolve(),
            Err(MappingError::DuplicateKey { .. })
        ));
    }

    /// 지원하지 않는 키 이름을 거부하는지 확인한다.
    #[test]
    fn unknown_key_name_is_rejected() {
        let mapping = KeyboardMapping {
            brake: vec!["Keyy".into()],
            ..KeyboardMapping::default()
        };
        assert_eq!(
            mapping.resolve(),
            Err(MappingError::UnknownKey {
                action: KeyAction::Brake,
                name: "Keyy".into()
            })
        );
    }

    /// 한 키를 두 동작에 할당하면 거부하는지 확인한다.
    #[test]
    fn duplicate_key_is_rejected() {
        let mapping = KeyboardMapping {
            steer_right: vec!["KeyJ".into()],
            ..KeyboardMapping::default()
        };
        assert_eq!(
            mapping.resolve(),
            Err(MappingError::DuplicateKey {
                name: "KeyJ".into(),
                first: KeyAction::SteerRight,
                second: KeyAction::Gear(Gear::Drive),
            })
        );
    }

    /// 키가 하나도 없는 동작을 거부하는지 확인한다.
    #[test]
    fn unbound_action_is_rejected() {
        let mapping = KeyboardMapping {
            emergency_stop: Vec::new(),
            ..KeyboardMapping::default()
        };
        assert_eq!(
            mapping.resolve(),
            Err(MappingError::Unbound {
                action: KeyAction::EmergencyStop
            })
        );
    }
}
