//! 차량 파라미터 보정 도구(Phase 2-5).
//!
//! 회전반경·제동거리 예상값을 계산하고, 같은 코스에서 반복하는 제동·가속 시험을 고정 주기마다 측정한다.
//! 시작 위치 복귀 요청을 처리해 같은 출발점에서 시험을 되풀이할 수 있게 한다.
//! 모든 값은 **시뮬레이터 임시값**으로 계산한 결과이며 실차 성능을 나타내지 않는다.

use bevy::prelude::*;

use crate::SimSet;
use crate::vehicle::controller::{
    ControlFeedback, Feedback, Vehicle, VehicleContact, VehicleDynamics, VehicleMotion, step_motion,
};
use crate::vehicle::{DynamicsParams, EngineState, Gear, Motion, VehicleState};
use crate::world::ActiveCourse;

/// 이 조향각(rad)보다 작으면 직진으로 보고 회전반경을 표시하지 않는다.
const STRAIGHT_STEER_RAD: f32 = 1.0e-4;
/// 가속 시험에서 최고 속도에 도달했다고 보는 오차(m/s).
const SPEED_LIMIT_EPS_MPS: f32 = 1.0e-4;

/// 입력 장치가 보내는 시작 위치 복귀 요청. 정지 상태에서만 처리한다.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetCourseRequest;

/// 시작 위치 복귀가 실제로 적용되었음을 알리는 메시지. 카메라 시점 초기화 등에 쓴다.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CourseResetDone;

/// 보정 측정 리소스와 시스템을 등록하는 플러그인.
pub struct CalibrationPlugin;

impl Plugin for CalibrationPlugin {
    /// 측정 기록 리소스, 복귀 요청 메시지, 복귀 처리·측정 시스템을 앱에 등록한다.
    ///
    /// 복귀 요청은 명령 처리 직후(고정 주기 루프 직전), 측정은 운동 계산 직후 고정 주기마다 돈다.
    fn build(&self, app: &mut App) {
        app.init_resource::<CalibrationLog>()
            .init_resource::<ActiveCourse>()
            .add_message::<ResetCourseRequest>()
            .add_message::<CourseResetDone>()
            .add_systems(
                bevy::app::RunFixedMainLoop,
                apply_course_reset
                    .in_set(SimSet::Control)
                    .after(crate::vehicle::controller::apply_controls),
            )
            .add_systems(
                FixedUpdate,
                record_trials
                    .after(step_motion)
                    .run_if(crate::driving_active),
            );
    }
}

/// 최대 제동으로 멈춘 시험 결과.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StopResult {
    /// 제동을 시작할 때의 속도 크기(m/s).
    pub from_speed_mps: f32,
    /// 제동 시작부터 정지까지 이동한 경로 길이(m).
    pub distance_m: f32,
    /// 제동 시작부터 정지까지 걸린 시간(s).
    pub time_s: f32,
    /// 비상 정지 감속이 포함됐는지.
    pub emergency: bool,
}

/// 정지 상태에서 가속한 시험 결과.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AccelResult {
    /// 시험이 끝날 때의 속도 크기(m/s).
    pub to_speed_mps: f32,
    /// 가속 시작부터 끝까지 이동한 경로 길이(m).
    pub distance_m: f32,
    /// 가속 시작부터 끝까지 걸린 시간(s).
    pub time_s: f32,
    /// 최고 속도에 도달해 끝났는지(아니면 가속 입력을 떼서 끝남).
    pub reached_limit: bool,
}

/// 진행 중인 시험 하나의 누적값.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Trial {
    /// 시작할 때의 속도 크기(m/s).
    start_speed_mps: f32,
    /// 지금까지의 경로 길이(m).
    distance_m: f32,
    /// 지금까지의 경과 시간(s).
    time_s: f32,
    /// 비상 정지 감속이 한 번이라도 포함됐는지(제동 시험에서만 사용).
    emergency: bool,
}

