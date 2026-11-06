//! TOML 설정 파일 로드.
//!
//! 파일이 없거나 내용이 잘못되면 경고를 남기고 기본값을 사용한다.
//! 모든 수치는 실차 검증 전 **시뮬레이터 임시값**이다.

use std::{fmt, fs, io, path::Path};

use bevy::prelude::*;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::camera::{CameraSettings, OperatorView};
use crate::input::{GamepadBindings, GamepadMapping, KeyBindings, KeyboardMapping};
use crate::vehicle::dynamics::DynamicsParams;
use crate::vehicle::state::{DEFAULT_STOP_THRESHOLD_MPS, MAX_STOP_THRESHOLD_MPS};

/// 차량 설정 파일 경로(저장소 루트 기준).
pub const VEHICLE_CONFIG_PATH: &str = "config/vehicle.toml";
/// 입력 설정 파일 경로(저장소 루트 기준).
pub const INPUT_CONFIG_PATH: &str = "config/input.toml";
/// 카메라 설정 파일 경로(저장소 루트 기준).
pub const CAMERA_CONFIG_PATH: &str = "config/camera.toml";

/// `config/vehicle.toml` 전체 구조.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VehicleConfig {
    pub state_machine: StateMachineConfig,
    pub dynamics: DynamicsParams,
}

/// `[state_machine]` 구역.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StateMachineConfig {
    pub stop_threshold_mps: f32,
}

impl Default for StateMachineConfig {
    /// 상태 머신 기본 정지 임계값을 사용한다.
    fn default() -> Self {
        Self {
            stop_threshold_mps: DEFAULT_STOP_THRESHOLD_MPS,
        }
    }
}

impl VehicleConfig {
    /// 상태 머신·운동 모델 값의 범위를 검사한다.
    pub fn validate(&self) -> Result<(), String> {
        let threshold = self.state_machine.stop_threshold_mps;
        if !(0.0..=MAX_STOP_THRESHOLD_MPS).contains(&threshold) {
            return Err(format!(
                "state_machine.stop_threshold_mps must be within 0..={MAX_STOP_THRESHOLD_MPS}, got {threshold}"
            ));
        }
        self.dynamics.validate()
    }
}

/// `config/input.toml` 전체 구조.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InputConfig {
    pub keyboard: KeyboardMapping,
    pub gamepad: GamepadMapping,
}

/// `config/camera.toml` 전체 구조.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CameraConfig {
    pub operator_view: OperatorView,
}

/// 설정 파일 처리 실패 사유.
#[derive(Debug)]
pub enum ConfigError {
    Read(io::Error),
    Parse(toml::de::Error),
    Invalid(String),
}

