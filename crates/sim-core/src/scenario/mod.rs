//! 교육 시나리오 정의와 파일 읽기(Phase 5-1).
//!
//! 시나리오 하나는 TOML 파일(`assets/scenarios/*.toml`) 하나다. 시작 조건([`StartDef`]), 차례로 끝내야 하는
//! 단계([`StepDef`], 목표), 조건이 맞으면 한 번 일어나는 이벤트([`EventDef`]), 실패 조건([`EndDef`])을 담는다.
//! 코드를 바꾸지 않고 파일만 고쳐 안내 문구·위치·시간 제한 같은 값을 바꿀 수 있게 하는 것이 목적이다.
//! 실행 중 판정은 [`run::ScenarioRun`]이 한다. 위치 조건은 차체 중심 기준이며, 좌표는 코스 지면 (x, z) m다.
//! 모든 기준값은 시나리오 작성자가 정한 **훈련용 임시값**이며 공식 평가 기준이 아니다(5-5 참고).
//!
//! Phase 5-2~5-5에서 시나리오가 놓는 고정 장애물([`ObstacleDef`]), 정해진 경로로 움직이는 보행자 대체 객체
//! ([`HazardDef`]), 기록 기준([`RecordDef`])과 임시 점수 기준([`ScoreDef`])을 더했다.

pub mod record;
pub mod result;
pub mod run;
#[cfg(test)]
mod scripted_tests;

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::Path;

use glam::Vec2;
use serde::Deserialize;

use crate::vehicle::GroundRect;
use crate::vehicle::state::Gear;

/// 저장소에 들어 있는 시나리오 폴더(저장소 루트 기준).
pub const SCENARIO_DIR: &str = "assets/scenarios";
/// 지금 읽을 수 있는 시나리오 파일 형식 번호.
pub const SCENARIO_FORMAT: u32 = 1;

/// 파일에 쓰는 기어 이름(`"P"`, `"R"`, `"N"`, `"D"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum GearName {
    P,
    R,
    N,
    D,
}

impl GearName {
    /// 상태 머신의 기어로 바꾼다.
    pub fn gear(self) -> Gear {
        match self {
            GearName::P => Gear::Park,
            GearName::R => Gear::Reverse,
            GearName::N => Gear::Neutral,
            GearName::D => Gear::Drive,
        }
    }
}

/// 시작 조건. 시작·재시작 때마다 이 상태로 만든다.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StartDef {
    /// 시동을 켠 채 시작하는지.
    pub engine_on: bool,
    /// 시작 기어. P가 아니면 `engine_on = true`여야 한다.
    pub gear: GearName,
    /// 시작 후륜축 중심 (x, z) m. 없으면 코스 시작 자세를 쓴다.
    pub position_m: Option<[f32; 2]>,
    /// 시작 방향(deg, 코스 좌표 +Y축 회전, 0이면 -Z). `position_m`이 있을 때만 쓴다(없으면 0).
    pub heading_deg: Option<f32>,
}

impl Default for StartDef {
    /// 시동 OFF, P, 코스 시작 자세.
    fn default() -> Self {
        Self {
            engine_on: false,
            gear: GearName::P,
            position_m: None,
            heading_deg: None,
        }
    }
}

/// 단계 완료·이벤트 발생 조건. 매 틱 끝 상태로 판정한다.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    /// 시동이 켜짐(비상 정지 중 제외).
    EngineOn {},
    /// 시동이 꺼짐(비상 정지 중 제외).
    EngineOff {},
    /// 기어가 `gear`임.
    Gear { gear: GearName },
    /// 정지 판정(상태 머신 임계값).
    Stopped {},
    /// 속도 크기가 `kmh` 이상.
    SpeedAtLeast { kmh: f32 },
    /// 시나리오 시작 뒤 움직인 거리(전진·후진 모두, m)가 `distance_m` 이상.
    Travel { distance_m: f32 },
    /// 시나리오 시작 뒤 후진한 거리(m)가 `distance_m` 이상.
    Reversed { distance_m: f32 },
    /// 차체 중심이 `position_m`에서 `radius_m` 안.
    Reach { position_m: [f32; 2], radius_m: f32 },
    /// 차체 중심이 `position_m`에서 `radius_m` 안에서 정지.
    StopIn { position_m: [f32; 2], radius_m: f32 },
    /// 차체 중심이 코스 목표(지도 경로 끝)에서 `radius_m` 안에서 정지. 목표가 없는 코스에는 쓸 수 없다.
    StopAtGoal { radius_m: f32 },
    /// 시나리오 경과 시간(일시정지 제외, s)이 `s` 이상.
    Elapsed { s: f32 },
    /// 단계 `step`을 끝냄. 이벤트 조건에만 쓴다.
    StepDone { step: String },
}

