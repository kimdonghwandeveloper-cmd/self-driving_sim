//! 시나리오 결과 파일(Phase 5-5): 세션 정보·완료 여부·사건 기록·요약·임시 점수·재현용 입력 열을 JSON으로 남긴다.
//!
//! 결과 파일에는 시나리오 파일 내용과 입력 열이 함께 들어 있어, 같은 코스·차량 설정으로 다시 돌리면 같은 기록이
//! 나오는지 [`replay_check`]로 확인할 수 있다. 점수는 **임시 기준**이며 교육 담당자 승인 전 공식 평가로 쓰지 않는다.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::parse_scenario;
use super::record::{RecordEvent, RecordKind, TrackCommand, TrackEntry, parse_track_command};
use super::run::{Control, Outcome, Phase, ScenarioRun};
use crate::config::VehicleConfig;
use crate::session::{DT, DeviceTick};
use crate::world::CourseDef;

/// 결과 파일 형식 이름.
pub const RESULT_FORMAT: &str = "scenario-result-v1";
/// 결과 파일 폴더(저장소 루트 기준, git에 올리지 않는 `logs/` 아래).
pub const RESULT_DIR: &str = "logs/scenarios";
/// 점수 옆에 항상 붙이는 안내.
pub const SCORE_NOTICE: &str = "임시 기준 점수입니다. 교육 담당자가 승인한 평가 기준이 아니며 공식 합격·불합격 판정에 쓰지 않습니다.";

/// 단계 하나의 결과.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepResult {
    pub id: String,
    /// 끝낸 시각(s, 경과 시간 기준). 끝내지 못했으면 `None`.
    pub done_s: Option<f32>,
}

/// 기록 요약.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub gear_errors: u32,
    /// 과도한 입력(과속·급제동·급조향)을 합친 수.
    pub harsh_inputs: u32,
    /// 모든 충돌(코스 장애물 + 보행자 대체 객체).
    pub collisions: u32,
    /// 그중 보행자 대체 객체와의 충돌.
    pub hazard_collisions: u32,
    pub emergency_stops: u32,
    pub course_resets: u32,
    /// 위치 정지 단계마다 정지 위치 오차(m).
    pub stop_errors_m: Vec<f32>,
    /// 위험 등장마다 제동 반응 시간(s). 끝날 때까지 제동하지 않았으면 등장부터 끝까지의 시간이다.
    pub reaction_s: Vec<f32>,
    /// 끝날 때까지 제동하지 않은 위험 수.
    pub missed_reactions: u32,
}

/// 감점 하나.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deduction {
    pub reason: String,
    pub points: f32,
}

/// 임시 점수.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProvisionalScore {
    /// 100에서 감점을 뺀 값(0 아래로 내려가지 않는다).
    pub value: f32,
    pub pass_score: f32,
    /// 성공했고 `value >= pass_score`이면 `true`(임시 합격).
    pub passed: bool,
    pub deductions: Vec<Deduction>,
    pub notice: String,
}

/// 결과 파일 하나의 내용.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioResult {
    /// [`RESULT_FORMAT`].
    pub format: String,
    /// 시작 시각과 시나리오 파일 이름으로 만든 세션 ID(예: `20261111-093012-scenario-a-basic`).
    pub session_id: String,
    /// 시나리오 파일 경로(연 그대로).
    pub scenario_path: String,
    pub scenario_title: String,
    /// 시나리오 파일 내용(재현용).
    pub scenario_text: String,
    pub course: String,
    /// 시작·끝 시각(UTC, ISO 8601). 시작하지 않고 끝냈으면 시작 시각은 끝 시각과 같다.
    pub started_at: String,
    pub ended_at: String,
    /// `success`, `time_limit`, `collisions`, `aborted`.
    pub outcome: String,
    /// 모든 단계를 끝냈는지(`outcome == "success"`).
    pub completed: bool,
    pub elapsed_s: f32,
    pub ticks: u64,
    pub steps: Vec<StepResult>,
    pub summary: Summary,
    pub score: ProvisionalScore,
    pub records: Vec<RecordEvent>,
    /// 재현용 입력 열.
    pub input_track: Vec<TrackEntry>,
}