impl Trial {
    /// `start_speed_mps`에서 시작하는 빈 시험을 만든다.
    fn new(start_speed_mps: f32) -> Self {
        Self {
            start_speed_mps,
            distance_m: 0.0,
            time_s: 0.0,
            emergency: false,
        }
    }
}

/// 진행 중인 시험 종류. HUD 표시에 쓴다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActiveTrial {
    /// 제동 시험 중: 경로 길이(m), 경과 시간(s).
    Braking { distance_m: f32, time_s: f32 },
    /// 가속 시험 중: 경로 길이(m), 경과 시간(s).
    Accelerating { distance_m: f32, time_s: f32 },
}

/// 제동·가속 시험 측정 기록. 마지막 결과를 남겨 같은 코스의 반복 시험을 비교한다.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct CalibrationLog {
    /// 진행 중인 제동 시험.
    brake: Option<Trial>,
    /// 진행 중인 가속 시험.
    accel: Option<Trial>,
    /// 직전 단계의 부호 있는 속도(m/s). 시험 시작 속도로 쓴다.
    prev_speed_mps: f32,
    /// 직전 단계의 후륜축 중심 위치(x, z; m). 이동 거리 누적에 쓴다.
    prev_pos: (f32, f32),
    /// 마지막으로 끝난 제동 시험.
    last_stop: Option<StopResult>,
    /// 마지막으로 끝난 가속 시험.
    last_accel: Option<AccelResult>,
    /// 끝난 제동 시험 횟수.
    stop_trials: u32,
    /// 끝난 가속 시험 횟수.
    accel_trials: u32,
}

impl CalibrationLog {
    /// 마지막 제동 시험 결과를 반환한다.
    pub fn last_stop(&self) -> Option<StopResult> {
        self.last_stop
    }

    /// 마지막 가속 시험 결과를 반환한다.
    pub fn last_accel(&self) -> Option<AccelResult> {
        self.last_accel
    }

    /// 끝난 제동·가속 시험 횟수를 반환한다.
    pub fn trial_counts(&self) -> (u32, u32) {
        (self.stop_trials, self.accel_trials)
    }

    /// 진행 중인 시험을 반환한다. 제동 시험을 우선한다.
    pub fn active(&self) -> Option<ActiveTrial> {
        if let Some(trial) = self.brake {
            return Some(ActiveTrial::Braking {
                distance_m: trial.distance_m,
                time_s: trial.time_s,
            });
        }
        self.accel.map(|trial| ActiveTrial::Accelerating {
            distance_m: trial.distance_m,
            time_s: trial.time_s,
        })
    }

    /// 시작 위치 복귀 후 진행 중인 시험을 버리고 기준 위치·속도를 다시 잡는다. 끝난 결과는 유지한다.
    pub fn restart_from(&mut self, state: &VehicleState, motion: &Motion) {
        self.brake = None;
        self.accel = None;
        self.prev_speed_mps = state.speed_mps();
        self.prev_pos = (motion.x_m, motion.z_m);
    }

    /// 고정 단계 하나가 끝난 상태로 시험을 진행·종료한다.
    ///
    /// - 제동 시험: 움직이는 중 가속 없이 제동 입력이나 비상 정지 감속이 생기면 시작하고, 속도가 0이 되면 끝난다.
    ///   정지하는 단계까지 제동(또는 비상 정지)이 유지되지 않았거나, 장애물에 막혀 이동이 잘리면 결과 없이 버린다.
    /// - 가속 시험: 정지 상태에서 가속이 허용된 입력이 생기면 시작하고, 가속을 떼거나 최고 속도에 닿으면 끝난다.
    ///   장애물에 막혀 이동이 잘리면 결과 없이 버린다.
    ///
    /// `blocked`는 이번 단계에서 장애물 때문에 이동이 잘렸는지다([`ContactState::blocked`]).
    /// 장애물 근처(접촉 거리 안)에 있을 뿐 막히지 않은 시험은 정상 결과로 남긴다.
    ///
    /// [`ContactState::blocked`]: crate::vehicle::ContactState::blocked
    pub fn observe(
        &mut self,
        state: &VehicleState,
        motion: &Motion,
        params: &DynamicsParams,
        blocked: bool,
        dt: f32,
    ) {
        let speed = state.speed_mps();
        if !state.speed_valid() || !(dt.is_finite() && dt > 0.0) {
            self.brake = None;
            self.accel = None;
            return;
        }
        let pos = (motion.x_m, motion.z_m);
        let step_m = (pos.0 - self.prev_pos.0).hypot(pos.1 - self.prev_pos.1);
        let throttle = state.effective_throttle() > 0.0;
        let emergency = state.engine() == EngineState::EmergencyStop;
        let braking = !throttle && (state.input().brake > 0.0 || emergency);

        self.observe_brake(speed, step_m, braking, emergency, blocked, dt);
        self.observe_accel(state, speed, step_m, throttle, blocked, params, dt);

        self.prev_speed_mps = speed;
        self.prev_pos = pos;
    }

