//! 차체 서스펜션 움직임(2-8). **보이는 움직임만** 계산한다.
//!
//! 바퀴가 그리는 평면 주행(위치·방향·속도)은 그대로 두고, 그 위에 얹힌 차체의 앞뒤 기울어짐(피치)·좌우 기울어짐(롤)·
//! 상하 움직임을 스프링–댐퍼로 계산한다. 주행 경로·충돌 판정·재생 결과에는 영향을 주지 않는다.
//!
//! 입력은 차체가 받는 가속도다. 앞뒤 가속도로 피치(제동하면 앞이 숙여짐), 옆 가속도로 롤(회전하면 바깥으로 기욺)이 생긴다.
//! 가속도는 틱 사이 속도·옆 미끄러짐·방향 변화로 구하므로, 충돌(2-7)로 속도가 한 틱에 바뀌면 그만큼 큰 가속도가 되어
//! 차체가 출렁인다(충격량 ÷ 질량 = 속도 변화). 강성은 바퀴마다 같은 스프링(`질량/4 × ω²`)이 축간거리·윤거만큼 떨어져
//! 있다고 보고 정한다. 무게중심 높이·고유 진동수·감쇠비·상한은 모두 **시뮬레이터 임시값**이며 실차 미검증이다.

use serde::Deserialize;

use super::dynamics::{DynamicsParams, Motion, wrap_angle};

/// `config/vehicle.toml`의 `[suspension]` 구역. 차체 움직임 파라미터(모두 임시값).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SuspensionParams {
    /// 차체 움직임을 계산할지. 끄면 피치·롤·상하가 늘 0이다.
    pub enabled: bool,
    /// 무게중심 높이(m, 지면 기준).
    pub cg_height_m: f32,
    /// 좌우 바퀴 사이 거리(윤거, m).
    pub track_width_m: f32,
    /// 피치·상하 고유 진동수(Hz).
    pub natural_freq_hz: f32,
    /// 롤 고유 진동수(Hz). 좌우 흔들림 방지 막대(스태빌라이저) 몫으로 피치보다 높게 둔다.
    pub roll_freq_hz: f32,
    /// 감쇠비(0보다 크고 2 이하). 1보다 작으면 한두 번 출렁이고 멈춘다.
    pub damping_ratio: f32,
    /// 피치 상한(도, 앞뒤 대칭).
    pub max_pitch_deg: f32,
    /// 롤 상한(도, 좌우 대칭).
    pub max_roll_deg: f32,
    /// 상하 변위 상한(m, 위아래 대칭).
    pub max_heave_m: f32,
}

impl Default for SuspensionParams {
    /// 시뮬레이터 임시 기본값을 만든다(실차 미검증).
    fn default() -> Self {
        Self {
            enabled: true,
            cg_height_m: 0.8,
            track_width_m: 1.6,
            natural_freq_hz: 1.3,
            roll_freq_hz: 1.8,
            damping_ratio: 0.6,
            max_pitch_deg: 3.0,
            max_roll_deg: 4.0,
            max_heave_m: 0.05,
        }
    }
}

impl SuspensionParams {
    /// 모든 값이 유한한 양수인지, 감쇠비가 2 이하인지 검사한다.
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("cg_height_m", self.cg_height_m),
            ("track_width_m", self.track_width_m),
            ("natural_freq_hz", self.natural_freq_hz),
            ("roll_freq_hz", self.roll_freq_hz),
            ("damping_ratio", self.damping_ratio),
            ("max_pitch_deg", self.max_pitch_deg),
            ("max_roll_deg", self.max_roll_deg),
            ("max_heave_m", self.max_heave_m),
        ] {
            if !(value.is_finite() && value > 0.0) {
                return Err(format!(
                    "suspension.{name} must be a positive number, got {value}"
                ));
            }
        }
        if self.damping_ratio > 2.0 {
            return Err(format!(
                "suspension.damping_ratio must be at most 2, got {}",
                self.damping_ratio
            ));
        }
        Ok(())
    }

    /// 바퀴 하나의 스프링 강성(N/m). 질량의 4분의 1이 고유 진동수 `freq_hz`로 흔들리게 정한다.
    fn wheel_rate(mass_kg: f32, freq_hz: f32) -> f32 {
        let omega = std::f32::consts::TAU * freq_hz;
        mass_kg / 4.0 * omega * omega
    }

    /// 피치 강성(N·m/rad). 바퀴 스프링 4개가 무게중심에서 축간거리 절반만큼 앞뒤로 떨어져 있다고 본다.
    pub fn pitch_stiffness_nm_per_rad(&self, dynamics: &DynamicsParams) -> f32 {
        Self::wheel_rate(dynamics.mass_kg, self.natural_freq_hz) * dynamics.wheelbase_m.powi(2)
    }

    /// 롤 강성(N·m/rad). 바퀴 스프링 4개가 윤거 절반만큼 좌우로 떨어져 있다고 본다.
    pub fn roll_stiffness_nm_per_rad(&self, dynamics: &DynamicsParams) -> f32 {
        Self::wheel_rate(dynamics.mass_kg, self.roll_freq_hz) * self.track_width_m.powi(2)
    }
}