/// 차례로 끝내야 하는 단계(목표) 하나.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepDef {
    /// 파일 안에서 겹치지 않는 이름(이벤트·기록에서 가리킬 때 쓴다).
    pub id: String,
    /// 이 단계 동안 HUD에 보일 안내 문구.
    pub guide: String,
    /// 완료 조건.
    pub done: Condition,
}

/// 이벤트가 하는 일.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// HUD에 `text`를 `duration_s`초 동안(일시정지 제외) 띄운다.
    Message { text: String, duration_s: f32 },
    /// 보행자 대체 객체 `hazard`([`HazardDef`]의 `id`)를 등장시킨다(5-4).
    SpawnHazard { hazard: String },
}

/// 시나리오가 코스에 더하는 고정 장애물(5-3). 회전 없는 상자이며 코스의 다른 충돌 경계와 똑같이 막는다.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObstacleDef {
    /// 중심 (x, z) m.
    pub position_m: [f32; 2],
    /// 크기 (x 방향, z 방향) m.
    pub size_m: [f32; 2],
    /// 표시 높이(m). 충돌 판정은 높이를 보지 않는다.
    #[serde(default = "default_obstacle_height")]
    pub height_m: f32,
}

/// 고정 장애물 기본 표시 높이(m, 임시값).
fn default_obstacle_height() -> f32 {
    0.8
}

/// 정해진 경로를 일정 속도로 지나가는 보행자 대체 객체(5-4). AI가 아니라 결정적 스크립트다.
///
/// 이벤트 동작 `spawn_hazard`로 등장해 `path_m`의 첫 점에서 마지막 점까지 `speed_mps`로 걷는다. 끝점에 닿으면
/// `remain_at_end`가 아니면 사라진다. 차량을 막지 않으며, 차체와 겹치면 충돌로 기록한다.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HazardDef {
    /// 파일 안에서 겹치지 않는 이름.
    pub id: String,
    /// 지나갈 점들 (x, z) m. 두 개 이상.
    pub path_m: Vec<[f32; 2]>,
    /// 걷는 속도(m/s).
    pub speed_mps: f32,
    /// 바닥 크기(폭, 앞뒤) m.
    #[serde(default = "default_hazard_size")]
    pub size_m: [f32; 2],
    /// 표시 높이(m).
    #[serde(default = "default_hazard_height")]
    pub height_m: f32,
    /// 끝점에 닿은 뒤에도 그 자리에 남는지.
    #[serde(default)]
    pub remain_at_end: bool,
}

/// 보행자 대체 객체 기본 바닥 크기(m, 임시값).
fn default_hazard_size() -> [f32; 2] {
    [0.6, 0.6]
}

/// 보행자 대체 객체 기본 높이(m, 임시값).
fn default_hazard_height() -> f32 {
    1.7
}

/// 기록 기준(5-2·5-4). 모두 훈련용 임시값이다.
///
/// 키보드는 가속·제동이 0 또는 1뿐이므로 과도한 입력은 입력 크기만이 아니라 차량 속도와 함께 본다:
/// 과속(속도가 `speed_limit_kmh`를 넘기 시작), 급제동·급조향(`*_kmh` 이상 속도에서 입력이 기준 이상이 되기 시작).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RecordDef {
    /// 과속 기준 속도(km/h).
    pub speed_limit_kmh: f32,
    /// 급제동으로 볼 제동 입력(0..=1)과 그때의 최소 속도(km/h).
    pub harsh_brake: f32,
    pub harsh_brake_kmh: f32,
    /// 급조향으로 볼 조향 입력 크기(0..=1)와 그때의 최소 속도(km/h).
    pub harsh_steer: f32,
    pub harsh_steer_kmh: f32,
    /// 위험 등장 뒤 제동 입력이 이 값 이상이 되면 제동 시작으로 본다(0..=1, 반응 시간 기준).
    pub reaction_brake: f32,
}

