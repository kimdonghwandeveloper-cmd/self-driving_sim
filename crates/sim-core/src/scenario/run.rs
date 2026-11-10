//! 시나리오 한 번의 실행 상태(Phase 5-1): 시작 전 안내 → 진행 ⇄ 일시정지 → 끝(성공·실패·중단).
//!
//! [`ScenarioRun`]은 주행 세션([`DriveSession`])을 진행 중에만 한 틱씩 돌리고, 틱마다 현재 단계의 완료 조건·
//! 이벤트 조건·실패 조건을 판정한다. 시간은 진행 중 틱 수 × [`DT`]라서 일시정지·화면 프레임과 무관하게 재현된다.

use glam::Vec2;

use super::{Action, Condition, ScenarioDef, ScenarioError, StepDef};
use crate::config::VehicleConfig;
use crate::session::{DT, DeviceTick, DriveSession, TickReport};
use crate::vehicle::collision::VEHICLE_FOOTPRINT;
use crate::vehicle::dynamics::Motion;
use crate::vehicle::state::{EngineState, Gear, VehicleCommand};
use crate::world::CourseDef;

/// 시나리오가 끝난 방식.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 모든 단계를 끝냄.
    Success,
    /// 시간 제한을 넘김.
    TimeLimit,
    /// 허용 충돌 횟수를 넘김.
    Collisions,
    /// 사용자가 끝냄.
    Aborted,
}

impl Outcome {
    /// 계약 이름(`"success"` 등)을 반환한다.
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Success => "success",
            Outcome::TimeLimit => "time_limit",
            Outcome::Collisions => "collisions",
            Outcome::Aborted => "aborted",
        }
    }
}

/// 실행 단계. 주행 세션은 [`Phase::Running`]에서만 틱이 돈다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// 시작 전 안내. 차량은 시작 조건 그대로 멈춰 있다.
    Briefing,
    /// 진행 중.
    Running,
    /// 일시정지. 차량·시간이 멈춘다.
    Paused,
    /// 끝남. 차량은 끝난 순간 그대로 멈춘다.
    Finished(Outcome),
}

impl Phase {
    /// 계약 이름(`"briefing"`, `"running"`, `"paused"`, `"finished"`)을 반환한다.
    pub fn name(self) -> &'static str {
        match self {
            Phase::Briefing => "briefing",
            Phase::Running => "running",
            Phase::Paused => "paused",
            Phase::Finished(_) => "finished",
        }
    }
}

/// 사용자 조작(시작·일시정지·계속·재시작·종료).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// 시작 전 안내 → 진행.
    Start,
    /// 진행 → 일시정지.
    Pause,
    /// 일시정지 → 진행.
    Resume,
    /// 어느 단계에서든 시작 조건으로 되돌리고 시작 전 안내로.
    Restart,
    /// 끝나지 않았으면 중단으로 끝낸다.
    End,
}

impl Control {
    /// 계약 이름(`"start"` 등)을 조작으로 바꾼다. 모르는 이름이면 `None`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "start" => Some(Control::Start),
            "pause" => Some(Control::Pause),
            "resume" => Some(Control::Resume),
            "restart" => Some(Control::Restart),
            "end" => Some(Control::End),
            _ => None,
        }
    }
}

/// 지금 단계에서 받을 수 없는 조작.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlRejected;

/// 틱 하나에서 시나리오에 일어난 일(처리 순서대로).
#[derive(Debug, Clone, PartialEq)]
pub enum ScenarioUpdate {
    /// 단계(번호)를 끝냄.
    StepDone(usize),
    /// 메시지 이벤트가 일어남.
    Message(String),
    /// 시나리오가 끝남.
    Finished(Outcome),
}

