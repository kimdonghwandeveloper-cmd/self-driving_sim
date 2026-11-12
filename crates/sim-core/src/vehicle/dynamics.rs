//! 차량 운동 모델(Phase 2 임시 기준 모델, 2-6 중속 보강).
//!
//! 종방향은 출력 한계가 있는 구동 가속, 구름·공기 저항, 제동·관성·비상 감속을 쓴다.
//! 횡방향은 후륜축 기준 Kinematic Bicycle Model에 정상 상태 언더스티어 식과 횡가속도 상한을 더한다.
//! 전진 최고 속도(40 km/h)만 사용자가 알려 준 실차 사양이고, 나머지 파라미터는 실차 검증 전
//! **시뮬레이터 임시값**이다.
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
/// 중력 가속도(m/s²).
const GRAVITY_MPS2: f32 = 9.80665;
/// 공기 밀도(kg/m³). 15 °C 해수면 표준 대기 값.
const AIR_DENSITY_KG_M3: f32 = 1.225;

/// 조향 입력이 중립(0)일 때 바퀴 조향각 처리 방식. 실차 동작 미확정이라 설정으로 고른다.
///
/// 조향 입력이 있는 동안에는 방식과 관계없이 입력 위치가 목표 조향각이다. 따라서 `Hold`에서 반대쪽
/// 입력을 주면 바퀴는 가운데에서 멈추지 않고 반대쪽 목표 각도까지 움직인다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SteerReturn {
    /// 입력을 놓으면 `steer_return_rate_deg_s` 속도로 0도로 돌아간다(기본값, 기존 동작).
    #[default]
    Auto,
    /// 입력을 놓으면 현재 바퀴 각도를 유지한다.
    Hold,
}

/// `config/vehicle.toml`의 `[dynamics]` 구역. 운동 모델 파라미터.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DynamicsParams {
    /// D 최고 속도(m/s). 기본 11.11 m/s(40 km/h)는 실차 사양.
    pub max_forward_speed_mps: f32,
    /// R 최고 속도(m/s, 크기).
    pub max_reverse_speed_mps: f32,
    /// 가속 입력 1.0일 때 저속 구동 가속도 상한(m/s²). 속도가 오르면 출력 한계로 줄어든다.
    pub max_accel_mps2: f32,
    /// 차량 질량(kg). 출력·공기 저항을 가속도로 바꿀 때 쓴다.
    pub mass_kg: f32,
    /// 최대 구동 출력(kW). 구동 가속도는 `출력 ÷ (질량 × 속도)`를 넘지 않는다.
    pub max_drive_power_kw: f32,
    /// 구름 저항 계수(무차원, 0 허용). 움직이는 동안 `계수 × g`의 감속 성분이 된다.
    pub rolling_resistance_coeff: f32,
    /// 공기 저항 면적 Cd·A(m², 0 허용). 감속 성분은 `½ρ·CdA·v² ÷ 질량`이다.
    pub drag_area_m2: f32,
    /// 제동 입력 1.0일 때 더하는 감속 성분(m/s²).
    ///
    /// 제동·관성·비상·구름/공기 저항 감속은 최종 감속도가 아니라 **더해지는 성분**이다. 기본값·저속에서 가속 입력 없이
    /// 최대 제동하면 3.0 + 0.15 + 0.147(구름 저항) ≈ 3.3, 비상 정지만이면 5.0 + 0.15 + 0.147 ≈ 5.3, 둘 다면 약 8.3 m/s²로
    /// 감속한다. 속도가 높으면 공기 저항이 조금 더해진다.
    pub max_brake_decel_mps2: f32,
    /// 가속 입력이 없을 때(관성 주행·N) 더하는 감속 성분(m/s²). 구름·공기 저항과 별도로 더해진다.
    /// 0이고 저항도 0이면 감속 없이 관성으로 계속 굴러간다.
    pub coast_decel_mps2: f32,
    /// 비상 정지 중 더하는 감속 성분(m/s²).
    pub emergency_decel_mps2: f32,
    /// 축간거리(m).
    pub wheelbase_m: f32,
    /// 최대 바퀴 조향각(도).
    pub max_steer_angle_deg: f32,
    /// 바퀴 조향각 변화 속도(도/s).
    pub steer_rate_deg_s: f32,
    /// 언더스티어 계수 K(rad/g, 0 허용). 곡률은 `tan δ ÷ (L + K·v²/g)`이며 0이면 기존 기구학 모델과 같다.
    pub understeer_gradient_rad_per_g: f32,
    /// 횡가속도 상한(m/s²). 곡률이 `상한 ÷ v²`를 넘지 않아 고속 급회전이 불가능하다.
    pub max_lateral_accel_mps2: f32,
    /// 조향 감도(배율). 조향 입력에 곱한 뒤 -1..=1로 자른다.
    /// 1.0이면 입력 그대로, 1보다 작으면 최대 입력에서도 최대 조향각보다 덜 꺾이고,
    /// 1보다 크면 작은 입력으로 최대 조향각에 먼저 도달한다.
    pub steer_sensitivity: f32,
    /// 조향 입력이 중립일 때 처리 방식(`"auto"` 또는 `"hold"`).
    pub steer_return: SteerReturn,
    /// `auto`에서 입력을 놓았을 때 0도로 돌아가는 속도(도/s). `hold`에서는 쓰지 않는다.
    pub steer_return_rate_deg_s: f32,
    /// 수직축 회전 관성(kg·m², 2-7). 충돌 충격량이 차를 얼마나 돌리는지 정한다. 기본은 질량 3000 kg 직사각형 차체
    /// 4.2×1.8 m의 `m(L²+W²)/12` ≈ 5220.
    pub yaw_inertia_kg_m2: f32,
    /// 충돌 반발 계수(0..=1, 2-7). 0이면 튕기지 않고, 1이면 닿는 방향 속도를 그대로 되돌린다.
    pub restitution: f32,
    /// 닿는 방향 속도가 이보다 느리면(m/s, 0 허용) 반발 없이 붙는다(2-7). 벽을 계속 밀 때 떨림을 막는다.
    pub bounce_min_speed_mps: f32,
    /// 차체–장애물 마찰 계수(0 허용, 2-7). 벽을 따라 미끄러질 때 접선 방향 속도를 줄인다.
    pub body_friction_coeff: f32,
    /// 타이어 옆방향 마찰 계수(2-7). 충돌로 생긴 옆 미끄러짐·회전을 `계수 × g`로 줄여 평소 주행으로 되돌린다.
    pub tire_lateral_friction_coeff: f32,
}

