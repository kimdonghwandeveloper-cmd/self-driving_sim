//! 엔진과 무관한 TOML 설정 구조와 파싱(Phase GD-2).
//!
//! 파일이 없거나 잘못됐을 때 기본값으로 바꾸고 알리는 일은 각 앱(Bevy 로그, Godot 경고 목록)이 한다.
//! 모든 수치는 실차 검증 전 **시뮬레이터 임시값**이다.

use std::{fmt, fs, io, path::Path};

use serde::Deserialize;
use serde::de::DeserializeOwned;

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
pub fn read_config<T>(
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 빈 차량 설정은 기본값이 되고, 범위 밖 정지 임계값은 거부하는지 확인한다.
    #[test]
    fn vehicle_config_defaults_and_range() {
        assert_eq!(parse_vehicle_config("").unwrap(), VehicleConfig::default());
        assert!(matches!(
            parse_vehicle_config("[state_machine]\nstop_threshold_mps = 9.0\n"),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            parse_vehicle_config("[unknown]\n"),
            Err(ConfigError::Parse(_))
        ));
    }

    /// 저장소의 차량 설정 파일이 파싱·검증을 통과하는지 확인한다.
    #[test]
    fn repository_vehicle_config_is_valid() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(VEHICLE_CONFIG_PATH);
        assert!(read_config(&path, parse_vehicle_config).unwrap().is_some());
    }
}
