//! 재생 입력 열 검사와 노드 없는 재생(Godot 인터페이스 계약 v1 5-2·5-3).
//!
//! Godot `SimReplay.run`은 값의 형(정수·실수·문자열)을 검사해 [`ReplayEntry`]로 옮긴 뒤 이 모듈을 부른다.
//! 범위·이름·상한 검사와 재생 순서는 여기서 정한다.

use super::{CommandSource, DeviceTick, DriveCommand, DriveSession};
use crate::vehicle::state::{Gear, VehicleInput};

/// 항목 하나의 최대 틱 수(64 Hz로 10분).
pub const MAX_ENTRY_TICKS: i64 = 38_400;
/// 열 전체의 최대 틱 수(64 Hz로 1시간).
pub const MAX_TOTAL_TICKS: i64 = 230_400;

/// 형 검사를 마친 재생 항목 하나. 범위는 아직 검사하지 않은 값이다.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReplayEntry {
    pub ticks: i64,
    pub command: Option<String>,
    pub throttle: Option<f64>,
    pub brake: Option<f64>,
    pub steering: Option<f64>,
}

/// 항목이 끝날 때마다 남기는 표본(계약 5-3 `Sample`).
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// 누적 틱 번호(1부터).
    pub tick: u64,
    pub x_m: f32,
    pub z_m: f32,
    pub heading_rad: f32,
    pub speed_mps: f32,
    pub gear: Gear,
    /// 그 항목에서 상태 머신이 거부한 `send_command` 명령 이름.
    pub rejected: Vec<String>,
}

/// 검사를 통과한 항목.
#[derive(Debug, Clone, PartialEq)]
struct CheckedEntry {
    ticks: u32,
    command: Option<DriveCommand>,
    throttle: Option<f32>,
    brake: Option<f32>,
    steering: Option<f32>,
}

/// 연속 입력 값 하나를 검사한다. 유한하고 `min..=max` 안이어야 한다.
fn check_axis(
    index: usize,
    key: &str,
    value: Option<f64>,
    min: f64,
    max: f64,
) -> Result<Option<f32>, String> {
    match value {
        None => Ok(None),
        Some(value) if value.is_finite() && (min..=max).contains(&value) => Ok(Some(value as f32)),
        Some(value) => Err(format!(
            "entries[{index}].{key}: {value} is outside {min}..={max}"
        )),
    }
}

/// 열 전체를 첫 틱 전에 검사한다. 하나라도 어기면 항목 번호·키·사유를 담은 오류를 돌려준다.
fn check_entries(entries: &[ReplayEntry]) -> Result<Vec<CheckedEntry>, String> {
    if entries.is_empty() {
        return Err("entries: empty".to_owned());
    }
    let mut total = 0_i64;
    let mut checked = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        if !(1..=MAX_ENTRY_TICKS).contains(&entry.ticks) {
            return Err(format!(
                "entries[{index}].ticks: {} is outside 1..={MAX_ENTRY_TICKS}",
                entry.ticks
            ));
        }
        total += entry.ticks;
        if total > MAX_TOTAL_TICKS {
            return Err(format!(
                "entries[{index}].ticks: total {total} exceeds {MAX_TOTAL_TICKS}"
            ));
        }
        let command =
            match &entry.command {
                None => None,
                Some(name) => Some(DriveCommand::parse(name).ok_or_else(|| {
                    format!("entries[{index}].command: unknown command \"{name}\"")
                })?),
            };
        checked.push(CheckedEntry {
            ticks: entry.ticks as u32,
            command,
            throttle: check_axis(index, "throttle", entry.throttle, 0.0, 1.0)?,
            brake: check_axis(index, "brake", entry.brake, 0.0, 1.0)?,
            steering: check_axis(index, "steering", entry.steering, -1.0, 1.0)?,
        });
    }
    Ok(checked)
}