impl Default for RecordDef {
    /// 과속 15 km/h, 급제동 0.8 이상 · 10 km/h 이상, 급조향 0.8 이상 · 10 km/h 이상, 제동 시작 0.1(임시값).
    fn default() -> Self {
        Self {
            speed_limit_kmh: 15.0,
            harsh_brake: 0.8,
            harsh_brake_kmh: 10.0,
            harsh_steer: 0.8,
            harsh_steer_kmh: 10.0,
            reaction_brake: 0.1,
        }
    }
}

/// 임시 점수 기준(5-5). 100점에서 기록마다 뺀다. **교육 담당자가 승인한 평가 기준이 아니다.**
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScoreDef {
    /// 성공했고 점수가 이 값 이상이면 임시 합격.
    pub pass_score: f32,
    /// 기어 변경 오류 하나당 감점.
    pub gear_error: f32,
    /// 과도한 입력(과속·급제동·급조향) 하나당 감점.
    pub harsh_input: f32,
    /// 충돌 하나당 감점.
    pub collision: f32,
    /// 비상 정지 하나당 감점.
    pub emergency_stop: f32,
    /// 정지 위치 오차 1 m당 감점.
    pub stop_error_per_m: f32,
    /// 위험 반응 시간이 이 값(s)을 넘으면 넘은 1초당 `slow_reaction_per_s`를 뺀다.
    pub reaction_limit_s: f32,
    /// 반응 시간 초과 1초당 감점.
    pub slow_reaction_per_s: f32,
}

impl Default for ScoreDef {
    /// 합격 70, 기어 오류 5, 과도한 입력 5, 충돌 20, 비상 정지 10, 정지 오차 1 m당 10,
    /// 반응 1.5 s 초과 1초당 10(모두 임시값).
    fn default() -> Self {
        Self {
            pass_score: 70.0,
            gear_error: 5.0,
            harsh_input: 5.0,
            collision: 20.0,
            emergency_stop: 10.0,
            stop_error_per_m: 10.0,
            reaction_limit_s: 1.5,
            slow_reaction_per_s: 10.0,
        }
    }
}

/// 조건이 처음 맞는 틱에 한 번 일어나는 이벤트.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventDef {
    /// 파일 안에서 겹치지 않는 이름.
    pub id: String,
    /// 발생 조건.
    pub when: Condition,
    /// 할 일.
    pub action: Action,
}

/// 실패 조건. 모든 단계를 끝내면 성공이다.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EndDef {
    /// 경과 시간 상한(s). 넘으면 실패. 없으면 제한 없음.
    pub time_limit_s: Option<f32>,
    /// 허용 충돌 횟수. 넘으면 실패. 없으면 제한 없음(0이면 첫 충돌에서 실패).
    pub max_collisions: Option<u32>,
}

/// 시나리오 파일 하나의 내용.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioDef {
    /// 파일 형식 번호([`SCENARIO_FORMAT`]).
    pub format: u32,
    /// 메뉴·HUD에 보일 제목.
    pub title: String,
    /// 시작 전 안내(브리핑).
    #[serde(default)]
    pub briefing: String,
    /// 코스 선택 문자열(`training`, `calibration`, `map:<파일>`).
    pub course: String,
    #[serde(default)]
    pub start: StartDef,
    /// 단계(목표). 한 개 이상, 적힌 순서대로 끝낸다.
    pub steps: Vec<StepDef>,
    #[serde(default)]
    pub events: Vec<EventDef>,
    #[serde(default)]
    pub end: EndDef,
    /// 코스에 더할 고정 장애물(5-3).
    #[serde(default)]
    pub obstacles: Vec<ObstacleDef>,
    /// 보행자 대체 객체(5-4). 이벤트 `spawn_hazard`로 등장한다.
    #[serde(default)]
    pub hazards: Vec<HazardDef>,
    #[serde(default)]
    pub record: RecordDef,
    #[serde(default)]
    pub score: ScoreDef,
}

/// 시나리오 파일을 읽지 못하거나 내용이 규칙을 어긴 이유(한국어).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioError(pub String);

impl fmt::Display for ScenarioError {
    /// 사람이 읽는 한국어 오류 문구를 쓴다.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "시나리오 오류: {}", self.0)
    }
}

impl std::error::Error for ScenarioError {}

