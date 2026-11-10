//! 저장소의 시나리오 A·B·C를 스크립트 운전자로 끝까지 몰아 보는 시험(Phase 5-2~5-5).
//!
//! 운전자는 단계 이름을 보고 명령을 한 번씩 넣고, 차체 중심이 경로점을 따라가도록 조향하며, 정지 지점까지 남은
//! 거리로 제동을 시작한다. 사람의 조작을 흉내 낸 것이 아니라 시나리오가 끝까지 풀리는지와 기록·재현을 보는 용도다.

use std::path::Path;

use glam::Vec2;

use super::record::RecordKind;
use super::result::{ScenarioResult, replay_check};
use super::run::{Control, Outcome, Phase, ScenarioRun};
use super::{Condition, load_scenario};
use crate::config::VehicleConfig;
use crate::session::{DeviceTick, DriveCommand, DriveSession};
use crate::vehicle::collision::VEHICLE_FOOTPRINT;
use crate::vehicle::state::{Gear, VehicleCommand, VehicleInput};
use crate::world::CourseId;

/// 시험 한 번의 최대 틱 수(64 Hz로 5분).
const MAX_TICKS: usize = 64 * 300;

/// 저장소 시나리오 파일 경로(저장소 루트 기준)와 내용.
fn scenario_file(name: &str) -> (String, String) {
    let path = format!("assets/scenarios/{name}");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join(&path)).unwrap();
    (path, text)
}

/// 저장소 시나리오 파일로 실행 상태와 세션을 만들고 시작한다.
fn start(name: &str) -> (ScenarioRun, DriveSession) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let def = load_scenario(&root.join("assets/scenarios").join(name)).unwrap();
    let course = CourseId::parse(&def.course).unwrap().build();
    let (mut run, mut drive) = ScenarioRun::new(def, course, &VehicleConfig::default()).unwrap();
    run.control(Control::Start, &mut drive).unwrap();
    (run, drive)
}

/// 차체 중심.
fn center(drive: &DriveSession) -> Vec2 {
    VEHICLE_FOOTPRINT.rect_at(drive.motion()).center
}

/// 차체 중심이 `target`을 향하도록 조향 입력(-1..=1, 오른쪽 양수)을 만든다.
fn steer_toward(drive: &DriveSession, target: Vec2) -> f32 {
    let motion = drive.motion();
    let (sin, cos) = motion.heading_rad.sin_cos();
    let right = Vec2::new(cos, -sin);
    let to = (target - center(drive)).normalize_or_zero();
    (2.5 * right.dot(to)).clamp(-1.0, 1.0)
}

/// 경로점 목록에서 차체 중심보다 `lookahead` m 앞(진행 방향 -Z 기준 z가 더 작은)의 첫 점을 고른다.
fn lookahead(path: &[Vec2], drive: &DriveSession, lookahead: f32) -> Vec2 {
    let z = center(drive).y;
    *path
        .iter()
        .find(|point| point.y < z - lookahead)
        .unwrap_or(path.last().unwrap())
}

/// 전진하며 경로를 따라가다 `stop`(차체 중심)에서 멈추는 입력. `cruise_mps` 아래면 가속한다.
fn follow(
    drive: &DriveSession,
    path: &[Vec2],
    stop: Option<Vec2>,
    cruise_mps: f32,
) -> VehicleInput {
    let speed = drive.state().speed_mps();
    let steering = steer_toward(drive, lookahead(path, drive, 2.5));
    let braking = stop.is_some_and(|stop| {
        let left = center(drive).y - stop.y;
        left <= speed * speed / (2.0 * 2.4) + 0.1
    });
    if braking {
        VehicleInput {
            brake: 1.0,
            steering,
            ..VehicleInput::default()
        }
    } else {
        VehicleInput {
            throttle: if speed < cruise_mps { 0.6 } else { 0.0 },
            steering,
            ..VehicleInput::default()
        }
    }
}

/// 단계마다 한 번만 명령을 넣기 위한 기록.
struct Once(Option<String>);