/// 시나리오 한 번의 실행 상태.
#[derive(Debug, Clone)]
pub struct ScenarioRun {
    def: ScenarioDef,
    /// 시작 자세를 반영한 코스. 재시작 때 이 코스로 주행 세션을 새로 만든다.
    course: CourseDef,
    config: VehicleConfig,
    phase: Phase,
    /// 진행 중 처리한 틱 수.
    ticks: u64,
    /// 시작 뒤 움직인 거리(m, 속도 크기 적분).
    travel_m: f32,
    /// 시작 뒤 새로 시작된 충돌 횟수.
    collisions: u32,
    /// 지금 단계 번호. 모든 단계를 끝내면 단계 수와 같다.
    step: usize,
    /// 끝낸 단계마다 끝낸 틱.
    step_done_ticks: Vec<u64>,
    /// 이벤트마다 이미 일어났는지.
    fired: Vec<bool>,
    /// 띄우고 있는 메시지와 사라질 틱.
    message: Option<(String, u64)>,
}

impl ScenarioRun {
    /// 검사를 마친 시나리오와 코스로 실행 상태와 시작 조건을 맞춘 주행 세션을 만든다(시작 전 안내 단계).
    /// 목표가 필요한 조건인데 코스에 목표가 없거나 시작 조건을 적용하지 못하면 오류다.
    pub fn new(
        def: ScenarioDef,
        mut course: CourseDef,
        config: &VehicleConfig,
    ) -> Result<(Self, DriveSession), ScenarioError> {
        if def.needs_goal() && course.goal.is_none() {
            return Err(ScenarioError(format!(
                "코스 \"{}\"에 목표가 없어 stop_at_goal 조건을 쓸 수 없습니다",
                def.course
            )));
        }
        if let Some([x_m, z_m]) = def.start.position_m {
            course.start = Motion {
                x_m,
                z_m,
                heading_rad: def.start.heading_deg.unwrap_or(0.0).to_radians(),
                ..Motion::default()
            };
        }
        let run = Self {
            fired: vec![false; def.events.len()],
            def,
            course,
            config: config.clone(),
            phase: Phase::Briefing,
            ticks: 0,
            travel_m: 0.0,
            collisions: 0,
            step: 0,
            step_done_ticks: Vec::new(),
            message: None,
        };
        let drive = run.fresh_session()?;
        Ok((run, drive))
    }

    /// 시작 조건(시동·기어·자세)을 맞춘 새 주행 세션을 만든다.
    fn fresh_session(&self) -> Result<DriveSession, ScenarioError> {
        let mut drive = DriveSession::new(self.course.clone(), &self.config);
        let mut commands = Vec::new();
        if self.def.start.engine_on {
            commands.push(VehicleCommand::ToggleEngine);
        }
        let gear = self.def.start.gear.gear();
        if gear != Gear::Park {
            commands.push(VehicleCommand::RequestGear(gear));
        }
        for command in commands {
            drive.apply_before_start(command).map_err(|error| {
                ScenarioError(format!("시작 조건을 적용하지 못했습니다: {error}"))
            })?;
        }
        Ok(drive)
    }

    /// 시나리오 정의를 반환한다.
    pub fn def(&self) -> &ScenarioDef {
        &self.def
    }

    /// 실행 단계를 반환한다.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 진행 중이면 `true`(주행 세션이 틱을 도는 단계).
    pub fn is_running(&self) -> bool {
        self.phase == Phase::Running
    }

    /// 경과 시간(s, 일시정지 제외).
    pub fn elapsed_s(&self) -> f32 {
        self.ticks as f32 * DT
    }

    /// 시작 뒤 움직인 거리(m).
    pub fn travel_m(&self) -> f32 {
        self.travel_m
    }

    /// 시작 뒤 충돌 횟수.
    pub fn collisions(&self) -> u32 {
        self.collisions
    }

    /// 끝낸 단계 수.
    pub fn steps_done(&self) -> usize {
        self.step
    }