impl fmt::Display for ConfigError {
    /// 실패 사유를 출력한다.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Read(error) => write!(f, "read failed: {error}"),
            ConfigError::Parse(error) => write!(f, "parse failed: {error}"),
            ConfigError::Invalid(reason) => write!(f, "invalid value: {reason}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// TOML 문자열을 설정 구조로 변환한다.
pub fn parse_toml<T: DeserializeOwned>(text: &str) -> Result<T, ConfigError> {
    toml::from_str(text).map_err(ConfigError::Parse)
}

/// 파일을 읽어 `parse`로 변환·검증한다. 파일이 없으면 `Ok(None)`.
fn read_config<T>(
    path: &Path,
    parse: fn(&str) -> Result<T, ConfigError>,
) -> Result<Option<T>, ConfigError> {
    match fs::read_to_string(path) {
        Ok(text) => parse(&text).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ConfigError::Read(error)),
    }
}

/// 차량 설정 문자열을 변환하고 검증한다. 파일 로더도 이 함수를 쓴다.
pub fn parse_vehicle_config(text: &str) -> Result<VehicleConfig, ConfigError> {
    let config: VehicleConfig = parse_toml(text)?;
    config.validate().map_err(ConfigError::Invalid)?;
    Ok(config)
}

/// 입력 설정 문자열을 변환하고 키 매핑을 검증한다. 파일 로더도 이 함수를 쓴다.
pub fn parse_key_bindings(text: &str) -> Result<KeyBindings, ConfigError> {
    let config: InputConfig = parse_toml(text)?;
    config
        .keyboard
        .resolve()
        .map_err(|error| ConfigError::Invalid(error.to_string()))
}

/// 입력 설정 문자열을 변환하고 게임패드 매핑을 검증한다. 파일 로더도 이 함수를 쓴다.
pub fn parse_gamepad_bindings(text: &str) -> Result<GamepadBindings, ConfigError> {
    let config: InputConfig = parse_toml(text)?;
    config.gamepad.resolve().map_err(ConfigError::Invalid)
}

/// 카메라 설정 문자열을 변환하고 검증한다. 파일 로더도 이 함수를 쓴다.
pub fn parse_camera_config(text: &str) -> Result<CameraSettings, ConfigError> {
    let config: CameraConfig = parse_toml(text)?;
    config
        .operator_view
        .validate()
        .map_err(ConfigError::Invalid)?;
    Ok(CameraSettings(config.operator_view))
}

/// 차량 설정 파일을 읽는다. 실패하면 경고 후 기본값을 반환한다.
pub fn load_vehicle_config(path: &Path) -> VehicleConfig {
    finish_load(path, read_config(path, parse_vehicle_config))
}

/// 입력 설정 파일을 읽어 키 매핑을 검증한다. 실패하면 경고 후 기본 매핑을 반환한다.
pub fn load_key_bindings(path: &Path) -> KeyBindings {
    finish_load(path, read_config(path, parse_key_bindings))
}

/// 입력 설정 파일을 읽어 게임패드 매핑을 검증한다. 실패하면 경고 후 기본 매핑을 반환한다.
pub fn load_gamepad_bindings(path: &Path) -> GamepadBindings {
    finish_load(path, read_config(path, parse_gamepad_bindings))
}

/// 카메라 설정 파일을 읽는다. 실패하면 경고 후 기본값을 반환한다.
pub fn load_camera_config(path: &Path) -> CameraSettings {
    finish_load(path, read_config(path, parse_camera_config))
}

/// 로드 결과를 로그로 남기고, 없거나 실패하면 기본값을 반환한다.
fn finish_load<T: Default>(path: &Path, loaded: Result<Option<T>, ConfigError>) -> T {
    match loaded {
        Ok(Some(value)) => {
            info!("config loaded: {}", path.display());
            value
        }
        Ok(None) => {
            warn!("config not found, using defaults: {}", path.display());
            T::default()
        }
        Err(error) => {
            error!("config {} {error}; using defaults", path.display());
            T::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::mapping::KeyAction;
    use crate::vehicle::dynamics::SteerReturn;

    /// 저장소의 설정 파일 경로를 만든다.
    fn repo_file(relative: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
    }

    /// 저장소의 설정 파일이 파싱·검증을 통과하는지 확인한다(값이 기본값과 같을 필요는 없다).
    #[test]
    fn repository_config_files_are_valid() {
        let vehicle = fs::read_to_string(repo_file(VEHICLE_CONFIG_PATH)).unwrap();
        if let Err(error) = parse_vehicle_config(&vehicle) {
            panic!("{VEHICLE_CONFIG_PATH}: {error}");
        }

        let input = fs::read_to_string(repo_file(INPUT_CONFIG_PATH)).unwrap();
        if let Err(error) = parse_key_bindings(&input) {
            panic!("{INPUT_CONFIG_PATH}: {error}");
        }
        if let Err(error) = parse_gamepad_bindings(&input) {
            panic!("{INPUT_CONFIG_PATH}: {error}");
        }

        let camera = fs::read_to_string(repo_file(CAMERA_CONFIG_PATH)).unwrap();
        if let Err(error) = parse_camera_config(&camera) {
            panic!("{CAMERA_CONFIG_PATH}: {error}");
        }
    }

    /// 빈 설정 문자열은 코드에 정의된 기본값과 같은 결과가 되는지 확인한다.
    #[test]
    fn empty_config_uses_defaults() {
        assert_eq!(parse_vehicle_config("").unwrap(), VehicleConfig::default());
        assert_eq!(parse_key_bindings("").unwrap(), KeyBindings::default());
        assert_eq!(
            parse_gamepad_bindings("").unwrap(),
            GamepadBindings::default()
        );
        assert_eq!(parse_camera_config("").unwrap(), CameraSettings::default());
    }

    /// 카메라 설정 일부만 지정하면 나머지는 기본값을 유지하고, 잘못된 값·필드는 거부하는지 확인한다.
    #[test]
    fn camera_section_is_parsed_and_validated() {
        let settings = parse_camera_config("[operator_view]\nvertical_fov_deg = 70.0\n").unwrap();
        assert_eq!(settings.0.vertical_fov_deg, 70.0);
        assert_eq!(settings.0.eye_m, OperatorView::default().eye_m);

        for invalid in [
            "[operator_view]\nvertical_fov_deg = 120.0\n",
            "[operator_view]\nnear_clip_m = 0.0\n",
            "[operator_view]\neye_m = [0.0, nan, -1.0]\n",
            "[operator_view]\neye_m = [0.0, inf, -1.0]\n",
            "[operator_view]\npitch_deg = -90.0\n",
        ] {
            assert!(
                matches!(parse_camera_config(invalid), Err(ConfigError::Invalid(_))),
                "{invalid}"
            );
        }
        for malformed in [
            "[operator_view]\nfov = 70.0\n",
            "[operator_view]\neye_m = [0.0, 1.6]\n",
            "[operator_view\n",
        ] {
            assert!(
                matches!(parse_camera_config(malformed), Err(ConfigError::Parse(_))),
                "{malformed}"
            );
        }
    }

    /// 카메라 설정 파일이 없으면 기본값을 쓰는지 확인한다.
    #[test]
    fn missing_camera_file_uses_defaults() {
        let path = repo_file("config/does-not-exist-camera.toml");
        assert_eq!(load_camera_config(&path), CameraSettings::default());
    }

    /// 허용 범위를 벗어난 정지 임계값을 거부하는지 확인한다.
    #[test]
    fn stop_threshold_out_of_range_is_rejected() {
        let result = parse_vehicle_config("[state_machine]\nstop_threshold_mps = 2.0\n");
        assert!(matches!(result, Err(ConfigError::Invalid(_))));
        let result = parse_vehicle_config("[state_machine]\nstop_threshold_mps = -0.1\n");
        assert!(matches!(result, Err(ConfigError::Invalid(_))));
    }

    /// 저장소의 `config/vehicle.toml`이 오류 없이 읽히고 운동 모델 값이 코드 기본값과 같은지 확인한다.
    /// 최고 속도는 소수 표기(11.111111) 때문에 작은 오차를 허용한다.
    #[test]
    fn shipped_vehicle_config_matches_defaults() {
        let config = parse_vehicle_config(include_str!("../config/vehicle.toml")).unwrap();
        let defaults = DynamicsParams::default();
        assert!((config.dynamics.max_forward_speed_mps * 3.6 - 40.0).abs() < 1e-4);
        assert_eq!(
            DynamicsParams {
                max_forward_speed_mps: defaults.max_forward_speed_mps,
                ..config.dynamics
            },
            defaults
        );
    }

    /// 운동 모델 일부 값만 지정하면 나머지는 기본값을 유지하고, 잘못된 값은 거부하는지 확인한다.
    #[test]
    fn dynamics_section_is_parsed_and_validated() {
        let config = parse_vehicle_config("[dynamics]\nwheelbase_m = 3.2\n").unwrap();
        assert_eq!(config.dynamics.wheelbase_m, 3.2);
        assert_eq!(
            config.dynamics.max_accel_mps2,
            DynamicsParams::default().max_accel_mps2
        );

        let result = parse_vehicle_config("[dynamics]\nmax_brake_decel_mps2 = -1.0\n");
        assert!(matches!(result, Err(ConfigError::Invalid(_))));
    }

    /// 조향 중립 복귀 방식을 `auto`/`hold` 문자열로 읽고, 그 밖의 값은 파싱 오류로 거부하는지 확인한다.
    #[test]
    fn steer_return_mode_is_parsed() {
        assert_eq!(
            VehicleConfig::default().dynamics.steer_return,
            SteerReturn::Auto
        );
        let config = parse_vehicle_config("[dynamics]\nsteer_return = \"hold\"\n").unwrap();
        assert_eq!(config.dynamics.steer_return, SteerReturn::Hold);
        let result = parse_vehicle_config("[dynamics]\nsteer_return = \"center\"\n");
        assert!(matches!(result, Err(ConfigError::Parse(_))));
    }

    /// 오타 등 알 수 없는 필드를 파싱 오류로 거부하는지 확인한다.
    #[test]
    fn unknown_field_is_rejected() {
        let result = parse_vehicle_config("[state_machine]\nstop_treshold_mps = 0.1\n");
        assert!(matches!(result, Err(ConfigError::Parse(_))));
    }

    /// 일부 키만 지정한 입력 설정이 나머지 동작에 기본 키를 유지하는지 확인한다.
    #[test]
    fn partial_input_config_keeps_other_defaults() {
        let bindings = parse_key_bindings("[keyboard]\nemergency_stop = [\"KeyE\"]\n").unwrap();
        assert_eq!(bindings.keys(KeyAction::EmergencyStop), [KeyCode::KeyE]);
        assert_eq!(
            bindings.keys(KeyAction::Throttle),
            [KeyCode::KeyW, KeyCode::ArrowUp]
        );
    }

    /// 게임패드 설정의 배치·버튼을 읽고, 잘못된 값은 키보드 매핑과 별도로 거부하는지 확인한다.
    #[test]
    fn gamepad_section_is_parsed_and_validated() {
        let bindings =
            parse_gamepad_bindings("[gamepad]\nlayout = \"triggers\"\nemergency_stop = \"LB\"\n")
                .unwrap();
        assert_eq!(bindings.drive_labels().2, "left stick");
        assert_eq!(bindings.label(KeyAction::EmergencyStop), "LB");

        let invalid = "[gamepad]\nstick_deadzone = 2.0\n";
        assert!(matches!(
            parse_gamepad_bindings(invalid),
            Err(ConfigError::Invalid(_))
        ));
        assert!(parse_key_bindings(invalid).is_ok());
        let result = parse_gamepad_bindings("[gamepad]\nlayout = \"wheel\"\n");
        assert!(matches!(result, Err(ConfigError::Parse(_))));
    }

    /// 다른 동작과 겹치는 키 매핑을 검증 오류로 거부하는지 확인한다.
    #[test]
    fn invalid_key_mapping_is_rejected() {
        let result = parse_key_bindings("[keyboard]\nsteer_right = [\"KeyJ\"]\n");
        assert!(matches!(result, Err(ConfigError::Invalid(_))));
    }
}
