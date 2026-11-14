//! 시나리오 진행 중 기록(Phase 5-2·5-4·5-5): 사건 기록([`RecordEvent`])과 재현용 입력 열([`TrackEntry`]).
//!
//! 사건은 진행 중 틱마다 [`super::run::ScenarioRun`]이 판정해 쌓는다. 입력 열은 틱마다 실제로 쓴 연속 입력과
//! 처리된 명령을 같은 값끼리 묶어 둔 것이며, 같은 시나리오·코스·차량 설정으로 다시 돌리면 같은 기록이 나온다
//! ([`super::result::replay`]). 모든 기준값은 훈련용 임시값이다.

use serde::{Deserialize, Serialize};

use crate::input::ControlRequest;
use crate::session::{CommandResult, DT, DriveCommand};
use crate::vehicle::state::{Gear, VehicleInput};

/// 기록 사건 종류. JSON에서는 `kind` 이름(snake_case)으로 구분한다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecordKind {
    /// 단계를 끝냄.
    StepDone { step: String },
    /// 기어 변경이 거부됨(움직이는 중 변경, 시동 OFF, 기어 키 동시 입력 등).
    GearError { command: String, reason: String },
    /// 속도가 과속 기준을 넘기 시작함(과도한 입력).
    OverSpeed { speed_kmh: f32 },
    /// 기준 속도 이상에서 제동 입력이 기준 이상으로 들어가기 시작함(급제동, 과도한 입력).
    HarshBrake { value: f32, speed_kmh: f32 },
    /// 기준 속도 이상에서 조향 입력 크기가 기준 이상이 되기 시작함(급조향, 과도한 입력).
    HarshSteer { value: f32, speed_kmh: f32 },
    /// 비상 정지를 켬.
    EmergencyStop { speed_kmh: f32 },
    /// 충돌. `hazard`는 보행자 대체 객체와 겹친 경우 그 이름, 코스 장애물이면 `None`.
    Collision {
        speed_kmh: f32,
        x_m: f32,
        z_m: f32,
        hazard: Option<String>,
    },
    /// 위치 정지 단계를 끝낸 자리와 목표 중심까지의 오차(차체 중심 기준).
    StopPosition {
        step: String,
        error_m: f32,
        x_m: f32,
        z_m: f32,
    },
    /// 보행자 대체 객체가 등장함. `distance_m`은 그때 차체 중심에서 객체 중심까지 거리.
    HazardAppeared {
        hazard: String,
        x_m: f32,
        z_m: f32,
        speed_kmh: f32,
        distance_m: f32,
    },
    /// 위험 등장 뒤 처음으로 제동을 시작함. `reaction_s`는 등장부터 제동 시작까지 시간.
    BrakeStart {
        hazard: String,
        reaction_s: f32,
        speed_kmh: f32,
        distance_m: f32,
    },
    /// 진행 중 시작 위치로 되돌림(R 키). 시나리오 진행은 그대로다.
    CourseReset,
    /// 시나리오가 끝남(`success`, `time_limit`, `collisions`, `aborted`).
    Finished { outcome: String },
}

/// 기록 사건 하나와 일어난 틱(시나리오 시작부터, 첫 틱이 1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordEvent {
    pub tick: u64,
    /// 경과 시간(s) = `tick` × [`DT`].
    pub time_s: f32,
    #[serde(flatten)]
    pub kind: RecordKind,
}

impl RecordEvent {
    /// 틱 번호로 사건을 만든다.
    pub fn new(tick: u64, kind: RecordKind) -> Self {
        Self {
            tick,
            time_s: tick as f32 * DT,
            kind,
        }
    }
}

/// 입력 열 한 구간: `ticks`틱 동안 같은 연속 입력을 쓰고, 첫 틱에 `commands`를 처리 순서대로 처리했다.
/// 입력은 f32 값을 그대로 옮긴 f64다(JSON을 거쳐도 같은 값으로 돌아오게).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackEntry {
    pub ticks: u32,
    pub throttle: f64,
    pub brake: f64,
    pub steering: f64,
    /// 계약 명령 이름(`gear_drive` 등). 기어 키 동시 입력은 `gear_conflict:R+D`처럼 기어를 붙인다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
}