impl Once {
    /// 이 단계에서 처음이면 `command`를 대기열에 넣는다.
    fn command(&mut self, step: &str, drive: &mut DriveSession, command: VehicleCommand) {
        if self.0.as_deref() != Some(step) {
            self.0 = Some(step.to_owned());
            drive.enqueue(DriveCommand::Vehicle(command)).unwrap();
        }
    }
}

/// 스크립트 운전자로 시나리오가 끝날 때까지(또는 [`MAX_TICKS`]) 돌린다. `script`는 단계 이름과 세션으로 입력을 정한다.
fn drive_until_finished(
    run: &mut ScenarioRun,
    drive: &mut DriveSession,
    mut script: impl FnMut(&str, &ScenarioRun, &mut DriveSession) -> VehicleInput,
) {
    for _ in 0..MAX_TICKS {
        let step = run
            .current_step()
            .map_or(String::new(), |step| step.id.clone());
        let input = script(&step, run, drive);
        drive.set_manual_input(input);
        if run.tick(drive, &DeviceTick::default()).is_none() {
            return;
        }
    }
    panic!(
        "시나리오가 끝나지 않음: 단계 {:?}, 중심 {:?}",
        run.current_step().map(|step| &step.id),
        center(drive)
    );
}

/// 시나리오 A 스크립트: 직진해 정지선 앞에 멈추고 R로 8 m 후진, 멈춰서 P·시동 OFF.
fn script_a(
    once: &mut Once,
) -> impl FnMut(&str, &ScenarioRun, &mut DriveSession) -> VehicleInput + '_ {
    let straight = [Vec2::new(0.0, -40.0)];
    let stop_line = Vec2::new(0.0, -17.9);
    move |step, _, drive| match step {
        "engine" | "engine-off" => {
            once.command(step, drive, VehicleCommand::ToggleEngine);
            VehicleInput::default()
        }
        "drive" => {
            once.command(step, drive, VehicleCommand::RequestGear(Gear::Drive));
            VehicleInput::default()
        }
        "go" | "stop-line" => follow(drive, &straight, Some(stop_line), 2.5),
        "reverse" => {
            once.command(step, drive, VehicleCommand::RequestGear(Gear::Reverse));
            VehicleInput {
                brake: 1.0,
                ..VehicleInput::default()
            }
        }
        "back" => VehicleInput {
            throttle: 0.5,
            ..VehicleInput::default()
        },
        "park" => {
            once.command(step, drive, VehicleCommand::RequestGear(Gear::Park));
            VehicleInput {
                brake: 1.0,
                ..VehicleInput::default()
            }
        }
        _ => VehicleInput {
            brake: 1.0,
            ..VehicleInput::default()
        },
    }
}

/// 시나리오 A를 끝까지 하면 성공하고, 정지 위치 오차가 기록되며, 기어 오류·과도한 입력·충돌이 없는지 확인한다.
#[test]
fn scenario_a_completes_and_records_stop_position() {
    let (mut run, mut drive) = start("scenario-a-basic.toml");
    let mut once = Once(None);
    drive_until_finished(&mut run, &mut drive, script_a(&mut once));
    assert_eq!(run.phase(), Phase::Finished(Outcome::Success));
    assert!(run.reversed_m() >= 8.0);
    let stop = run
        .records()
        .iter()
        .find_map(|event| match &event.kind {
            RecordKind::StopPosition { step, error_m, .. } => Some((step.clone(), *error_m)),
            _ => None,
        })
        .expect("정지 위치 기록");
    assert_eq!(stop.0, "stop-line");
    assert!(stop.1 <= 1.5, "{stop:?}");
    let unexpected: Vec<_> = run
        .records()
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                RecordKind::GearError { .. }
                    | RecordKind::OverSpeed { .. }
                    | RecordKind::HarshBrake { .. }
                    | RecordKind::HarshSteer { .. }
                    | RecordKind::Collision { .. }
            )
        })
        .collect();
    assert!(unexpected.is_empty(), "{unexpected:?}");
    assert!(matches!(
        run.records().last().map(|event| &event.kind),
        Some(RecordKind::Finished { outcome }) if outcome == "success"
    ));
}