impl Default for DynamicsParams {
    /// 시뮬레이터 임시 기본값을 만든다(실차 미검증).
    fn default() -> Self {
        Self {
            max_forward_speed_mps: 40.0 / 3.6,
            max_reverse_speed_mps: 1.5,
            max_accel_mps2: 1.0,
            mass_kg: 3000.0,
            max_drive_power_kw: 20.0,
            rolling_resistance_coeff: 0.015,
            drag_area_m2: 3.5,
            max_brake_decel_mps2: 3.0,
            coast_decel_mps2: 0.15,
            emergency_decel_mps2: 5.0,
            wheelbase_m: 2.8,
            max_steer_angle_deg: 30.0,
            steer_rate_deg_s: 60.0,
            understeer_gradient_rad_per_g: 0.03,
            max_lateral_accel_mps2: 6.0,
            steer_sensitivity: 1.0,
            steer_return: SteerReturn::Auto,
            steer_return_rate_deg_s: 60.0,
            yaw_inertia_kg_m2: 5220.0,
            restitution: 0.2,
            bounce_min_speed_mps: 0.5,
            body_friction_coeff: 0.3,
            tire_lateral_friction_coeff: 0.7,
        }
    }
}

impl DynamicsParams {
    /// 모든 값이 유한한 양수인지(관성 감속·저항·언더스티어 계수는 0 허용), 최대 조향각이 상한 이하인지 검사한다.
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("coast_decel_mps2", self.coast_decel_mps2),
            ("rolling_resistance_coeff", self.rolling_resistance_coeff),
            ("drag_area_m2", self.drag_area_m2),
            (
                "understeer_gradient_rad_per_g",
                self.understeer_gradient_rad_per_g,
            ),
            ("bounce_min_speed_mps", self.bounce_min_speed_mps),
            ("body_friction_coeff", self.body_friction_coeff),
        ] {
            if !(value.is_finite() && value >= 0.0) {
                return Err(format!(
                    "dynamics.{name} must be zero or a positive number, got {value}"
                ));
            }
        }
        for (name, value) in [
            ("max_forward_speed_mps", self.max_forward_speed_mps),
            ("max_reverse_speed_mps", self.max_reverse_speed_mps),
            ("max_accel_mps2", self.max_accel_mps2),
            ("mass_kg", self.mass_kg),
            ("max_drive_power_kw", self.max_drive_power_kw),
            ("max_lateral_accel_mps2", self.max_lateral_accel_mps2),
            ("max_brake_decel_mps2", self.max_brake_decel_mps2),
            ("emergency_decel_mps2", self.emergency_decel_mps2),
            ("wheelbase_m", self.wheelbase_m),
            ("max_steer_angle_deg", self.max_steer_angle_deg),
            ("steer_rate_deg_s", self.steer_rate_deg_s),
            ("steer_sensitivity", self.steer_sensitivity),
            ("steer_return_rate_deg_s", self.steer_return_rate_deg_s),
            ("yaw_inertia_kg_m2", self.yaw_inertia_kg_m2),
            (
                "tire_lateral_friction_coeff",
                self.tire_lateral_friction_coeff,
            ),
        ] {
            if !(value.is_finite() && value > 0.0) {
                return Err(format!(
                    "dynamics.{name} must be a positive number, got {value}"
                ));
            }
        }
        if !(0.0..=1.0).contains(&self.restitution) {
            return Err(format!(
                "dynamics.restitution must be between 0 and 1, got {}",
                self.restitution
            ));
        }
        if self.max_steer_angle_deg > MAX_STEER_ANGLE_LIMIT_DEG {
            return Err(format!(
                "dynamics.max_steer_angle_deg must be at most {MAX_STEER_ANGLE_LIMIT_DEG}, got {}",
                self.max_steer_angle_deg
            ));
        }
        Ok(())
    }

    /// 구름 저항이 만드는 감속도 크기(m/s², `계수 × g`)를 계산한다. 속도와 무관하다.
    pub fn rolling_decel_mps2(&self) -> f32 {
        self.rolling_resistance_coeff * GRAVITY_MPS2
    }

    /// 속도 `speed_mps`(부호 무관)에서 구름·공기 저항이 만드는 감속도 크기(m/s²)를 계산한다. 정지 중이면 0.
    pub fn resistance_decel_mps2(&self, speed_mps: f32) -> f32 {
        if speed_mps == 0.0 {
            return 0.0;
        }
        let drag =
            0.5 * AIR_DENSITY_KG_M3 * self.drag_area_m2 * speed_mps * speed_mps / self.mass_kg;
        self.rolling_decel_mps2() + drag
    }

    /// 가속 입력 1.0일 때 속도 `speed_mps`에서 낼 수 있는 구동 가속도 크기(m/s²)를 계산한다.
    ///
    /// 저속에서는 `max_accel_mps2`, 속도가 오르면 `출력 ÷ (질량 × 속도)`로 줄어든다.
    pub fn drive_accel_limit_mps2(&self, speed_mps: f32) -> f32 {
        let speed = speed_mps.abs();
        if speed == 0.0 {
            return self.max_accel_mps2;
        }
        let power_limited = self.max_drive_power_kw * 1000.0 / (self.mass_kg * speed);
        self.max_accel_mps2.min(power_limited)
    }

    /// 속도 `speed_mps`와 바퀴 조향각 `steer_angle_rad`에서 후륜축 경로의 곡률(1/m, 양수=우회전)을 계산한다.
    ///
    /// 정상 상태 언더스티어 식 `tan δ ÷ (L + K·v²/g)`에 횡가속도 상한 `a_lat ÷ v²`를 적용한다.
    /// 정지 부근에서는 기구학 모델 `tan δ ÷ L`과 같다.
    pub fn path_curvature(&self, speed_mps: f32, steer_angle_rad: f32) -> f32 {
        let speed_sq = speed_mps * speed_mps;
        let curvature = steer_angle_rad.tan()
            / (self.wheelbase_m + self.understeer_gradient_rad_per_g * speed_sq / GRAVITY_MPS2);
        if speed_sq == 0.0 {
            return curvature;
        }
        let limit = self.max_lateral_accel_mps2 / speed_sq;
        curvature.clamp(-limit, limit)
    }
}

