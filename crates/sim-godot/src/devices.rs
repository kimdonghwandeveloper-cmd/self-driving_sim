//! 키보드·게임패드 읽기(Godot 인터페이스 계약 v1 5-1 2단계).
//!
//! Godot `Input` 싱글턴을 직접 읽어 장치마다 `DeviceFrame`을 만들고, 합치기·명령 변환은
//! Bevy 판과 같은 `sim_core::input` 규칙을 쓴다. 키 이름은 `config/input.toml`의 Bevy `KeyCode` 이름이다.

use std::path::Path;

use godot::classes::Input;
use godot::global::{JoyAxis, JoyButton, Key};
use godot::obj::{EngineEnum, Gd, Singleton};
use sim_core::camera::{ViewDirection, resolve_view_selection};
use sim_core::config::{INPUT_CONFIG_PATH, InputConfig, parse_input_config, read_config};
use sim_core::input::gamepad::{GamepadBindings, GamepadMapping, PadAxes, gamepad_vehicle_input};
use sim_core::input::mapping::{KeyAction, KeyBindings, KeyboardMapping};
use sim_core::input::{
    DeviceFrame, InputSource, command_requests, gears, keyboard_vehicle_input, merge_key_frames,
    merge_vehicle_input, needs_safe_stop, select_source,
};
use sim_core::session::DeviceTick;

/// Bevy `KeyCode` 이름을 Godot 물리 키로 바꾼다. 왼쪽·오른쪽 Shift·Ctrl·Alt는 Godot `Key`에서 구분되지 않아
/// 같은 키가 된다(두 쪽을 다른 동작에 쓰면 중복 할당으로 거부된다).
pub fn parse_key_name(name: &str) -> Option<Key> {
    if let Some(letter) = name.strip_prefix("Key")
        && letter.len() == 1
        && letter.as_bytes()[0].is_ascii_uppercase()
    {
        return Some(Key::from_ord(i32::from(letter.as_bytes()[0])));
    }
    if let Some(digit) = name.strip_prefix("Digit")
        && digit.len() == 1
        && digit.as_bytes()[0].is_ascii_digit()
    {
        return Some(Key::from_ord(i32::from(digit.as_bytes()[0])));
    }
    let key = match name {
        "ArrowUp" => Key::UP,
        "ArrowDown" => Key::DOWN,
        "ArrowLeft" => Key::LEFT,
        "ArrowRight" => Key::RIGHT,
        "Space" => Key::SPACE,
        "Enter" => Key::ENTER,
        "Backspace" => Key::BACKSPACE,
        "Escape" => Key::ESCAPE,
        "Tab" => Key::TAB,
        "ShiftLeft" | "ShiftRight" => Key::SHIFT,
        "ControlLeft" | "ControlRight" => Key::CTRL,
        "AltLeft" | "AltRight" => Key::ALT,
        _ => return None,
    };
    Some(key)
}

/// Xbox 버튼 이름을 Godot `JoyButton`으로 바꾼다. LT/RT는 축이라 버튼 동작에 쓸 수 없다.
pub fn parse_button_name(name: &str) -> Option<JoyButton> {
    let button = match name {
        "A" => JoyButton::A,
        "B" => JoyButton::B,
        "X" => JoyButton::X,
        "Y" => JoyButton::Y,
        "LB" => JoyButton::LEFT_SHOULDER,
        "RB" => JoyButton::RIGHT_SHOULDER,
        "Back" => JoyButton::BACK,
        "Start" => JoyButton::START,
        "LS" => JoyButton::LEFT_STICK,
        "RS" => JoyButton::RIGHT_STICK,
        "DPadUp" => JoyButton::DPAD_UP,
        "DPadDown" => JoyButton::DPAD_DOWN,
        "DPadLeft" => JoyButton::DPAD_LEFT,
        "DPadRight" => JoyButton::DPAD_RIGHT,
        _ => return None,
    };
    Some(button)
}