impl ScenarioResult {
    /// 끝난 실행으로 결과를 만든다. 아직 끝나지 않았으면 `None`.
    /// `scenario_path`는 표시·세션 ID용, `scenario_text`는 재현용 시나리오 파일 내용이다.
    pub fn new(run: &ScenarioRun, scenario_path: &str, scenario_text: &str) -> Option<Self> {
        let Phase::Finished(outcome) = run.phase() else {
            return None;
        };
        let def = run.def();
        let ended = run.ended_at().unwrap_or_else(SystemTime::now);
        let started = run.started_at().unwrap_or(ended);
        let stem = Path::new(scenario_path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("scenario");
        let done_times = run.step_done_times_s();
        let steps = def
            .steps
            .iter()
            .enumerate()
            .map(|(index, step)| StepResult {
                id: step.id.clone(),
                done_s: done_times.get(index).copied(),
            })
            .collect();
        let summary = summarize(run.records(), run.ticks());
        let score = provisional_score(&summary, outcome, &def.score);
        Some(Self {
            format: RESULT_FORMAT.to_owned(),
            session_id: format!("{}-{stem}", compact_time(started)),
            scenario_path: scenario_path.to_owned(),
            scenario_title: def.title.clone(),
            scenario_text: scenario_text.to_owned(),
            course: def.course.clone(),
            started_at: iso_time(started),
            ended_at: iso_time(ended),
            outcome: outcome.name().to_owned(),
            completed: outcome == Outcome::Success,
            elapsed_s: run.elapsed_s(),
            ticks: run.ticks(),
            steps,
            summary,
            score,
            records: run.records().to_vec(),
            input_track: run.track().to_vec(),
        })
    }

    /// 들여쓴 JSON 문자열로 바꾼다.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("result serializes")
    }
}

/// 사건 기록으로 요약을 만든다. 제동하지 않은 위험의 반응 시간은 등장부터 `end_tick`까지로 본다.
pub fn summarize(records: &[RecordEvent], end_tick: u64) -> Summary {
    let mut summary = Summary::default();
    let mut appeared: BTreeMap<&str, u64> = BTreeMap::new();
    let mut reacted: BTreeMap<&str, f32> = BTreeMap::new();
    for event in records {
        match &event.kind {
            RecordKind::GearError { .. } => summary.gear_errors += 1,
            RecordKind::OverSpeed { .. }
            | RecordKind::HarshBrake { .. }
            | RecordKind::HarshSteer { .. } => summary.harsh_inputs += 1,
            RecordKind::EmergencyStop { .. } => summary.emergency_stops += 1,
            RecordKind::Collision { hazard, .. } => {
                summary.collisions += 1;
                if hazard.is_some() {
                    summary.hazard_collisions += 1;
                }
            }
            RecordKind::StopPosition { error_m, .. } => summary.stop_errors_m.push(*error_m),
            RecordKind::HazardAppeared { hazard, .. } => {
                appeared.insert(hazard, event.tick);
            }
            RecordKind::BrakeStart {
                hazard, reaction_s, ..
            } => {
                reacted.insert(hazard, *reaction_s);
            }
            RecordKind::CourseReset => summary.course_resets += 1,
            RecordKind::StepDone { .. } | RecordKind::Finished { .. } => {}
        }
    }
    for (hazard, tick) in appeared {
        match reacted.get(hazard) {
            Some(reaction_s) => summary.reaction_s.push(*reaction_s),
            None => {
                summary.missed_reactions += 1;
                summary
                    .reaction_s
                    .push(end_tick.saturating_sub(tick) as f32 * DT);
            }
        }
    }
    summary
}

/// 요약과 결과로 임시 점수를 매긴다(100에서 감점, 0 아래로 내려가지 않음).
pub fn provisional_score(
    summary: &Summary,
    outcome: Outcome,
    score: &super::ScoreDef,
) -> ProvisionalScore {
    let mut deductions = Vec::new();
    let mut deduct = |reason: String, points: f32| {
        if points > 0.0 {
            deductions.push(Deduction { reason, points });
        }
    };
    deduct(
        format!("기어 변경 오류 {}회", summary.gear_errors),
        summary.gear_errors as f32 * score.gear_error,
    );
    deduct(
        format!("과도한 입력(과속·급제동·급조향) {}회", summary.harsh_inputs),
        summary.harsh_inputs as f32 * score.harsh_input,
    );
    deduct(
        format!("충돌 {}회", summary.collisions),
        summary.collisions as f32 * score.collision,
    );
    deduct(
        format!("비상 정지 {}회", summary.emergency_stops),
        summary.emergency_stops as f32 * score.emergency_stop,
    );
    let stop_error: f32 = summary.stop_errors_m.iter().sum();
    deduct(
        format!("정지 위치 오차 합 {stop_error:.2} m"),
        stop_error * score.stop_error_per_m,
    );
    for reaction_s in &summary.reaction_s {
        deduct(
            format!(
                "제동 반응 {reaction_s:.2} s(기준 {:.2} s)",
                score.reaction_limit_s
            ),
            (reaction_s - score.reaction_limit_s).max(0.0) * score.slow_reaction_per_s,
        );
    }
    let total: f32 = deductions.iter().map(|deduction| deduction.points).sum();
    let value = (100.0 - total).max(0.0);
    ProvisionalScore {
        value,
        pass_score: score.pass_score,
        passed: outcome == Outcome::Success && value >= score.pass_score,
        deductions,
        notice: SCORE_NOTICE.to_owned(),
    }
}