// 루트 앱 테스트(보정 도구)도 쓰므로 crate 밖에 공개한다(GD-1a). 실제 주행 설정에는 쓰지 않는다.
impl DynamicsParams {
    /// 테스트용 Phase 2 기준 모델 값을 만든다: 최고 5 m/s, 관성 감속 0.3, 저항·출력 한계·언더스티어·
    /// 횡가속도 상한 없음. 운동 모델이 아닌 충돌·보정 로직을 예전 수치로 확인할 때 쓴다.
    #[doc(hidden)]
    pub fn phase2_reference() -> Self {
        Self {
            max_forward_speed_mps: 5.0,
            coast_decel_mps2: 0.3,
            rolling_resistance_coeff: 0.0,
            drag_area_m2: 0.0,
            max_drive_power_kw: 1.0e6,
            understeer_gradient_rad_per_g: 0.0,
            max_lateral_accel_mps2: 1.0e6,
            ..Self::default()
        }
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
    /// 충돌로 생긴 후륜축의 옆 미끄러짐 속도(m/s, 양수=오른쪽, 2-7). 평소 주행에서는 0이다.
    pub lateral_mps: f32,
    /// 충돌로 생긴, 조향이 정하는 회전 외의 추가 회전 속도(rad/s, heading 증가 방향=왼쪽, 2-7). 평소 주행에서는 0이다.
    pub slip_yaw_rate_rad_s: f32,
}

impl Motion {
    /// 차량 전방 단위 벡터(x, z)를 반환한다.
    pub fn forward(&self) -> (f32, f32) {
        let (sin, cos) = self.heading_rad.sin_cos();
        (-sin, -cos)
    }

