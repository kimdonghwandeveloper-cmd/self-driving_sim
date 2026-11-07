//! 상태 머신과 Bevy 앱을 연결하는 차량 컨트롤러.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use super::collision::{self, ContactState, GroundRect, Obstacle, VEHICLE_FOOTPRINT};
use super::dynamics::{DynamicsParams, Motion};
use super::state::{EngineState, Gear, TransitionOutcome, VehicleCommand, VehicleState};
use bevy::app::RunFixedMainLoop;

use crate::SimSet;
use crate::input::{ControlRequest, DriverInput};

/// 앱에서 사용하는 차량 상태 리소스.
#[derive(Resource, Debug, Clone, Default)]
pub struct Vehicle(pub VehicleState);

/// 운동 모델이 적분하는 차량 자세·바퀴 상태 리소스.
#[derive(Resource, Debug, Clone, Default)]
pub struct VehicleMotion(pub Motion);

/// 운동 모델 파라미터 리소스. 설정 파일에서 읽는다.
#[derive(Resource, Debug, Clone, Default)]
pub struct VehicleDynamics(pub DynamicsParams);

/// 차체와 장애물의 접촉 상태 리소스. 충돌 중복 판정과 HUD 접촉 표시에 쓴다.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct VehicleContact(pub ContactState);

/// 차체가 장애물에 실제로 부딪힌(이동이 막힌) 순간의 충돌 메시지. 교육 기록으로 전달한다.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct CollisionEvent {
    /// 고정 주기 시뮬레이션 경과 시간(s).
    pub sim_time_s: f32,
    /// 닿기 직전의 부호 있는 속도(m/s).
    pub speed_mps: f32,
    /// 멈춘 자세의 후륜축 중심 x 좌표(m).
    pub x_m: f32,
    /// 멈춘 자세의 후륜축 중심 z 좌표(m).
    pub z_m: f32,
}

/// 마지막 명령 처리 결과. HUD에 표시한다.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlFeedback {
    pub last: Option<Feedback>,
}

/// 명령 하나의 처리 결과와 표시 문장.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feedback {
    pub accepted: bool,
    pub message: String,
}

/// 차량 리소스와 명령 처리 시스템을 등록하는 플러그인.
pub struct VehiclePlugin;

impl Plugin for VehiclePlugin {
    /// 차량 관련 리소스와 시스템을 앱에 등록한다. 이미 넣은 `Vehicle`·`VehicleDynamics`는 유지한다.
    ///
    /// 명령 처리는 매 프레임 고정 주기 루프 직전에, 운동 모델은 렌더 프레임과 독립된 고정 주기
    /// (`FixedUpdate`)로 돈다. 따라서 이번 프레임의 입력·명령이 같은 프레임의 운동 계산에 반영된다.
    fn build(&self, app: &mut App) {
        app.init_resource::<Vehicle>()
            .init_resource::<VehicleMotion>()
            .init_resource::<VehicleDynamics>()
            .init_resource::<ControlFeedback>()
            .init_resource::<VehicleContact>()
            .add_message::<CollisionEvent>()
            .add_systems(RunFixedMainLoop, apply_controls.in_set(SimSet::Control))
            .add_systems(FixedUpdate, step_motion.run_if(crate::driving_active));
    }
}

/// 연속 입력을 상태에 반영하고, 받은 요청을 순서대로 처리해 결과를 로그와 HUD에 남긴다.
pub fn apply_controls(
    mut vehicle: ResMut<Vehicle>,
    driver: Res<DriverInput>,
    mut requests: MessageReader<ControlRequest>,
    mut feedback: ResMut<ControlFeedback>,
) {
    vehicle.0.set_input(driver.0);
    for request in requests.read() {
        let result = handle_request(&mut vehicle.0, request);
        let speed = vehicle.0.speed_mps();
        if result.accepted {
            info!("control: {} (speed {speed:+.2} m/s)", result.message);
        } else {
            warn!(
                "control rejected: {} (speed {speed:+.2} m/s)",
                result.message
            );
        }
        feedback.last = Some(result);
    }
}

/// 장면의 고정 장애물 목록. 시스템 전용 버퍼를 재사용해 고정 단계마다 새로 할당하지 않는다.
#[derive(SystemParam)]
pub struct ObstacleSet<'w, 's> {
    query: Query<'w, 's, &'static Obstacle>,
    buffer: Local<'s, Vec<GroundRect>>,
}

impl ObstacleSet<'_, '_> {
    /// 현재 장애물 영역을 버퍼에 다시 채워 반환한다. 버퍼 용량은 유지된다.
    fn rects(&mut self) -> &[GroundRect] {
        self.buffer.clear();
        self.buffer
            .extend(self.query.iter().map(|obstacle| obstacle.0));
        &self.buffer
    }
}