/// 결과를 `dir`에 `<세션 ID>.json`으로 저장한다. 같은 이름이 있으면 `-2`, `-3`…을 붙인다. 저장한 경로를 돌려준다.
pub fn save_result(dir: &Path, result: &ScenarioResult) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let mut path = dir.join(format!("{}.json", result.session_id));
    let mut number = 2;
    while path.exists() {
        path = dir.join(format!("{}-{number}.json", result.session_id));
        number += 1;
    }
    fs::write(&path, result.to_json())?;
    Ok(path)
}

/// 재현 검사 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayReport {
    /// 다시 돌린 결과(`success` 등).
    pub outcome: String,
    /// 결과 파일의 결과·사건 기록과 같은지.
    pub matches: bool,
    /// 다르면 처음 다른 곳 설명.
    pub difference: Option<String>,
}

/// 결과 파일(JSON 문자열)의 시나리오 내용과 입력 열을 다시 돌려 같은 결과·사건 기록이 나오는지 확인한다.
/// `build_course`는 코스 이름(`training`, `map:<파일>` 등)으로 코스를 만든다. 차량 설정은 기록 때와 같아야 한다.
pub fn replay_check(
    json: &str,
    build_course: impl Fn(&str) -> Result<CourseDef, String>,
    config: &VehicleConfig,
) -> Result<ReplayReport, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("JSON을 읽을 수 없습니다: {error}"))?;
    let field = |name: &str| {
        value
            .get(name)
            .ok_or_else(|| format!("결과 파일에 {name}이 없습니다"))
    };
    if field("format")?.as_str() != Some(RESULT_FORMAT) {
        return Err(format!("format이 {RESULT_FORMAT}이 아닙니다"));
    }
    let text = field("scenario_text")?
        .as_str()
        .ok_or("scenario_text가 문자열이 아닙니다")?;
    let outcome = field("outcome")?
        .as_str()
        .ok_or("outcome이 문자열이 아닙니다")?;
    let track: Vec<TrackEntry> = serde_json::from_value(field("input_track")?.clone())
        .map_err(|error| format!("input_track을 읽을 수 없습니다: {error}"))?;
    let def = parse_scenario(text).map_err(|error| error.to_string())?;
    let course = build_course(&def.course)?;
    let (mut run, mut drive) =
        ScenarioRun::new(def, course, config).map_err(|error| error.to_string())?;
    run.control(Control::Start, &mut drive)
        .map_err(|_| "시나리오를 시작할 수 없습니다")?;
    'track: for (index, entry) in track.iter().enumerate() {
        for tick in 0..entry.ticks {
            let mut device = DeviceTick {
                input: Some(entry.input()),
                ..DeviceTick::default()
            };
            if tick == 0 {
                for name in &entry.commands {
                    match parse_track_command(name) {
                        Some(TrackCommand::Queue(command)) => drive
                            .enqueue(command)
                            .map_err(|_| format!("input_track[{index}]: 명령이 너무 많습니다"))?,
                        Some(TrackCommand::Device(request)) => device.requests.push(request),
                        None => return Err(format!("input_track[{index}]: 모르는 명령 {name}")),
                    }
                }
            }
            if run.tick(&mut drive, &device).is_none() {
                break 'track;
            }
        }
    }
    if run.is_running() && outcome == Outcome::Aborted.name() {
        let _ = run.control(Control::End, &mut drive);
    }
    let replayed = match run.phase() {
        Phase::Finished(outcome) => outcome.name().to_owned(),
        phase => phase.name().to_owned(),
    };
    // 숫자 표기를 맞추려고 새 기록도 JSON 글자를 거쳐 값으로 바꿔 비교한다.
    let records: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(run.records()).expect("records serialize"))
            .expect("records parse");
    let stored = field("records")?;
    let difference = if replayed != outcome {
        Some(format!("결과가 다릅니다: 파일 {outcome}, 재현 {replayed}"))
    } else {
        first_difference(stored, &records)
    };
    Ok(ReplayReport {
        outcome: replayed,
        matches: difference.is_none(),
        difference,
    })
}

