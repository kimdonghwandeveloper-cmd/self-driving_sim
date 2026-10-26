//! TOML 설정 파일 로드.
//!
//! 파일이 없거나 내용이 잘못되면 경고를 남기고 기본값을 사용한다.
//! 모든 수치는 실차 검증 전 **시뮬레이터 임시값**이다.

use std::{fmt, fs, io, path::Path};

use bevy::prelude::*;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::input::{KeyBindings, KeyboardMapping};
use crate::vehicle::state::{DEFAULT_STOP_THRESHOLD_MPS, MAX_STOP_THRESHOLD_MPS};

/// 차량 설정 파일 경로(저장소 루트 기준).
pub const VEHICLE_CONFIG_PATH: &str = "config/vehicle.toml";
/// 입력 설정 파일 경로(저장소 루트 기준).
pub const INPUT_CONFIG_PATH: &str = "config/input.toml";

/// `config/vehicle.toml` 전체 구조.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VehicleConfig {
    pub state_machine: StateMachineConfig,
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
    /// 값의 범위를 검사한다.
    pub fn validate(&self) -> Result<(), String> {
        let threshold = self.state_machine.stop_threshold_mps;
        if !(0.0..=MAX_STOP_THRESHOLD_MPS).contains(&threshold) {
            return Err(format!(
                "state_machine.stop_threshold_mps must be within 0..={MAX_STOP_THRESHOLD_MPS}, got {threshold}"
            ));
        }
        Ok(())
    }
}

/// `config/input.toml` 전체 구조.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InputConfig {
    pub keyboard: KeyboardMapping,
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

/// 파일을 읽어 설정 구조로 변환한다. 파일이 없으면 `Ok(None)`.
fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, ConfigError> {
    match fs::read_to_string(path) {
        Ok(text) => parse_toml(&text).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ConfigError::Read(error)),
    }
}

/// 차량 설정 문자열을 변환하고 검증한다.
pub fn parse_vehicle_config(text: &str) -> Result<VehicleConfig, ConfigError> {
    let config: VehicleConfig = parse_toml(text)?;
    config.validate().map_err(ConfigError::Invalid)?;
    Ok(config)
}

/// 입력 설정 문자열을 변환하고 키 매핑을 검증한다.
pub fn parse_key_bindings(text: &str) -> Result<KeyBindings, ConfigError> {
    let config: InputConfig = parse_toml(text)?;
    config
        .keyboard
        .resolve()
        .map_err(|error| ConfigError::Invalid(error.to_string()))
}

/// 차량 설정 파일을 읽는다. 실패하면 경고 후 기본값을 반환한다.
pub fn load_vehicle_config(path: &Path) -> VehicleConfig {
    let loaded = read_toml::<VehicleConfig>(path).and_then(|config| {
        if let Some(config) = &config {
            config.validate().map_err(ConfigError::Invalid)?;
        }
        Ok(config)
    });
    finish_load(path, loaded)
}

/// 입력 설정 파일을 읽어 키 매핑을 검증한다. 실패하면 경고 후 기본 매핑을 반환한다.
pub fn load_key_bindings(path: &Path) -> KeyBindings {
    let loaded = read_toml::<InputConfig>(path).and_then(|config| {
        config
            .map(|config| config.keyboard.resolve())
            .transpose()
            .map_err(|error| ConfigError::Invalid(error.to_string()))
    });
    finish_load(path, loaded)
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
    }

    /// 빈 설정 문자열은 코드에 정의된 기본값과 같은 결과가 되는지 확인한다.
    #[test]
    fn empty_config_uses_defaults() {
        assert_eq!(parse_vehicle_config("").unwrap(), VehicleConfig::default());
        assert_eq!(parse_key_bindings("").unwrap(), KeyBindings::default());
    }

    /// 허용 범위를 벗어난 정지 임계값을 거부하는지 확인한다.
    #[test]
    fn stop_threshold_out_of_range_is_rejected() {
        let result = parse_vehicle_config("[state_machine]\nstop_threshold_mps = 2.0\n");
        assert!(matches!(result, Err(ConfigError::Invalid(_))));
        let result = parse_vehicle_config("[state_machine]\nstop_threshold_mps = -0.1\n");
        assert!(matches!(result, Err(ConfigError::Invalid(_))));
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

    /// 다른 동작과 겹치는 키 매핑을 검증 오류로 거부하는지 확인한다.
    #[test]
    fn invalid_key_mapping_is_rejected() {
        let result = parse_key_bindings("[keyboard]\nsteer_right = [\"KeyJ\"]\n");
        assert!(matches!(result, Err(ConfigError::Invalid(_))));
    }
}