/// 고정 시간 간격마다 운동 모델을 한 단계 적분하고, 장애물 관통을 막으며 새 충돌을 알린다.
#[allow(clippy::too_many_arguments)]
pub fn step_motion(
    time: Res<Time>,
    params: Res<VehicleDynamics>,
    mut obstacles: ObstacleSet,
    mut vehicle: ResMut<Vehicle>,
    mut motion: ResMut<VehicleMotion>,
    mut contact: ResMut<VehicleContact>,
    mut collisions: MessageWriter<CollisionEvent>,
    session: Option<Res<crate::SessionClock>>,
) {
    let impact = collision::step_with_obstacles(
        &mut vehicle.0,
        &mut motion.0,
        &params.0,
        time.delta_secs(),
        &VEHICLE_FOOTPRINT,
        obstacles.rects(),
        &mut contact.0,
    );
    if let Some(impact) = impact {
        collisions.write(CollisionEvent {
            sim_time_s: time.elapsed_secs() - session.map_or(0.0, |clock| clock.started_s),
            speed_mps: impact.speed_mps,
            x_m: impact.x_m,
            z_m: impact.z_m,
        });
    }
}

/// 요청 하나를 상태 머신에 적용하고 처리 결과를 만든다.
pub fn handle_request(state: &mut VehicleState, request: &ControlRequest) -> Feedback {
    match request {
        ControlRequest::Command(command) => match state.apply(*command) {
            Ok(outcome) => Feedback {
                accepted: true,
                message: format!("{}: {}", command_label(*command), outcome_label(outcome)),
            },
            Err(error) => Feedback {
                accepted: false,
                message: format!("{}: {error}", command_label(*command)),
            },
        },
        ControlRequest::GearConflict(gears) => {
            let names: Vec<_> = gears.iter().map(|gear| gear_label(*gear)).collect();
            Feedback {
                accepted: false,
                message: format!(
                    "gear request ignored: simultaneous gear keys ({})",
                    names.join("+")
                ),
            }
        }
    }
}

/// 명령의 표시 이름을 반환한다.
pub fn command_label(command: VehicleCommand) -> String {
    match command {
        VehicleCommand::ToggleEngine => "engine toggle".to_owned(),
        VehicleCommand::RequestGear(gear) => format!("gear {}", gear_label(gear)),
        VehicleCommand::EngageEmergencyStop => "emergency stop".to_owned(),
        VehicleCommand::ReleaseEmergencyStop => "emergency release".to_owned(),
    }
}

/// 전이 결과의 표시 문장을 반환한다.
fn outcome_label(outcome: TransitionOutcome) -> String {
    match outcome {
        TransitionOutcome::EngineChanged(engine) => format!("engine -> {}", engine_label(engine)),
        TransitionOutcome::GearChanged { from, to } => {
            format!("{} -> {}", gear_label(from), gear_label(to))
        }
        TransitionOutcome::Unchanged => "no change".to_owned(),
    }
}

/// 기어의 한 글자 표시(P/R/N/D)를 반환한다.
pub fn gear_label(gear: Gear) -> &'static str {
    match gear {
        Gear::Park => "P",
        Gear::Reverse => "R",
        Gear::Neutral => "N",
        Gear::Drive => "D",
    }
}

/// 시동 상태의 표시 이름을 반환한다.
pub fn engine_label(engine: EngineState) -> &'static str {
    match engine {
        EngineState::Off => "OFF",
        EngineState::Ready => "READY",
        EngineState::EmergencyStop => "EMERGENCY STOP",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 수락된 명령이 전이 결과 문장을 만들고 상태를 바꾸는지 확인한다.
    #[test]
    fn accepted_command_reports_transition() {
        let mut state = VehicleState::default();
        let result = handle_request(
            &mut state,
            &ControlRequest::Command(VehicleCommand::ToggleEngine),
        );
        assert!(result.accepted);
        assert_eq!(result.message, "engine toggle: engine -> READY");
        assert_eq!(state.engine(), EngineState::Ready);
    }

    /// 거부된 명령이 거부 사유 문장을 만드는지 확인한다.
    #[test]
    fn rejected_command_reports_reason() {
        let mut state = VehicleState::default();
        let result = handle_request(
            &mut state,
            &ControlRequest::Command(VehicleCommand::RequestGear(Gear::Drive)),
        );
        assert!(!result.accepted);
        assert_eq!(result.message, "gear D: engine is off");
    }

    /// 기어 충돌 요청은 상태를 바꾸지 않고 거부 문장만 만드는지 확인한다.
    #[test]
    fn gear_conflict_is_rejected_without_state_change() {
        let mut state = VehicleState::default();
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        let before = state.clone();
        let result = handle_request(
            &mut state,
            &ControlRequest::GearConflict(vec![Gear::Reverse, Gear::Drive]),
        );
        assert!(!result.accepted);
        assert_eq!(
            result.message,
            "gear request ignored: simultaneous gear keys (R+D)"
        );
        assert_eq!(state, before);
    }
}