    /// 제동 시험을 시작·누적·종료한다. `observe`에서만 부른다.
    fn observe_brake(
        &mut self,
        speed: f32,
        step_m: f32,
        braking: bool,
        emergency: bool,
        blocked: bool,
        dt: f32,
    ) {
        if self.brake.is_none() && braking && self.prev_speed_mps != 0.0 {
            self.brake = Some(Trial::new(self.prev_speed_mps.abs()));
        }
        let Some(mut trial) = self.brake else {
            return;
        };
        // 이번 단계에 제동이 유지되지 않았으면(관성만으로 멈춘 경우 포함) 정지 여부보다 먼저 버린다.
        if blocked || !braking {
            self.brake = None;
            return;
        }
        trial.distance_m += step_m;
        trial.time_s += dt;
        trial.emergency |= emergency;
        if speed != 0.0 {
            self.brake = Some(trial);
            return;
        }
        self.brake = None;
        self.last_stop = Some(StopResult {
            from_speed_mps: trial.start_speed_mps,
            distance_m: trial.distance_m,
            time_s: trial.time_s,
            emergency: trial.emergency,
        });
        self.stop_trials += 1;
    }

    /// 가속 시험을 시작·누적·종료한다. `observe`에서만 부른다.
    #[allow(clippy::too_many_arguments)]
    fn observe_accel(
        &mut self,
        state: &VehicleState,
        speed: f32,
        step_m: f32,
        throttle: bool,
        blocked: bool,
        params: &DynamicsParams,
        dt: f32,
    ) {
        if self.accel.is_none() && throttle && self.prev_speed_mps == 0.0 {
            self.accel = Some(Trial::new(0.0));
        }
        let Some(mut trial) = self.accel else {
            return;
        };
        if blocked {
            self.accel = None;
            return;
        }
        if !throttle {
            // 가속을 뗀 단계의 이동은 관성·제동 구간이므로 넣지 않는다.
            self.finish_accel(trial, self.prev_speed_mps.abs(), false);
            return;
        }
        trial.distance_m += step_m;
        trial.time_s += dt;
        let limit = match state.gear() {
            Gear::Reverse => params.max_reverse_speed_mps,
            _ => params.max_forward_speed_mps,
        };
        if speed.abs() >= limit - SPEED_LIMIT_EPS_MPS {
            self.finish_accel(trial, speed.abs(), true);
        } else {
            self.accel = Some(trial);
        }
    }

    /// 가속 시험을 결과로 남기고 끝낸다.
    fn finish_accel(&mut self, trial: Trial, to_speed_mps: f32, reached_limit: bool) {
        self.accel = None;
        if trial.time_s > 0.0 {
            self.last_accel = Some(AccelResult {
                to_speed_mps,
                distance_m: trial.distance_m,
                time_s: trial.time_s,
                reached_limit,
            });
            self.accel_trials += 1;
        }
    }
}