/// 움직이는 중 R을 넣으면 기어 오류로, 과속·급제동은 과도한 입력으로 기록되는지 확인한다(시나리오 A).
#[test]
fn scenario_a_records_gear_error_and_harsh_input() {
    let (mut run, mut drive) = start("scenario-a-basic.toml");
    for command in [
        VehicleCommand::ToggleEngine,
        VehicleCommand::RequestGear(Gear::Drive),
    ] {
        drive.enqueue(DriveCommand::Vehicle(command)).unwrap();
    }
    drive.set_manual_input(VehicleInput {
        throttle: 1.0,
        ..VehicleInput::default()
    });
    // 13 km/h를 넘을 때까지 가속(과속 기준 12 km/h).
    while drive.state().speed_mps() * 3.6 < 13.0 {
        run.tick(&mut drive, &DeviceTick::default()).unwrap();
    }
    drive
        .enqueue(DriveCommand::Vehicle(VehicleCommand::RequestGear(
            Gear::Reverse,
        )))
        .unwrap();
    drive.set_manual_input(VehicleInput {
        brake: 1.0,
        ..VehicleInput::default()
    });
    run.tick(&mut drive, &DeviceTick::default()).unwrap();
    let kinds: Vec<&RecordKind> = run.records().iter().map(|event| &event.kind).collect();
    assert!(
        kinds
            .iter()
            .any(|kind| matches!(kind, RecordKind::OverSpeed { .. })),
        "{kinds:?}"
    );
    assert!(
        kinds.iter().any(|kind| matches!(
            kind,
            RecordKind::GearError { command, reason }
                if command == "gear_reverse" && reason == "vehicle must be fully stopped"
        )),
        "{kinds:?}"
    );
    assert!(
        kinds
            .iter()
            .any(|kind| matches!(kind, RecordKind::HarshBrake { .. })),
        "{kinds:?}"
    );
}

/// 시나리오 B 경로(차체 중심): 원뿔 왼쪽 → 문 가운데 → 통로 가운데 → 정차 구역.
fn path_b() -> Vec<Vec2> {
    [
        (0.0, -6.0),
        (-1.0, -13.0),
        (-1.0, -21.0),
        (0.25, -27.0),
        (0.25, -37.0),
        (0.0, -41.0),
        (0.0, -70.0),
    ]
    .into_iter()
    .map(|(x, z)| Vec2::new(x, z))
    .collect()
}

/// 시나리오 B를 경로대로 지나면 접촉 없이 성공하고, 시나리오 원뿔이 충돌 경계로 들어가 있는지 확인한다.
#[test]
fn scenario_b_completes_without_contact() {
    let (mut run, mut drive) = start("scenario-b-space.toml");
    assert_eq!(
        run.course().colliders.len(),
        CourseId::Training.build().colliders.len() + 2
    );
    let path = path_b();
    let mut once = Once(None);
    drive_until_finished(&mut run, &mut drive, |step, _, drive| match step {
        "drive" => {
            once.command(step, drive, VehicleCommand::RequestGear(Gear::Drive));
            VehicleInput::default()
        }
        "park" => {
            once.command(step, drive, VehicleCommand::RequestGear(Gear::Park));
            VehicleInput {
                brake: 1.0,
                ..VehicleInput::default()
            }
        }
        _ => follow(drive, &path, Some(Vec2::new(0.0, -60.0)), 1.6),
    });
    assert_eq!(run.phase(), Phase::Finished(Outcome::Success));
    assert_eq!(run.collisions(), 0, "{:?}", run.records());
}

/// 시나리오 B에서 원뿔로 똑바로 가면 접촉이 기록되는지 확인한다.
#[test]
fn scenario_b_records_cone_contact() {
    let (mut run, mut drive) = start("scenario-b-space.toml");
    drive
        .enqueue(DriveCommand::Vehicle(VehicleCommand::RequestGear(
            Gear::Drive,
        )))
        .unwrap();
    let straight = [Vec2::new(0.0, -40.0)];
    for _ in 0..64 * 30 {
        let input = follow(&drive, &straight, None, 1.5);
        drive.set_manual_input(input);
        run.tick(&mut drive, &DeviceTick::default()).unwrap();
        if run.collisions() > 0 {
            break;
        }
    }
    let contact = run
        .records()
        .iter()
        .find_map(|event| match &event.kind {
            RecordKind::Collision { hazard, z_m, .. } => Some((hazard.clone(), *z_m)),
            _ => None,
        })
        .expect("원뿔 접촉 기록");
    assert_eq!(contact.0, None);
    // 원뿔(z = -17.5..-18.5) 앞에서 막힘: 후륜축 z = 원뿔 앞면 + 차체 앞쪽 3.5 m 근처.
    assert!((contact.1 - (-17.5 + 3.5)).abs() < 0.2, "{contact:?}");
}