/// 검사 실패 사유 하나를 만든다.
fn invalid(message: String) -> ScenarioError {
    ScenarioError(message)
}

impl Condition {
    /// 수치가 유한하고 범위 안인지, 쓸 수 있는 자리인지 검사한다. `place`는 오류 문구에 쓸 위치다.
    fn validate(
        &self,
        place: &str,
        in_event: bool,
        step_ids: &HashSet<&str>,
    ) -> Result<(), ScenarioError> {
        let non_negative = |name: &str, value: f32| {
            if value.is_finite() && value >= 0.0 {
                Ok(())
            } else {
                Err(invalid(format!(
                    "{place}의 {name}은 0 이상의 유한한 값이어야 합니다"
                )))
            }
        };
        let positive_radius = |radius_m: f32| {
            if radius_m.is_finite() && radius_m > 0.0 {
                Ok(())
            } else {
                Err(invalid(format!(
                    "{place}의 radius_m은 0보다 큰 유한한 값이어야 합니다"
                )))
            }
        };
        let finite_position = |position_m: &[f32; 2]| {
            if position_m.iter().all(|value| value.is_finite()) {
                Ok(())
            } else {
                Err(invalid(format!(
                    "{place}의 position_m에 유한하지 않은 값이 있습니다"
                )))
            }
        };
        match self {
            Condition::EngineOn {}
            | Condition::EngineOff {}
            | Condition::Gear { .. }
            | Condition::Stopped {} => Ok(()),
            Condition::SpeedAtLeast { kmh } => non_negative("kmh", *kmh),
            Condition::Travel { distance_m } | Condition::Reversed { distance_m } => {
                non_negative("distance_m", *distance_m)
            }
            Condition::Elapsed { s } => non_negative("s", *s),
            Condition::Reach {
                position_m,
                radius_m,
            }
            | Condition::StopIn {
                position_m,
                radius_m,
            } => {
                finite_position(position_m)?;
                positive_radius(*radius_m)
            }
            Condition::StopAtGoal { radius_m } => positive_radius(*radius_m),
            Condition::StepDone { step } => {
                if !in_event {
                    Err(invalid(format!(
                        "{place}: step_done 조건은 이벤트(when)에만 쓸 수 있습니다"
                    )))
                } else if !step_ids.contains(step.as_str()) {
                    Err(invalid(format!(
                        "{place}: 없는 단계 \"{step}\"를 가리킵니다"
                    )))
                } else {
                    Ok(())
                }
            }
        }
    }

    /// 코스 목표가 있어야 판정할 수 있는 조건인지 반환한다.
    pub fn needs_goal(&self) -> bool {
        matches!(self, Condition::StopAtGoal { .. })
    }
}

impl HazardDef {
    /// 경로 점 수·좌표·속도·크기를 검사한다.
    fn validate(&self, index: usize) -> Result<(), ScenarioError> {
        if self.id.trim().is_empty() {
            return Err(invalid(format!("hazards[{index}].id가 비어 있습니다")));
        }
        if self.path_m.len() < 2 {
            return Err(invalid(format!(
                "hazards[{index}].path_m에는 점이 두 개 이상 있어야 합니다"
            )));
        }
        if !self.path_m.iter().flatten().all(|value| value.is_finite()) {
            return Err(invalid(format!(
                "hazards[{index}].path_m에 유한하지 않은 값이 있습니다"
            )));
        }
        if !(self.speed_mps.is_finite() && self.speed_mps > 0.0) {
            return Err(invalid(format!(
                "hazards[{index}].speed_mps는 0보다 큰 유한한 값이어야 합니다"
            )));
        }
        let positive = self
            .size_m
            .iter()
            .chain(std::iter::once(&self.height_m))
            .all(|value| value.is_finite() && *value > 0.0);
        if !positive {
            return Err(invalid(format!(
                "hazards[{index}]의 size_m·height_m은 0보다 커야 합니다"
            )));
        }
        Ok(())
    }