/// 입력 설정 파일을 읽어 장치별로 검증한다. 잘못된 장치 매핑은 그 장치만 기본값으로 바꾸고 경고를 더한다.
pub fn load_bindings(
    config_dir: Option<&Path>,
    warnings: &mut Vec<String>,
) -> (KeyBindings<Key>, GamepadBindings<JoyButton>) {
    let config = match config_dir {
        None => InputConfig::default(),
        Some(dir) => {
            let path = dir.join(
                Path::new(INPUT_CONFIG_PATH)
                    .file_name()
                    .expect("config path has a file name"),
            );
            match read_config(&path, parse_input_config) {
                Ok(Some(config)) => config,
                Ok(None) => {
                    warnings.push(format!(
                        "설정 파일이 없어 기본값을 씁니다: {}",
                        path.display()
                    ));
                    InputConfig::default()
                }
                Err(error) => {
                    warnings.push(format!(
                        "설정 파일 {} {error}; 기본값을 씁니다",
                        path.display()
                    ));
                    InputConfig::default()
                }
            }
        }
    };
    let keys = config
        .keyboard
        .resolve_with(parse_key_name)
        .unwrap_or_else(|error| {
            warnings.push(format!("키보드 매핑 오류({error}); 기본 키 매핑을 씁니다"));
            KeyboardMapping::default()
                .resolve_with(parse_key_name)
                .expect("default keyboard mapping must be valid")
        });
    let pad = config
        .gamepad
        .resolve_with(parse_button_name)
        .unwrap_or_else(|error| {
            warnings.push(format!(
                "게임패드 매핑 오류({error}); 기본 게임패드 매핑을 씁니다"
            ));
            GamepadMapping::default()
                .resolve_with(parse_button_name)
                .expect("default gamepad mapping must be valid")
        });
    (keys, pad)
}

/// 조작에 쓰는 게임패드.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pad {
    device: i32,
    /// 선택된 뒤 입력이 중립이 된 적이 있어 조작에 쓸 수 있음.
    ready: bool,
}

/// 한 틱의 장치 읽기 결과.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceRead {
    /// 세션에 넘길 주행 입력·명령.
    pub tick: DeviceTick,
    /// 이번 틱에 새로 눌린 시점 키와 눌려 있는 시점 키.
    pub views_new: Vec<ViewDirection>,
    pub views_held: Vec<ViewDirection>,
    /// 디버그 카메라 키가 새로 눌림.
    pub debug_toggle: bool,
}

impl DeviceRead {
    /// 시점 키로 다음 시점을 정한다(Bevy `select_view`와 같은 규칙).
    pub fn next_view(&self, current: ViewDirection) -> ViewDirection {
        resolve_view_selection(current, &self.views_new, &self.views_held)
    }
}

/// 키보드·게임패드 상태와 직전 틱 눌림 이력.
#[derive(Debug, Clone)]
pub struct Devices {
    keys: KeyBindings<Key>,
    pad_bindings: GamepadBindings<JoyButton>,
    /// `KeyAction::ALL` 순서의 직전 틱 키보드 눌림.
    previous_keys: [bool; KeyAction::ALL.len()],
    /// `KeyAction::ALL` 순서의 직전 틱 게임패드 버튼 눌림.
    previous_buttons: [bool; KeyAction::ALL.len()],
    pad: Option<Pad>,
    /// 키보드 입력이 중립이 된 적이 있어 조작에 쓸 수 있음(`read_input` 전환·포커스 복귀 뒤 거짓).
    keyboard_ready: bool,
    source: InputSource,
    /// 다음 틱에 눌림 이력을 현재 상태로 맞춰, 이미 눌려 있던 키를 새 눌림으로 보지 않는다.
    resync: bool,
    focused: bool,
}

impl Devices {
    /// 검증된 매핑으로 장치 상태를 만든다. 첫 틱에는 이미 눌린 키를 새 눌림으로 보지 않는다.
    pub fn new(keys: KeyBindings<Key>, pad_bindings: GamepadBindings<JoyButton>) -> Self {
        Self {
            keys,
            pad_bindings,
            previous_keys: [false; KeyAction::ALL.len()],
            previous_buttons: [false; KeyAction::ALL.len()],
            pad: None,
            keyboard_ready: true,
            source: InputSource::Keyboard,
            resync: true,
            focused: true,
        }
    }

