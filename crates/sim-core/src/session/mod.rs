//! 차량 한 대의 고정 간격 주행 세션(Godot 인터페이스 계약 v1 5-1의 3~7단계).
//!
//! Godot `SimVehicle`과 재생 시험 `SimReplay`가 같은 코드를 쓰도록, 명령 대기열·시작 위치 복귀·
//! 연속 입력·운동 모델·틱 번호와 틱 결과([`TickReport`])를 엔진 없이 처리한다.
//! 장치 입력 읽기(2단계)와 시그널 보내기(8단계)는 엔진 쪽이 한다.

pub mod replay;

use std::collections::VecDeque;

use crate::config::VehicleConfig;
use crate::input::ControlRequest;
use crate::vehicle::collision::{ContactState, Impact, VEHICLE_FOOTPRINT};
use crate::vehicle::collision::{ObstacleGrid, step_with_grid};
use crate::vehicle::dynamics::{DynamicsParams, Motion};
use crate::vehicle::state::{EngineState, Gear, VehicleCommand, VehicleInput, VehicleState};
use crate::world::CourseDef;

/// 고정 시간 간격(s). Bevy `FixedUpdate` 64 Hz와 같다.
pub const DT: f32 = 1.0 / 64.0;
/// `send_command` 대기열 상한(계약 2절).
pub const QUEUE_LIMIT: usize = 16;
/// 정지하지 않은 상태에서 시작 위치 복귀를 거부할 때의 사유(계약 5-1 3단계).
pub const RESET_NOT_STOPPED: &str = "vehicle must be stopped";

/// 계약의 명령 이름 하나.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveCommand {
    /// 상태 머신 명령.
    Vehicle(VehicleCommand),
    /// 정지 상태에서 코스 시작 자세로 복귀.
    ResetCourse,
}

impl DriveCommand {
    /// 계약의 모든 명령(이름 검사·문서용 순서).
    pub const ALL: [DriveCommand; 8] = [
        DriveCommand::Vehicle(VehicleCommand::RequestGear(Gear::Park)),
        DriveCommand::Vehicle(VehicleCommand::RequestGear(Gear::Reverse)),
        DriveCommand::Vehicle(VehicleCommand::RequestGear(Gear::Neutral)),
        DriveCommand::Vehicle(VehicleCommand::RequestGear(Gear::Drive)),
        DriveCommand::Vehicle(VehicleCommand::ToggleEngine),
        DriveCommand::Vehicle(VehicleCommand::EngageEmergencyStop),
        DriveCommand::Vehicle(VehicleCommand::ReleaseEmergencyStop),
        DriveCommand::ResetCourse,
    ];

    /// 계약 이름(`"gear_park"` 등)을 명령으로 바꾼다. 모르는 이름이면 `None`.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.name() == name)
    }

    /// 계약 이름을 반환한다.
    pub fn name(self) -> &'static str {
        match self {
            DriveCommand::Vehicle(VehicleCommand::RequestGear(gear)) => match gear {
                Gear::Park => "gear_park",
                Gear::Reverse => "gear_reverse",
                Gear::Neutral => "gear_neutral",
                Gear::Drive => "gear_drive",
            },
            DriveCommand::Vehicle(VehicleCommand::ToggleEngine) => "engine_toggle",
            DriveCommand::Vehicle(VehicleCommand::EngageEmergencyStop) => "emergency_stop",
            DriveCommand::Vehicle(VehicleCommand::ReleaseEmergencyStop) => "emergency_release",
            DriveCommand::ResetCourse => "reset_course",
        }
    }
}

/// 명령이 들어온 곳(계약 시그널 인자 `source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSource {
    /// `send_command`(대기열).
    Api,
    /// 키보드·게임패드.
    Device,
}

impl CommandSource {
    /// 계약 이름(`"api"`, `"device"`)을 반환한다.
    pub fn name(self) -> &'static str {
        match self {
            CommandSource::Api => "api",
            CommandSource::Device => "device",
        }
    }
}

/// 명령 하나의 처리 결과(계약 `command_applied`/`command_rejected`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    /// 계약 명령 이름. 장치의 서로 다른 기어 동시 요청은 `"gear_conflict"`다.
    pub command: String,
    pub source: CommandSource,
    pub accepted: bool,
    /// 거부 사유. 성공이면 빈 문자열.
    pub reason: String,
}

