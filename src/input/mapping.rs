//! 키보드 키 이름 매핑과 기어 요청 판정.
//!
//! 설정 파일의 키 이름은 Bevy `KeyCode` 변형 이름(예: `"ArrowUp"`, `"KeyW"`)을 사용한다.

use std::fmt;

use bevy::prelude::*;
use serde::Deserialize;

use crate::vehicle::Gear;

/// 기어 키 처리 순서(P, R, N, D).
const GEARS: [Gear; 4] = [Gear::Park, Gear::Reverse, Gear::Neutral, Gear::Drive];

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
}

impl KeyAction {
    /// 매핑 검증과 HUD 표시에 사용하는 전체 동작 목록.
    pub const ALL: [KeyAction; 11] = [
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

/// 이번 프레임의 기어 키 판정 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GearSelection {
    None,
    Requested(Gear),
    /// 서로 다른 기어 키가 동시에 눌려 요청을 거부함.
    Conflict(Vec<Gear>),
}

/// 새로 눌린 기어 키와 눌려 있는 기어 키로 요청을 판정한다.
///
/// 새로 눌린 키가 없으면 요청 없음. 새로 눌린 키를 포함해 서로 다른 기어가 둘 이상
/// 눌려 있으면 동시 요청으로 거부한다.
pub fn resolve_gear_selection(just_pressed: &[Gear], held: &[Gear]) -> GearSelection {
    if just_pressed.is_empty() {
        return GearSelection::None;
    }
    let active: Vec<Gear> = GEARS
        .into_iter()
        .filter(|gear| just_pressed.contains(gear) || held.contains(gear))
        .collect();
    match active.as_slice() {
        [gear] => GearSelection::Requested(*gear),
        _ => GearSelection::Conflict(active),
    }
}

/// 기어 키 처리 순서 목록을 반환한다.
pub fn gears() -> [Gear; 4] {
    GEARS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mapping_is_valid() {
        let bindings = KeyboardMapping::default().resolve().unwrap();
        assert_eq!(
            bindings.keys(KeyAction::Throttle),
            [KeyCode::KeyW, KeyCode::ArrowUp]
        );
        assert_eq!(bindings.label(KeyAction::Gear(Gear::Drive)), "KeyJ");
    }

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

    #[test]
    fn single_gear_press_is_requested() {
        assert_eq!(
            resolve_gear_selection(&[Gear::Drive], &[Gear::Drive]),
            GearSelection::Requested(Gear::Drive)
        );
    }

    #[test]
    fn no_new_press_means_no_request() {
        assert_eq!(
            resolve_gear_selection(&[], &[Gear::Drive]),
            GearSelection::None
        );
    }

    #[test]
    fn simultaneous_gear_presses_conflict() {
        assert_eq!(
            resolve_gear_selection(&[Gear::Drive, Gear::Reverse], &[]),
            GearSelection::Conflict(vec![Gear::Reverse, Gear::Drive])
        );
    }

    #[test]
    fn new_press_while_other_gear_held_conflicts() {
        assert_eq!(
            resolve_gear_selection(&[Gear::Reverse], &[Gear::Drive, Gear::Reverse]),
            GearSelection::Conflict(vec![Gear::Reverse, Gear::Drive])
        );
    }
}