/// 지금 속도에서 바퀴 조향각으로 도는 후륜축 중심 회전반경(m)을 계산한다. 거의 직진이면 `None`.
///
/// 운동 모델과 같은 곡률(언더스티어·횡가속도 상한 포함)을 쓰므로, 고속에서는 기구학 반경보다 크다.
pub fn effective_turn_radius_m(
    params: &DynamicsParams,
    speed_mps: f32,
    steer_angle_rad: f32,
) -> Option<f32> {
    if steer_angle_rad.abs() < STRAIGHT_STEER_RAD {
        return None;
    }
    Some(1.0 / params.path_curvature(speed_mps, steer_angle_rad).abs())
}

/// 후륜축 중심 기준 회전반경(m)을 Kinematic Bicycle Model로 계산한다. 거의 직진이면 `None`.
pub fn turn_radius_m(wheelbase_m: f32, steer_angle_rad: f32) -> Option<f32> {
    if steer_angle_rad.abs() < STRAIGHT_STEER_RAD {
        return None;
    }
    Some(wheelbase_m / steer_angle_rad.abs().tan())
}

/// 최대 조향각에서의 후륜축 중심 최소 회전반경(m)을 계산한다.
pub fn min_turn_radius_m(params: &DynamicsParams) -> f32 {
    params.wheelbase_m / params.max_steer_angle_deg.to_radians().tan()
}

/// 지금 속도에서 가속 없이 최대 제동했을 때의 예상 정지거리(m)를 v²/(2a)로 계산한다.
///
/// 감속도는 최대 제동 + 관성 감속 + 구름 저항 성분이며, 비상 정지 중이면 비상 감속 성분도 더한다.
/// 속도에 따라 줄어드는 공기 저항은 넣지 않아 실제보다 조금 길게(보수적으로) 예상한다.
/// 이산 적분 결과와는 한 단계 이동량 정도 차이가 날 수 있다.
pub fn brake_distance_estimate_m(state: &VehicleState, params: &DynamicsParams) -> f32 {
    let speed = state.speed_mps();
    if !speed.is_finite() || speed == 0.0 {
        return 0.0;
    }
    let mut decel =
        params.max_brake_decel_mps2 + params.coast_decel_mps2 + params.rolling_decel_mps2();
    if state.engine() == EngineState::EmergencyStop {
        decel += params.emergency_decel_mps2;
    }
    speed * speed / (2.0 * decel)
}

/// HUD에 표시할 보정 값 묶음.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CalibrationReadout {
    /// 현재 속도·바퀴 각도의 회전반경(m). 직진이면 `None`.
    pub turn_radius_m: Option<f32>,
    /// 최대 조향각의 최소 회전반경(m).
    pub min_turn_radius_m: f32,
    /// 지금 최대 제동 시 예상 정지거리(m).
    pub brake_estimate_m: f32,
    /// 진행 중인 시험.
    pub active: Option<ActiveTrial>,
    /// 마지막 제동 시험 결과.
    pub last_stop: Option<StopResult>,
    /// 마지막 가속 시험 결과.
    pub last_accel: Option<AccelResult>,
    /// 끝난 제동·가속 시험 횟수.
    pub trials: (u32, u32),
}

impl CalibrationReadout {
    /// 현재 상태·파라미터·측정 기록으로 HUD 표시값을 만든다.
    pub fn new(
        state: &VehicleState,
        motion: &Motion,
        params: &DynamicsParams,
        log: &CalibrationLog,
    ) -> Self {
        Self {
            turn_radius_m: effective_turn_radius_m(
                params,
                state.speed_mps(),
                motion.steer_angle_rad,
            ),
            min_turn_radius_m: min_turn_radius_m(params),
            brake_estimate_m: brake_distance_estimate_m(state, params),
            active: log.active(),
            last_stop: log.last_stop(),
            last_accel: log.last_accel(),
            trials: log.trial_counts(),
        }
    }
}