    /// 마지막으로 고른 주행 장치(`"keyboard"`, `"gamepad"`)를 반환한다.
    pub fn source_name(&self) -> &'static str {
        match self.source {
            InputSource::Keyboard => "keyboard",
            InputSource::Gamepad => "gamepad",
        }
    }

    /// 눌림 이력을 비우고 키보드·게임패드를 다시 중립 대기로 둔다(`read_input` 전환·포커스 복귀).
    pub fn reset_history(&mut self) {
        self.resync = true;
        self.keyboard_ready = false;
        if let Some(pad) = self.pad.as_mut() {
            pad.ready = false;
        }
    }

    /// 창 포커스 변화를 기록한다. 잃으면 다시 얻을 때까지 장치 입력을 0으로 본다(계약 신규 규칙).
    pub fn set_focused(&mut self, focused: bool) {
        if focused && !self.focused {
            self.reset_history();
        }
        self.focused = focused;
    }

    /// 이번 틱의 장치 입력을 읽어 합친다. `stopped`는 틱 시작 시 차량 정지 여부(게임패드 분리 판정용).
    pub fn read(&mut self, stopped: bool) -> DeviceRead {
        let input = Input::singleton();
        let key_now: [bool; KeyAction::ALL.len()] = KeyAction::ALL.map(|action| {
            self.keys
                .keys(action)
                .iter()
                .any(|key| input.is_physical_key_pressed(*key))
        });
        let (pad_now, axes, gamepad_lost) = self.read_pad(&input);
        if self.resync || !self.focused {
            self.previous_keys = key_now;
            self.previous_buttons = pad_now;
            self.resync = false;
        }
        if !self.focused {
            return DeviceRead {
                tick: DeviceTick {
                    input: Some(Default::default()),
                    ..DeviceTick::default()
                },
                ..DeviceRead::default()
            };
        }

        let mut keyboard = frame(&key_now, &self.previous_keys, || {
            let held = |action| key_now[index(action)];
            keyboard_vehicle_input(
                held(KeyAction::Throttle),
                held(KeyAction::Brake),
                held(KeyAction::SteerLeft),
                held(KeyAction::SteerRight),
            )
        });
        let keyboard_neutral = keyboard == DeviceFrame::default();
        self.keyboard_ready |= keyboard_neutral;
        if !self.keyboard_ready {
            keyboard = keyboard.emergency_only();
        }
        let mut gamepad = frame(&pad_now, &self.previous_buttons, || {
            gamepad_vehicle_input(axes, &self.pad_bindings)
        });
        gamepad.reset_pressed = false;
        let emergency_only = gamepad.emergency_only();
        let neutral = gamepad == emergency_only;
        let pad_ready = self.pad.as_mut().is_some_and(|pad| {
            pad.ready |= neutral;
            pad.ready
        });
        if !pad_ready {
            gamepad = emergency_only;
        }

        let safe_stop = needs_safe_stop(gamepad_lost, self.source, stopped);
        if gamepad_lost {
            self.source = InputSource::Keyboard;
        }
        self.source = select_source(self.source, keyboard.input, gamepad.input);
        let vehicle_input = merge_vehicle_input(self.source, keyboard.input, gamepad.input);
        let mut merged = merge_key_frames(&keyboard, &gamepad);
        merged.emergency_stop_pressed |= safe_stop;
        let reset = keyboard.reset_pressed && !merged.emergency_stop_held;

        let views_new =
            view_list(|action| key_now[index(action)] && !self.previous_keys[index(action)]);
        let views_held = view_list(|action| key_now[index(action)]);
        let debug = index(KeyAction::DebugCamera);
        let debug_toggle = key_now[debug] && !self.previous_keys[debug];

        self.previous_keys = key_now;
        self.previous_buttons = pad_now;
        DeviceRead {
            tick: DeviceTick {
                input: Some(vehicle_input),
                requests: command_requests(merged),
                reset,
            },
            views_new,
            views_held,
            debug_toggle,
        }
    }

    /// 조작할 게임패드를 고르고 버튼·축을 읽는다. 쓰던 게임패드가 사라졌으면 `gamepad_lost`가 참이다.
    fn read_pad(&mut self, input: &Gd<Input>) -> ([bool; KeyAction::ALL.len()], PadAxes, bool) {
        let connected: Vec<i32> = input
            .get_connected_joypads()
            .iter_shared()
            .filter_map(|device| i32::try_from(device).ok())
            .collect();
        let previous = self.pad.map(|pad| pad.device);
        let kept = previous.filter(|device| connected.contains(device));
        let selected = kept.or_else(|| connected.iter().copied().min());
        let lost = previous.is_some() && kept.is_none();
        if selected != previous {
            self.pad = selected.map(|device| Pad {
                device,
                ready: false,
            });
        }
        let Some(device) = selected else {
            return ([false; KeyAction::ALL.len()], PadAxes::default(), lost);
        };
        let buttons = KeyAction::ALL.map(|action| {
            is_button_action(action)
                && input.is_joy_button_pressed(device, self.pad_bindings.button(action))
        });
        let axis = |axis| input.get_joy_axis(device, axis);
        let axes = PadAxes {
            left_x: axis(JoyAxis::LEFT_X),
            // Godot은 아래가 +, Bevy·sim-core는 위가 +다.
            left_y: -axis(JoyAxis::LEFT_Y),
            right_x: axis(JoyAxis::RIGHT_X),
            left_trigger: axis(JoyAxis::TRIGGER_LEFT),
            right_trigger: axis(JoyAxis::TRIGGER_RIGHT),
        };
        (buttons, axes, lost)
    }
}