    /// 등장 뒤 `walked_m`만큼 걸었을 때의 중심 (x, z)와 진행 방향(rad, [`crate::vehicle::Motion`] 규칙: 0이면 -Z).
    /// 끝점을 지났는데 `remain_at_end`가 아니면 `None`(사라짐).
    pub fn pose_at(&self, walked_m: f32) -> Option<(Vec2, f32)> {
        let points: Vec<Vec2> = self.path_m.iter().map(|point| Vec2::from(*point)).collect();
        let mut left = walked_m.max(0.0);
        let mut heading = 0.0;
        for pair in points.windows(2) {
            let segment = pair[1] - pair[0];
            let length = segment.length();
            if length > 0.0 {
                heading = (-segment.x).atan2(-segment.y);
            }
            if left <= length {
                let t = if length > 0.0 { left / length } else { 0.0 };
                return Some((pair[0] + segment * t, heading));
            }
            left -= length;
        }
        self.remain_at_end
            .then(|| (*points.last().expect("checked two points"), heading))
    }

    /// 중심·방향에서 이 객체가 차지하는 지면 직사각형.
    pub fn rect_at(&self, center: Vec2, heading_rad: f32) -> GroundRect {
        GroundRect {
            center,
            half_size: Vec2::from(self.size_m) / 2.0,
            heading_rad,
        }
    }
}

impl RecordDef {
    /// 입력 기준은 0보다 크고 1 이하, 속도 기준은 0 이상의 유한한 값인지 검사한다.
    fn validate(&self) -> Result<(), ScenarioError> {
        for (name, value) in [
            ("harsh_brake", self.harsh_brake),
            ("harsh_steer", self.harsh_steer),
            ("reaction_brake", self.reaction_brake),
        ] {
            if !(value.is_finite() && value > 0.0 && value <= 1.0) {
                return Err(invalid(format!(
                    "record.{name}는 0보다 크고 1 이하여야 합니다"
                )));
            }
        }
        for (name, value) in [
            ("speed_limit_kmh", self.speed_limit_kmh),
            ("harsh_brake_kmh", self.harsh_brake_kmh),
            ("harsh_steer_kmh", self.harsh_steer_kmh),
        ] {
            if !(value.is_finite() && value >= 0.0) {
                return Err(invalid(format!(
                    "record.{name}는 0 이상의 유한한 값이어야 합니다"
                )));
            }
        }
        Ok(())
    }
}

impl ScoreDef {
    /// 모든 값이 0 이상의 유한한 값인지 검사한다.
    fn validate(&self) -> Result<(), ScenarioError> {
        for (name, value) in [
            ("pass_score", self.pass_score),
            ("gear_error", self.gear_error),
            ("harsh_input", self.harsh_input),
            ("collision", self.collision),
            ("emergency_stop", self.emergency_stop),
            ("stop_error_per_m", self.stop_error_per_m),
            ("reaction_limit_s", self.reaction_limit_s),
            ("slow_reaction_per_s", self.slow_reaction_per_s),
        ] {
            if !(value.is_finite() && value >= 0.0) {
                return Err(invalid(format!(
                    "score.{name}는 0 이상의 유한한 값이어야 합니다"
                )));
            }
        }
        Ok(())
    }
}

