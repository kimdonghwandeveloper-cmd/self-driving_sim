//! 저속 차량 운동 모델(Phase 2 임시 기준 모델).
//!
//! 종방향은 가속·제동·관성 감속, 횡방향은 후륜축 기준 Kinematic Bicycle Model을 사용한다.
//! 모든 파라미터는 실차 검증 전 **시뮬레이터 임시값**이다.
//!
//! 단위는 위치 m, 속도 m/s, 가속도 m/s², 각도 rad로 통일한다(설정 파일의 각도만 도 단위).
//! 좌표는 Bevy 지면(x, z)이며, heading은 +Y축 회전각이다. heading 0일 때 전방은 -Z이고,
//! heading이 커지면 좌회전 방향이다. 조향각은 양수가 우회전이다.

use std::f32::consts::{PI, TAU};

use serde::Deserialize;

use super::state::{EngineState, Gear, VehicleState};

/// 구동력이 없을 때 이 속도(m/s) 미만이면 0으로 맞춰 정지 부근 떨림을 없앤다.
const REST_SPEED_MPS: f32 = 1.0e-3;
/// 최대 조향각 허용 상한(도). 90도에 가까우면 tan이 발산한다.
const MAX_STEER_ANGLE_LIMIT_DEG: f32 = 60.0;

/// `config/vehicle.toml`의 `[dynamics]` 구역. 운동 모델 파라미터.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DynamicsParams {
    /// D 최고 속도(m/s).
    pub max_forward_speed_mps: f32,
    /// R 최고 속도(m/s, 크기).
    pub max_reverse_speed_mps: f32,
    /// 가속 입력 1.0일 때 가속도(m/s²).
    pub max_accel_mps2: f32,
    /// 제동 입력 1.0일 때 감속도(m/s²).
    pub max_brake_decel_mps2: f32,
    /// 가속 입력이 없을 때(관성 주행·N) 감속도(m/s²).
    pub coast_decel_mps2: f32,
    /// 비상 정지 중 감속도(m/s²).
    pub emergency_decel_mps2: f32,
    /// 축간거리(m).
    pub wheelbase_m: f32,
    /// 최대 바퀴 조향각(도).
    pub max_steer_angle_deg: f32,
    /// 바퀴 조향각 변화 속도(도/s).
    pub steer_rate_deg_s: f32,
}

impl Default for DynamicsParams {
    /// 시뮬레이터 임시 기본값을 만든다(실차 미검증).
    fn default() -> Self {
        Self {
            max_forward_speed_mps: 5.0,
            max_reverse_speed_mps: 1.5,
            max_accel_mps2: 1.0,
            max_brake_decel_mps2: 3.0,
            coast_decel_mps2: 0.3,
            emergency_decel_mps2: 5.0,
            wheelbase_m: 2.8,
            max_steer_angle_deg: 30.0,
            steer_rate_deg_s: 60.0,
        }
    }
}

impl DynamicsParams {
    /// 모든 값이 유한한 양수인지, 최대 조향각이 상한 이하인지 검사한다.
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("max_forward_speed_mps", self.max_forward_speed_mps),
            ("max_reverse_speed_mps", self.max_reverse_speed_mps),
            ("max_accel_mps2", self.max_accel_mps2),
            ("max_brake_decel_mps2", self.max_brake_decel_mps2),
            ("coast_decel_mps2", self.coast_decel_mps2),
            ("emergency_decel_mps2", self.emergency_decel_mps2),
            ("wheelbase_m", self.wheelbase_m),
            ("max_steer_angle_deg", self.max_steer_angle_deg),
            ("steer_rate_deg_s", self.steer_rate_deg_s),
        ] {
            if !(value.is_finite() && value > 0.0) {
                return Err(format!(
                    "dynamics.{name} must be a positive number, got {value}"
                ));
            }
        }
        if self.max_steer_angle_deg > MAX_STEER_ANGLE_LIMIT_DEG {
            return Err(format!(
                "dynamics.max_steer_angle_deg must be at most {MAX_STEER_ANGLE_LIMIT_DEG}, got {}",
                self.max_steer_angle_deg
            ));
        }
        Ok(())
    }
}

/// 운동 모델이 적분하는 차량 자세와 바퀴 상태. 속도는 [`VehicleState`]가 가진다.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Motion {
    /// 후륜축 중심의 지면 x 좌표(m).
    pub x_m: f32,
    /// 후륜축 중심의 지면 z 좌표(m).
    pub z_m: f32,
    /// +Y축 회전각(rad, -π..=π). 0이면 -Z 방향을 본다.
    pub heading_rad: f32,
    /// 마지막 단계의 종방향 가속도(m/s², 전방 기준 부호).
    pub accel_mps2: f32,
    /// 실제 바퀴 조향각(rad, 양수=우). 입력을 조향 속도 제한으로 따라간다.
    pub steer_angle_rad: f32,
}