/// 시나리오 C 스크립트: 등장을 보면 `reaction_ticks` 뒤 제동해 멈추고, 보행자가 사라지면 다시 가서 정차한다.
/// `reaction_ticks`가 `None`이면 제동하지 않고 그대로 간다.
fn run_c(reaction_ticks: Option<u64>) -> ScenarioRun {
    let (mut run, mut drive) = start("scenario-c-hazard.toml");
    let straight = [Vec2::new(0.0, -50.0)];
    let mut once = Once(None);
    let mut seen_at: Option<u64> = None;
    drive_until_finished(&mut run, &mut drive, |step, run, drive| {
        if seen_at.is_none() && !run.visible_hazards().is_empty() {
            seen_at = Some(run.ticks());
        }
        match step {
            "engine" => {
                once.command(step, drive, VehicleCommand::ToggleEngine);
                VehicleInput::default()
            }
            "drive" => {
                once.command(step, drive, VehicleCommand::RequestGear(Gear::Drive));
                VehicleInput::default()
            }
            "park" => {
                once.command(step, drive, VehicleCommand::RequestGear(Gear::Park));
                VehicleInput {
                    brake: 1.0,
                    ..VehicleInput::default()
                }
            }
            "go" | "yield" => match (seen_at, reaction_ticks) {
                (Some(seen), Some(delay)) if run.ticks() >= seen + delay => VehicleInput {
                    brake: 1.0,
                    ..VehicleInput::default()
                },
                _ => follow(drive, &straight, None, 4.0),
            },
            _ if !run.visible_hazards().is_empty() => VehicleInput {
                brake: 1.0,
                ..VehicleInput::default()
            },
            _ => follow(drive, &straight, Some(Vec2::new(0.0, -37.0)), 2.5),
        }
    });
    run
}

/// 시나리오 C에서 0.5 s 뒤 제동하면 보행자와 닿지 않고 성공하며, 등장·제동 시작(반응 0.5 s)이 기록되는지 확인한다.
#[test]
fn scenario_c_reaction_is_recorded_and_avoids_pedestrian() {
    let run = run_c(Some(32));
    assert_eq!(
        run.phase(),
        Phase::Finished(Outcome::Success),
        "{:?}",
        run.records()
    );
    assert_eq!(run.collisions(), 0);
    let appeared = run
        .records()
        .iter()
        .find_map(|event| match &event.kind {
            RecordKind::HazardAppeared {
                hazard,
                x_m,
                z_m,
                distance_m,
                ..
            } => Some((hazard.clone(), *x_m, *z_m, *distance_m)),
            _ => None,
        })
        .expect("등장 기록");
    assert_eq!(
        (appeared.0.as_str(), appeared.1, appeared.2),
        ("pedestrian", 4.0, -26.0)
    );
    assert!(appeared.3 > 10.0, "{appeared:?}");
    let reaction = run
        .records()
        .iter()
        .find_map(|event| match &event.kind {
            RecordKind::BrakeStart { reaction_s, .. } => Some(*reaction_s),
            _ => None,
        })
        .expect("제동 시작 기록");
    assert!((reaction - 0.5).abs() < 0.02, "{reaction}");
}

/// 같은 입력으로 두 번 하면 위험이 같은 틱·같은 자리에 나오고 기록 전체가 같은지 확인한다(결정적 스크립트).
#[test]
fn scenario_c_hazard_repeats_identically() {
    let first = run_c(Some(32));
    let second = run_c(Some(32));
    assert_eq!(first.records(), second.records());
    assert_eq!(first.track(), second.track());
}