impl ScenarioDef {
    /// 형식 번호·이름 중복·수치 범위·가리키는 단계·시작 조건을 검사한다. 어기면 첫 번째 위반을 돌려준다.
    pub fn validate(&self) -> Result<(), ScenarioError> {
        if self.format != SCENARIO_FORMAT {
            return Err(invalid(format!(
                "format = {}은 읽을 수 없습니다({SCENARIO_FORMAT}만 지원)",
                self.format
            )));
        }
        if self.title.trim().is_empty() {
            return Err(invalid("title이 비어 있습니다".to_owned()));
        }
        if self.course.trim().is_empty() {
            return Err(invalid("course가 비어 있습니다".to_owned()));
        }
        if self.start.gear != GearName::P && !self.start.engine_on {
            return Err(invalid(
                "start.gear가 P가 아니면 start.engine_on = true여야 합니다".to_owned(),
            ));
        }
        if let Some(position_m) = self.start.position_m
            && !position_m.iter().all(|value| value.is_finite())
        {
            return Err(invalid(
                "start.position_m에 유한하지 않은 값이 있습니다".to_owned(),
            ));
        }
        if let Some(heading_deg) = self.start.heading_deg {
            if self.start.position_m.is_none() {
                return Err(invalid(
                    "start.heading_deg는 start.position_m과 함께 써야 합니다".to_owned(),
                ));
            }
            if !heading_deg.is_finite() {
                return Err(invalid("start.heading_deg가 유한하지 않습니다".to_owned()));
            }
        }
        if self.steps.is_empty() {
            return Err(invalid("steps가 하나 이상 있어야 합니다".to_owned()));
        }
        let mut step_ids = HashSet::new();
        for (index, step) in self.steps.iter().enumerate() {
            if step.id.trim().is_empty() {
                return Err(invalid(format!("steps[{index}].id가 비어 있습니다")));
            }
            if !step_ids.insert(step.id.as_str()) {
                return Err(invalid(format!("단계 이름 \"{}\"가 겹칩니다", step.id)));
            }
        }
        for (index, step) in self.steps.iter().enumerate() {
            if step.guide.trim().is_empty() {
                return Err(invalid(format!("steps[{index}].guide가 비어 있습니다")));
            }
            step.done
                .validate(&format!("steps[{index}].done"), false, &step_ids)?;
        }
        let mut hazard_ids = HashSet::new();
        for (index, hazard) in self.hazards.iter().enumerate() {
            hazard.validate(index)?;
            if !hazard_ids.insert(hazard.id.as_str()) {
                return Err(invalid(format!(
                    "보행자 대체 객체 이름 \"{}\"가 겹칩니다",
                    hazard.id
                )));
            }
        }
        let mut spawned = HashSet::new();
        let mut event_ids = HashSet::new();
        for (index, event) in self.events.iter().enumerate() {
            if event.id.trim().is_empty() {
                return Err(invalid(format!("events[{index}].id가 비어 있습니다")));
            }
            if !event_ids.insert(event.id.as_str()) {
                return Err(invalid(format!("이벤트 이름 \"{}\"가 겹칩니다", event.id)));
            }
            event
                .when
                .validate(&format!("events[{index}].when"), true, &step_ids)?;
            match &event.action {
                Action::Message { text, duration_s } => {
                    if text.trim().is_empty() {
                        return Err(invalid(format!(
                            "events[{index}].action.text가 비어 있습니다"
                        )));
                    }
                    if !(duration_s.is_finite() && *duration_s > 0.0) {
                        return Err(invalid(format!(
                            "events[{index}].action.duration_s는 0보다 큰 유한한 값이어야 합니다"
                        )));
                    }
                }
                Action::SpawnHazard { hazard } => {
                    if !hazard_ids.contains(hazard.as_str()) {
                        return Err(invalid(format!(
                            "events[{index}].action: 없는 보행자 대체 객체 \"{hazard}\"를 가리킵니다"
                        )));
                    }
                    if !spawned.insert(hazard.as_str()) {
                        return Err(invalid(format!(
                            "events[{index}].action: 보행자 대체 객체 \"{hazard}\"를 두 번 등장시킵니다"
                        )));
                    }
                }
            }
        }
        for (index, obstacle) in self.obstacles.iter().enumerate() {
            let finite = obstacle
                .position_m
                .iter()
                .chain(&obstacle.size_m)
                .all(|value| value.is_finite());
            if !finite
                || obstacle.size_m.iter().any(|size| *size <= 0.0)
                || !(obstacle.height_m.is_finite() && obstacle.height_m > 0.0)
            {
                return Err(invalid(format!(
                    "obstacles[{index}]: 위치는 유한하고 크기·높이는 0보다 커야 합니다"
                )));
            }
        }
        self.record.validate()?;
        self.score.validate()?;
        if let Some(limit) = self.end.time_limit_s
            && !(limit.is_finite() && limit > 0.0)
        {
            return Err(invalid(
                "end.time_limit_s는 0보다 큰 유한한 값이어야 합니다".to_owned(),
            ));
        }
        Ok(())
    }

    /// 단계·이벤트 중 코스 목표가 필요한 조건이 있는지 반환한다.
    pub fn needs_goal(&self) -> bool {
        self.steps.iter().any(|step| step.done.needs_goal())
            || self.events.iter().any(|event| event.when.needs_goal())
    }
}

/// 시나리오 TOML 문자열을 읽고 검사한다.
pub fn parse_scenario(text: &str) -> Result<ScenarioDef, ScenarioError> {
    let def: ScenarioDef = toml::from_str(text)
        .map_err(|error| invalid(format!("파일 형식이 맞지 않습니다: {error}")))?;
    def.validate()?;
    Ok(def)
}