/// 엔진 쪽이 2단계에서 만든 이번 틱의 장치 입력.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceTick {
    /// 장치 연속 입력. `None`이면 마지막 수동 입력(`set_manual_input`)을 쓴다.
    pub input: Option<VehicleInput>,
    /// 합치기 규칙(`input::command_requests`)을 거친 장치 명령 요청.
    pub requests: Vec<ControlRequest>,
    /// 장치의 시작 위치 복귀 요청(비상 정지 키가 눌려 있지 않을 때만).
    pub reset: bool,
}

/// 틱 하나의 결과. 엔진 쪽이 8단계에서 시그널로 바꾼다.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TickReport {
    /// 방금 끝난 틱 번호(첫 틱이 1).
    pub tick: u64,
    /// 처리 순서대로의 명령 결과.
    pub results: Vec<CommandResult>,
    /// 틱 시작과 끝 기어가 다르면 끝 값.
    pub gear_changed: Option<Gear>,
    /// 틱 시작과 끝 시동(OFF가 아님)이 다르면 끝 값.
    pub engine_changed: Option<bool>,
    /// 틱 시작과 끝 비상 정지가 다르면 끝 값.
    pub emergency_changed: Option<bool>,
    /// 이번 틱에 시작 위치 복귀가 한 번 이상 성공함.
    pub course_reset: bool,
    /// 이번 틱에 새로 시작된 충돌.
    pub collided: Option<Impact>,
}

/// 대기열이 가득 차 명령을 받지 않음.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueFull;

/// 차량 한 대의 주행 세션.
#[derive(Debug, Clone)]
pub struct DriveSession {
    course: CourseDef,
    /// 충돌 경계와 그 격자 색인(4-6). 한 단계 검사 대상을 닿을 수 있는 장애물로 줄인다(결과는 전체 검사와 같음).
    obstacles: ObstacleGrid,
    params: DynamicsParams,
    state: VehicleState,
    motion: Motion,
    contact: ContactState,
    queue: VecDeque<DriveCommand>,
    manual: VehicleInput,
    tick: u64,
}

impl DriveSession {
    /// 코스와 차량 설정으로 초기 상태(시동 OFF, P, 비상 정지 없음, 코스 시작 자세, 틱 0)의 세션을 만든다.
    pub fn new(course: CourseDef, config: &VehicleConfig) -> Self {
        let obstacles = ObstacleGrid::new(course.collision_rects());
        let motion = course.start;
        Self {
            course,
            obstacles,
            params: config.dynamics.clone(),
            state: VehicleState::new(config.state_machine.stop_threshold_mps),
            motion,
            contact: ContactState::default(),
            queue: VecDeque::new(),
            manual: VehicleInput::default(),
            tick: 0,
        }
    }

    /// 코스 정의를 반환한다.
    pub fn course(&self) -> &CourseDef {
        &self.course
    }

    /// 상태 머신(시동·기어·속도)을 반환한다.
    pub fn state(&self) -> &VehicleState {
        &self.state
    }

    /// 자세·바퀴 조향각을 반환한다.
    pub fn motion(&self) -> &Motion {
        &self.motion
    }

    /// 장애물 접촉 상태를 반환한다.
    pub fn contact(&self) -> &ContactState {
        &self.contact
    }

    /// 처리를 마친 틱 수를 반환한다.
    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    /// 시동이 OFF가 아닌지(비상 정지 포함) 반환한다.
    pub fn engine_on(&self) -> bool {
        self.state.engine() != EngineState::Off
    }

    /// 비상 정지 중인지 반환한다.
    pub fn emergency_active(&self) -> bool {
        self.state.engine() == EngineState::EmergencyStop
    }

    /// 다음 틱 대기열에 명령을 넣는다. 상한([`QUEUE_LIMIT`])을 넘으면 넣지 않는다.
    pub fn enqueue(&mut self, command: DriveCommand) -> Result<(), QueueFull> {
        if self.queue.len() >= QUEUE_LIMIT {
            return Err(QueueFull);
        }
        self.queue.push_back(command);
        Ok(())
    }

    /// 대기열의 명령을 모두 버린다(화면 이탈 등).
    pub fn clear_queue(&mut self) {
        self.queue.clear();
    }

    /// 수동 연속 입력을 바꾼다. 범위 밖·NaN은 잘라서 다음 틱부터 쓴다(래치).
    pub fn set_manual_input(&mut self, input: VehicleInput) {
        self.manual = input.sanitized();
    }

    /// 마지막 수동 입력을 반환한다.
    pub fn manual_input(&self) -> VehicleInput {
        self.manual
    }