    /// 차량 오른쪽 단위 벡터(x, z)를 반환한다.
    pub fn right(&self) -> (f32, f32) {
        let (sin, cos) = self.heading_rad.sin_cos();
        (cos, -sin)
    }

    /// 충돌로 생긴 옆 미끄러짐이나 추가 회전이 남아 있는지 반환한다.
    pub fn sliding(&self) -> bool {
        self.lateral_mps != 0.0 || self.slip_yaw_rate_rad_s != 0.0
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

    if let Some((target, rate_deg_s)) =
        steer_target(state.input().steering, motion.steer_angle_rad, params)
    {
        let max_change = rate_deg_s.to_radians() * dt;
        motion.steer_angle_rad += (target - motion.steer_angle_rad).clamp(-max_change, max_change);
    }

    // 우조향(양수)이면 전진 시 heading이 줄어든다. 후진이면 속도 부호로 회전 방향이 자동으로 바뀐다.
    let yaw_rate = steering_yaw_rate(params, next_speed, motion.steer_angle_rad);
    motion.heading_rad = wrap_angle(motion.heading_rad + yaw_rate * dt);
    let (forward_x, forward_z) = motion.forward();
    motion.x_m += forward_x * next_speed * dt;
    motion.z_m += forward_z * next_speed * dt;
    // 충돌로 생긴 미끄러짐이 없으면 아래를 건너뛰어 평소 주행 결과가 예전과 비트 단위로 같다.
    if motion.sliding() {
        slide(motion, params, dt);
    }
}

/// 충돌로 생긴 옆 미끄러짐·추가 회전만큼 자세를 옮기고, 타이어 옆방향 마찰로 두 값을 0 쪽으로 줄인다(2-7).
///
/// 옆 미끄러짐은 `μ·g`로 줄인다. 추가 회전은 앞·뒤 바퀴가 무게중심에서 축간거리 절반 떨어져 반씩 무게를 받는다고 보고
/// `μ·m·g·(L/2) ÷ I`로 줄인다. 둘 다 0을 넘어 반대로 돌지 않는다. 실차 타이어 특성을 반영하지 않은 임시 모델이다.
fn slide(motion: &mut Motion, params: &DynamicsParams, dt: f32) {
    motion.heading_rad = wrap_angle(motion.heading_rad + motion.slip_yaw_rate_rad_s * dt);
    let (right_x, right_z) = motion.right();
    motion.x_m += right_x * motion.lateral_mps * dt;
    motion.z_m += right_z * motion.lateral_mps * dt;
    let grip = params.tire_lateral_friction_coeff * GRAVITY_MPS2;
    let yaw_grip = grip * params.mass_kg * params.wheelbase_m / 2.0 / params.yaw_inertia_kg_m2;
    motion.lateral_mps = approach_zero(motion.lateral_mps, grip * dt);
    motion.slip_yaw_rate_rad_s = approach_zero(motion.slip_yaw_rate_rad_s, yaw_grip * dt);
}

/// 속도 `speed_mps`·조향각 `steer_angle_rad`에서 조향이 정하는 회전 속도(rad/s, heading 증가 방향)를 계산한다.
///
/// [`step`]이 자세를 돌리는 식과 같다. 우조향(양수)이면 전진 시 heading이 줄어든다.
pub fn steering_yaw_rate(params: &DynamicsParams, speed_mps: f32, steer_angle_rad: f32) -> f32 {
    -speed_mps * params.path_curvature(speed_mps, steer_angle_rad)
}

/// 조향 입력(-1..=1)에 조향 감도를 곱해 -1..=1 범위의 조향 지령으로 바꾼다.
fn steering_command(steering: f32, params: &DynamicsParams) -> f32 {
    (steering * params.steer_sensitivity).clamp(-1.0, 1.0)
}

/// 이번 단계의 목표 바퀴 조향각(rad)과 그쪽으로 움직이는 속도(도/s)를 정한다.
///
/// 입력이 있으면 입력 위치의 목표 각도로 `steer_rate_deg_s` 속도로 움직인다. 입력이 중립이면
/// `auto`는 0도로 `steer_return_rate_deg_s` 속도로 돌아가고, `hold`는 각도를 바꾸지 않는다(`None`).
fn steer_target(steering: f32, current_rad: f32, params: &DynamicsParams) -> Option<(f32, f32)> {
    let command = steering_command(steering, params);
    if command != 0.0 {
        return Some((
            command * params.max_steer_angle_deg.to_radians(),
            params.steer_rate_deg_s,
        ));
    }
    match params.steer_return {
        SteerReturn::Auto if current_rad != 0.0 => Some((0.0, params.steer_return_rate_deg_s)),
        SteerReturn::Auto | SteerReturn::Hold => None,
    }
}

/// 다음 단계의 부호 있는 속도를 계산한다.
///
/// - P는 주차 잠금으로 0을 유지한다(P는 정지 상태에서만 들어갈 수 있다).
/// - 구동 가속은 상태 머신이 허용한 가속 입력(`effective_throttle`)과 D/R 방향으로만 생기며,
///   크기는 출력 한계(`drive_accel_limit_mps2`)를 따른다.
/// - 제동·관성·비상 정지·구름/공기 저항 감속은 각 성분을 더한 값으로, 항상 속도를 0 쪽으로만 당기며
///   0을 넘지 않는다. 저항 때문에 차가 반대로 움직이지 않는다.
/// - 최고 속도 상한은 감속을 적용한 뒤에 자르므로, 구동이 저항보다 크면 정확히 상한을 유지한다.
///   충돌로 튕겨 이미 상한보다 빠른 속도는 자르지 않는다(감속으로만 줄어든다, 2-7).
fn longitudinal_speed(state: &VehicleState, params: &DynamicsParams, speed: f32, dt: f32) -> f32 {
    if state.gear() == Gear::Park {
        return 0.0;
    }
    let drive_accel = match state.gear().drive_direction() {
        Some(direction) => {
            direction * state.effective_throttle() * params.drive_accel_limit_mps2(speed)
        }
        None => 0.0,
    };
    let mut next = speed + drive_accel * dt;

    let mut decel =
        state.input().brake * params.max_brake_decel_mps2 + params.resistance_decel_mps2(speed);
    if drive_accel == 0.0 {
        decel += params.coast_decel_mps2;
    }
    if state.engine() == EngineState::EmergencyStop {
        decel += params.emergency_decel_mps2;
    }
    // 최고 속도는 구동으로 넘지 못하게 하는 한계다. 충돌로 튕겨 이미 그보다 빠르면(2-7) 자르지 않고 감속으로만 줄인다.
    // 평소에는 속도가 늘 범위 안이므로 결과가 예전과 같다.
    next = approach_zero(next, decel * dt).clamp(
        (-params.max_reverse_speed_mps).min(speed),
        params.max_forward_speed_mps.max(speed),
    );
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
pub(crate) fn wrap_angle(angle: f32) -> f32 {
    let wrapped = (angle + PI).rem_euclid(TAU) - PI;
    if wrapped == -PI { PI } else { wrapped }
}

/// 최대 조향각에서의 최소 회전 반경(m, 후륜축 중심 기준)을 자전거 모델로 계산한다.
pub fn min_turn_radius_m(params: &DynamicsParams) -> f32 {
    params.wheelbase_m / params.max_steer_angle_deg.to_radians().tan()
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

    /// D에서 가속하면 전방(-Z)으로 움직이고, 저속에서는 `최대 가속 − 저항`으로 가속하며
    /// 최고 속도(40 km/h)에 도달한 뒤에는 넘지 않는지 확인한다.
    #[test]
    fn drive_accelerates_forward_up_to_limit() {
        let params = DynamicsParams::default();
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        run(&mut state, &mut motion, 1.0, DT, input(1.0, 0.0, 0.0));
        let expected = params.max_accel_mps2 - params.rolling_decel_mps2();
        assert!(
            (state.speed_mps() - expected).abs() < 0.01,
            "{}",
            state.speed_mps()
        );
        assert!(motion.z_m < 0.0);

        run(&mut state, &mut motion, 60.0, DT, input(1.0, 0.0, 0.0));
        assert_eq!(state.speed_mps(), params.max_forward_speed_mps);
        assert!((params.max_forward_speed_mps * 3.6 - 40.0).abs() < 1e-4);
    }

    /// 출력 한계 때문에 고속에서는 같은 가속 입력으로도 가속도가 줄어드는지 확인한다.
    #[test]
    fn power_limit_reduces_accel_at_speed() {
        let params = DynamicsParams::default();
        assert_eq!(params.drive_accel_limit_mps2(0.0), params.max_accel_mps2);
        // 20 kW ÷ (3 000 kg × 10 m/s) ≈ 0.667 m/s².
        assert!((params.drive_accel_limit_mps2(10.0) - 20.0 / 30.0).abs() < 1e-4);
        assert_eq!(
            params.drive_accel_limit_mps2(-10.0),
            params.drive_accel_limit_mps2(10.0)
        );

        let accel_at = |speed: f32| {
            let mut state = ready_in(Gear::Drive);
            state.set_speed_mps(speed);
            state.set_input(input(1.0, 0.0, 0.0));
            let mut motion = Motion::default();
            step(&mut state, &mut motion, &params, DT);
            motion.accel_mps2
        };
        assert!(accel_at(2.0) > 0.8, "{}", accel_at(2.0));
        assert!(accel_at(10.0) < 0.5, "{}", accel_at(10.0));
        assert!(accel_at(10.0) > 0.0);
    }

    /// 구름·공기 저항이 관성 주행 감속에 더해지고, 차를 반대 방향으로 밀지 않는지 확인한다(D·R·N).
    #[test]
    fn resistance_slows_without_reversing() {
        let params = DynamicsParams::default();
        let mut state = ready_in(Gear::Drive);
        state.set_speed_mps(10.0);
        state.set_input(input(0.0, 0.0, 0.0));
        let mut motion = Motion::default();
        step(&mut state, &mut motion, &params, DT);
        let expected = params.coast_decel_mps2 + params.resistance_decel_mps2(10.0);
        assert!(
            (-motion.accel_mps2 - expected).abs() < 1e-3,
            "{}",
            motion.accel_mps2
        );
        assert!(params.resistance_decel_mps2(10.0) > params.resistance_decel_mps2(2.0));
        assert_eq!(params.resistance_decel_mps2(0.0), 0.0);

        for (gear, speed) in [(Gear::Drive, 0.01), (Gear::Reverse, -0.01)] {
            let mut state = ready_in(gear);
            state.set_speed_mps(speed);
            state
                .apply(VehicleCommand::RequestGear(Gear::Neutral))
                .unwrap();
            state.set_input(input(0.0, 0.0, 0.0));
            let mut motion = Motion::default();
            for _ in 0..64 {
                step(&mut state, &mut motion, &params, DT);
                assert!(state.speed_mps() * speed >= 0.0, "{gear:?}");
            }
            assert_eq!(state.speed_mps(), 0.0, "{gear:?}");
        }
    }

    /// 정지 부근에서는 곡률이 기구학 모델 `tan δ ÷ L`과 같고, 속도가 오르면 언더스티어로 줄어드는지 확인한다.
    #[test]
    fn curvature_matches_kinematic_at_low_speed() {
        let params = DynamicsParams::default();
        let steer = 10f32.to_radians();
        let kinematic = steer.tan() / params.wheelbase_m;
        assert_eq!(params.path_curvature(0.0, steer), kinematic);
        assert!((params.path_curvature(0.5, steer) / kinematic - 1.0).abs() < 1e-3);
        let fast = params.path_curvature(8.0, steer);
        assert!(fast < kinematic && fast > 0.0, "{fast} vs {kinematic}");
        assert_eq!(params.path_curvature(8.0, -steer), -fast);
    }

    /// 40 km/h에서 끝까지 조향해도 실제 횡가속도(속도 × 회전 각속도)가 상한을 넘지 않는지 확인한다.
    #[test]
    fn lateral_accel_is_limited_at_top_speed() {
        let params = DynamicsParams::default();
        let mut state = ready_in(Gear::Drive);
        state.set_speed_mps(params.max_forward_speed_mps);
        state.set_input(input(1.0, 0.0, 1.0));
        let mut motion = Motion {
            steer_angle_rad: params.max_steer_angle_deg.to_radians(),
            ..Motion::default()
        };
        let before = motion.heading_rad;
        step(&mut state, &mut motion, &params, DT);
        let yaw_rate = (motion.heading_rad - before) / DT;
        let lateral = (state.speed_mps() * yaw_rate).abs();
        assert!(
            lateral <= params.max_lateral_accel_mps2 * 1.001,
            "{lateral}"
        );
        // 같은 조향각의 기구학 모델이라면 약 25 m/s²였다.
        let kinematic =
            state.speed_mps().powi(2) * motion.steer_angle_rad.tan() / params.wheelbase_m;
        assert!(kinematic > 20.0, "{kinematic}");
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

    /// 제동·관성·비상 감속이 성분 합으로 적용되는지 확인한다(최대 제동 + 비상 정지 = 3.0 + 0.3 + 5.0).
    /// 저항이 없는 기준 모델 값으로 성분 합만 확인한다. 저항은 `resistance_slows_without_reversing`이 확인한다.
    #[test]
    fn deceleration_components_add_up() {
        let params = DynamicsParams::phase2_reference();
        let speed_drop = |brake: f32, emergency: bool| {
            let mut state = ready_in(Gear::Drive);
            state.set_speed_mps(4.0);
            if emergency {
                state.apply(VehicleCommand::EngageEmergencyStop).unwrap();
            }
            state.set_input(input(0.0, brake, 0.0));
            let mut motion = Motion::default();
            step(&mut state, &mut motion, &params, DT);
            (4.0 - state.speed_mps()) / DT
        };
        let close = |actual: f32, expected: f32| (actual - expected).abs() < 1e-3;
        assert!(
            close(speed_drop(1.0, false), 3.3),
            "{}",
            speed_drop(1.0, false)
        );
        assert!(
            close(speed_drop(0.0, true), 5.3),
            "{}",
            speed_drop(0.0, true)
        );
        assert!(
            close(speed_drop(1.0, true), 8.3),
            "{}",
            speed_drop(1.0, true)
        );
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

    /// 조향 감도가 목표 바퀴 각도를 배율로 바꾸고, 최대 조향각은 넘지 않는지 확인한다.
    #[test]
    fn steer_sensitivity_scales_target_angle() {
        let max = DynamicsParams::default().max_steer_angle_deg.to_radians();
        let settled_angle = |sensitivity: f32, steering: f32| {
            let params = DynamicsParams {
                steer_sensitivity: sensitivity,
                ..DynamicsParams::default()
            };
            let mut state = ready_in(Gear::Drive);
            state.set_input(input(0.0, 0.0, steering));
            let mut motion = Motion::default();
            for _ in 0..128 {
                step(&mut state, &mut motion, &params, DT);
            }
            motion.steer_angle_rad
        };
        assert!((settled_angle(0.5, 1.0) - max * 0.5).abs() < 1e-5);
        assert!((settled_angle(2.0, -0.5) + max).abs() < 1e-5);
        assert!((settled_angle(2.0, 1.0) - max).abs() < 1e-5);
    }

    /// `auto`에서 입력을 놓으면 조향 속도와 별개인 복귀 속도로 0도까지 돌아가는지 확인한다.
    #[test]
    fn auto_return_uses_separate_return_rate() {
        let params = DynamicsParams {
            steer_return_rate_deg_s: 120.0,
            ..DynamicsParams::default()
        };
        let max_deg = params.max_steer_angle_deg;
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        let hold_input = |state: &mut VehicleState, motion: &mut Motion, steering, steps| {
            state.set_input(input(0.0, 0.0, steering));
            for _ in 0..steps {
                step(state, motion, &params, DT);
            }
        };
        hold_input(&mut state, &mut motion, 1.0, 64);
        assert!((motion.steer_angle_rad - max_deg.to_radians()).abs() < 1e-5);

        // 0.125초 동안 120도/s로 15도 돌아온다(조향 속도 60도/s였다면 7.5도).
        hold_input(&mut state, &mut motion, 0.0, 8);
        assert!(
            (motion.steer_angle_rad - (max_deg - 15.0).to_radians()).abs() < 1e-4,
            "{}",
            motion.steer_angle_rad.to_degrees()
        );
        hold_input(&mut state, &mut motion, 0.0, 64);
        assert_eq!(motion.steer_angle_rad, 0.0);
    }

    /// `hold`에서 입력을 놓으면 각도를 유지하고, 반대 입력은 반대쪽 목표 각도까지 움직이는지 확인한다.
    #[test]
    fn hold_keeps_angle_until_new_input() {
        let params = DynamicsParams {
            steer_return: SteerReturn::Hold,
            ..DynamicsParams::default()
        };
        let max = params.max_steer_angle_deg.to_radians();
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        let hold_input = |state: &mut VehicleState, motion: &mut Motion, steering, steps| {
            state.set_input(input(0.0, 0.0, steering));
            for _ in 0..steps {
                step(state, motion, &params, DT);
            }
        };
        // 0.25초 우조향하면 15도에서 놓는다.
        hold_input(&mut state, &mut motion, 1.0, 16);
        let held = motion.steer_angle_rad;
        assert!((held - 15f32.to_radians()).abs() < 1e-4);
        hold_input(&mut state, &mut motion, 0.0, 128);
        assert_eq!(motion.steer_angle_rad, held);

        hold_input(&mut state, &mut motion, -1.0, 128);
        assert!((motion.steer_angle_rad + max).abs() < 1e-5);
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
            DynamicsParams {
                steer_sensitivity: 0.0,
                ..DynamicsParams::default()
            },
            DynamicsParams {
                coast_decel_mps2: -0.1,
                ..DynamicsParams::default()
            },
            DynamicsParams {
                steer_return_rate_deg_s: 0.0,
                ..DynamicsParams::default()
            },
            DynamicsParams {
                mass_kg: 0.0,
                ..DynamicsParams::default()
            },
            DynamicsParams {
                drag_area_m2: -1.0,
                ..DynamicsParams::default()
            },
            DynamicsParams {
                max_lateral_accel_mps2: 0.0,
                ..DynamicsParams::default()
            },
            DynamicsParams {
                understeer_gradient_rad_per_g: f32::NAN,
                ..DynamicsParams::default()
            },
        ] {
            assert!(params.validate().is_err(), "{params:?}");
        }
    }

    /// 관성 감속과 저항이 모두 0이면 허용되고, N에서 속도를 줄이지 않고 계속 굴러가는지 확인한다.
    #[test]
    fn zero_coast_decel_keeps_rolling_in_neutral() {
        let params = DynamicsParams {
            coast_decel_mps2: 0.0,
            ..DynamicsParams::phase2_reference()
        };
        assert!(params.validate().is_ok());
        let mut state = ready_in(Gear::Drive);
        state.set_speed_mps(2.0);
        state
            .apply(VehicleCommand::RequestGear(Gear::Neutral))
            .unwrap();
        state.set_input(input(0.0, 0.0, 0.0));
        let mut motion = Motion::default();
        for _ in 0..128 {
            step(&mut state, &mut motion, &params, DT);
        }
        assert_eq!(state.speed_mps(), 2.0);
        assert!((-motion.z_m - 4.0).abs() < 1e-3, "z={}", motion.z_m);
    }

    /// 충돌로 튕겨 후진 최고 속도보다 빠르게 뒤로 움직이면 잘리지 않고 감속으로만 줄어드는지 확인한다(2-7).
    #[test]
    fn bounce_beyond_speed_limit_is_not_clipped() {
        let params = DynamicsParams::default();
        let mut state = ready_in(Gear::Drive);
        state.set_speed_mps(-3.0);
        let mut motion = Motion::default();
        step(&mut state, &mut motion, &params, DT);
        let decel = params.coast_decel_mps2 + params.resistance_decel_mps2(-3.0);
        assert!(
            (state.speed_mps() - (-3.0 + decel * DT)).abs() < 1e-4,
            "{}",
            state.speed_mps()
        );
        assert!(state.speed_mps() < -params.max_reverse_speed_mps);
    }

    /// 각도 정규화가 -π..=π 범위를 유지하는지 확인한다.
    #[test]
    fn wrap_angle_stays_in_range() {
        assert!((wrap_angle(3.0 * PI / 2.0) + PI / 2.0).abs() < 1e-5);
        assert!((wrap_angle(-3.0 * PI / 2.0) - PI / 2.0).abs() < 1e-5);
        assert_eq!(wrap_angle(PI), PI);
    }
}
