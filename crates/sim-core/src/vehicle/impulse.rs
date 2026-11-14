//! 충돌 충격량 반응(2-7).
//!
//! 차체를 지면 위의 평면 강체(무게중심 속도 + 수직축 회전)로 보고, 접촉점([`Contact`])에 법선 충격량(반발)과
//! 접선 충격량(쿨롱 마찰)을 준다. 운동 모델의 상태(후륜축 앞뒤 속도, 옆 미끄러짐, 조향 외 추가 회전)와
//! 강체 속도 사이를 오간다. 무게중심은 차체 중심([`Footprint::rear_axle_to_center_m`])으로 둔다.
//! 반발·마찰 계수와 회전 관성은 실차 자료가 없는 **시뮬레이터 임시값**이며, 실제 충돌 거동과 같다고 주장하지 않는다.

use glam::Vec2;

use super::collision::{Contact, Footprint};
use super::dynamics::{DynamicsParams, Motion, steering_yaw_rate, wrap_angle};
use super::state::VehicleState;

/// 충격량을 준 뒤 이보다 작은 앞뒤 속도(m/s)는 계산 잔여로 보고 0으로 둔다.
const SPEED_RESIDUE_MPS: f32 = 1.0e-5;

/// 닿은 점이 이미 장애물에서 떨어지는 중인데 다시 막힌 경우에 줄 이탈 속도(m/s, 임시값).
///
/// 자세를 한 단계씩 근사해 옮기므로(회전한 뒤 새 방향으로 이동), 돌면서 벽을 스치는 차체는 닿은 점의 속도가 0이어도
/// 한 단계에 수십 µm 벽 쪽으로 밀릴 수 있다. 그때마다 남은 시간을 버리면 벽에 붙은 채 앞으로 가지 못하므로
/// 이 경우에만 아주 작은 이탈 속도를 준다. 운동 에너지를 아주 조금 더할 수 있다.
pub const DRIFT_SEPARATION_MPS: f32 = 0.02;

/// 평면 강체 속도. 무게중심 속도(x, z)와 회전 속도(rad/s, heading 증가 방향).
#[derive(Debug, Clone, Copy, PartialEq)]
struct RigidVelocity {
    /// 무게중심 위치 (x, z)(m).
    center: Vec2,
    /// 무게중심 속도 (x, z)(m/s).
    linear: Vec2,
    /// 회전 속도(rad/s, heading 증가 방향).
    yaw_rate: f32,
}

/// 회전 속도 `yaw_rate`로 돌 때 회전 중심에서 `offset`만큼 떨어진 점의 속도(x, z).
///
/// heading이 늘면 전방 `(-sin, -cos)`가 왼쪽으로 돌므로, `offset` (x, z)의 속도는 `yaw_rate × (z, -x)`다.
fn spin_velocity(yaw_rate: f32, offset: Vec2) -> Vec2 {
    Vec2::new(offset.y, -offset.x) * yaw_rate
}

/// 충격량 방향 `direction`이 `offset` 점에 주는 회전 팔(충격량 1당 회전 충격량).
fn lever(offset: Vec2, direction: Vec2) -> f32 {
    direction.dot(Vec2::new(offset.y, -offset.x))
}

impl RigidVelocity {
    /// 운동 모델 상태에서 강체 속도를 만든다.
    fn from_motion(
        state: &VehicleState,
        motion: &Motion,
        params: &DynamicsParams,
        footprint: &Footprint,
    ) -> Self {
        let speed = state.speed_mps();
        let forward = Vec2::from(motion.forward());
        let right = Vec2::from(motion.right());
        let axle = Vec2::new(motion.x_m, motion.z_m);
        let yaw_rate =
            steering_yaw_rate(params, speed, motion.steer_angle_rad) + motion.slip_yaw_rate_rad_s;
        let center = axle + forward * footprint.rear_axle_to_center_m;
        let axle_velocity = forward * speed + right * motion.lateral_mps;
        Self {
            center,
            linear: axle_velocity + spin_velocity(yaw_rate, center - axle),
            yaw_rate,
        }
    }

