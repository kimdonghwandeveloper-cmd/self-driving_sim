//! 키보드 키 이름을 Bevy `KeyCode`로 바꾸는 매핑.
//!
//! 동작 목록·설정 구조·검증은 Godot 판과 함께 쓰도록 `sim_core::input::mapping`에 있고,
//! 기어 요청 판정은 `sim_core::input::gear`에 있다.

use bevy::prelude::*;

pub use sim_core::input::gear::{GearSelection, gears, resolve_gear_selection};
pub use sim_core::input::mapping::{KeyAction, KeyboardMapping, MappingError};

/// 검증을 마친 Bevy 키 매핑 리소스. 입력 시스템과 HUD가 사용한다.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct KeyBindings(sim_core::input::mapping::KeyBindings<KeyCode>);

impl Default for KeyBindings {
    /// 기본 키 매핑을 검증한 결과를 만든다.
    fn default() -> Self {
        KeyboardMapping::default()
            .resolve()
            .expect("default keyboard mapping must be valid")
    }
}

impl KeyBindings {
    /// 동작에 연결된 `KeyCode` 목록을 반환한다.
    pub fn keys(&self, action: KeyAction) -> &[KeyCode] {
        self.0.keys(action)
    }

    /// 동작에 연결된 키 이름을 `/`로 이어 HUD용 문자열로 반환한다.
    pub fn label(&self, action: KeyAction) -> String {
        self.0.label(action)
    }
}

/// 키보드 매핑 설정을 Bevy 키 매핑으로 검증·변환하는 확장.
pub trait ResolveKeyboard {
    /// 키 이름을 `KeyCode`로 변환하고, 미할당·알 수 없는 키·중복 할당을 검사한다.
    fn resolve(&self) -> Result<KeyBindings, MappingError>;
}

impl ResolveKeyboard for KeyboardMapping {
    /// [`parse_key_name`]으로 `sim_core` 공통 검증을 거친다.
    fn resolve(&self) -> Result<KeyBindings, MappingError> {
        self.resolve_with(parse_key_name).map(KeyBindings)
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
    use crate::camera::ViewDirection;
    use crate::vehicle::Gear;
    use sim_core::input::mapping::SUPPORTED_KEY_NAMES;

    /// 공통 지원 키 이름을 Bevy가 모두 알아보는지 확인한다.
    #[test]
    fn every_supported_key_name_parses() {
        for name in SUPPORTED_KEY_NAMES {
            assert!(parse_key_name(name).is_some(), "{name}");
        }
    }

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
