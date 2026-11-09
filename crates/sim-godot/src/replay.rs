//! 노드 없는 재생 `SimReplay.run`(Godot 인터페이스 계약 v1 5-2·5-3).
//!
//! Godot 값의 형(정수·실수·문자열·알 수 없는 키)만 여기서 검사해 `sim_core::session::replay::ReplayEntry`로
//! 옮긴다. 범위·이름·상한 검사와 재생은 `SimVehicle`과 같은 `DriveSession`으로 sim-core가 한다.

use std::panic::{AssertUnwindSafe, catch_unwind};

use godot::prelude::*;
use sim_core::session::DriveSession;
use sim_core::session::replay::{self, ReplayEntry, Sample};

use crate::course::{build_course, panic_message};
use crate::vehicle::{gear_name, load_vehicle_config, resolve_config_dir};

/// 실수·정수 값을 `f64`로 꺼낸다. 다른 형이면 항목 번호·키가 들어간 오류.
fn number(index: usize, key: &str, value: &Variant) -> Result<f64, String> {
    match value.get_type() {
        VariantType::FLOAT => Ok(value.to::<f64>()),
        VariantType::INT => Ok(value.to::<i64>() as f64),
        other => Err(format!(
            "entries[{index}].{key}: expected float or int, got {other:?}"
        )),
    }
}

/// Godot 배열 하나를 형 검사해 재생 항목 목록으로 바꾼다.
pub fn entries_from_array(entries: &VarArray) -> Result<Vec<ReplayEntry>, String> {
    let mut parsed = Vec::with_capacity(entries.len());
    for (index, item) in entries.iter_shared().enumerate() {
        let dict = item
            .try_to::<VarDictionary>()
            .map_err(|_| format!("entries[{index}]: expected Dictionary"))?;
        let mut entry = ReplayEntry::default();
        let mut has_ticks = false;
        for (key, value) in dict.iter_shared() {
            let key = key
                .try_to::<GString>()
                .map_err(|_| format!("entries[{index}]: keys must be String"))?
                .to_string();
            match key.as_str() {
                "ticks" => {
                    if value.get_type() != VariantType::INT {
                        return Err(format!("entries[{index}].ticks: expected int"));
                    }
                    entry.ticks = value.to::<i64>();
                    has_ticks = true;
                }
                "command" => {
                    let name = value
                        .try_to::<GString>()
                        .map_err(|_| format!("entries[{index}].command: expected String"))?;
                    entry.command = Some(name.to_string());
                }
                "throttle" => entry.throttle = Some(number(index, "throttle", &value)?),
                "brake" => entry.brake = Some(number(index, "brake", &value)?),
                "steering" => entry.steering = Some(number(index, "steering", &value)?),
                other => return Err(format!("entries[{index}]: unknown key \"{other}\"")),
            }
        }
        if !has_ticks {
            return Err(format!("entries[{index}].ticks: missing"));
        }
        parsed.push(entry);
    }
    Ok(parsed)
}

/// 표본 하나를 계약 5-3 `Sample` 사전으로 바꾼다.
fn sample_dict(sample: &Sample) -> VarDictionary {
    let rejected: PackedStringArray = sample
        .rejected
        .iter()
        .map(|name| GString::from(name.as_str()))
        .collect();
    vdict! {
        "tick" => sample.tick as i64,
        "position" => Vector3::new(sample.x_m, 0.0, sample.z_m),
        "heading_rad" => sample.heading_rad,
        "speed_mps" => sample.speed_mps,
        "gear" => gear_name(sample.gear),
        "rejected" => &rejected,
    }
}

/// 재생 결과 사전을 만든다. 실패면 `samples`는 빈 배열이다.
fn result_dict(result: Result<Vec<Sample>, String>) -> VarDictionary {
    match result {
        Ok(samples) => {
            let samples: VarArray = samples
                .iter()
                .map(|sample| sample_dict(sample).to_variant())
                .collect();
            vdict! { "ok" => true, "error" => "", "samples" => &samples }
        }
        Err(error) => {
            vdict! { "ok" => false, "error" => error.as_str(), "samples" => &VarArray::new() }
        }
    }
}

/// 재생 시험의 기대값을 만드는 정적 함수 모음.
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct SimReplay {}

#[godot_api]
impl SimReplay {
    /// `SimVehicle`과 같은 설정·초기 상태에서 입력 열을 재생해 `{ok, error, samples}`를 돌려준다.
    #[func]
    fn run(
        course: GString,
        entries: VarArray,
        #[opt(default = "")] config_dir: GString,
    ) -> VarDictionary {
        let result = entries_from_array(&entries).and_then(|entries| {
            let course = build_course(&course.to_string())?;
            let mut warnings = Vec::new();
            let config =
                load_vehicle_config(&resolve_config_dir(&config_dir.to_string()), &mut warnings);
            for warning in &warnings {
                godot_warn!("{warning}");
            }
            let session = DriveSession::new(course, &config);
            catch_unwind(AssertUnwindSafe(|| replay::run(session, &entries)))
                .map_err(|payload| format!("내부 오류: {}", panic_message(payload.as_ref())))?
        });
        result_dict(result)
    }
}