/// 직전 틱의 평면 주행 값. 가속도를 틱 사이 차이로 구할 때 쓴다.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Previous {
    /// 후륜축 앞뒤 속도(m/s).
    speed_mps: f32,
    /// 옆 미끄러짐 속도(m/s, 오른쪽 양수).
    lateral_mps: f32,
    /// 방향(rad).
    heading_rad: f32,
}

/// 차체 움직임 상태(2-8). 부호: 피치는 앞이 내려가면 양수, 롤은 오른쪽이 내려가면 양수, 상하는 위가 양수.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BodyMotion {
    /// 피치(rad, 앞이 내려가면 양수).
    pub pitch_rad: f32,
    /// 롤(rad, 오른쪽이 내려가면 양수).
    pub roll_rad: f32,
    /// 상하 변위(m, 위가 양수). 1차는 노면 높이 입력이 없어(4-7) 늘 0 근처다.
    pub heave_m: f32,
    /// 피치 각속도(rad/s).
    pitch_rate: f32,
    /// 롤 각속도(rad/s).
    roll_rate: f32,
    /// 상하 속도(m/s).
    heave_rate: f32,
    /// 직전 틱 값. 처음이나 시작 위치 복귀 뒤에는 없다(그 틱은 가속도 0으로 본다).
    previous: Option<Previous>,
}

/// 차체가 받는 가속도(m/s²). (앞쪽 양수, 오른쪽 양수).
fn body_acceleration(previous: &Previous, speed_mps: f32, motion: &Motion, dt: f32) -> (f32, f32) {
    // heading이 늘면 왼쪽으로 돈다. 차체 기준 좌표가 돌므로 a앞 = v̇ + 옆속도·ψ̇, a오른 = 옆속도̇ − v·ψ̇.
    let yaw_rate = wrap_angle(motion.heading_rad - previous.heading_rad) / dt;
    let forward = (speed_mps - previous.speed_mps) / dt + motion.lateral_mps * yaw_rate;
    let right = (motion.lateral_mps - previous.lateral_mps) / dt - speed_mps * yaw_rate;
    (forward, right)
}

/// 스프링–댐퍼 한 축을 `dt`만큼 반자동 오일러로 적분하고 `±limit`로 자른다.
///
/// `forcing`은 단위 관성당 외력(정상값 × ω²)이다. 상한에 닿으면 더 나가려는 속도를 없앤다.
fn integrate(
    value: &mut f32,
    rate: &mut f32,
    omega: f32,
    zeta: f32,
    forcing: f32,
    limit: f32,
    dt: f32,
) {
    *rate += (forcing - omega * omega * *value - 2.0 * zeta * omega * *rate) * dt;
    *value += *rate * dt;
    if value.abs() > limit {
        *value = value.clamp(-limit, limit);
        if *rate * *value > 0.0 {
            *rate = 0.0;
        }
    }
}