/// 동작의 `KeyAction::ALL` 순번을 반환한다.
fn index(action: KeyAction) -> usize {
    KeyAction::ALL
        .iter()
        .position(|known| *known == action)
        .expect("every action is listed in ALL")
}

/// 게임패드 버튼에 매핑되는 동작(기어·시동·비상 정지·해제)인지 확인한다.
fn is_button_action(action: KeyAction) -> bool {
    matches!(
        action,
        KeyAction::Gear(_)
            | KeyAction::EngineToggle
            | KeyAction::EmergencyStop
            | KeyAction::EmergencyRelease
    )
}

/// 눌림 배열과 직전 틱 배열로 장치 프레임을 만든다. 연속 입력은 `input`이 계산한다.
fn frame(
    now: &[bool; KeyAction::ALL.len()],
    previous: &[bool; KeyAction::ALL.len()],
    input: impl FnOnce() -> sim_core::vehicle::VehicleInput,
) -> DeviceFrame {
    let held = |action| now[index(action)];
    let new = |action| now[index(action)] && !previous[index(action)];
    DeviceFrame {
        input: input(),
        emergency_stop_pressed: new(KeyAction::EmergencyStop),
        emergency_stop_held: held(KeyAction::EmergencyStop),
        emergency_release_pressed: new(KeyAction::EmergencyRelease),
        engine_toggle_pressed: new(KeyAction::EngineToggle),
        gears_just_pressed: gears()
            .into_iter()
            .filter(|gear| new(KeyAction::Gear(*gear)))
            .collect(),
        gears_held: gears()
            .into_iter()
            .filter(|gear| held(KeyAction::Gear(*gear)))
            .collect(),
        reset_pressed: new(KeyAction::ResetCourse),
    }
}

/// 조건을 만족하는 시점 방향 목록을 `ViewDirection::ALL` 순서로 만든다.
fn view_list(active: impl Fn(KeyAction) -> bool) -> Vec<ViewDirection> {
    ViewDirection::ALL
        .into_iter()
        .filter(|direction| active(KeyAction::View(*direction)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::input::gamepad::SUPPORTED_BUTTON_NAMES;
    use sim_core::input::mapping::SUPPORTED_KEY_NAMES;

    /// 공통 지원 키·버튼 이름을 Godot 쪽이 모두 알아보는지 확인한다.
    #[test]
    fn every_supported_name_parses() {
        for name in SUPPORTED_KEY_NAMES {
            assert!(parse_key_name(name).is_some(), "{name}");
        }
        for name in SUPPORTED_BUTTON_NAMES {
            assert!(parse_button_name(name).is_some(), "{name}");
        }
        assert_eq!(parse_key_name("KeyW"), Some(Key::W));
        assert_eq!(parse_key_name("Digit1"), Some(Key::KEY_1));
        assert_eq!(parse_key_name("Keyw"), None);
    }

    /// 기본 매핑이 Godot 키·버튼으로 검증을 통과하는지 확인한다.
    #[test]
    fn default_bindings_resolve() {
        let mut warnings = Vec::new();
        let (keys, pad) = load_bindings(None, &mut warnings);
        assert!(warnings.is_empty());
        assert_eq!(keys.keys(KeyAction::Throttle), [Key::W, Key::UP]);
        assert_eq!(
            pad.button(KeyAction::EmergencyStop),
            JoyButton::RIGHT_SHOULDER
        );
    }
}