/// 정지 상태이면 차량을 코스 시작 자세 `start`로 되돌린다. 움직이는 중이면 거부한다.
///
/// 시동·기어·비상 정지 상태는 바꾸지 않고, 장애물 접촉 기록과 진행 중인 시험만 초기화한다.
pub fn reset_to_start(
    state: &mut VehicleState,
    motion: &mut Motion,
    start: &Motion,
    contact: &mut VehicleContact,
    log: &mut CalibrationLog,
) -> Feedback {
    if !state.is_stopped() {
        return Feedback {
            accepted: false,
            message: "reset to start: vehicle must be stopped".to_owned(),
        };
    }
    state.set_speed_mps(0.0);
    *motion = *start;
    *contact = VehicleContact::default();
    log.restart_from(state, motion);
    Feedback {
        accepted: true,
        message: "reset to start: vehicle returned to start pose".to_owned(),
    }
}

/// 받은 시작 위치 복귀 요청을 처리하고 결과를 HUD 피드백과 로그에 남긴다. 적용되면 [`CourseResetDone`]을 보낸다.
/// 복귀 자세는 현재 코스([`ActiveCourse`])의 시작 자세다.
#[allow(clippy::too_many_arguments)]
pub fn apply_course_reset(
    mut requests: MessageReader<ResetCourseRequest>,
    mut done: MessageWriter<CourseResetDone>,
    mut vehicle: ResMut<Vehicle>,
    mut motion: ResMut<VehicleMotion>,
    mut contact: ResMut<VehicleContact>,
    mut log: ResMut<CalibrationLog>,
    mut feedback: ResMut<ControlFeedback>,
    course: Res<ActiveCourse>,
) {
    for _ in requests.read() {
        let result = reset_to_start(
            &mut vehicle.0,
            &mut motion.0,
            &course.0.start,
            &mut contact,
            &mut log,
        );
        if result.accepted {
            info!("control: {}", result.message);
            done.write(CourseResetDone);
        } else {
            warn!("control rejected: {}", result.message);
        }
        feedback.last = Some(result);
    }
}