    /// 끝낸 단계마다 끝낸 시각(s, 경과 시간 기준).
    pub fn step_done_times_s(&self) -> Vec<f32> {
        self.step_done_ticks
            .iter()
            .map(|tick| *tick as f32 * DT)
            .collect()
    }

    /// 지금 해야 할 단계. 모든 단계를 끝냈으면 `None`.
    pub fn current_step(&self) -> Option<&StepDef> {
        self.def.steps.get(self.step)
    }

    /// 지금 띄우고 있는 메시지.
    pub fn message(&self) -> Option<&str> {
        self.message
            .as_ref()
            .filter(|(_, until)| self.ticks < *until)
            .map(|(text, _)| text.as_str())
    }

    /// 사용자 조작을 바로 적용한다. 재시작은 `drive`를 시작 조건의 새 세션으로 바꾼다.
    /// 지금 단계에서 받을 수 없는 조작이면 아무것도 바꾸지 않는다.
    pub fn control(
        &mut self,
        control: Control,
        drive: &mut DriveSession,
    ) -> Result<(), ControlRejected> {
        self.phase = match (control, self.phase) {
            (Control::Start, Phase::Briefing) | (Control::Resume, Phase::Paused) => Phase::Running,
            (Control::Pause, Phase::Running) => Phase::Paused,
            (Control::End, Phase::Briefing | Phase::Running | Phase::Paused) => {
                Phase::Finished(Outcome::Aborted)
            }
            (Control::Restart, _) => {
                // 시작 조건은 생성 때 한 번 적용해 봤으므로 다시 실패하지 않는다.
                *drive = self.fresh_session().map_err(|_| ControlRejected)?;
                self.ticks = 0;
                self.travel_m = 0.0;
                self.collisions = 0;
                self.step = 0;
                self.step_done_ticks.clear();
                self.fired.fill(false);
                self.message = None;
                Phase::Briefing
            }
            _ => return Err(ControlRejected),
        };
        Ok(())
    }

    /// 진행 중이면 주행 세션을 한 틱 돌리고 단계·이벤트·끝 조건을 판정한다. 진행 중이 아니면 `None`(세션은 그대로).
    ///
    /// 판정 순서: 충돌·거리 누적 → 단계 완료(같은 틱에 여러 단계 가능) → 이벤트 → 충돌 초과 실패 → 성공 → 시간 초과 실패.
    pub fn tick(
        &mut self,
        drive: &mut DriveSession,
        device: &DeviceTick,
    ) -> Option<(TickReport, Vec<ScenarioUpdate>)> {
        if !self.is_running() {
            return None;
        }
        let report = drive.tick(device);
        let mut updates = Vec::new();
        self.ticks += 1;
        self.travel_m += drive.state().speed_mps().abs() * DT;
        if report.collided.is_some() {
            self.collisions += 1;
        }
        while let Some(step) = self.def.steps.get(self.step) {
            if !self.holds(&step.done, drive) {
                break;
            }
            self.step_done_ticks.push(self.ticks);
            updates.push(ScenarioUpdate::StepDone(self.step));
            self.step += 1;
        }
        for index in 0..self.def.events.len() {
            if self.fired[index] || !self.holds(&self.def.events[index].when, drive) {
                continue;
            }
            self.fired[index] = true;
            match &self.def.events[index].action {
                Action::Message { text, duration_s } => {
                    let duration_ticks = (duration_s / DT).round().max(1.0) as u64;
                    self.message = Some((text.clone(), self.ticks + duration_ticks));
                    updates.push(ScenarioUpdate::Message(text.clone()));
                }
            }
        }
        let outcome = if self
            .def
            .end
            .max_collisions
            .is_some_and(|max| self.collisions > max)
        {
            Some(Outcome::Collisions)
        } else if self.step == self.def.steps.len() {
            Some(Outcome::Success)
        } else if self
            .def
            .end
            .time_limit_s
            .is_some_and(|limit| self.elapsed_s() >= limit)
        {
            Some(Outcome::TimeLimit)
        } else {
            None
        };
        if let Some(outcome) = outcome {
            self.phase = Phase::Finished(outcome);
            updates.push(ScenarioUpdate::Finished(outcome));
        }
        Some((report, updates))
    }