impl Motion {
    /// 차량 전방 단위 벡터(x, z)를 반환한다.
    pub fn forward(&self) -> (f32, f32) {
        let (sin, cos) = self.heading_rad.sin_cos();
        (-sin, -cos)
    }
}

/// 고정 시간 간격 `dt`(s)만큼 속도·조향각·자세를 갱신한다.
///
/// 속도가 잘못된 값(NaN/무한대)이면 적분하지 않고 그대로 둔다. 상태 머신이 이상 상태로 판정한다.
pub fn step(state: &mut VehicleState, motion: &mut Motion, params: &DynamicsParams, dt: f32) {
    if !(dt.is_finite() && dt > 0.0) || !state.speed_valid() {
        motion.accel_mps2 = 0.0;
        return;
    }
    let speed = state.speed_mps();
    let next_speed = longitudinal_speed(state, params, speed, dt);
    motion.accel_mps2 = (next_speed - speed) / dt;
    state.set_speed_mps(next_speed);

    let target = state.input().steering * params.max_steer_angle_deg.to_radians();
    let max_change = params.steer_rate_deg_s.to_radians() * dt;
    motion.steer_angle_rad += (target - motion.steer_angle_rad).clamp(-max_change, max_change);

    // 우조향(양수)이면 전진 시 heading이 줄어든다. 후진이면 속도 부호로 회전 방향이 자동으로 바뀐다.
    let yaw_rate = -next_speed * motion.steer_angle_rad.tan() / params.wheelbase_m;
    motion.heading_rad = wrap_angle(motion.heading_rad + yaw_rate * dt);
    let (forward_x, forward_z) = motion.forward();
    motion.x_m += forward_x * next_speed * dt;
    motion.z_m += forward_z * next_speed * dt;
}

/// 다음 단계의 부호 있는 속도를 계산한다.
///
/// - P는 주차 잠금으로 0을 유지한다(P는 정지 상태에서만 들어갈 수 있다).
/// - 구동 가속은 상태 머신이 허용한 가속 입력(`effective_throttle`)과 D/R 방향으로만 생긴다.
/// - 제동·관성·비상 정지 감속은 항상 속도를 0 쪽으로만 당기며 0을 넘지 않는다.
fn longitudinal_speed(state: &VehicleState, params: &DynamicsParams, speed: f32, dt: f32) -> f32 {
    if state.gear() == Gear::Park {
        return 0.0;
    }
    let drive_accel = match state.gear().drive_direction() {
        Some(direction) => direction * state.effective_throttle() * params.max_accel_mps2,
        None => 0.0,
    };
    let mut next = (speed + drive_accel * dt)
        .clamp(-params.max_reverse_speed_mps, params.max_forward_speed_mps);

    let mut decel = state.input().brake * params.max_brake_decel_mps2;
    if drive_accel == 0.0 {
        decel += params.coast_decel_mps2;
    }
    if state.engine() == EngineState::EmergencyStop {
        decel += params.emergency_decel_mps2;
    }
    next = approach_zero(next, decel * dt);
    if drive_accel == 0.0 && next.abs() < REST_SPEED_MPS {
        next = 0.0;
    }
    next
}

/// 값을 0 쪽으로 `amount`만큼 줄인다. 0을 넘어 부호가 바뀌지 않는다.
fn approach_zero(value: f32, amount: f32) -> f32 {
    if value > 0.0 {
        (value - amount).max(0.0)
    } else {
        (value + amount).min(0.0)
    }
}