    /// 차체 위 점 `point`의 속도(x, z).
    fn at(&self, point: Vec2) -> Vec2 {
        self.linear + spin_velocity(self.yaw_rate, point - self.center)
    }

    /// 점 `point`에 충격량 `impulse`(N·s, x·z)를 준다.
    fn apply(&mut self, params: &DynamicsParams, point: Vec2, impulse: Vec2) {
        self.linear += impulse / params.mass_kg;
        self.yaw_rate += lever(point - self.center, impulse) / params.yaw_inertia_kg_m2;
    }

    /// 점 `point`에서 방향 `direction` 속도를 1 바꾸는 데 드는 충격량 크기의 역수(유효 질량의 역수).
    fn inverse_mass(&self, params: &DynamicsParams, point: Vec2, direction: Vec2) -> f32 {
        let arm = lever(point - self.center, direction);
        1.0 / params.mass_kg + arm * arm / params.yaw_inertia_kg_m2
    }

    /// 운동 에너지(J). 시험에서 충돌 뒤 에너지가 늘지 않는지 볼 때 쓴다.
    #[cfg(test)]
    fn energy(&self, params: &DynamicsParams) -> f32 {
        0.5 * params.mass_kg * self.linear.length_squared()
            + 0.5 * params.yaw_inertia_kg_m2 * self.yaw_rate * self.yaw_rate
    }

    /// 강체 속도를 운동 모델 상태(앞뒤 속도, 옆 미끄러짐, 조향 외 추가 회전)로 되돌린다.
    fn store(&self, state: &mut VehicleState, motion: &mut Motion, params: &DynamicsParams) {
        let forward = Vec2::from(motion.forward());
        let right = Vec2::from(motion.right());
        let axle = Vec2::new(motion.x_m, motion.z_m);
        let axle_velocity = self.at(axle);
        let speed = axle_velocity.dot(forward);
        // 정면 충돌에서 계산 순서 때문에 남는 아주 작은 앞뒤 속도(10⁻⁹ m/s 수준)는 0으로 둔다(정지 판정이 흔들리지 않게).
        // 옆 미끄러짐·회전은 작아도 벽을 따라 미끄러지는 데 필요하므로 그대로 둔다.
        let speed = if speed.abs() < SPEED_RESIDUE_MPS {
            0.0
        } else {
            speed
        };
        state.set_speed_mps(speed);
        motion.lateral_mps = axle_velocity.dot(right);
        motion.slip_yaw_rate_rad_s =
            self.yaw_rate - steering_yaw_rate(params, speed, motion.steer_angle_rad);
    }
}

/// 충격량 하나를 준 결과(2-7 기록·화면용). 크기는 모두 시뮬레이터 임시값으로 계산한 값이다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Response {
    /// 닿는 순간 접촉점이 장애물 쪽으로 다가오던 속도(m/s, 0 이상). 면 접촉이면 두 끝 중 빠른 쪽.
    pub normal_speed_mps: f32,
    /// 법선 충격량 합(N·s, 0 이상). 마찰 충격량은 넣지 않는다.
    pub impulse_ns: f32,
    /// 무게중심 속도 변화 크기(m/s). 마찰로 줄어든 접선 속도도 들어간다.
    pub delta_v_mps: f32,
}

/// 접촉 `contact`에서 차체가 장애물 쪽으로 움직이고 있으면 충격량을 줘 속도를 바꾼다. 줬으면 그 결과를 반환한다.
///
/// 법선 충격량 `j = −(1+e)·vₙ ÷ (1/m + (r×n)²/I)`로 닿는 방향 속도를 없애고 반발 계수 `e`만큼 되돌린다.
/// 닿는 속도가 [`DynamicsParams::bounce_min_speed_mps`]보다 느리면 `e = 0`이다(벽을 밀 때 떨림 방지).
/// 그다음 접선 방향 미끄러짐을 멈추는 충격량을 `μ·j` 안으로 잘라 준다(쿨롱 마찰).
///
/// `separation_mps`는 충격량을 준 뒤 닿은 점이 장애물에서 떨어지는 최소 속도(m/s)다. 보통 0이고, 이미 떨어지는 중인데도
/// 계산 근사 때문에 다시 막힌 경우에만 [`DRIFT_SEPARATION_MPS`]를 준다.
pub fn resolve(
    state: &mut VehicleState,
    motion: &mut Motion,
    params: &DynamicsParams,
    footprint: &Footprint,
    contact: &Contact,
    separation_mps: f32,
) -> Option<Response> {
    let mut body = RigidVelocity::from_motion(state, motion, params, footprint);
    let before = body.linear;
    let (normal_speed_mps, impulse_ns) = push(&mut body, params, contact, separation_mps)?;
    body.store(state, motion, params);
    Some(Response {
        normal_speed_mps,
        impulse_ns,
        delta_v_mps: (body.linear - before).length(),
    })
}

