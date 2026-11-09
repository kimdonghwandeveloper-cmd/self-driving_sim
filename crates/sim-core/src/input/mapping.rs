//! 키보드 키 이름 매핑의 엔진 무관 부분(Phase GD-2).
//!
//! 설정 파일의 키 이름은 Bevy `KeyCode` 변형 이름(예: `"ArrowUp"`, `"KeyW"`)을 쓴다.
//! 이름을 실제 키 값으로 바꾸는 일은 각 엔진이 [`KeyboardMapping::resolve_with`]에 넘기는 함수가 한다.
//! 두 엔진은 [`SUPPORTED_KEY_NAMES`]를 모두 지원해야 한다.

use std::fmt;

use serde::Deserialize;

use crate::camera::ViewDirection;
use crate::vehicle::Gear;

/// 설정 파일에 쓸 수 있는 키 이름 전체(`config/input.toml` 주석과 같다).
pub const SUPPORTED_KEY_NAMES: [&str; 51] = [
    "KeyA",
    "KeyB",
    "KeyC",
    "KeyD",
    "KeyE",
    "KeyF",
    "KeyG",
    "KeyH",
    "KeyI",
    "KeyJ",
    "KeyK",
    "KeyL",
    "KeyM",
    "KeyN",
    "KeyO",
    "KeyP",
    "KeyQ",
    "KeyR",
    "KeyS",
    "KeyT",
    "KeyU",
    "KeyV",
    "KeyW",
    "KeyX",
    "KeyY",
    "KeyZ",
    "Digit0",
    "Digit1",
    "Digit2",
    "Digit3",
    "Digit4",
    "Digit5",
    "Digit6",
    "Digit7",
    "Digit8",
    "Digit9",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "Space",
    "Enter",
    "Backspace",
    "Escape",
    "Tab",
    "ShiftLeft",
    "ShiftRight",
    "ControlLeft",
    "ControlRight",
    "AltLeft",
    "AltRight",
];

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

    /// 키 이름을 `parse`로 엔진의 키 값으로 바꾸고, 미할당·알 수 없는 키·중복 할당을 검사한다.
    pub fn resolve_with<K: Copy + PartialEq>(
        &self,
        parse: impl Fn(&str) -> Option<K>,
    ) -> Result<KeyBindings<K>, MappingError> {
        let mut bindings = Vec::with_capacity(KeyAction::ALL.len());
        let mut used: Vec<(K, KeyAction)> = Vec::new();
        for action in KeyAction::ALL {
            let names = self.names(action);
            if names.is_empty() {
                return Err(MappingError::Unbound { action });
            }
            let mut keys = Vec::with_capacity(names.len());
            for name in names {
                let key = parse(name).ok_or_else(|| MappingError::UnknownKey {
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
struct Binding<K> {
    action: KeyAction,
    keys: Vec<K>,
    names: Vec<String>,
}

/// 검증을 마친 키 매핑. `K`는 엔진의 키 값(Bevy `KeyCode`, Godot `Key`)이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBindings<K> {
    bindings: Vec<Binding<K>>,
}

impl<K> KeyBindings<K> {
    /// 동작에 연결된 키 목록을 찾는다(검증 후에는 모든 동작이 존재한다).
    fn binding(&self, action: KeyAction) -> &Binding<K> {
        self.bindings
            .iter()
            .find(|binding| binding.action == action)
            .expect("every action is bound after validation")
    }

    /// 동작에 연결된 키 목록을 반환한다.
    pub fn keys(&self, action: KeyAction) -> &[K] {
        &self.binding(action).keys
    }

    /// 동작에 연결된 키 이름을 `/`로 이어 HUD용 문자열로 반환한다.
    pub fn label(&self, action: KeyAction) -> String {
        self.binding(action).names.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 지원 키 이름을 목록 순번으로 바꾸는 시험용 변환.
    fn index_of(name: &str) -> Option<usize> {
        SUPPORTED_KEY_NAMES.iter().position(|known| *known == name)
    }

    /// 기본 매핑이 지원 이름만 쓰고 검증을 통과하는지 확인한다.
    #[test]
    fn default_mapping_uses_supported_names() {
        let bindings = KeyboardMapping::default().resolve_with(index_of).unwrap();
        assert_eq!(bindings.label(KeyAction::Throttle), "KeyW/ArrowUp");
        assert_eq!(
            bindings.keys(KeyAction::DebugCamera),
            [index_of("KeyC").unwrap()]
        );
    }

    /// 지원 이름 목록에 중복이 없는지 확인한다.
    #[test]
    fn supported_names_are_unique() {
        for (index, name) in SUPPORTED_KEY_NAMES.iter().enumerate() {
            assert_eq!(index_of(name), Some(index), "{name}");
        }
    }
}