impl BodyMotion {
    /// 모든 값을 0으로 되돌린다(시작 위치 복귀). 다음 틱은 가속도 0으로 본다.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// 평면 주행 한 틱(`dt`초)이 끝난 뒤의 속도·자세로 차체 움직임을 갱신한다.
    ///
    /// 피치 정상값은 `질량 × 감속도 × 무게중심 높이 ÷ 피치 강성`, 롤 정상값은 `질량 × 옆 가속도 × 무게중심 높이 ÷ 롤 강성`이다.
    /// `params.enabled`가 꺼져 있으면 0을 유지한다.
    pub fn update(
        &mut self,
        params: &SuspensionParams,
        dynamics: &DynamicsParams,
        speed_mps: f32,
        motion: &Motion,
        dt: f32,
    ) {
        if !params.enabled {
            self.reset();
            return;
        }
        let current = Previous {
            speed_mps,
            lateral_mps: motion.lateral_mps,
            heading_rad: motion.heading_rad,
        };
        let (forward, right) = self
            .previous
            .replace(current)
            .map_or((0.0, 0.0), |previous| {
                body_acceleration(&previous, speed_mps, motion, dt)
            });
        let mass = dynamics.mass_kg;
        let height = params.cg_height_m;
        let zeta = params.damping_ratio;
        let pitch_omega = std::f32::consts::TAU * params.natural_freq_hz;
        let roll_omega = std::f32::consts::TAU * params.roll_freq_hz;
        // 정상값 × ω² = 하중 이동 모멘트 ÷ 관성. 관성은 강성 ÷ ω²로 둔다(고유 진동수가 설정값이 되게).
        let pitch_target = -mass * forward * height / params.pitch_stiffness_nm_per_rad(dynamics);
        let roll_target = -mass * right * height / params.roll_stiffness_nm_per_rad(dynamics);
        integrate(
            &mut self.pitch_rad,
            &mut self.pitch_rate,
            pitch_omega,
            zeta,
            pitch_target * pitch_omega * pitch_omega,
            params.max_pitch_deg.to_radians(),
            dt,
        );
        integrate(
            &mut self.roll_rad,
            &mut self.roll_rate,
            roll_omega,
            zeta,
            roll_target * roll_omega * roll_omega,
            params.max_roll_deg.to_radians(),
            dt,
        );
        integrate(
            &mut self.heave_m,
            &mut self.heave_rate,
            pitch_omega,
            zeta,
            0.0,
            params.max_heave_m,
            dt,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 64 Hz 한 틱(s).
    const DT: f32 = 1.0 / 64.0;

    /// `ticks`번 같은 감속도 `decel`(m/s², 양수면 감속)로 속도를 바꿔 가며 갱신한다. 마지막 속도를 반환한다.
    fn brake(
        body: &mut BodyMotion,
        params: &SuspensionParams,
        start: f32,
        decel: f32,
        ticks: usize,
    ) -> f32 {
        let dynamics = DynamicsParams::default();
        let motion = Motion::default();
        let mut speed = start;
        for _ in 0..ticks {
            speed -= decel * DT;
            body.update(params, &dynamics, speed, &motion, DT);
        }
        speed
    }

    /// 입력이 없으면 0에 머물고, 처음 갱신은 직전 값이 없어 가속도 0으로 보는지 확인한다.
    #[test]
    fn stays_level_without_input() {
        let params = SuspensionParams::default();
        let dynamics = DynamicsParams::default();
        let mut body = BodyMotion::default();
        let motion = Motion {
            heading_rad: 1.0,
            ..Motion::default()
        };
        for _ in 0..640 {
            body.update(&params, &dynamics, 5.0, &motion, DT);
        }
        assert_eq!(
            (body.pitch_rad, body.roll_rad, body.heave_m),
            (0.0, 0.0, 0.0)
        );
    }

    /// 일정하게 제동하면 앞이 숙여지고(양수) 정상값이 `m·a·h ÷ 피치 강성`과 같으며, 제동을 멈추면 흔들리다 0으로 돌아오는지 확인한다.
    #[test]
    fn steady_braking_pitch_matches_load_transfer_and_settles() {
        let params = SuspensionParams::default();
        let dynamics = DynamicsParams::default();
        let mut body = BodyMotion::default();
        let decel = 3.0;
        brake(&mut body, &params, 100.0, decel, 64 * 10);
        let expected = dynamics.mass_kg * decel * params.cg_height_m
            / params.pitch_stiffness_nm_per_rad(&dynamics);
        assert!(expected > 0.01, "{expected}");
        assert!(
            (body.pitch_rad - expected).abs() < expected * 0.01,
            "{} vs {expected}",
            body.pitch_rad
        );
        // 제동을 멈추면 한 번 뒤로 넘어갔다가(감쇠비 < 1) 0으로 돌아온다.
        let mut lowest = f32::MAX;
        for _ in 0..64 * 5 {
            brake(&mut body, &params, 0.0, 0.0, 1);
            lowest = lowest.min(body.pitch_rad);
        }
        assert!(lowest < 0.0, "no overshoot: {lowest}");
        assert!(body.pitch_rad.abs() < expected * 0.01, "{}", body.pitch_rad);
    }

    /// 가속하면 뒤가 내려앉는다(피치 음수).
    #[test]
    fn acceleration_squats_rear() {
        let params = SuspensionParams::default();
        let mut body = BodyMotion::default();
        brake(&mut body, &params, 0.0, -1.0, 64 * 3);
        assert!(body.pitch_rad < -0.001, "{}", body.pitch_rad);
    }

    /// 왼쪽으로 돌면(heading 증가) 바깥인 오른쪽이 내려가고(롤 양수), 오른쪽으로 돌면 반대인지 확인한다.
    #[test]
    fn turning_rolls_outward() {
        let params = SuspensionParams::default();
        let dynamics = DynamicsParams::default();
        for (turn, sign) in [(0.3f32, 1.0f32), (-0.3, -1.0)] {
            let mut body = BodyMotion::default();
            let mut motion = Motion::default();
            for _ in 0..64 * 3 {
                motion.heading_rad = wrap_angle(motion.heading_rad + turn * DT);
                body.update(&params, &dynamics, 8.0, &motion, DT);
            }
            // 옆 가속도 v·ψ̇ = 2.4 m/s²의 정상값.
            let expected = dynamics.mass_kg * 8.0 * 0.3 * params.cg_height_m
                / params.roll_stiffness_nm_per_rad(&dynamics);
            assert!(
                (body.roll_rad - sign * expected).abs() < expected * 0.02,
                "turn {turn}: {} vs {expected}",
                body.roll_rad
            );
        }
    }

    /// 정면 충돌처럼 한 틱에 속도가 크게 줄면 앞이 숙여지되 상한을 넘지 않고, 곧 0으로 돌아오는지 확인한다.
    #[test]
    fn impact_pitch_is_bounded_and_dies_out() {
        let params = SuspensionParams::default();
        let dynamics = DynamicsParams::default();
        let motion = Motion::default();
        let mut body = BodyMotion::default();
        body.update(&params, &dynamics, 5.0, &motion, DT);
        body.update(&params, &dynamics, -2.0, &motion, DT);
        let limit = params.max_pitch_deg.to_radians();
        let mut highest = 0.0f32;
        for _ in 0..64 * 4 {
            body.update(&params, &dynamics, -2.0, &motion, DT);
            highest = highest.max(body.pitch_rad);
            assert!(body.pitch_rad.abs() <= limit + 1e-6);
        }
        assert!(highest > limit * 0.5, "{highest}");
        assert!(body.pitch_rad.abs() < 1e-3, "{}", body.pitch_rad);
    }

    /// 오른쪽 옆을 낮게(연석·기둥 높이) 맞아 왼쪽으로 밀리면 바닥 쪽만 먼저 밀리고 무게중심은 뒤처지므로
    /// 맞은 쪽인 오른쪽으로 기우는지(롤 양수) 확인한다. 회전할 때 바깥으로 기우는 것과 같은 원리다.
    #[test]
    fn low_side_impact_leans_toward_hit() {
        let params = SuspensionParams::default();
        let dynamics = DynamicsParams::default();
        let mut motion = Motion::default();
        let mut body = BodyMotion::default();
        body.update(&params, &dynamics, 0.0, &motion, DT);
        motion.lateral_mps = -1.0;
        body.update(&params, &dynamics, 0.0, &motion, DT);
        body.update(&params, &dynamics, 0.0, &motion, DT);
        assert!(body.roll_rad > 0.0 || body.roll_rate > 0.0, "{body:?}");
    }

    /// 끄면 입력이 있어도 늘 0이고, 복귀하면 0으로 돌아가며 다음 틱은 가속도 0으로 보는지 확인한다.
    #[test]
    fn disabled_and_reset_keep_level() {
        let mut params = SuspensionParams::default();
        let mut body = BodyMotion::default();
        brake(&mut body, &params, 100.0, 3.0, 64);
        assert!(body.pitch_rad > 0.0);
        body.reset();
        assert_eq!(body, BodyMotion::default());
        // 복귀 뒤 첫 틱은 속도가 크게 달라도 가속도 0으로 본다.
        brake(&mut body, &params, 0.0, 0.0, 1);
        assert_eq!(body.pitch_rad, 0.0);

        params.enabled = false;
        brake(&mut body, &params, 100.0, 3.0, 64);
        assert_eq!(body, BodyMotion::default());
    }

    /// 감쇠비 상한·양수 검사.
    #[test]
    fn validate_rejects_bad_values() {
        assert!(SuspensionParams::default().validate().is_ok());
        for broken in [
            SuspensionParams {
                damping_ratio: 2.5,
                ..SuspensionParams::default()
            },
            SuspensionParams {
                cg_height_m: 0.0,
                ..SuspensionParams::default()
            },
            SuspensionParams {
                max_roll_deg: f32::NAN,
                ..SuspensionParams::default()
            },
        ] {
            assert!(broken.validate().is_err(), "{broken:?}");
        }
    }
}