/// 충격량을 나눠 주는 반복 횟수(임시값). 면 접촉의 두 끝과 마찰이 서로 영향을 주므로 몇 번 되풀이해 맞춘다.
const SOLVER_ITERATIONS: usize = 8;

/// [`resolve`]의 강체 계산 부분. 충격량을 줬으면 (닿는 속도 m/s, 법선 충격량 합 N·s)을 반환한다.
///
/// 접촉 구간의 끝점마다 목표 법선 속도(반발 속도, 반발이 없으면 0)를 정하고, 끝점별 누적 법선 충격량을
/// 0 이상으로 유지하며 되풀이해 맞춘다(순차 충격량 방식). 마찰은 접촉점 가운데에서 누적 법선 충격량 합 × 마찰 계수 안으로 준다.
fn push(
    body: &mut RigidVelocity,
    params: &DynamicsParams,
    contact: &Contact,
    separation_mps: f32,
) -> Option<(f32, f32)> {
    let normal = contact.normal;
    let ends: &[Vec2] = if contact.ends[0].distance(contact.ends[1]) < 1e-6 {
        &contact.ends[..1]
    } else {
        &contact.ends
    };
    let approach: Vec<f32> = ends.iter().map(|&end| body.at(end).dot(normal)).collect();
    if !approach.iter().any(|&speed| speed < separation_mps) {
        return None;
    }
    let targets: Vec<f32> = approach
        .iter()
        .map(|&speed| {
            let restitution = if -speed > params.bounce_min_speed_mps {
                params.restitution
            } else {
                0.0
            };
            (-restitution * speed).max(separation_mps)
        })
        .collect();
    let mut pushed = vec![0.0f32; ends.len()];
    let mut friction = 0.0f32;
    let tangent = normal.perp();
    for _ in 0..SOLVER_ITERATIONS {
        solve_normal(body, params, ends, normal, &targets, &mut pushed);
        let limit = params.body_friction_coeff * pushed.iter().sum::<f32>();
        let slip = body.at(contact.point).dot(tangent);
        let next = (friction - slip / body.inverse_mass(params, contact.point, tangent))
            .clamp(-limit, limit);
        body.apply(params, contact.point, tangent * (next - friction));
        friction = next;
    }
    // 마지막 마찰이 닿은 점을 다시 장애물 쪽으로 돌렸으면 법선 충격량을 한 번 더 맞춘다.
    solve_normal(body, params, ends, normal, &targets, &mut pushed);
    let closing = approach
        .iter()
        .fold(0.0f32, |fastest, &speed| fastest.max(-speed));
    Some((closing, pushed.iter().sum()))
}