/// 세션에서 열을 재생해 항목마다 표본 하나를 돌려준다.
///
/// 항목마다 (a) 명령을 대기열에 넣어 첫 틱 3단계에서 처리하고 (b) 있는 연속 입력 키만 바꾼 뒤
/// (c) `ticks`번 틱을 돈다. 상태 머신의 정상 거부는 실패가 아니며 표본의 `rejected`에 남는다.
pub fn run(mut session: DriveSession, entries: &[ReplayEntry]) -> Result<Vec<Sample>, String> {
    let checked = check_entries(entries)?;
    let mut input = VehicleInput::default();
    let mut samples = Vec::with_capacity(checked.len());
    for entry in checked {
        if let Some(command) = entry.command {
            session
                .enqueue(command)
                .map_err(|_| "replay command queue is full".to_owned())?;
        }
        input.throttle = entry.throttle.unwrap_or(input.throttle);
        input.brake = entry.brake.unwrap_or(input.brake);
        input.steering = entry.steering.unwrap_or(input.steering);
        session.set_manual_input(input);
        let mut rejected = Vec::new();
        for _ in 0..entry.ticks {
            let report = session.tick(&DeviceTick::default());
            rejected.extend(
                report
                    .results
                    .into_iter()
                    .filter(|result| result.source == CommandSource::Api && !result.accepted)
                    .map(|result| result.command),
            );
        }
        let motion = session.motion();
        samples.push(Sample {
            tick: session.tick_count(),
            x_m: motion.x_m,
            z_m: motion.z_m,
            heading_rad: motion.heading_rad,
            speed_mps: session.state().speed_mps(),
            gear: session.state().gear(),
            rejected,
        });
    }
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VehicleConfig;
    use crate::world::CourseId;

    /// 기본 설정의 training 코스 세션을 만든다.
    fn session() -> DriveSession {
        DriveSession::new(CourseId::Training.build(), &VehicleConfig::default())
    }

    /// 명령 항목을 만든다.
    fn command(ticks: i64, name: &str) -> ReplayEntry {
        ReplayEntry {
            ticks,
            command: Some(name.to_owned()),
            ..ReplayEntry::default()
        }
    }

    /// 계약 5-2 예시 열: 시동 → D → 가속 → 제동. 표본 수·틱 번호·전진·거부 기록을 확인한다.
    #[test]
    fn contract_example_drives_forward_then_brakes() {
        let entries = vec![
            command(64, "engine_toggle"),
            command(1, "gear_drive"),
            ReplayEntry {
                ticks: 320,
                throttle: Some(0.5),
                brake: Some(0.0),
                steering: Some(0.0),
                ..ReplayEntry::default()
            },
            ReplayEntry {
                ticks: 64,
                brake: Some(1.0),
                ..ReplayEntry::default()
            },
        ];
        let samples = run(session(), &entries).unwrap();
        let ticks: Vec<u64> = samples.iter().map(|sample| sample.tick).collect();
        assert_eq!(ticks, [64, 65, 385, 449]);
        assert_eq!(samples[1].gear, Gear::Drive);
        assert!(samples[2].speed_mps > 1.0);
        assert!(samples[2].z_m < -1.0, "heading 0 drives toward -Z");
        assert!(samples[3].speed_mps < samples[2].speed_mps);
        assert!(samples.iter().all(|sample| sample.rejected.is_empty()));
    }

    /// 같은 열을 두 번 재생하면 표본이 완전히 같은지(결정적인지) 확인한다.
    #[test]
    fn replay_is_deterministic() {
        let entries = vec![
            command(2, "engine_toggle"),
            command(1, "gear_drive"),
            ReplayEntry {
                ticks: 200,
                throttle: Some(1.0),
                steering: Some(0.3),
                ..ReplayEntry::default()
            },
        ];
        assert_eq!(
            run(session(), &entries).unwrap(),
            run(session(), &entries).unwrap()
        );
    }

    /// 상태 머신 거부는 실패가 아니라 표본의 `rejected`에 남는지 확인한다.
    #[test]
    fn state_machine_rejection_is_recorded() {
        let samples = run(session(), &[command(1, "gear_drive")]).unwrap();
        assert_eq!(samples[0].rejected, ["gear_drive"]);
    }

    /// 빈 열, 틱 범위·합계, 모르는 명령, 범위 밖 입력을 항목 번호·키와 함께 거부하는지 확인한다.
    #[test]
    fn invalid_entries_are_rejected_before_running() {
        assert_eq!(run(session(), &[]).unwrap_err(), "entries: empty");
        assert_eq!(
            run(session(), &[command(0, "engine_toggle")]).unwrap_err(),
            "entries[0].ticks: 0 is outside 1..=38400"
        );
        let long = vec![command(MAX_ENTRY_TICKS, "engine_toggle"); 7];
        assert_eq!(
            run(session(), &long).unwrap_err(),
            "entries[6].ticks: total 268800 exceeds 230400"
        );
        assert_eq!(
            run(session(), &[command(1, "gear_sport")]).unwrap_err(),
            "entries[0].command: unknown command \"gear_sport\""
        );
        let entry = ReplayEntry {
            ticks: 1,
            throttle: Some(1.5),
            ..ReplayEntry::default()
        };
        assert_eq!(
            run(session(), &[command(1, "engine_toggle"), entry]).unwrap_err(),
            "entries[1].throttle: 1.5 is outside 0..=1"
        );
        let entry = ReplayEntry {
            ticks: 1,
            steering: Some(f64::NAN),
            ..ReplayEntry::default()
        };
        assert!(
            run(session(), &[entry])
                .unwrap_err()
                .starts_with("entries[0].steering")
        );
    }

    /// `godot/tests/drive_replay_test.gd`와 같은 입력 열.
    fn godot_test_entries() -> Vec<ReplayEntry> {
        let input = |ticks, command: Option<&str>, throttle, brake, steering| ReplayEntry {
            ticks,
            command: command.map(str::to_owned),
            throttle,
            brake,
            steering,
        };
        vec![
            command(64, "engine_toggle"),
            command(1, "gear_drive"),
            input(320, None, Some(0.5), Some(0.0), Some(0.2)),
            input(64, Some("gear_reverse"), Some(0.3), None, Some(-0.4)),
            input(192, None, Some(0.0), Some(1.0), Some(0.0)),
            command(1, "gear_park"),
            input(64, Some("reset_course"), None, Some(0.0), None),
        ]
    }

    /// Godot 재생 시험과 같은 열의 기대 표본을 고정해 운동 모델·상태 머신 회귀를 잡는다(계약 5-3).
    /// 값은 2026-11-10 현재 기본 설정으로 재생한 결과다. 동역학을 일부러 바꿨다면 다시 뽑아 고친다.
    #[test]
    fn godot_test_entries_match_pinned_samples() {
        // (틱, x, z, heading, 속도, 기어, 거부 명령)
        type Pinned = (u64, f32, f32, f32, f32, Gear, &'static [&'static str]);
        let pinned: [Pinned; 7] = [
            (64, 0.0, 0.0, 0.0, 0.0, Gear::Park, &[]),
            (65, 0.0, 0.0, 0.0, 0.0, Gear::Drive, &[]),
            (
                385,
                0.368_786,
                -4.411_375,
                -0.166_052_1,
                1.763_104_1,
                Gear::Drive,
                &[],
            ),
            (
                449,
                0.591_883_5,
                -6.236_104,
                -0.055_700_064,
                1.913_594_1,
                Gear::Drive,
                &["gear_reverse"],
            ),
            (
                641,
                0.616_744_8,
                -6.775_709_6,
                -0.044_084_787,
                0.0,
                Gear::Drive,
                &[],
            ),
            (
                642,
                0.616_744_8,
                -6.775_709_6,
                -0.044_084_787,
                0.0,
                Gear::Park,
                &[],
            ),
            (706, 0.0, 0.0, 0.0, 0.0, Gear::Park, &[]),
        ];
        let samples = run(session(), &godot_test_entries()).unwrap();
        assert_eq!(samples.len(), pinned.len());
        for (sample, (tick, x, z, heading, speed, gear, rejected)) in samples.iter().zip(pinned) {
            assert_eq!(sample.tick, tick);
            assert!(
                (sample.x_m - x).abs() < 1e-4 && (sample.z_m - z).abs() < 1e-4,
                "{sample:?}"
            );
            assert!((sample.heading_rad - heading).abs() < 1e-5, "{sample:?}");
            assert!((sample.speed_mps - speed).abs() < 1e-4, "{sample:?}");
            assert_eq!(sample.gear, gear);
            assert_eq!(sample.rejected, rejected);
        }
    }
}