    /// 고정 간격 한 틱을 처리한다(계약 5-1 3~7단계).
    ///
    /// 대기열 명령을 들어온 순서대로, 그다음 장치 명령·장치 복귀 요청을 처리한다. 각 명령은 틱 시작
    /// 속도로 판정된다(복귀 성공 시에는 속도 0). 그 뒤 연속 입력을 확정하고 운동 모델을 [`DT`]만큼 한 번 돌린다.
    pub fn tick(&mut self, device: &DeviceTick) -> TickReport {
        let gear_before = self.state.gear();
        let engine_before = self.engine_on();
        let emergency_before = self.emergency_active();
        let mut report = TickReport::default();

        while let Some(command) = self.queue.pop_front() {
            let result = self.apply(command, CommandSource::Api, &mut report);
            report.results.push(result);
        }
        for request in &device.requests {
            let result = match request {
                ControlRequest::Command(command) => self.apply(
                    DriveCommand::Vehicle(*command),
                    CommandSource::Device,
                    &mut report,
                ),
                ControlRequest::GearConflict(gears) => CommandResult {
                    command: "gear_conflict".to_owned(),
                    source: CommandSource::Device,
                    accepted: false,
                    reason: format!("simultaneous gear keys ({})", gear_names(gears)),
                },
            };
            report.results.push(result);
        }
        if device.reset {
            let result = self.apply(
                DriveCommand::ResetCourse,
                CommandSource::Device,
                &mut report,
            );
            report.results.push(result);
        }

        self.state.set_input(device.input.unwrap_or(self.manual));
        report.collided = step_with_grid(
            &mut self.state,
            &mut self.motion,
            &self.params,
            DT,
            &VEHICLE_FOOTPRINT,
            &self.obstacles,
            &mut self.contact,
        );
        self.tick += 1;
        report.tick = self.tick;

        let gear_after = self.state.gear();
        report.gear_changed = (gear_after != gear_before).then_some(gear_after);
        let engine_after = self.engine_on();
        report.engine_changed = (engine_after != engine_before).then_some(engine_after);
        let emergency_after = self.emergency_active();
        report.emergency_changed = (emergency_after != emergency_before).then_some(emergency_after);
        report
    }

    /// 명령 하나를 적용하고 결과를 만든다. 복귀가 성공하면 `report.course_reset`을 켠다.
    fn apply(
        &mut self,
        command: DriveCommand,
        source: CommandSource,
        report: &mut TickReport,
    ) -> CommandResult {
        let outcome = match command {
            DriveCommand::Vehicle(vehicle_command) => self
                .state
                .apply(vehicle_command)
                .map(|_| ())
                .map_err(|error| error.to_string()),
            DriveCommand::ResetCourse => self.reset_to_start(),
        };
        if outcome.is_ok() && command == DriveCommand::ResetCourse {
            report.course_reset = true;
        }
        CommandResult {
            command: command.name().to_owned(),
            source,
            accepted: outcome.is_ok(),
            reason: outcome.err().unwrap_or_default(),
        }
    }

    /// 정지 상태면 자세·속도·조향각·접촉을 코스 시작값으로 되돌린다(Bevy `reset_to_start`와 같은 조건).
    /// 기어·시동·비상 정지·수동 입력·대기열은 그대로 둔다.
    fn reset_to_start(&mut self) -> Result<(), String> {
        if !self.state.is_stopped() {
            return Err(RESET_NOT_STOPPED.to_owned());
        }
        self.state.set_speed_mps(0.0);
        self.motion = self.course.start;
        self.contact = ContactState::default();
        Ok(())
    }
}