/// 접촉 끝점(1~2개)의 누적 법선 충격량 `pushed`를 목표 법선 속도 `targets`에 맞게 다시 정하고 차이만큼 준다.
///
/// 끝점이 둘이면 두 충격량을 함께 푼다(2×2 연립, 음수가 나오면 한쪽만 미는 경우를 차례로 본다). 차례로 하나씩 풀면
/// 정면으로 대칭인 충돌에서도 계산 순서 때문에 작은 회전이 생기므로 함께 푼다. 누적 충격량은 0 이상이다(당기지 않음).
fn solve_normal(
    body: &mut RigidVelocity,
    params: &DynamicsParams,
    ends: &[Vec2],
    normal: Vec2,
    targets: &[f32],
    pushed: &mut [f32],
) {
    let speed = |body: &RigidVelocity, index: usize| body.at(ends[index]).dot(normal);
    let next: Vec<f32> = if let [_] = ends {
        vec![
            (pushed[0]
                + (targets[0] - speed(body, 0)) / body.inverse_mass(params, ends[0], normal))
            .max(0.0),
        ]
    } else {
        let arm = |index: usize| lever(ends[index] - body.center, normal);
        let (a1, a2) = (arm(0), arm(1));
        let inverse_mass = 1.0 / params.mass_kg;
        let inertia = params.yaw_inertia_kg_m2;
        let (k11, k22, k12) = (
            inverse_mass + a1 * a1 / inertia,
            inverse_mass + a2 * a2 / inertia,
            inverse_mass + a1 * a2 / inertia,
        );
        // 누적 충격량을 뺀 법선 속도와, 거기서 목표까지 필요한 속도 변화.
        let base1 = speed(body, 0) - (k11 * pushed[0] + k12 * pushed[1]);
        let base2 = speed(body, 1) - (k12 * pushed[0] + k22 * pushed[1]);
        let (need1, need2) = (targets[0] - base1, targets[1] - base2);
        let det = k11 * k22 - k12 * k12;
        let both = (det > 1e-9 * k11 * k22).then(|| {
            (
                (need1 * k22 - k12 * need2) / det,
                (k11 * need2 - k12 * need1) / det,
            )
        });
        let first = need1 / k11;
        let second = need2 / k22;
        let solution = match both {
            Some((x1, x2)) if x1 >= 0.0 && x2 >= 0.0 => (x1, x2),
            _ if first >= 0.0 && k12 * first >= need2 => (first, 0.0),
            _ if second >= 0.0 && k12 * second >= need1 => (0.0, second),
            _ if need1 <= 0.0 && need2 <= 0.0 => (0.0, 0.0),
            _ => (pushed[0], pushed[1]),
        };
        vec![solution.0, solution.1]
    };
    for ((&end, total), value) in ends.iter().zip(pushed.iter_mut()).zip(next) {
        body.apply(params, end, normal * (value - *total));
        *total = value;
    }
}