/// 두 사건 기록 배열에서 처음 다른 항목을 설명한다. 같으면 `None`.
fn first_difference(stored: &serde_json::Value, replayed: &serde_json::Value) -> Option<String> {
    let (Some(stored), Some(replayed)) = (stored.as_array(), replayed.as_array()) else {
        return Some("records가 배열이 아닙니다".to_owned());
    };
    for (index, (a, b)) in stored.iter().zip(replayed).enumerate() {
        if a != b {
            return Some(format!("records[{index}]: 파일 {a}, 재현 {b}"));
        }
    }
    (stored.len() != replayed.len()).then(|| {
        format!(
            "기록 수가 다릅니다: 파일 {}, 재현 {}",
            stored.len(),
            replayed.len()
        )
    })
}

/// 1970-01-01부터의 일 수를 (년, 월, 일)로 바꾼다(그레고리력, UTC).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// 시각을 UTC (년, 월, 일, 시, 분, 초)로 나눈다. 1970년 전이면 1970-01-01 00:00:00으로 본다.
fn utc_parts(time: SystemTime) -> (i64, u32, u32, u64, u64, u64) {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let (year, month, day) = civil_from_days((seconds / 86_400) as i64);
    let rest = seconds % 86_400;
    (year, month, day, rest / 3_600, rest % 3_600 / 60, rest % 60)
}

/// ISO 8601 UTC 표기(`2026-11-11T09:30:12Z`).
pub fn iso_time(time: SystemTime) -> String {
    let (year, month, day, hour, minute, second) = utc_parts(time);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// 파일 이름용 UTC 표기(`20261111-093012`).
fn compact_time(time: SystemTime) -> String {
    let (year, month, day, hour, minute, second) = utc_parts(time);
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::scenario::ScoreDef;

    /// 알려진 시각이 ISO·파일 이름 표기로 맞게 바뀌는지 확인한다(윤년 2월 29일 포함).
    #[test]
    fn utc_formatting() {
        let at = |seconds: u64| UNIX_EPOCH + Duration::from_secs(seconds);
        assert_eq!(iso_time(at(0)), "1970-01-01T00:00:00Z");
        // 2024-02-29T12:34:56Z
        assert_eq!(iso_time(at(1_709_210_096)), "2024-02-29T12:34:56Z");
        // 2026-11-11T09:30:12Z
        assert_eq!(compact_time(at(1_794_389_412)), "20261111-093012");
    }

    /// 감점 합이 100에서 빠지고, 성공이 아니면 점수와 상관없이 임시 합격이 아닌지 확인한다.
    #[test]
    fn score_deducts_and_requires_success() {
        let summary = Summary {
            gear_errors: 1,
            harsh_inputs: 2,
            collisions: 1,
            stop_errors_m: vec![0.5],
            reaction_s: vec![2.0],
            ..Summary::default()
        };
        let score = provisional_score(&summary, Outcome::Success, &ScoreDef::default());
        // 5 + 10 + 20 + 5 + 5 = 45
        assert!((score.value - 55.0).abs() < 1e-4, "{score:?}");
        assert!(!score.passed);
        assert_eq!(score.deductions.len(), 5);
        assert_eq!(score.notice, SCORE_NOTICE);

        let clean = provisional_score(&Summary::default(), Outcome::Success, &ScoreDef::default());
        assert_eq!((clean.value, clean.passed), (100.0, true));
        let aborted =
            provisional_score(&Summary::default(), Outcome::Aborted, &ScoreDef::default());
        assert!(!aborted.passed);
    }

    /// 제동하지 않은 위험은 등장부터 끝까지를 반응 시간으로 보고 놓친 수로 센다.
    #[test]
    fn missed_reaction_counts_until_end() {
        let records = vec![RecordEvent::new(
            64,
            RecordKind::HazardAppeared {
                hazard: "p".to_owned(),
                x_m: 0.0,
                z_m: 0.0,
                speed_kmh: 10.0,
                distance_m: 12.0,
            },
        )];
        let summary = summarize(&records, 64 * 4);
        assert_eq!(summary.missed_reactions, 1);
        assert_eq!(summary.reaction_s, vec![3.0]);
    }
}