    /// 조건이 지금(틱 끝) 맞는지 판정한다. 위치는 차체 중심 기준이다.
    fn holds(&self, condition: &Condition, drive: &DriveSession) -> bool {
        let state = drive.state();
        let center = VEHICLE_FOOTPRINT.rect_at(drive.motion()).center;
        let within = |position_m: &[f32; 2], radius_m: f32| {
            center.distance(Vec2::from(*position_m)) <= radius_m
        };
        match condition {
            Condition::EngineOn {} => state.engine() == EngineState::Ready,
            Condition::Gear { gear } => state.gear() == gear.gear(),
            Condition::Stopped {} => state.is_stopped(),
            Condition::SpeedAtLeast { kmh } => state.speed_mps().abs() * 3.6 >= *kmh,
            Condition::Travel { distance_m } => self.travel_m >= *distance_m,
            Condition::Reach {
                position_m,
                radius_m,
            } => within(position_m, *radius_m),
            Condition::StopIn {
                position_m,
                radius_m,
            } => state.is_stopped() && within(position_m, *radius_m),
            Condition::StopAtGoal { radius_m } => {
                state.is_stopped()
                    && self.course.goal.is_some_and(|goal| {
                        center.distance(VEHICLE_FOOTPRINT.rect_at(&goal).center) <= *radius_m
                    })
            }
            Condition::Elapsed { s } => self.elapsed_s() >= *s,
            Condition::StepDone { step } => self.def.steps[..self.step]
                .iter()
                .any(|done| &done.id == step),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::parse_scenario;
    use crate::scenario::tests::SAMPLE;
    use crate::vehicle::state::VehicleInput;
    use crate::world::CourseId;

    /// 시나리오 문자열로 training 코스 실행 상태와 세션을 만든다.
    fn start(text: &str) -> (ScenarioRun, DriveSession) {
        let def = parse_scenario(text).unwrap();
        let course = CourseId::parse(&def.course).unwrap().build();
        ScenarioRun::new(def, course, &VehicleConfig::default()).unwrap()
    }

    /// 수동 입력만 쓰는 장치 틱(장치 입력 없음).
    fn idle() -> DeviceTick {
        DeviceTick::default()
    }

    /// 진행 중인 동안 최대 `limit` 틱을 돌리고 일어난 일을 모은다.
    fn run_ticks(
        run: &mut ScenarioRun,
        drive: &mut DriveSession,
        limit: usize,
    ) -> Vec<ScenarioUpdate> {
        let mut updates = Vec::new();
        for _ in 0..limit {
            match run.tick(drive, &idle()) {
                Some((_, mut more)) => updates.append(&mut more),
                None => break,
            }
        }
        updates
    }

    /// 시작 조건(시동·D)이 적용되고, 시작 전에는 틱이 돌지 않는지 확인한다.
    #[test]
    fn briefing_applies_start_and_holds_still() {
        let (mut run, mut drive) = start(SAMPLE);
        assert_eq!(run.phase(), Phase::Briefing);
        assert!(drive.engine_on());
        assert_eq!(drive.state().gear(), Gear::Drive);
        drive.set_manual_input(VehicleInput {
            throttle: 1.0,
            ..VehicleInput::default()
        });
        assert!(run.tick(&mut drive, &idle()).is_none());
        assert_eq!(drive.tick_count(), 0);
        assert_eq!(run.elapsed_s(), 0.0);
    }

    /// 가속해 5 m 지나 첫 단계·이벤트, 제동 정지로 둘째 단계를 끝내 성공하는지 확인한다.
    #[test]
    fn steps_and_events_lead_to_success() {
        let (mut run, mut drive) = start(SAMPLE);
        run.control(Control::Start, &mut drive).unwrap();
        drive.set_manual_input(VehicleInput {
            throttle: 0.5,
            ..VehicleInput::default()
        });
        let mut updates = Vec::new();
        while run.steps_done() == 0 {
            updates.extend(run_ticks(&mut run, &mut drive, 1));
            assert!(run.elapsed_s() < 20.0, "5 m에 닿지 못함");
        }
        assert!(run.travel_m() >= 5.0);
        assert_eq!(
            updates,
            vec![
                ScenarioUpdate::StepDone(0),
                ScenarioUpdate::Message("잘했습니다".to_owned())
            ]
        );
        assert_eq!(run.message(), Some("잘했습니다"));
        assert_eq!(run.current_step().unwrap().id, "stop");
        drive.set_manual_input(VehicleInput {
            brake: 1.0,
            ..VehicleInput::default()
        });
        let updates = run_ticks(&mut run, &mut drive, 64 * 10);
        assert_eq!(
            updates,
            vec![
                ScenarioUpdate::StepDone(1),
                ScenarioUpdate::Finished(Outcome::Success)
            ]
        );
        assert_eq!(run.phase(), Phase::Finished(Outcome::Success));
        assert_eq!(run.step_done_times_s().len(), 2);
        let ticks = drive.tick_count();
        assert!(run.tick(&mut drive, &idle()).is_none(), "끝난 뒤에는 멈춤");
        assert_eq!(drive.tick_count(), ticks);
    }

    /// 일시정지 동안 차량·시간·메시지가 멈추고 계속하면 이어지는지 확인한다.
    #[test]
    fn pause_freezes_vehicle_and_clock() {
        let (mut run, mut drive) = start(SAMPLE);
        run.control(Control::Start, &mut drive).unwrap();
        drive.set_manual_input(VehicleInput {
            throttle: 0.5,
            ..VehicleInput::default()
        });
        run_ticks(&mut run, &mut drive, 64);
        run.control(Control::Pause, &mut drive).unwrap();
        let (motion, elapsed, travel) = (*drive.motion(), run.elapsed_s(), run.travel_m());
        for _ in 0..100 {
            assert!(run.tick(&mut drive, &idle()).is_none());
        }
        assert_eq!(*drive.motion(), motion);
        assert_eq!(run.elapsed_s(), elapsed);
        assert_eq!(run.travel_m(), travel);
        assert_eq!(
            run.control(Control::Pause, &mut drive),
            Err(ControlRejected)
        );
        assert_eq!(
            run.control(Control::Start, &mut drive),
            Err(ControlRejected)
        );
        run.control(Control::Resume, &mut drive).unwrap();
        run_ticks(&mut run, &mut drive, 1);
        assert!(run.travel_m() > travel);
    }

    /// 재시작하면 차량·시간·단계·이벤트가 처음으로 돌아가고 같은 입력이 같은 결과를 내는지 확인한다.
    #[test]
    fn restart_repeats_identically() {
        let (mut run, mut drive) = start(SAMPLE);
        let drive_once = |run: &mut ScenarioRun, drive: &mut DriveSession| {
            run.control(Control::Start, drive).unwrap();
            drive.set_manual_input(VehicleInput {
                throttle: 0.7,
                steering: 0.1,
                ..VehicleInput::default()
            });
            let updates = run_ticks(run, drive, 200);
            (updates, *drive.motion(), run.travel_m())
        };
        let first = drive_once(&mut run, &mut drive);
        run.control(Control::Restart, &mut drive).unwrap();
        assert_eq!(run.phase(), Phase::Briefing);
        assert_eq!(
            (run.steps_done(), run.elapsed_s(), run.message()),
            (0, 0.0, None)
        );
        assert_eq!(drive.tick_count(), 0);
        assert_eq!(drive.state().gear(), Gear::Drive);
        let second = drive_once(&mut run, &mut drive);
        assert_eq!(first, second);
    }

    /// 사용자가 끝내면 중단으로 끝나고, 끝난 뒤에는 재시작 말고는 받지 않는지 확인한다.
    #[test]
    fn end_aborts_and_only_restart_follows() {
        let (mut run, mut drive) = start(SAMPLE);
        run.control(Control::End, &mut drive).unwrap();
        assert_eq!(run.phase(), Phase::Finished(Outcome::Aborted));
        for control in [
            Control::Start,
            Control::Pause,
            Control::Resume,
            Control::End,
        ] {
            assert_eq!(run.control(control, &mut drive), Err(ControlRejected));
        }
        run.control(Control::Restart, &mut drive).unwrap();
        assert_eq!(run.phase(), Phase::Briefing);
    }

    /// 파일의 시간 제한 값만 바꿔도(코드 변경 없음) 실패 시각이 따라 바뀌는지 확인한다.
    #[test]
    fn time_limit_comes_from_file() {
        for limit in [1.0_f32, 2.5] {
            let text =
                SAMPLE.replacen("time_limit_s = 60.0", &format!("time_limit_s = {limit}"), 1);
            let (mut run, mut drive) = start(&text);
            run.control(Control::Start, &mut drive).unwrap();
            let updates = run_ticks(&mut run, &mut drive, 64 * 10);
            assert_eq!(updates, vec![ScenarioUpdate::Finished(Outcome::TimeLimit)]);
            assert!((run.elapsed_s() - limit).abs() <= DT, "{limit}");
        }
    }

    /// 허용 충돌 수를 넘으면 실패하는지 확인한다(training 앞쪽 끝 벽에 부딪힘).
    #[test]
    fn collisions_over_limit_fail() {
        let text = SAMPLE.replacen("distance_m = 5.0", "distance_m = 500.0", 1);
        let (mut run, mut drive) =
            start(&text.replacen("time_limit_s = 60.0", "time_limit_s = 120.0", 1));
        run.control(Control::Start, &mut drive).unwrap();
        drive.set_manual_input(VehicleInput {
            throttle: 1.0,
            ..VehicleInput::default()
        });
        let updates = run_ticks(&mut run, &mut drive, 64 * 120);
        assert_eq!(run.collisions(), 1);
        assert_eq!(
            updates.last(),
            Some(&ScenarioUpdate::Finished(Outcome::Collisions))
        );
    }

    /// 시작 자세를 파일에서 바꾸면 세션이 그 자세에서 시작하고, 목표 없는 코스의 목표 조건은 거부되는지 확인한다.
    #[test]
    fn start_pose_and_goal_requirement() {
        let text = SAMPLE.replacen(
            "gear = \"D\"",
            "gear = \"D\"\nposition_m = [0.5, -3.0]\nheading_deg = 90.0",
            1,
        );
        let (_, drive) = start(&text);
        assert_eq!((drive.motion().x_m, drive.motion().z_m), (0.5, -3.0));
        assert!((drive.motion().heading_rad - std::f32::consts::FRAC_PI_2).abs() < 1e-6);

        let text = SAMPLE.replacen(
            "done = { kind = \"stopped\" }",
            "done = { kind = \"stop_at_goal\", radius_m = 3.0 }",
            1,
        );
        let def = parse_scenario(&text).unwrap();
        let error = ScenarioRun::new(def, CourseId::Training.build(), &VehicleConfig::default())
            .unwrap_err();
        assert!(error.to_string().contains("목표가 없어"), "{error}");
    }

    /// 조작 이름이 바뀌고 모르는 이름은 거부하는지 확인한다.
    #[test]
    fn control_names_parse() {
        assert_eq!(Control::parse("pause"), Some(Control::Pause));
        assert_eq!(Control::parse("stop"), None);
    }
}