/// 제동하지 않으면 보행자와 겹쳐 충돌로 실패하는지 확인한다.
#[test]
fn scenario_c_without_braking_hits_pedestrian() {
    let run = run_c(None);
    assert_eq!(run.phase(), Phase::Finished(Outcome::Collisions));
    assert!(run.records().iter().any(|event| matches!(
        &event.kind,
        RecordKind::Collision { hazard: Some(id), .. } if id == "pedestrian"
    )));
    assert!(
        !run.records()
            .iter()
            .any(|event| matches!(event.kind, RecordKind::BrakeStart { .. }))
    );
}

/// 결과 파일을 만들어 JSON으로 저장·재현하면 같은 결과·기록이 나오고, 기록을 바꾸면 다르다고 알려 주는지 확인한다.
#[test]
fn result_json_replays_identically() {
    for (name, run) in [
        ("scenario-c-hazard.toml", run_c(Some(32))),
        ("scenario-c-hazard.toml", run_c(None)),
    ] {
        let (path, text) = scenario_file(name);
        let result = ScenarioResult::new(&run, &path, &text).unwrap();
        assert_eq!(result.outcome, run_outcome(&run));
        assert_eq!(result.score.notice, super::result::SCORE_NOTICE);
        let json = result.to_json();
        let build = |course: &str| {
            CourseId::parse(course)
                .map(CourseId::build)
                .ok_or_else(|| format!("코스 {course}"))
        };
        let report = replay_check(&json, build, &VehicleConfig::default()).unwrap();
        assert!(report.matches, "{name}: {:?}", report.difference);

        let tampered = json.replacen("\"pedestrian\"", "\"someone\"", 1);
        let report = replay_check(&tampered, build, &VehicleConfig::default()).unwrap();
        assert!(!report.matches);
    }
}

/// 결과 이름(`success` 등).
fn run_outcome(run: &ScenarioRun) -> String {
    match run.phase() {
        Phase::Finished(outcome) => outcome.name().to_owned(),
        phase => phase.name().to_owned(),
    }
}

/// 시나리오 A 결과 파일이 저장되고, 요약·임시 점수·세션 정보가 들어 있는지 확인한다.
#[test]
fn result_file_is_saved_with_summary_and_score() {
    let (mut run, mut drive) = start("scenario-a-basic.toml");
    let mut once = Once(None);
    drive_until_finished(&mut run, &mut drive, script_a(&mut once));
    let (path, text) = scenario_file("scenario-a-basic.toml");
    let result = ScenarioResult::new(&run, &path, &text).unwrap();
    assert!(result.completed);
    assert!(
        result.session_id.ends_with("-scenario-a-basic"),
        "{}",
        result.session_id
    );
    assert_eq!(result.steps.len(), 9);
    assert!(result.steps.iter().all(|step| step.done_s.is_some()));
    assert_eq!(result.summary.stop_errors_m.len(), 1);
    assert!(result.score.passed, "{:?}", result.score);
    let dir = std::env::temp_dir().join(format!("sim-core-result-{}", std::process::id()));
    let first = super::result::save_result(&dir, &result).unwrap();
    let second = super::result::save_result(&dir, &result).unwrap();
    assert_ne!(first, second, "같은 세션 ID는 번호를 붙여 따로 저장");
    let saved: ScenarioResult =
        serde_json::from_str(&std::fs::read_to_string(&first).unwrap()).unwrap();
    assert_eq!(saved.session_id, result.session_id);
    assert_eq!(saved.input_track, result.input_track);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// 정지 위치 단계의 목표가 파일 값에서 오는지 확인한다(시나리오 A의 정지선 앞 구역).
#[test]
fn scenario_a_stop_zone_comes_from_file() {
    let (run, _) = start("scenario-a-basic.toml");
    let stop = run
        .def()
        .steps
        .iter()
        .find(|step| step.id == "stop-line")
        .unwrap();
    assert_eq!(
        stop.done,
        Condition::StopIn {
            position_m: [0.0, -17.9],
            radius_m: 1.5
        }
    );
}
