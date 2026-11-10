//! 교육 시나리오 정의와 파일 읽기(Phase 5-1).
//!
//! 시나리오 하나는 TOML 파일(`assets/scenarios/*.toml`) 하나다. 시작 조건([`StartDef`]), 차례로 끝내야 하는
//! 단계([`StepDef`], 목표), 조건이 맞으면 한 번 일어나는 이벤트([`EventDef`]), 실패 조건([`EndDef`])을 담는다.
//! 코드를 바꾸지 않고 파일만 고쳐 안내 문구·위치·시간 제한 같은 값을 바꿀 수 있게 하는 것이 목적이다.
//! 실행 중 판정은 [`run::ScenarioRun`]이 한다. 위치 조건은 차체 중심 기준이며, 좌표는 코스 지면 (x, z) m다.
//! 모든 기준값은 시나리오 작성자가 정한 **훈련용 임시값**이며 공식 평가 기준이 아니다(5-5 참고).

pub mod run;

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::Path;

use serde::Deserialize;

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
    /// 기어가 `gear`임.
    Gear { gear: GearName },
    /// 정지 판정(상태 머신 임계값).
    Stopped {},
    /// 속도 크기가 `kmh` 이상.
    SpeedAtLeast { kmh: f32 },
    /// 시나리오 시작 뒤 움직인 거리(전진·후진 모두, m)가 `distance_m` 이상.
    Travel { distance_m: f32 },
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
            Condition::EngineOn {} | Condition::Gear { .. } | Condition::Stopped {} => Ok(()),
            Condition::SpeedAtLeast { kmh } => non_negative("kmh", *kmh),
            Condition::Travel { distance_m } => non_negative("distance_m", *distance_m),
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
            }
        }
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

    /// 저장소의 시나리오 파일이 모두 읽히고 목록에 제목으로 나오는지 확인한다.
    #[test]
    fn bundled_scenarios_load() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = list_scenario_files(&root);
        assert!(!files.is_empty(), "{SCENARIO_DIR}에 시나리오가 없음");
        for file in files {
            let def = load_scenario(&root.join(&file.path))
                .unwrap_or_else(|error| panic!("{}: {error}", file.path));
            assert_eq!(file.label, def.title);
        }
    }
}