/// 바뀐 속도로 `dt`(s) 동안 힘 없이 움직인 자세를 반환한다(막힌 뒤 남은 시간, 2-7).
///
/// [`super::dynamics::step`]처럼 먼저 돌고 새 방향으로 움직인다. 가속·제동·마찰 감속은 다음 단계에서 한다.
pub fn coast(state: &VehicleState, motion: &Motion, params: &DynamicsParams, dt: f32) -> Motion {
    let speed = state.speed_mps();
    let yaw_rate =
        steering_yaw_rate(params, speed, motion.steer_angle_rad) + motion.slip_yaw_rate_rad_s;
    let mut next = *motion;
    next.heading_rad = wrap_angle(motion.heading_rad + yaw_rate * dt);
    let forward = Vec2::from(next.forward());
    let right = Vec2::from(next.right());
    let moved = (forward * speed + right * motion.lateral_mps) * dt;
    next.x_m += moved.x;
    next.z_m += moved.y;
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::collision::{ContactKind, GroundRect, VEHICLE_FOOTPRINT};
    use crate::vehicle::{Gear, VehicleCommand};

    /// 시동 Ready, D, 속도 `speed_mps`인 차량 상태를 만든다.
    fn moving(speed_mps: f32) -> VehicleState {
        let mut state = VehicleState::default();
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        state
            .apply(VehicleCommand::RequestGear(Gear::Drive))
            .unwrap();
        state.set_speed_mps(speed_mps);
        state
    }

    /// 접촉 정보를 만든다.
    fn contact(point: Vec2, normal: Vec2) -> Contact {
        Contact {
            point,
            ends: [point; 2],
            normal,
            kind: ContactKind::Face,
        }
    }

    /// 앞범퍼 가운데(후륜축 3.5 m 앞) 접촉, 법선 +Z(정면 벽).
    fn front_wall() -> Contact {
        contact(Vec2::new(0.0, -3.5), Vec2::Y)
    }

    /// 정면 벽에 빠르게 닿으면 반발 계수만큼 뒤로 튕기고, 옆 미끄러짐·회전은 생기지 않는지 확인한다.
    /// 결과의 닿는 속도는 5 m/s, 속도 변화는 `5(1+e)`, 충격량은 `m·5(1+e)`다.
    #[test]
    fn head_on_bounces_back_by_restitution() {
        let params = DynamicsParams::default();
        let mut state = moving(5.0);
        let mut motion = Motion::default();
        let response = resolve(
            &mut state,
            &mut motion,
            &params,
            &VEHICLE_FOOTPRINT,
            &front_wall(),
            0.0,
        )
        .unwrap();
        let change = 5.0 * (1.0 + params.restitution);
        assert!(
            (response.normal_speed_mps - 5.0).abs() < 1e-4,
            "{response:?}"
        );
        assert!((response.delta_v_mps - change).abs() < 1e-3, "{response:?}");
        assert!(
            (response.impulse_ns - params.mass_kg * change).abs() < 1.0,
            "{response:?}"
        );
        assert!((state.speed_mps() + 5.0 * params.restitution).abs() < 1e-4);
        assert!(motion.lateral_mps.abs() < 1e-5);
        assert!(motion.slip_yaw_rate_rad_s.abs() < 1e-5);
    }

    /// 느리게 닿으면 튕기지 않고 붙고, 멀어지는 중이면 충격량을 주지 않는지 확인한다.
    #[test]
    fn slow_contact_sticks_and_separating_is_ignored() {
        let params = DynamicsParams::default();
        let mut state = moving(0.3);
        let mut motion = Motion::default();
        assert!(
            resolve(
                &mut state,
                &mut motion,
                &params,
                &VEHICLE_FOOTPRINT,
                &front_wall(),
                0.0
            )
            .is_some()
        );
        assert_eq!(state.speed_mps(), 0.0);

        let mut leaving = moving(-0.3);
        let mut still = Motion::default();
        assert!(
            resolve(
                &mut leaving,
                &mut still,
                &params,
                &VEHICLE_FOOTPRINT,
                &front_wall(),
                0.0
            )
            .is_none()
        );
        assert_eq!(leaving.speed_mps(), -0.3);
        assert_eq!(still, Motion::default());
    }

    /// 오른쪽 앞 모서리가 정면 벽에 닿으면 차가 오른쪽으로 돈다(heading 감소)는 것을 확인한다.
    /// 오른쪽 앞이 막히고 왼쪽은 계속 나아가므로 차체가 오른쪽으로 돈다.
    #[test]
    fn off_center_hit_spins_toward_blocked_side() {
        let params = DynamicsParams::default();
        let mut state = moving(5.0);
        let mut motion = Motion::default();
        let corner = contact(Vec2::new(0.9, -3.5), Vec2::Y);
        resolve(
            &mut state,
            &mut motion,
            &params,
            &VEHICLE_FOOTPRINT,
            &corner,
            0.0,
        );
        assert!(motion.slip_yaw_rate_rad_s < -0.1, "{motion:?}");
        // 닿은 점은 벽 쪽으로 더 들어가지 않는다.
        let body = RigidVelocity::from_motion(&state, &motion, &params, &VEHICLE_FOOTPRINT);
        assert!(body.at(corner.point).dot(Vec2::Y) >= -1e-4);
    }

    /// 비스듬히(30°) 벽에 닿으면 벽을 따라가는 속도가 남는지(미끄러짐) 확인한다. 마찰이 0이면 무게중심의
    /// 벽 방향 속도가 그대로이고, 마찰이 있으면 그보다 줄어든다(닿은 점의 접선 속도는 회전 때문에 달라질 수 있다).
    #[test]
    fn oblique_hit_keeps_sliding_along_wall() {
        let mut params = DynamicsParams::default();
        let motion = Motion {
            heading_rad: 30f32.to_radians(),
            ..Motion::default()
        };
        let state = moving(5.0);
        let rect = VEHICLE_FOOTPRINT.rect_at(&motion);
        let wall = wall_in_front(&rect);
        let touch = rect.contact_with(&wall);
        let along = touch.normal.perp();
        let start = RigidVelocity::from_motion(&state, &motion, &params, &VEHICLE_FOOTPRINT);
        let mut slid = Vec::new();
        for friction in [0.0, params.body_friction_coeff] {
            params.body_friction_coeff = friction;
            let mut body = start;
            push(&mut body, &params, &touch, 0.0).unwrap();
            slid.push(body.linear.dot(along));
        }
        let before = start.linear.dot(along);
        assert!((slid[0] - before).abs() < 1e-4, "{before} -> {slid:?}");
        assert!(
            slid[1].abs() < slid[0].abs() && slid[1].abs() > 0.5,
            "{slid:?}"
        );
    }

    /// 여러 접촉점·법선·속도에서 충격량을 준 뒤 운동 에너지가 늘지 않고, 닿은 점의 법선 속도가 0 이상인지 확인한다.
    #[test]
    fn impulse_never_adds_energy() {
        let params = DynamicsParams::default();
        let points = [
            Vec2::new(0.0, -3.5),
            Vec2::new(0.9, -3.5),
            Vec2::new(-0.9, -2.0),
            Vec2::new(0.9, 0.7),
            Vec2::new(-0.9, 0.7),
        ];
        let mut checked = 0;
        for &point in &points {
            for angle in (0..12).map(|i| i as f32 * std::f32::consts::TAU / 12.0) {
                let normal = Vec2::from_angle(angle);
                for (speed, steer, lateral, slip) in [
                    (5.0, 0.0, 0.0, 0.0),
                    (-1.5, 0.3, 0.0, 0.0),
                    (11.0, -0.2, 0.5, 0.3),
                    (2.0, 0.5, -1.0, -0.8),
                ] {
                    let state = moving(speed);
                    let motion = Motion {
                        steer_angle_rad: steer,
                        lateral_mps: lateral,
                        slip_yaw_rate_rad_s: slip,
                        ..Motion::default()
                    };
                    let mut body =
                        RigidVelocity::from_motion(&state, &motion, &params, &VEHICLE_FOOTPRINT);
                    let energy = body.energy(&params);
                    let touch = contact(point, normal);
                    if push(&mut body, &params, &touch, 0.0).is_none() {
                        continue;
                    }
                    checked += 1;
                    assert!(
                        body.energy(&params) <= energy * (1.0 + 1e-5) + 1e-3,
                        "energy grew at {point:?} {normal:?} {speed}"
                    );
                    assert!(body.at(point).dot(normal) > -1e-3);
                }
            }
        }
        assert!(checked > 50, "{checked}");
    }

    /// 강체 속도로 바꿨다가 되돌려도 운동 모델 상태가 그대로인지 확인한다.
    #[test]
    fn rigid_velocity_round_trips() {
        let params = DynamicsParams::default();
        let mut state = moving(3.0);
        let mut motion = Motion {
            x_m: 2.0,
            z_m: -1.0,
            heading_rad: 0.7,
            steer_angle_rad: -0.2,
            lateral_mps: 0.4,
            slip_yaw_rate_rad_s: -0.3,
            ..Motion::default()
        };
        let original = (state.speed_mps(), motion);
        RigidVelocity::from_motion(&state, &motion, &params, &VEHICLE_FOOTPRINT).store(
            &mut state,
            &mut motion,
            &params,
        );
        assert!((state.speed_mps() - original.0).abs() < 1e-5);
        assert!((motion.lateral_mps - original.1.lateral_mps).abs() < 1e-5);
        assert!((motion.slip_yaw_rate_rad_s - original.1.slip_yaw_rate_rad_s).abs() < 1e-5);
    }

    /// 차체 `rect`의 가장 앞(가장 작은 z) 꼭짓점에 1e-5 m 떨어져 맞닿은 넓은 축 정렬 벽을 만든다.
    fn wall_in_front(rect: &GroundRect) -> GroundRect {
        let front = rect
            .vertices()
            .iter()
            .map(|corner| corner.y)
            .fold(f32::INFINITY, f32::min);
        GroundRect::axis_aligned(
            Vec2::new(rect.center.x, front - 1e-5 - 0.5),
            Vec2::new(20.0, 1.0),
        )
    }
}