/// 각도를 -π..=π 범위로 정규화한다.
fn wrap_angle(angle: f32) -> f32 {
    let wrapped = (angle + PI).rem_euclid(TAU) - PI;
    if wrapped == -PI { PI } else { wrapped }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::{TransitionError, VehicleCommand, VehicleInput};

    /// 테스트용 고정 시간 간격(s). Bevy 기본 FixedUpdate 주기(64Hz)와 같다.
    const DT: f32 = 1.0 / 64.0;

    /// 시동 Ready, 지정 기어, 정지 상태의 차량을 만든다.
    fn ready_in(gear: Gear) -> VehicleState {
        let mut state = VehicleState::default();
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        state.apply(VehicleCommand::RequestGear(gear)).unwrap();
        state
    }

    /// 가속·제동·조향 입력을 만든다.
    fn input(throttle: f32, brake: f32, steering: f32) -> VehicleInput {
        VehicleInput {
            throttle,
            brake,
            steering,
        }
    }

    /// 같은 입력으로 `seconds` 동안 `dt` 간격으로 적분한다.
    fn run(
        state: &mut VehicleState,
        motion: &mut Motion,
        seconds: f32,
        dt: f32,
        driver: VehicleInput,
    ) {
        state.set_input(driver);
        let steps = (seconds / dt).round() as usize;
        for _ in 0..steps {
            step(state, motion, &DynamicsParams::default(), dt);
        }
    }

    /// D에서 가속하면 전방(-Z)으로 움직이고 최고 속도를 넘지 않는지 확인한다.
    #[test]
    fn drive_accelerates_forward_up_to_limit() {
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        run(&mut state, &mut motion, 1.0, DT, input(1.0, 0.0, 0.0));
        assert!((state.speed_mps() - 1.0).abs() < 1e-3);
        assert!(motion.z_m < 0.0);

        run(&mut state, &mut motion, 20.0, DT, input(1.0, 0.0, 0.0));
        assert_eq!(
            state.speed_mps(),
            DynamicsParams::default().max_forward_speed_mps
        );
    }

    /// R에서 가속하면 후방(+Z)으로 움직이고 후진 최고 속도를 넘지 않는지 확인한다.
    #[test]
    fn reverse_moves_backward_up_to_limit() {
        let mut state = ready_in(Gear::Reverse);
        let mut motion = Motion::default();
        run(&mut state, &mut motion, 10.0, DT, input(1.0, 0.0, 0.0));
        assert_eq!(
            state.speed_mps(),
            -DynamicsParams::default().max_reverse_speed_mps
        );
        assert!(motion.z_m > 0.0);
    }

    /// 제동하면 0까지 줄고 반대 방향으로 넘어가지 않는지 확인한다(D·R 모두).
    #[test]
    fn brake_stops_without_reversing() {
        for gear in [Gear::Drive, Gear::Reverse] {
            let mut state = ready_in(gear);
            let mut motion = Motion::default();
            run(&mut state, &mut motion, 2.0, DT, input(1.0, 0.0, 0.0));
            assert!(state.speed_mps() != 0.0);
            run(&mut state, &mut motion, 3.0, DT, input(0.0, 1.0, 0.0));
            assert_eq!(state.speed_mps(), 0.0, "{gear:?}");
            assert!(state.is_stopped());
        }
    }

    /// 가속 입력 없이 N으로 두면 관성 감속으로 정지하는지 확인한다.
    #[test]
    fn neutral_coasts_to_stop() {
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        run(&mut state, &mut motion, 2.0, DT, input(1.0, 0.0, 0.0));
        state
            .apply(VehicleCommand::RequestGear(Gear::Neutral))
            .unwrap();
        run(&mut state, &mut motion, 1.0, DT, input(1.0, 0.0, 0.0));
        assert!(state.speed_mps() > 0.0 && state.speed_mps() < 2.0);
        run(&mut state, &mut motion, 10.0, DT, input(1.0, 0.0, 0.0));
        assert_eq!(state.speed_mps(), 0.0);
    }

    /// 비상 정지 중에는 가속 입력을 무시하고 비상 감속으로 정지하는지 확인한다.
    #[test]
    fn emergency_stop_decelerates_to_zero() {
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        run(&mut state, &mut motion, 3.0, DT, input(1.0, 0.0, 0.0));
        state.apply(VehicleCommand::EngageEmergencyStop).unwrap();
        run(&mut state, &mut motion, 0.25, DT, input(1.0, 0.0, 0.0));
        assert!(motion.accel_mps2 < -5.0);
        run(&mut state, &mut motion, 1.0, DT, input(1.0, 0.0, 0.0));
        assert_eq!(state.speed_mps(), 0.0);
    }

    /// P와 시동 OFF에서는 가속 입력이 있어도 움직이지 않는지 확인한다.
    #[test]
    fn park_and_engine_off_do_not_move() {
        let mut parked = ready_in(Gear::Park);
        let mut off = VehicleState::default();
        for state in [&mut parked, &mut off] {
            let mut motion = Motion::default();
            run(state, &mut motion, 2.0, DT, input(1.0, 0.0, 1.0));
            assert_eq!(state.speed_mps(), 0.0);
            assert_eq!((motion.x_m, motion.z_m), (0.0, 0.0));
        }
    }

    /// 실제로 움직인 뒤에는 상태 머신이 D↔R 변경을 거부하는지 확인한다.
    #[test]
    fn moving_vehicle_rejects_drive_reverse_change() {
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        run(&mut state, &mut motion, 1.0, DT, input(1.0, 0.0, 0.0));
        assert_eq!(
            state.apply(VehicleCommand::RequestGear(Gear::Reverse)),
            Err(TransitionError::NotStopped)
        );

        run(&mut state, &mut motion, 2.0, DT, input(0.0, 1.0, 0.0));
        assert!(
            state
                .apply(VehicleCommand::RequestGear(Gear::Reverse))
                .is_ok()
        );
    }

    /// 전진 우조향은 오른쪽(+X)으로, 후진 우조향은 반대로 회전하는지 확인한다.
    #[test]
    fn steering_turns_in_expected_direction() {
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        run(&mut state, &mut motion, 3.0, DT, input(1.0, 0.0, 1.0));
        assert!(motion.heading_rad < 0.0);
        assert!(motion.x_m > 0.0);

        let mut state = ready_in(Gear::Reverse);
        let mut motion = Motion::default();
        run(&mut state, &mut motion, 3.0, DT, input(1.0, 0.0, 1.0));
        assert!(motion.heading_rad > 0.0);
    }

    /// 정지 상태 조향은 바퀴 각도만 바꾸고 차량 방향·위치는 그대로인지 확인한다.
    #[test]
    fn steering_while_stopped_only_turns_wheels() {
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        run(&mut state, &mut motion, 1.0, DT, input(0.0, 0.0, -1.0));
        assert!(motion.steer_angle_rad < 0.0);
        assert_eq!(motion.heading_rad, 0.0);
        assert_eq!((motion.x_m, motion.z_m), (0.0, 0.0));
    }

    /// 바퀴 조향각이 조향 속도 제한으로 천천히 따라가고 최대 각도에서 멈추는지 확인한다.
    #[test]
    fn steer_angle_is_rate_limited() {
        let params = DynamicsParams::default();
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        run(&mut state, &mut motion, 0.25, DT, input(0.0, 0.0, 1.0));
        let expected = (params.steer_rate_deg_s * 0.25).to_radians();
        assert!((motion.steer_angle_rad - expected).abs() < 1e-4);

        run(&mut state, &mut motion, 2.0, DT, input(0.0, 0.0, 1.0));
        assert!((motion.steer_angle_rad - params.max_steer_angle_deg.to_radians()).abs() < 1e-5);
    }

    /// 시간 간격이 달라도 같은 시간 동안의 주행 거리가 거의 같은지 확인한다.
    #[test]
    fn distance_is_independent_of_timestep() {
        let distance = |dt: f32| {
            let mut state = ready_in(Gear::Drive);
            let mut motion = Motion::default();
            run(&mut state, &mut motion, 4.0, dt, input(1.0, 0.0, 0.0));
            run(&mut state, &mut motion, 2.0, dt, input(0.0, 0.5, 0.0));
            -motion.z_m
        };
        let coarse = distance(1.0 / 30.0);
        let fine = distance(1.0 / 120.0);
        assert!((coarse - fine).abs() / fine < 0.02, "{coarse} vs {fine}");
    }

    /// 같은 입력 순서를 두 번 재생하면 결과가 정확히 같은지 확인한다.
    #[test]
    fn replay_is_deterministic() {
        let replay = || {
            let mut state = ready_in(Gear::Drive);
            let mut motion = Motion::default();
            run(&mut state, &mut motion, 2.0, DT, input(1.0, 0.0, 0.6));
            run(&mut state, &mut motion, 1.5, DT, input(0.3, 0.0, -0.8));
            run(&mut state, &mut motion, 2.0, DT, input(0.0, 0.7, 0.0));
            (state, motion)
        };
        assert_eq!(replay(), replay());
    }

    /// 잘못된 속도 값에서는 적분하지 않고 자세를 그대로 두는지 확인한다.
    #[test]
    fn invalid_speed_is_not_integrated() {
        let mut state = ready_in(Gear::Drive);
        state.set_speed_mps(f32::NAN);
        let mut motion = Motion::default();
        run(&mut state, &mut motion, 1.0, DT, input(1.0, 0.0, 1.0));
        assert_eq!(
            (motion.x_m, motion.z_m, motion.heading_rad),
            (0.0, 0.0, 0.0)
        );
        assert!(!state.speed_valid());
    }

    /// 0 이하·NaN 파라미터와 너무 큰 조향각을 거부하는지 확인한다.
    #[test]
    fn invalid_params_are_rejected() {
        assert!(DynamicsParams::default().validate().is_ok());
        for params in [
            DynamicsParams {
                wheelbase_m: 0.0,
                ..DynamicsParams::default()
            },
            DynamicsParams {
                max_accel_mps2: f32::NAN,
                ..DynamicsParams::default()
            },
            DynamicsParams {
                max_steer_angle_deg: 80.0,
                ..DynamicsParams::default()
            },
        ] {
            assert!(params.validate().is_err(), "{params:?}");
        }
    }

    /// 각도 정규화가 -π..=π 범위를 유지하는지 확인한다.
    #[test]
    fn wrap_angle_stays_in_range() {
        assert!((wrap_angle(3.0 * PI / 2.0) + PI / 2.0).abs() < 1e-5);
        assert!((wrap_angle(-3.0 * PI / 2.0) - PI / 2.0).abs() < 1e-5);
        assert_eq!(wrap_angle(PI), PI);
    }
}