impl TrackEntry {
    /// 이 구간의 연속 입력.
    pub fn input(&self) -> VehicleInput {
        VehicleInput {
            throttle: self.throttle as f32,
            brake: self.brake as f32,
            steering: self.steering as f32,
        }
    }
}

/// 진행 중 틱 하나의 입력을 입력 열 끝에 더한다. 명령이 없고 입력이 직전 구간과 같으면 그 구간을 늘린다.
pub fn push_track(track: &mut Vec<TrackEntry>, input: VehicleInput, results: &[CommandResult]) {
    let commands: Vec<String> = results.iter().map(track_command).collect();
    if commands.is_empty()
        && let Some(last) = track.last_mut()
        && last.input() == input
    {
        last.ticks += 1;
        return;
    }
    track.push(TrackEntry {
        ticks: 1,
        throttle: f64::from(input.throttle),
        brake: f64::from(input.brake),
        steering: f64::from(input.steering),
        commands,
    });
}

/// 명령 결과를 입력 열의 명령 이름으로 바꾼다. 기어 동시 입력은 거부 사유의 기어 목록을 붙인다.
fn track_command(result: &CommandResult) -> String {
    if result.command == "gear_conflict" {
        let gears = result
            .reason
            .split_once('(')
            .and_then(|(_, rest)| rest.strip_suffix(')'))
            .unwrap_or("");
        format!("gear_conflict:{gears}")
    } else {
        result.command.clone()
    }
}

/// 입력 열의 명령 이름 하나를 다시 넣을 방법.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackCommand {
    /// 대기열에 넣는다(처리 순서 유지).
    Queue(DriveCommand),
    /// 장치 요청으로 넣는다(기어 동시 입력).
    Device(ControlRequest),
}

/// 입력 열의 명령 이름을 다시 넣을 명령으로 바꾼다. 모르는 이름이면 `None`.
pub fn parse_track_command(name: &str) -> Option<TrackCommand> {
    if let Some(gears) = name.strip_prefix("gear_conflict:") {
        let gears = gears
            .split('+')
            .map(|gear| match gear {
                "P" => Some(Gear::Park),
                "R" => Some(Gear::Reverse),
                "N" => Some(Gear::Neutral),
                "D" => Some(Gear::Drive),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        return Some(TrackCommand::Device(ControlRequest::GearConflict(gears)));
    }
    DriveCommand::parse(name).map(TrackCommand::Queue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::CommandSource;

    /// 명령 결과 하나를 만든다.
    fn result(command: &str, reason: &str) -> CommandResult {
        CommandResult {
            command: command.to_owned(),
            source: CommandSource::Device,
            accepted: reason.is_empty(),
            reason: reason.to_owned(),
        }
    }

    /// 같은 입력은 한 구간으로 묶이고, 명령이 있거나 입력이 바뀌면 새 구간이 되는지 확인한다.
    #[test]
    fn track_groups_equal_inputs() {
        let mut track = Vec::new();
        let idle = VehicleInput::default();
        let push = VehicleInput {
            throttle: 0.5,
            ..idle
        };
        push_track(&mut track, idle, &[result("engine_toggle", "")]);
        push_track(&mut track, idle, &[]);
        push_track(&mut track, push, &[]);
        push_track(&mut track, push, &[]);
        push_track(&mut track, push, &[result("gear_drive", "")]);
        let shape: Vec<(u32, f64, usize)> = track
            .iter()
            .map(|entry| (entry.ticks, entry.throttle, entry.commands.len()))
            .collect();
        assert_eq!(shape, vec![(2, 0.0, 1), (2, 0.5, 0), (1, 0.5, 1)]);
    }

    /// 기어 동시 입력이 기어 목록과 함께 기록되고 다시 장치 요청으로 바뀌는지 확인한다.
    #[test]
    fn gear_conflict_round_trips() {
        let name = track_command(&result("gear_conflict", "simultaneous gear keys (R+D)"));
        assert_eq!(name, "gear_conflict:R+D");
        assert_eq!(
            parse_track_command(&name),
            Some(TrackCommand::Device(ControlRequest::GearConflict(vec![
                Gear::Reverse,
                Gear::Drive
            ])))
        );
        assert!(matches!(
            parse_track_command("gear_reverse"),
            Some(TrackCommand::Queue(_))
        ));
        assert_eq!(parse_track_command("gear_sport"), None);
    }
}
