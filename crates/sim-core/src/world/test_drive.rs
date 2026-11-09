//! 시험용 자동 주행(Phase 4). 경로 추종 조향기로 실제 운동 모델·충돌 판정을 써서 코스를 달려 본다.
//!
//! 사람의 주행이나 실차 주행을 대신하지 않는다. 생성한 코스가 차량으로 지나갈 수 있는지 확인하는 용도다.

use glam::Vec2;

use super::course::CourseDef;
use super::road_geom;
use crate::vehicle::collision::{ContactState, step_with_obstacles};
use crate::vehicle::{
    DynamicsParams, Gear, VEHICLE_FOOTPRINT, VehicleCommand, VehicleInput, VehicleState,
};

/// 시동 Ready, 지정 기어인 차량 상태를 만든다.
fn ready_in(gear: Gear) -> VehicleState {
    let mut state = VehicleState::default();
    state.apply(VehicleCommand::ToggleEngine).unwrap();
    state.apply(VehicleCommand::RequestGear(gear)).unwrap();
    state
}

/// 경로 추종 주행 결과.
#[derive(Debug)]
pub struct FollowResult {
    /// 멈춘 곳에서 중심선 끝까지 남은 길이(m).
    pub remaining_m: f32,
    /// 이동이 막힌 충돌 횟수.
    pub impacts: usize,
    /// 중심선에서 가장 멀리 벗어난 거리(m, 후륜축 기준).
    pub max_offset_m: f32,
    /// 첫 충돌 때의 후륜축 위치와 진행 방향각(rad). 실패 원인 확인용으로 단언 실패 메시지(`Debug`)에만 나온다.
    #[allow(dead_code)]
    pub first_impact: Option<(Vec2, f32)>,
}

/// 시험용 경로 추종 조향기(pure pursuit)로 경로 `path`를 따라 목표 속도로 달린다.
///
/// 중심선을 따라 4 m 앞의 목표점으로 조향각 `atan(2L·sin α / Ld)`를 정하고, 속도는 비례 제어한다.
/// 끝 벽 앞 12 m(중심선 길이 기준)에 닿거나 시간이 다하면 멈춘다. 운동 모델·충돌 판정은 실제 것을 쓴다.
pub fn follow_path(course: &CourseDef, path: &[Vec2], target_speed_mps: f32) -> FollowResult {
    const LOOKAHEAD_M: f32 = 4.0;
    let cumulative: Vec<f32> = std::iter::once(0.0)
        .chain(path.windows(2).scan(0.0, |acc, p| {
            *acc += p[0].distance(p[1]);
            Some(*acc)
        }))
        .collect();
    let total = *cumulative.last().unwrap();
    let params = DynamicsParams::default();
    let max_steer = params.max_steer_angle_deg.to_radians();
    let obstacles = course.collision_rects();
    let mut state = ready_in(Gear::Drive);
    let mut motion = course.start;
    let mut contact = ContactState::default();
    let (mut impacts, mut segment, mut along, mut max_offset) = (0, 0usize, 0.0f32, 0.0f32);
    let mut first_impact = None;
    for _ in 0..64 * 600 {
        let axle = Vec2::new(motion.x_m, motion.z_m);
        // 가까운 구간은 지금 구간부터 앞쪽 창 안에서만 찾아 되돌아가지 않게 하고, 구간 위 투영점으로 위치를 잰다.
        let window_end = (segment + 200).min(path.len() - 1);
        let (best, offset, position) = (segment..window_end)
            .map(|k| {
                let (a, b) = (path[k], path[k + 1]);
                let length = a.distance(b);
                let t = ((axle - a).dot(b - a) / (length * length)).clamp(0.0, 1.0);
                (k, axle.distance(a.lerp(b, t)), cumulative[k] + t * length)
            })
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .unwrap();
        (segment, along) = (best, position);
        max_offset = max_offset.max(offset);
        if total - along < 12.0 {
            break;
        }
        let target = road_geom::point_at_distance(path, (along + LOOKAHEAD_M).min(total))
            .map_or(path[path.len() - 1], |(point, _)| point);
        let (fx, fz) = motion.forward();
        let forward = Vec2::new(fx, fz);
        let relative = target - axle;
        let alpha = relative
            .dot(road_geom::right_of(forward))
            .atan2(relative.dot(forward));
        let steer = (2.0 * params.wheelbase_m * alpha.sin() / LOOKAHEAD_M).atan();
        let speed = state.speed_mps();
        state.set_input(VehicleInput {
            throttle: ((target_speed_mps - speed) * 2.0).clamp(0.0, 1.0),
            brake: ((speed - target_speed_mps) * 2.0).clamp(0.0, 1.0),
            steering: (steer / max_steer).clamp(-1.0, 1.0),
        });
        if step_with_obstacles(
            &mut state,
            &mut motion,
            &params,
            1.0 / 64.0,
            &VEHICLE_FOOTPRINT,
            &obstacles,
            &mut contact,
        )
        .is_some()
        {
            impacts += 1;
            first_impact.get_or_insert((Vec2::new(motion.x_m, motion.z_m), motion.heading_rad));
        }
    }
    FollowResult {
        remaining_m: total - along,
        impacts,
        max_offset_m: max_offset,
        first_impact,
    }
}