/// 기어 목록을 `R+D` 같은 표시 문자열로 만든다.
fn gear_names(gears: &[Gear]) -> String {
    let names: Vec<&str> = gears
        .iter()
        .map(|gear| match gear {
            Gear::Park => "P",
            Gear::Reverse => "R",
            Gear::Neutral => "N",
            Gear::Drive => "D",
        })
        .collect();
    names.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::CourseId;

    /// 기본 설정의 training 코스 세션을 만든다.
    fn session() -> DriveSession {
        DriveSession::new(CourseId::Training.build(), &VehicleConfig::default())
    }

    /// 가속 입력.
    fn throttle(value: f32) -> VehicleInput {
        VehicleInput {
            throttle: value,
            ..VehicleInput::default()
        }
    }

    /// 계약 이름과 명령이 서로 바뀌고, 모르는 이름은 거부하는지 확인한다.
    #[test]
    fn command_names_round_trip() {
        for command in DriveCommand::ALL {
            assert_eq!(DriveCommand::parse(command.name()), Some(command));
        }
        assert_eq!(DriveCommand::parse("gear_sport"), None);
    }

    /// 대기열 명령은 다음 틱에 처리되고, 상태 변화는 틱 끝 값으로 한 번만 보고되는지 확인한다.
    #[test]
    fn queued_commands_apply_on_next_tick_with_net_state_changes() {
        let mut session = session();
        session
            .enqueue(DriveCommand::Vehicle(VehicleCommand::ToggleEngine))
            .unwrap();
        assert!(!session.engine_on());
        let report = session.tick(&DeviceTick::default());
        assert_eq!(report.tick, 1);
        assert_eq!(report.engine_changed, Some(true));
        assert_eq!(report.results[0].command, "engine_toggle");
        assert!(report.results[0].accepted);

        for gear in [Gear::Drive, Gear::Reverse, Gear::Park] {
            session
                .enqueue(DriveCommand::Vehicle(VehicleCommand::RequestGear(gear)))
                .unwrap();
        }
        let report = session.tick(&DeviceTick::default());
        assert_eq!(report.results.len(), 3);
        assert!(report.results.iter().all(|result| result.accepted));
        assert_eq!(
            report.gear_changed, None,
            "P -> D -> R -> P has no net change"
        );
    }

    /// 거부된 명령이 사유와 함께 보고되고, 대기열 상한을 넘으면 받지 않는지 확인한다.
    #[test]
    fn rejected_command_and_queue_limit() {
        let mut session = session();
        session
            .enqueue(DriveCommand::Vehicle(VehicleCommand::RequestGear(
                Gear::Drive,
            )))
            .unwrap();
        let report = session.tick(&DeviceTick::default());
        assert!(!report.results[0].accepted);
        assert_eq!(report.results[0].reason, "engine is off");
        assert_eq!(report.results[0].source, CommandSource::Api);

        for _ in 0..QUEUE_LIMIT {
            session.enqueue(DriveCommand::ResetCourse).unwrap();
        }
        assert_eq!(session.enqueue(DriveCommand::ResetCourse), Err(QueueFull));
    }

    /// 움직이는 동안 복귀는 거부되고, 정지 후에는 자세만 되돌리고 기어·시동은 유지하는지 확인한다.
    #[test]
    fn reset_requires_stop_and_keeps_gear_and_engine() {
        let mut session = session();
        session
            .enqueue(DriveCommand::Vehicle(VehicleCommand::ToggleEngine))
            .unwrap();
        session
            .enqueue(DriveCommand::Vehicle(VehicleCommand::RequestGear(
                Gear::Drive,
            )))
            .unwrap();
        session.set_manual_input(throttle(1.0));
        for _ in 0..64 {
            session.tick(&DeviceTick::default());
        }
        assert!(session.state().speed_mps() > 0.5);
        session.enqueue(DriveCommand::ResetCourse).unwrap();
        let report = session.tick(&DeviceTick::default());
        assert!(!report.course_reset);
        assert_eq!(report.results[0].reason, RESET_NOT_STOPPED);

        session.set_manual_input(VehicleInput {
            brake: 1.0,
            ..VehicleInput::default()
        });
        for _ in 0..640 {
            session.tick(&DeviceTick::default());
        }
        assert!(session.state().is_stopped());
        session.enqueue(DriveCommand::ResetCourse).unwrap();
        let report = session.tick(&DeviceTick::default());
        assert!(report.course_reset);
        assert_eq!(session.state().gear(), Gear::Drive);
        assert!(session.engine_on());
        let start = session.course().start;
        assert_eq!(
            (session.motion().x_m, session.motion().z_m),
            (start.x_m, start.z_m)
        );
    }

    /// 장치 입력이 있으면 수동 입력 대신 쓰고, 장치 기어 충돌은 `gear_conflict`로 거부되는지 확인한다.
    #[test]
    fn device_input_and_gear_conflict() {
        let mut session = session();
        session.set_manual_input(throttle(1.0));
        let report = session.tick(&DeviceTick {
            input: Some(VehicleInput::default()),
            requests: vec![ControlRequest::GearConflict(vec![
                Gear::Reverse,
                Gear::Drive,
            ])],
            reset: false,
        });
        assert_eq!(session.state().input(), VehicleInput::default());
        assert_eq!(report.results[0].command, "gear_conflict");
        assert_eq!(report.results[0].source, CommandSource::Device);
        assert_eq!(report.results[0].reason, "simultaneous gear keys (R+D)");
    }
}