/// 시나리오 파일을 읽고 검사한다.
pub fn load_scenario(path: &Path) -> Result<ScenarioDef, ScenarioError> {
    let text = fs::read_to_string(path)
        .map_err(|error| invalid(format!("{}를 읽을 수 없습니다: {error}", path.display())))?;
    parse_scenario(&text)
}

/// 메뉴에 보일 시나리오 파일 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioFile {
    /// 제목. 파일을 읽지 못하면 파일 이름 뒤에 `(오류)`를 붙인다(여는 순간 사유를 보인다).
    pub label: String,
    /// 저장소 루트 기준 경로(예: `assets/scenarios/sample-start-stop.toml`).
    pub path: String,
}

/// `root`(저장소 루트)의 [`SCENARIO_DIR`]에서 시나리오 파일을 이름 순으로 찾는다. 폴더가 없으면 빈 목록이다.
pub fn list_scenario_files(root: &Path) -> Vec<ScenarioFile> {
    let Ok(entries) = fs::read_dir(root.join(SCENARIO_DIR)) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_owned();
            let label = match load_scenario(&path) {
                Ok(def) => def.title,
                Err(_) => format!("{name} (오류)"),
            };
            Some(ScenarioFile {
                label,
                path: format!("{SCENARIO_DIR}/{name}"),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시험용 최소 시나리오(단계 둘, 이벤트 하나).
    pub(crate) const SAMPLE: &str = r#"
format = 1
title = "시험"
briefing = "안내"
course = "training"

[start]
engine_on = true
gear = "D"

[[steps]]
id = "go"
guide = "앞으로 가세요"
done = { kind = "travel", distance_m = 5.0 }

[[steps]]
id = "stop"
guide = "멈추세요"
done = { kind = "stopped" }

[[events]]
id = "hint"
when = { kind = "step_done", step = "go" }
action = { kind = "message", text = "잘했습니다", duration_s = 2.0 }

[end]
time_limit_s = 60.0
max_collisions = 0
"#;

    /// 예시 파일 내용을 그대로 읽어 각 부분이 들어오는지 확인한다.
    #[test]
    fn sample_parses() {
        let def = parse_scenario(SAMPLE).unwrap();
        assert_eq!(def.title, "시험");
        assert_eq!(def.start.gear, GearName::D);
        assert_eq!(def.steps.len(), 2);
        assert_eq!(def.steps[0].done, Condition::Travel { distance_m: 5.0 });
        assert_eq!(
            def.events[0].action,
            Action::Message {
                text: "잘했습니다".to_owned(),
                duration_s: 2.0
            }
        );
        assert_eq!(def.end.max_collisions, Some(0));
        assert!(!def.needs_goal());
    }

    /// 규칙을 어긴 파일마다 해당 사유로 거부하는지 확인한다.
    #[test]
    fn invalid_files_are_rejected_with_reason() {
        let cases = [
            ("format = 1", "format = 2", "format = 2"),
            ("gear = \"D\"", "gear = \"X\"", "파일 형식"),
            ("engine_on = true", "engine_on = false", "engine_on"),
            ("distance_m = 5.0", "distance_m = -1.0", "distance_m"),
            ("step = \"go\"", "step = \"없음\"", "없는 단계"),
            ("id = \"stop\"", "id = \"go\"", "겹칩니다"),
            ("duration_s = 2.0", "duration_s = 0.0", "duration_s"),
            ("time_limit_s = 60.0", "time_limit_s = 0.0", "time_limit_s"),
            ("max_collisions = 0", "max_collisoins = 0", "파일 형식"),
            (
                "done = { kind = \"stopped\" }",
                "done = { kind = \"stopped\", radius_m = 1.0 }",
                "파일 형식",
            ),
            (
                "duration_s = 2.0",
                "duration_s = 2.0, color = 1",
                "파일 형식",
            ),
            (
                "done = { kind = \"stopped\" }",
                "done = { kind = \"step_done\", step = \"go\" }",
                "이벤트(when)에만",
            ),
        ];
        for (from, to, expected) in cases {
            let text = SAMPLE.replacen(from, to, 1);
            let error = parse_scenario(&text).unwrap_err().to_string();
            assert!(error.contains(expected), "{to}: {error}");
        }
    }

    /// 보행자 대체 객체·장애물·기록·점수 값이 규칙을 어기면 사유와 함께 거부하는지 확인한다.
    #[test]
    fn hazard_obstacle_record_rules() {
        let base = format!(
            "{SAMPLE}\n{}",
            r#"
[[hazards]]
id = "p"
path_m = [[2.0, -10.0], [-2.0, -10.0]]
speed_mps = 1.0

[[events]]
id = "spawn"
when = { kind = "travel", distance_m = 1.0 }
action = { kind = "spawn_hazard", hazard = "p" }

[[obstacles]]
position_m = [0.0, -5.0]
size_m = [1.0, 1.0]
"#
        );
        let def = parse_scenario(&base).unwrap();
        assert_eq!(def.obstacles[0].height_m, 0.8);
        assert_eq!(def.hazards[0].size_m, [0.6, 0.6]);
        let cases = [
            ("hazard = \"p\"", "hazard = \"q\"", "없는 보행자"),
            ("speed_mps = 1.0", "speed_mps = 0.0", "speed_mps"),
            (
                "path_m = [[2.0, -10.0], [-2.0, -10.0]]",
                "path_m = [[2.0, -10.0]]",
                "두 개 이상",
            ),
            ("size_m = [1.0, 1.0]", "size_m = [1.0, 0.0]", "obstacles[0]"),
            (
                "max_collisions = 0",
                "max_collisions = 0\n\n[record]\nharsh_brake = 1.5",
                "record.harsh_brake",
            ),
            (
                "max_collisions = 0",
                "max_collisions = 0\n\n[score]\ncollision = -1.0",
                "score.collision",
            ),
        ];
        for (from, to, expected) in cases {
            let text = base.replacen(from, to, 1);
            let error = parse_scenario(&text).unwrap_err().to_string();
            assert!(error.contains(expected), "{to}: {error}");
        }
        let twice = format!(
            "{base}\n[[events]]\nid = \"again\"\nwhen = {{ kind = \"travel\", distance_m = 2.0 }}\naction = {{ kind = \"spawn_hazard\", hazard = \"p\" }}\n"
        );
        let error = parse_scenario(&twice).unwrap_err().to_string();
        assert!(error.contains("두 번"), "{error}");
    }

    /// 경로를 따라 걸은 거리만큼 움직이고, 끝을 지나면 사라지거나(기본) 남는지 확인한다.
    #[test]
    fn hazard_pose_follows_path() {
        let mut hazard = HazardDef {
            id: "p".to_owned(),
            path_m: vec![[4.0, -26.0], [-4.0, -26.0]],
            speed_mps: 1.3,
            size_m: [0.6, 0.6],
            height_m: 1.7,
            remain_at_end: false,
        };
        let (center, heading) = hazard.pose_at(0.0).unwrap();
        assert_eq!(center, Vec2::new(4.0, -26.0));
        // -X로 걸으므로 진행 방향은 +90°(Motion 규칙: 0이면 -Z, 양수 회전이면 -X 쪽).
        assert!(
            (heading - std::f32::consts::FRAC_PI_2).abs() < 1e-6,
            "{heading}"
        );
        assert_eq!(hazard.pose_at(3.0).unwrap().0, Vec2::new(1.0, -26.0));
        assert!(hazard.pose_at(8.5).is_none());
        hazard.remain_at_end = true;
        assert_eq!(hazard.pose_at(8.5).unwrap().0, Vec2::new(-4.0, -26.0));
    }

    /// 목표가 필요한 조건을 찾아내는지 확인한다.
    #[test]
    fn goal_condition_is_detected() {
        let text = SAMPLE.replacen(
            "done = { kind = \"stopped\" }",
            "done = { kind = \"stop_at_goal\", radius_m = 3.0 }",
            1,
        );
        assert!(parse_scenario(&text).unwrap().needs_goal());
    }

    /// 배포 목록에 교육용 A·B·C만 있고 모든 파일이 제목과 함께 읽히는지 확인한다.
    #[test]
    fn bundled_scenarios_load() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = list_scenario_files(&root);
        assert_eq!(
            files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "assets/scenarios/scenario-a-basic.toml",
                "assets/scenarios/scenario-b-space.toml",
                "assets/scenarios/scenario-c-hazard.toml",
            ],
            "배포 메뉴에는 교육용 A·B·C만 포함해야 함"
        );
        for file in files {
            let def = load_scenario(&root.join(&file.path))
                .unwrap_or_else(|error| panic!("{}: {error}", file.path));
            assert_eq!(file.label, def.title);
        }
    }
}