/// 고정 단계마다 운동 계산 결과로 제동·가속 시험을 측정한다.
pub fn record_trials(
    time: Res<Time>,
    vehicle: Res<Vehicle>,
    motion: Res<VehicleMotion>,
    params: Res<VehicleDynamics>,
    contact: Res<VehicleContact>,
    mut log: ResMut<CalibrationLog>,
) {
    log.observe(
        &vehicle.0,
        &motion.0,
        &params.0,
        contact.0.blocked,
        time.delta_secs(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::collision::step_with_obstacles;
    use crate::vehicle::dynamics::step;
    use crate::vehicle::{
        ContactState, GroundRect, VEHICLE_FOOTPRINT, VehicleCommand, VehicleInput,
    };

    /// 테스트용 고정 시간 간격(s).
    const DT: f32 = 1.0 / 64.0;

    /// 시동 Ready, D, 정지 상태의 차량을 만든다.
    fn ready_in_drive() -> VehicleState {
        let mut state = VehicleState::default();
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        state
            .apply(VehicleCommand::RequestGear(Gear::Drive))
            .unwrap();
        state
    }

    /// 입력을 유지한 채 `steps`번 적분하고 매 단계 측정한다.
    fn drive(
        state: &mut VehicleState,
        motion: &mut Motion,
        log: &mut CalibrationLog,
        input: VehicleInput,
        steps: usize,
    ) {
        let params = DynamicsParams::phase2_reference();
        state.set_input(input);
        for _ in 0..steps {
            step(state, motion, &params, DT);
            log.observe(state, motion, &params, false, DT);
        }
    }

    /// 가속·제동 입력을 만든다.
    fn input(throttle: f32, brake: f32) -> VehicleInput {
        VehicleInput {
            throttle,
            brake,
            steering: 0.0,
        }
    }

    /// 정지에서 끝까지 가속하면 최고 속도 도달 시간·거리를 기록하는지 확인한다(기본 1 m/s², 5 m/s).
    #[test]
    fn accel_trial_measures_time_to_top_speed() {
        let mut state = ready_in_drive();
        let mut motion = Motion::default();
        let mut log = CalibrationLog::default();
        drive(&mut state, &mut motion, &mut log, input(1.0, 0.0), 64 * 6);
        let result = log.last_accel().expect("accel trial should finish");
        assert!(result.reached_limit);
        assert!((result.to_speed_mps - 5.0).abs() < 1e-3);
        assert!((result.time_s - 5.0).abs() < DT + 1e-3, "{result:?}");
        assert!((result.distance_m - 12.5).abs() < 0.1, "{result:?}");
        assert_eq!(log.trial_counts(), (0, 1));
    }

    /// 최대 제동으로 멈추면 측정 정지거리가 v²/(2a) 예상값과 거의 같은지 확인한다.
    #[test]
    fn brake_trial_matches_estimate() {
        let mut state = ready_in_drive();
        let mut motion = Motion::default();
        let mut log = CalibrationLog::default();
        drive(&mut state, &mut motion, &mut log, input(1.0, 0.0), 64 * 4);
        let speed = state.speed_mps();
        let estimate = brake_distance_estimate_m(&state, &DynamicsParams::phase2_reference());
        drive(&mut state, &mut motion, &mut log, input(0.0, 1.0), 64 * 3);

        let result = log.last_stop().expect("brake trial should finish");
        assert_eq!(state.speed_mps(), 0.0);
        assert!((result.from_speed_mps - speed).abs() < 1e-5);
        assert!((estimate - speed * speed / (2.0 * 3.3)).abs() < 1e-4);
        assert!(
            (result.distance_m - estimate).abs() < speed * DT,
            "{result:?} vs {estimate}"
        );
        assert!(!result.emergency);
        // 가속을 뗀 시점에 가속 시험도 끝난다.
        assert_eq!(log.trial_counts(), (1, 1));
        assert!(!log.last_accel().unwrap().reached_limit);
    }

    /// 정지 전에 제동을 풀면 제동 시험 결과를 남기지 않는지 확인한다.
    #[test]
    fn released_brake_discards_trial() {
        let mut state = ready_in_drive();
        let mut motion = Motion::default();
        let mut log = CalibrationLog::default();
        drive(&mut state, &mut motion, &mut log, input(1.0, 0.0), 64 * 3);
        drive(&mut state, &mut motion, &mut log, input(0.0, 1.0), 10);
        assert!(matches!(log.active(), Some(ActiveTrial::Braking { .. })));
        drive(&mut state, &mut motion, &mut log, input(0.0, 0.0), 1);
        assert_eq!(log.active(), None);
        assert_eq!(log.last_stop(), None);
    }

    /// 장애물 판정을 포함해 입력을 유지한 채 `steps`번 적분하고, 실제 이동 막힘 여부로 측정한다.
    fn drive_among(
        state: &mut VehicleState,
        motion: &mut Motion,
        log: &mut CalibrationLog,
        contact: &mut ContactState,
        obstacles: &[GroundRect],
        input: VehicleInput,
        steps: usize,
    ) {
        let params = DynamicsParams::phase2_reference();
        state.set_input(input);
        for _ in 0..steps {
            step_with_obstacles(
                state,
                motion,
                &params,
                DT,
                &VEHICLE_FOOTPRINT,
                obstacles,
                contact,
            );
            log.observe(state, motion, &params, contact.blocked, DT);
        }
    }

    /// 벽 옆 1 cm 간격(접촉 거리 안, 막힘 없음)에서 한 가속·제동 시험은 정상 결과로 남는지 확인한다.
    #[test]
    fn trials_beside_wall_without_impact_are_recorded() {
        // 차체 오른쪽 면(x = 0.9)에서 1 cm 떨어진, 진행 방향(-Z)으로 긴 벽.
        let wall = [GroundRect::axis_aligned(
            Vec2::new(0.91 + 0.5, -14.0),
            Vec2::new(1.0, 32.0),
        )];
        let mut state = ready_in_drive();
        let mut motion = Motion::default();
        let mut log = CalibrationLog::default();
        let mut contact = ContactState::default();
        let mut run = |input, steps| {
            drive_among(
                &mut state,
                &mut motion,
                &mut log,
                &mut contact,
                &wall,
                input,
                steps,
            );
        };
        run(input(1.0, 0.0), 64 * 6);
        run(input(0.0, 1.0), 64 * 3);
        assert!(contact.touching, "the wall should be within contact margin");
        assert!(!contact.blocked);
        let accel = log.last_accel().expect("accel beside wall is valid");
        assert!(accel.reached_limit);
        let stop = log.last_stop().expect("stop beside wall is valid");
        assert!((stop.from_speed_mps - 5.0).abs() < 1e-3);
        assert_eq!(log.trial_counts(), (1, 1));
    }

    /// 벽에 실제로 부딪혀 멈춘 제동 시험, 벽을 계속 미는 동안의 가속 시험은 결과로 남지 않는지 확인한다.
    #[test]
    fn trials_ended_by_impact_are_discarded() {
        // 앞범퍼(z = -3.5)에서 4 m 앞에 벽의 앞면(z = -7.5)이 있다.
        let wall = [GroundRect::axis_aligned(
            Vec2::new(0.0, -8.0),
            Vec2::new(6.0, 1.0),
        )];
        let mut state = ready_in_drive();
        let mut motion = Motion::default();
        let mut log = CalibrationLog::default();
        let mut contact = ContactState::default();
        // 약 3.4 m 가속 후 최대 제동: 정지거리 약 1 m라 벽에 막혀 멈춘다.
        drive_among(
            &mut state,
            &mut motion,
            &mut log,
            &mut contact,
            &wall,
            input(1.0, 0.0),
            166,
        );
        drive_among(
            &mut state,
            &mut motion,
            &mut log,
            &mut contact,
            &wall,
            input(0.0, 1.0),
            64 * 2,
        );
        assert_eq!(contact.reported, [0], "the wall should have been hit");
        assert_eq!(log.last_stop(), None);
        assert_eq!(log.trial_counts(), (0, 1), "only the released accel counts");

        // 이미 충돌을 보고한 벽을 정지 상태에서 계속 밀면 이동이 매 단계 막혀 가속 시험이 남지 않는다.
        let before = log.last_accel();
        drive_among(
            &mut state,
            &mut motion,
            &mut log,
            &mut contact,
            &wall,
            input(1.0, 0.0),
            64,
        );
        assert!(contact.blocked);
        assert_eq!(log.last_accel(), before);
        assert_eq!(log.active(), None);
    }

    /// 정지 직전에 제동을 풀어 관성 감속으로 멈추면 제동 시험 결과를 남기지 않는지 확인한다.
    #[test]
    fn release_just_before_stop_discards_trial() {
        let params = DynamicsParams::phase2_reference();
        let mut state = ready_in_drive();
        let mut motion = Motion::default();
        let mut log = CalibrationLog::default();
        drive(&mut state, &mut motion, &mut log, input(1.0, 0.0), 96);
        assert!((state.speed_mps() - 1.5).abs() < 1e-3);
        state.set_input(input(0.0, 1.0));
        // 다음 관성 감속 한 단계로 멈출 만큼 느려질 때까지 제동한다.
        while state.speed_mps() > params.coast_decel_mps2 * DT {
            step(&mut state, &mut motion, &params, DT);
            log.observe(&state, &motion, &params, false, DT);
        }
        assert!(
            state.speed_mps() > 0.0,
            "must still be moving before release"
        );
        drive(&mut state, &mut motion, &mut log, input(0.0, 0.0), 1);
        assert_eq!(state.speed_mps(), 0.0);
        assert_eq!(log.last_stop(), None);
        assert_eq!(log.active(), None);
        assert_eq!(log.trial_counts().0, 0);
    }

    /// 이동이 막힌 단계로 멈추면 제동 시험 결과를 남기지 않는지 확인한다.
    #[test]
    fn stop_against_obstacle_is_not_recorded() {
        let params = DynamicsParams::phase2_reference();
        let mut state = ready_in_drive();
        let mut motion = Motion::default();
        let mut log = CalibrationLog::default();
        drive(&mut state, &mut motion, &mut log, input(1.0, 0.0), 64 * 2);
        state.set_input(input(0.0, 1.0));
        step(&mut state, &mut motion, &params, DT);
        log.observe(&state, &motion, &params, false, DT);
        // 충돌로 막힌 것처럼 속도가 0이 되고 이동이 막힌 단계다.
        state.set_speed_mps(0.0);
        log.observe(&state, &motion, &params, true, DT);
        assert_eq!(log.last_stop(), None);
        assert_eq!(log.active(), None);
    }

    /// 비상 정지로 멈춘 제동 시험에는 비상 감속 포함 표시가 남는지 확인한다.
    #[test]
    fn emergency_stop_trial_is_marked() {
        let mut state = ready_in_drive();
        let mut motion = Motion::default();
        let mut log = CalibrationLog::default();
        drive(&mut state, &mut motion, &mut log, input(1.0, 0.0), 64 * 2);
        state.apply(VehicleCommand::EngageEmergencyStop).unwrap();
        drive(&mut state, &mut motion, &mut log, input(0.0, 0.0), 64 * 2);
        let result = log.last_stop().expect("emergency stop should be measured");
        assert!(result.emergency);
        // 비상 정지(5.0) + 관성(0.3) 감속.
        let speed = result.from_speed_mps;
        assert!((result.distance_m - speed * speed / (2.0 * 5.3)).abs() < speed * DT);
    }

    /// 기구학 회전반경이 L/tan(δ)이고 직진이면 없으며, 최소 회전반경은 최대 조향각 기준인지 확인한다.
    /// 실제 반경은 정지 부근에서 같고 고속에서 커지는지도 확인한다.
    #[test]
    fn turn_radius_follows_bicycle_model() {
        assert_eq!(turn_radius_m(2.8, 0.0), None);
        let radius = turn_radius_m(2.8, -30f32.to_radians()).unwrap();
        assert!((radius - 2.8 / 30f32.to_radians().tan()).abs() < 1e-5);
        let params = DynamicsParams::phase2_reference();
        assert!((min_turn_radius_m(&params) - radius).abs() < 1e-5);

        let params = DynamicsParams::default();
        let steer = -30f32.to_radians();
        assert_eq!(effective_turn_radius_m(&params, 0.0, 0.0), None);
        let slow = effective_turn_radius_m(&params, 0.0, steer).unwrap();
        assert!((slow - radius).abs() < 1e-4);
        let fast = effective_turn_radius_m(&params, params.max_forward_speed_mps, steer).unwrap();
        // 40 km/h, 횡가속도 상한 6 m/s²이면 v²/a ≈ 20.6 m.
        assert!(
            (fast - params.max_forward_speed_mps.powi(2) / 6.0).abs() < 0.01,
            "{fast}"
        );
    }

    /// 움직이는 중에는 복귀를 거부하고, 정지 상태에서는 시작 자세로 되돌리는지 확인한다.
    #[test]
    fn reset_requires_stop_and_restores_start_pose() {
        let mut state = ready_in_drive();
        let mut motion = Motion::default();
        let mut log = CalibrationLog::default();
        let mut contact = VehicleContact::default();
        drive(&mut state, &mut motion, &mut log, input(1.0, 0.0), 64);
        let moving = motion;
        // 원점이 아닌 시작 자세도 그대로 복원하는지 보기 위한 임의 자세.
        let start = Motion {
            x_m: 2.0,
            z_m: 1.5,
            heading_rad: 0.4,
            ..Motion::default()
        };
        let result = reset_to_start(&mut state, &mut motion, &start, &mut contact, &mut log);
        assert!(!result.accepted);
        assert_eq!(motion, moving);

        drive(&mut state, &mut motion, &mut log, input(0.0, 1.0), 64);
        contact.0.touching = true;
        let result = reset_to_start(&mut state, &mut motion, &start, &mut contact, &mut log);
        assert!(result.accepted, "{result:?}");
        assert_eq!(motion, start);
        assert_eq!(contact, VehicleContact::default());
        assert_eq!(state.gear(), Gear::Drive);
        assert!(log.last_stop().is_some(), "finished results are kept");
    }
}
