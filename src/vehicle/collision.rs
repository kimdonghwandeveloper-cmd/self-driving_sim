//! 차체와 고정 장애물의 지면 충돌 판정(Phase 2 임시 구현).
//!
//! 저속 셔틀이므로 높이는 무시하고, 지면(x, z) 위의 회전된 직사각형끼리 분리축 판정(SAT)을 한다.
//! 운동 모델이 한 단계 움직인 경로를 잘게 나눠 검사하고, 장애물에 닿으면 닿기 직전 자세에서 세운다.
//! 검사 지점 사이 구간은 "차체 위 어느 점도 장애물까지 거리보다 많이 움직이지 않았는지"로 보수적으로
//! 확인하고, 확인되지 않으면 구간을 더 나눈다. 회전 중 모서리가 검사 지점 사이에서만 스치는 경우도 잡는다.
//! 다만 최소 검사 구간([`MIN_CERTIFY_SPAN_M`])에 따른 근사와 부동소수점 오차가 있어 절대적인 관통 방지 보장은 아니다.
//! 미끄러지며 벽을 따라가는 처리는 하지 않는다(접촉하면 정지).
//! 차체 크기와 후륜축 위치는 실차 치수를 확인하지 않은 **시뮬레이터 임시값**이다.

use bevy::math::Vec2;
use bevy::prelude::Component;

use super::dynamics::{self, DynamicsParams, Motion, wrap_angle};
use super::state::VehicleState;

/// 경로 기본 검사 간격(m, 차체 위 점이 움직이는 최대 거리 기준). 구간 안은 보수 검사로 다시 확인한다.
const MAX_SAMPLE_STEP_M: f32 = 0.05;
/// 보수 검사로 더 나누지 않는 최소 구간(m). 이보다 짧은 구간은 양 끝 검사만 믿는 근사다
/// (이론상 이 값의 절반 정도까지의 겹침을 놓칠 수 있고, 부동소수점 오차는 별도다).
const MIN_CERTIFY_SPAN_M: f32 = 1e-4;
/// 닿는 지점을 좁히는 정밀도(m). 멈춘 자세는 실제 접촉 지점보다 이 거리 안쪽(닿기 전)이다.
const BOUNDARY_RESOLUTION_M: f32 = 1e-5;
/// 차체와 장애물 사이가 이 거리(m) 안이면 접촉으로 본다.
pub const CONTACT_MARGIN_M: f32 = 0.02;

/// 차체 충돌 박스 기본값(길이 4.2 m, 폭 1.8 m, 후륜축에서 차체 중심까지 1.4 m). 실차 미확인 임시값.
pub const VEHICLE_FOOTPRINT: Footprint = Footprint {
    length_m: 4.2,
    width_m: 1.8,
    rear_axle_to_center_m: 1.4,
};

/// 지면 위의 회전된 직사각형. heading 규칙은 [`Motion`]과 같다(0이면 길이 방향이 -Z).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundRect {
    /// 중심 (x, z) 좌표(m).
    pub center: Vec2,
    /// 반 크기(m). `x`는 좌우(폭/2), `y`는 앞뒤(길이/2).
    pub half_size: Vec2,
    /// +Y축 회전각(rad).
    pub heading_rad: f32,
}

impl GroundRect {
    /// 회전 없는 직사각형을 만든다. `size`는 (x 방향 크기, z 방향 크기)(m)다.
    pub fn axis_aligned(center: Vec2, size: Vec2) -> Self {
        Self {
            center,
            half_size: size / 2.0,
            heading_rad: 0.0,
        }
    }

    /// 직사각형의 (오른쪽, 전방) 단위 벡터를 (x, z) 좌표로 반환한다.
    fn axes(&self) -> [Vec2; 2] {
        let (sin, cos) = self.heading_rad.sin_cos();
        [Vec2::new(cos, -sin), Vec2::new(-sin, -cos)]
    }

    /// 미리 구한 단위 벡터 `axes`로, 축 `axis` 위에 투영했을 때 중심에서 가장자리까지의 길이를 구한다.
    fn projected_radius(&self, [right, forward]: [Vec2; 2], axis: Vec2) -> f32 {
        self.half_size.x * right.dot(axis).abs() + self.half_size.y * forward.dot(axis).abs()
    }

    /// 두 직사각형의 겹침 깊이(m)를 분리축 판정으로 구한다.
    ///
    /// 네 분리축 중 가장 얕게 겹친 축의 겹침 길이다. 양수면 겹치고, 0 이하면 그만큼 떨어져 있다.
    /// 떨어져 있을 때 `-penetration`은 실제 최단 거리 이하다(축에 투영하면 거리가 줄기만 한다).
    pub fn penetration(&self, other: &GroundRect) -> f32 {
        let offset = other.center - self.center;
        let (own, others) = (self.axes(), other.axes());
        own.into_iter()
            .chain(others)
            .map(|axis| {
                self.projected_radius(own, axis) + other.projected_radius(others, axis)
                    - offset.dot(axis).abs()
            })
            .fold(f32::INFINITY, f32::min)
    }

    /// 두 직사각형이 겹치는지 분리축 판정으로 검사한다.
    ///
    /// `margin`(m)만큼 떨어져 있어도 겹친 것으로 본다. 0이면 맞닿기만 한 경우는 겹침이 아니다.
    pub fn overlaps(&self, other: &GroundRect, margin: f32) -> bool {
        self.penetration(other) + margin > 0.0
    }
}

/// 차체 충돌 박스 치수. 기준점은 운동 모델과 같은 후륜축 중심이다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Footprint {
    /// 차체 길이(m).
    pub length_m: f32,
    /// 차체 폭(m).
    pub width_m: f32,
    /// 후륜축 중심에서 차체 중심까지의 전방 거리(m).
    pub rear_axle_to_center_m: f32,
}

impl Footprint {
    /// 자세 `motion`에서 차체가 차지하는 지면 직사각형을 만든다.
    pub fn rect_at(&self, motion: &Motion) -> GroundRect {
        let (forward_x, forward_z) = motion.forward();
        GroundRect {
            center: Vec2::new(motion.x_m, motion.z_m)
                + Vec2::new(forward_x, forward_z) * self.rear_axle_to_center_m,
            half_size: Vec2::new(self.width_m, self.length_m) / 2.0,
            heading_rad: motion.heading_rad,
        }
    }

    /// 후륜축 중심에서 가장 먼 차체 모서리까지의 거리(m). 회전할 때 모서리가 움직이는 거리 계산에 쓴다.
    fn reach_m(&self) -> f32 {
        Vec2::new(
            self.width_m / 2.0,
            self.rear_axle_to_center_m.abs() + self.length_m / 2.0,
        )
        .length()
    }
}

/// 장애물로 쓰는 고정 지면 직사각형 컴포넌트. 차량은 이 영역을 통과하지 못한다.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Obstacle(pub GroundRect);

/// 한 단계 이동 경로 검사 결과.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sweep {
    /// 장애물에 닿지 않고 갈 수 있는 이동 비율(0..=1).
    pub fraction: f32,
    /// 이동을 막은 장애물의 번호(`obstacles` 순서). 막히지 않았으면 `None`.
    pub blocked_by: Option<usize>,
}

impl Sweep {
    /// 장애물에 막혀 이동을 잘랐는지 반환한다.
    pub fn blocked(&self) -> bool {
        self.blocked_by.is_some()
    }
}

/// 차체와 장애물의 접촉 상태. 단계마다 [`step_with_obstacles`]가 갱신한다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContactState {
    /// 장애물과 접촉 거리([`CONTACT_MARGIN_M`]) 안에 있는지. HUD 표시용이며 충돌 기록 기준이 아니다.
    pub touching: bool,
    /// 마지막 단계에서 장애물에 막혀 이동이 잘렸는지. 이미 충돌을 보고한 장애물을 계속 미는 경우도 포함한다.
    /// 근접(`touching`)과 달리 실제 이동 막힘만 뜻하며, 보정 시험의 유효성 판정에 쓴다.
    pub blocked: bool,
    /// 현재 접촉 중이며 이미 충돌을 보고한 장애물 번호(`obstacles` 순서).
    /// 그 장애물의 접촉 거리 밖으로 벗어나면 빠지므로, 다른 장애물 옆에 붙어 있어도 재충돌을 놓치지 않는다.
    /// 장애물 목록의 순서는 단계마다 같다고 가정한다.
    pub reported: Vec<usize>,
}

/// 장애물에 실제로 부딪힌(이동이 막힌) 순간의 정보.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Impact {
    /// 닿기 직전의 부호 있는 속도(m/s).
    pub speed_mps: f32,
    /// 멈춘 자세의 후륜축 중심 x 좌표(m).
    pub x_m: f32,
    /// 멈춘 자세의 후륜축 중심 z 좌표(m).
    pub z_m: f32,
}

/// `from`과 `to` 사이를 비율 `t`(0..=1)로 보간한 자세를 만든다. 위치·방향 외 값은 `to`를 따른다.
pub fn pose_between(from: &Motion, to: &Motion, t: f32) -> Motion {
    Motion {
        x_m: from.x_m + (to.x_m - from.x_m) * t,
        z_m: from.z_m + (to.z_m - from.z_m) * t,
        heading_rad: wrap_angle(
            from.heading_rad + wrap_angle(to.heading_rad - from.heading_rad) * t,
        ),
        ..*to
    }
}

/// 자세 `motion`의 차체가 어떤 장애물과 `margin`(m) 안으로 겹치는지 검사한다.
fn hits_any(footprint: &Footprint, motion: &Motion, obstacles: &[GroundRect], margin: f32) -> bool {
    let body = footprint.rect_at(motion);
    obstacles
        .iter()
        .any(|obstacle| body.overlaps(obstacle, margin))
}

/// 차체가 장애물과 접촉 거리([`CONTACT_MARGIN_M`]) 안에 있는지 검사한다.
pub fn in_contact(footprint: &Footprint, motion: &Motion, obstacles: &[GroundRect]) -> bool {
    hits_any(footprint, motion, obstacles, CONTACT_MARGIN_M)
}

/// 경로 위 한 자세의 검사 결과.
#[derive(Debug, Clone, Copy)]
struct Probe {
    /// 이 자세에서 막는 장애물 번호. 없으면 `None`.
    hit: Option<usize>,
    /// 출발 때 겹치지 않았던 장애물까지의 최소 분리 거리 하한(m). 그런 장애물이 없으면 무한대.
    gap: f32,
}

/// 한 단계 경로(`from`→`to` 보간) 검사기. 출발 자세의 겹침 정보는 만들 때 한 번만 구한다.
struct PathCheck<'a> {
    footprint: &'a Footprint,
    from: &'a Motion,
    to: &'a Motion,
    obstacles: &'a [GroundRect],
    /// 출발 자세에서 이미 겹친 장애물의 (번호, 출발 겹침 깊이). 보통 비어 있어 할당하지 않는다.
    start_overlaps: Vec<(usize, f32)>,
    /// 비율 0→1 동안 차체 위 어느 점이든 움직이는 거리의 상한(m).
    travel_m: f32,
}

impl<'a> PathCheck<'a> {
    /// 경로 검사기를 만든다. 출발 겹침 깊이와 이동 거리 상한을 미리 계산한다.
    fn new(
        footprint: &'a Footprint,
        from: &'a Motion,
        to: &'a Motion,
        obstacles: &'a [GroundRect],
    ) -> Self {
        let start_body = footprint.rect_at(from);
        let start_overlaps = obstacles
            .iter()
            .enumerate()
            .map(|(index, obstacle)| (index, start_body.penetration(obstacle)))
            .filter(|&(_, depth)| depth > 0.0)
            .collect();
        // 위치·방향을 선형 보간하므로 점 이동 거리는 기준점 이동 + 회전각 × 기준점에서 가장 먼 모서리 거리 이하다.
        let travel_m = Vec2::new(to.x_m - from.x_m, to.z_m - from.z_m).length()
            + wrap_angle(to.heading_rad - from.heading_rad).abs() * footprint.reach_m();
        Self {
            footprint,
            from,
            to,
            obstacles,
            start_overlaps,
            travel_m,
        }
    }

    /// 비율 `a`에서 `b`까지 차체 위 점이 움직이는 거리의 상한(m)을 구한다.
    fn span_m(&self, a: f32, b: f32) -> f32 {
        self.travel_m * (b - a)
    }

    /// 비율 `t` 자세를 검사한다.
    ///
    /// 출발 때 겹친 장애물은 출발보다 깊어질 때만 막고, 나머지는 겹치면 막는다.
    fn probe(&self, t: f32) -> Probe {
        let body = self.footprint.rect_at(&pose_between(self.from, self.to, t));
        let mut gap = f32::INFINITY;
        for (index, obstacle) in self.obstacles.iter().enumerate() {
            let depth = body.penetration(obstacle);
            let start = self
                .start_overlaps
                .iter()
                .find(|&&(overlapped, _)| overlapped == index)
                .map(|&(_, start)| start);
            let blocked = match start {
                Some(start) => depth > start,
                None => {
                    gap = gap.min(-depth);
                    depth > 0.0
                }
            };
            if blocked {
                return Probe {
                    hit: Some(index),
                    gap,
                };
            }
        }
        Probe { hit: None, gap }
    }

    /// 막히지 않은 비율 `a`(분리 하한 `gap_a`)에서 `b`(검사 결과 `at_b`)까지 가장 먼저 닿는 곳을 찾는다.
    ///
    /// 양 끝 분리 거리의 합이 구간 이동 거리보다 크면 구간 안 어디서도 닿을 수 없다(각 점은 이동한 만큼만
    /// 가까워진다). 그렇지 않으면 반으로 나눠 앞 구간부터 확인한다. 닿으면 (닿기 직전 비율, 장애물 번호).
    fn first_contact(&self, a: f32, gap_a: f32, b: f32, at_b: Probe) -> Option<(f32, usize)> {
        if let Some(index) = at_b.hit {
            return Some(self.boundary(a, gap_a, b, index));
        }
        let span = self.span_m(a, b);
        if gap_a + at_b.gap > span || span <= MIN_CERTIFY_SPAN_M {
            return None;
        }
        let mid = (a + b) / 2.0;
        let at_mid = self.probe(mid);
        self.first_contact(a, gap_a, mid, at_mid)
            .or_else(|| self.first_contact(mid, at_mid.gap, b, at_b))
    }

    /// 막히지 않은 `clear`와 장애물 `index`에 막힌 `hit` 사이에서 처음 닿기 직전 비율을 좁힌다.
    ///
    /// 가운데가 비어 있으면 앞 구간에 더 이른 접촉이 숨어 있지 않은지 [`Self::first_contact`]로 확인한다.
    fn boundary(
        &self,
        mut clear: f32,
        mut gap: f32,
        mut hit: f32,
        mut index: usize,
    ) -> (f32, usize) {
        while self.span_m(clear, hit) > BOUNDARY_RESOLUTION_M {
            let mid = (clear + hit) / 2.0;
            let at_mid = self.probe(mid);
            if let Some(blocking) = at_mid.hit {
                hit = mid;
                index = blocking;
                continue;
            }
            if let Some(earlier) = self.first_contact(clear, gap, mid, at_mid) {
                return earlier;
            }
            clear = mid;
            gap = at_mid.gap;
        }
        (clear, index)
    }
}

/// `from`에서 `to`로 가는 경로를 검사해 장애물에 닿기 직전까지의 이동 비율을 구한다.
///
/// 기본 간격([`MAX_SAMPLE_STEP_M`])으로 나눈 검사 지점 사이도 보수적으로 확인하므로, 회전 중 모서리가
/// 지점 사이에서만 장애물을 스치는 경우도 막는다. 최소 검사 구간([`MIN_CERTIFY_SPAN_M`])에 따른 근사와
/// 부동소수점 오차가 있다.
///
/// 출발 자세에서 이미 겹친 장애물(장애물이 차량 위에 놓인 경우 등)은 빠져나갈 수 있도록,
/// 겹침 깊이가 출발 때보다 깊어지는 경우만 막는다. 이 장애물은 검사 지점에서만 확인한다.
/// 나머지 장애물은 평소처럼 닿으면 막는다.
pub fn sweep(footprint: &Footprint, from: &Motion, to: &Motion, obstacles: &[GroundRect]) -> Sweep {
    let free = Sweep {
        fraction: 1.0,
        blocked_by: None,
    };
    if obstacles.is_empty() {
        return free;
    }
    let path = PathCheck::new(footprint, from, to, obstacles);
    let samples = (path.travel_m / MAX_SAMPLE_STEP_M).ceil().max(1.0) as u32;
    // 출발 자세는 정의상 막히지 않는다(겹친 장애물은 출발 깊이 그대로다).
    let mut clear = 0.0;
    let mut gap = path.probe(0.0).gap;
    for index in 1..=samples {
        let t = index as f32 / samples as f32;
        let at_t = path.probe(t);
        if let Some((fraction, obstacle)) = path.first_contact(clear, gap, t, at_t) {
            return Sweep {
                fraction,
                blocked_by: Some(obstacle),
            };
        }
        clear = t;
        gap = at_t.gap;
    }
    free
}

/// 운동 모델을 한 단계 적분하고 장애물 관통을 막는다.
///
/// 경로가 장애물에 막히면 닿기 직전 자세에서 세우고 속도를 0으로 만든다.
/// `contact`는 직전 단계까지의 접촉 상태이며 이번 단계 결과로 갱신된다.
///
/// 충돌([`Impact`])은 이동이 실제로 막힌 단계에서만 반환한다. 장애물 가까이(접촉 거리 안)에
/// 멈추거나 스쳐 지나가기만 하면 충돌이 아니다. 같은 장애물을 계속 밀어도 한 번만 반환하고,
/// 그 장애물의 접촉 거리 밖으로 떨어진 뒤 다시 막히면 새 충돌로 반환한다. 중복 판정은 장애물마다
/// 따로 하므로, 다른 장애물에 붙어 있거나 다른 장애물에 새로 막혀도 각각 기록된다.
pub fn step_with_obstacles(
    state: &mut VehicleState,
    motion: &mut Motion,
    params: &DynamicsParams,
    dt: f32,
    footprint: &Footprint,
    obstacles: &[GroundRect],
    contact: &mut ContactState,
) -> Option<Impact> {
    let before = *motion;
    let speed_before = state.speed_mps();
    dynamics::step(state, motion, params, dt);
    let speed_at_contact = state.speed_mps();
    let result = sweep(footprint, &before, motion, obstacles);
    if result.blocked() {
        *motion = pose_between(&before, motion, result.fraction);
        state.set_speed_mps(0.0);
        motion.accel_mps2 = -speed_before / dt;
    }
    let body = footprint.rect_at(motion);
    let near = |index: usize| {
        obstacles
            .get(index)
            .is_some_and(|obstacle| body.overlaps(obstacle, CONTACT_MARGIN_M))
    };
    contact.touching = (0..obstacles.len()).any(near);
    contact.blocked = result.blocked();
    contact.reported.retain(|&index| near(index));
    let new_impact = match result.blocked_by {
        Some(index) if !contact.reported.contains(&index) => {
            contact.reported.push(index);
            true
        }
        _ => false,
    };
    new_impact.then_some(Impact {
        speed_mps: speed_at_contact,
        x_m: motion.x_m,
        z_m: motion.z_m,
    })
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_4;

    use super::*;
    use crate::vehicle::{Gear, VehicleCommand, VehicleInput};

    /// 테스트용 고정 시간 간격(s). Bevy 기본 FixedUpdate 주기(64Hz)와 같다.
    const DT: f32 = 1.0 / 64.0;

    /// 시동 Ready, 지정 기어, 정지 상태의 차량을 만든다.
    fn ready_in(gear: Gear) -> VehicleState {
        let mut state = VehicleState::default();
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        state.apply(VehicleCommand::RequestGear(gear)).unwrap();
        state
    }

    /// 차량 정면(-Z) `distance_m` 앞에 있는 폭 6 m, 두께 `thickness_m`의 벽을 만든다.
    fn wall_ahead(distance_m: f32, thickness_m: f32) -> GroundRect {
        let front = VEHICLE_FOOTPRINT.rear_axle_to_center_m + VEHICLE_FOOTPRINT.length_m / 2.0;
        GroundRect::axis_aligned(
            Vec2::new(0.0, -(front + distance_m + thickness_m / 2.0)),
            Vec2::new(6.0, thickness_m),
        )
    }

    /// 같은 입력으로 `steps`번 적분하고 새 충돌 목록을 반환한다. 관통 검사를 하지 않는다(초기 겹침 시험용).
    fn drive_unchecked(
        state: &mut VehicleState,
        motion: &mut Motion,
        contact: &mut ContactState,
        obstacles: &[GroundRect],
        driver: VehicleInput,
        steps: usize,
    ) -> Vec<Impact> {
        state.set_input(driver);
        (0..steps)
            .filter_map(|_| {
                step_with_obstacles(
                    state,
                    motion,
                    &DynamicsParams::default(),
                    DT,
                    &VEHICLE_FOOTPRINT,
                    obstacles,
                    contact,
                )
            })
            .collect()
    }

    /// 같은 입력으로 `steps`번 적분하며 매 단계 관통이 없는지 확인하고, 새 접촉 보고 목록을 반환한다.
    fn drive(
        state: &mut VehicleState,
        motion: &mut Motion,
        contact: &mut ContactState,
        obstacles: &[GroundRect],
        driver: VehicleInput,
        steps: usize,
    ) -> Vec<Impact> {
        state.set_input(driver);
        let mut impacts = Vec::new();
        for _ in 0..steps {
            impacts.extend(step_with_obstacles(
                state,
                motion,
                &DynamicsParams::default(),
                DT,
                &VEHICLE_FOOTPRINT,
                obstacles,
                contact,
            ));
            assert!(
                !hits_any(&VEHICLE_FOOTPRINT, motion, obstacles, 0.0),
                "vehicle penetrated an obstacle at {motion:?}"
            );
        }
        impacts
    }

    /// 가속 입력만 있는 연속 입력을 만든다.
    fn throttle() -> VehicleInput {
        VehicleInput {
            throttle: 1.0,
            ..VehicleInput::default()
        }
    }

    /// 겹침·맞닿음·떨어짐·회전 배치에서 분리축 판정이 맞는지 확인한다.
    #[test]
    fn rect_overlap_handles_rotation() {
        let unit = |x: f32, z: f32| GroundRect::axis_aligned(Vec2::new(x, z), Vec2::ONE);
        assert!(unit(0.0, 0.0).overlaps(&unit(0.9, 0.0), 0.0));
        assert!(!unit(0.0, 0.0).overlaps(&unit(1.0, 0.0), 0.0));
        assert!(unit(0.0, 0.0).overlaps(&unit(1.01, 0.0), CONTACT_MARGIN_M));
        assert!(!unit(0.0, 0.0).overlaps(&unit(1.1, 1.1), 0.0));

        // 45도 돌린 정사각형은 모서리가 0.707 m까지 나온다.
        let diamond = GroundRect {
            heading_rad: FRAC_PI_4,
            ..unit(0.0, 0.0)
        };
        assert!(diamond.overlaps(&unit(1.15, 0.0), 0.0));
        assert!(!diamond.overlaps(&unit(1.25, 0.0), 0.0));
        // 축 정렬 경계 상자로는 겹치지만 실제로는 떨어진 대각선 배치.
        assert!(!diamond.overlaps(&unit(0.95, 0.95), 0.0));
    }

    /// 차체 박스가 후륜축 기준으로 전방에 놓이고 방향을 따라 도는지 확인한다.
    #[test]
    fn footprint_sits_ahead_of_rear_axle() {
        let rect = VEHICLE_FOOTPRINT.rect_at(&Motion {
            heading_rad: -std::f32::consts::FRAC_PI_2,
            ..Motion::default()
        });
        // heading -90도면 전방은 +X다.
        assert!((rect.center - Vec2::new(1.4, 0.0)).length() < 1e-5);
        assert_eq!(rect.half_size, Vec2::new(0.9, 2.1));
    }

    /// 벽을 향해 계속 가속해도 관통하지 않고, 벽 바로 앞에서 정지하며 충돌은 한 번만 보고하는지 확인한다.
    #[test]
    fn driving_into_wall_stops_at_contact_once() {
        let wall = [wall_ahead(3.0, 0.5)];
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        let mut contact = ContactState::default();
        let impacts = drive(
            &mut state,
            &mut motion,
            &mut contact,
            &wall,
            throttle(),
            640,
        );

        assert_eq!(impacts.len(), 1, "{impacts:?}");
        assert!(impacts[0].speed_mps > 1.0);
        assert!(contact.touching);
        assert_eq!(state.speed_mps(), 0.0);
        assert!(in_contact(&VEHICLE_FOOTPRINT, &motion, &wall));
        // 이분 탐색으로 벽에 거의 붙어서 멈춘다.
        assert!(
            (motion.z_m + 3.0).abs() < 1e-3,
            "stopped at z={}",
            motion.z_m
        );
    }

    /// 출발·도착 자세가 모두 비어 있어도 경로 중간의 얇은 벽에서 멈추는지 확인한다.
    #[test]
    fn sweep_checks_path_between_endpoints() {
        // 6 m 이동하면 차체 전체가 벽(앞범퍼 기준 1 m 앞, 두께 0.1 m)을 지나간다.
        let wall = [wall_ahead(1.0, 0.1)];
        let from = Motion::default();
        let to = Motion { z_m: -6.0, ..from };
        assert!(!hits_any(&VEHICLE_FOOTPRINT, &to, &wall, 0.0));
        let result = sweep(&VEHICLE_FOOTPRINT, &from, &to, &wall);
        assert!(result.blocked());
        assert!((result.fraction - 1.0 / 6.0).abs() < 1e-3, "{result:?}");
    }

    /// 벽 1 cm 앞에 정지해 있으면 접촉으로만 표시하고 충돌은 0회이며,
    /// 거기서 벽으로 가속하면 충돌 1회, 계속 밀어도 1회를 유지하는지 확인한다.
    #[test]
    fn resting_near_wall_is_contact_not_collision() {
        let wall = [wall_ahead(1.0, 0.5)];
        let mut state = ready_in(Gear::Drive);
        // 앞범퍼와 벽 사이 1 cm.
        let mut motion = Motion {
            z_m: -0.99,
            ..Motion::default()
        };
        let mut contact = ContactState::default();
        let idle = drive(
            &mut state,
            &mut motion,
            &mut contact,
            &wall,
            VehicleInput::default(),
            64,
        );
        assert!(idle.is_empty(), "{idle:?}");
        assert!(contact.touching);

        // 정지 상태에서 1 cm를 메우려면 몇 단계가 걸리므로 1초 동안 가속한다.
        let first = drive(&mut state, &mut motion, &mut contact, &wall, throttle(), 64);
        assert_eq!(first.len(), 1, "{first:?}");
        assert!(first[0].speed_mps > 0.0);
        let pushing = drive(
            &mut state,
            &mut motion,
            &mut contact,
            &wall,
            throttle(),
            320,
        );
        assert!(pushing.is_empty(), "{pushing:?}");
        assert!(contact.touching);
    }

    /// 벽 옆을 1 cm 간격으로 나란히 지나가면 접촉으로만 표시하고 충돌로 세지 않는지 확인한다.
    #[test]
    fn passing_alongside_wall_is_not_collision() {
        // 차체 오른쪽 면(x=0.9)에서 1 cm 떨어진, 진행 방향(-Z)으로 긴 벽.
        let wall = [GroundRect::axis_aligned(
            Vec2::new(0.9 + 0.01 + 0.25, -20.0),
            Vec2::new(0.5, 50.0),
        )];
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        let mut contact = ContactState::default();
        let impacts = drive(
            &mut state,
            &mut motion,
            &mut contact,
            &wall,
            throttle(),
            320,
        );
        assert!(impacts.is_empty(), "{impacts:?}");
        assert!(contact.touching);
        assert!(state.speed_mps() > 4.0);
    }

    /// 최고 속도(40 km/h)에서도 검사 간격보다 두꺼운 얇은 벽(0.1 m)을 건너뛰지 않는지 확인한다.
    #[test]
    fn thin_wall_is_not_tunneled_at_top_speed() {
        let wall = [wall_ahead(30.0, 0.1)];
        let mut state = ready_in(Gear::Drive);
        // 30 m 안에서는 최고 속도까지 가속하지 못하므로 최고 속도에서 출발한다.
        state.set_speed_mps(DynamicsParams::default().max_forward_speed_mps);
        let mut motion = Motion::default();
        let mut contact = ContactState::default();
        let impacts = drive(
            &mut state,
            &mut motion,
            &mut contact,
            &wall,
            throttle(),
            64 * 15,
        );
        assert_eq!(impacts.len(), 1);
        assert_eq!(
            impacts[0].speed_mps,
            DynamicsParams::default().max_forward_speed_mps
        );
        assert!(motion.z_m > -31.0);
    }

    /// 후진으로 벽에서 떨어지면 접촉이 풀리고, 다시 닿으면 새 충돌로 보고하는지 확인한다.
    #[test]
    fn leaving_and_returning_reports_new_impact() {
        let wall = [wall_ahead(1.0, 0.5)];
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        let mut contact = ContactState::default();
        assert_eq!(
            drive(
                &mut state,
                &mut motion,
                &mut contact,
                &wall,
                throttle(),
                192
            )
            .len(),
            1
        );

        state
            .apply(VehicleCommand::RequestGear(Gear::Reverse))
            .unwrap();
        assert!(drive(&mut state, &mut motion, &mut contact, &wall, throttle(), 64).is_empty());
        assert!(!contact.touching);

        let brake = VehicleInput {
            brake: 1.0,
            ..VehicleInput::default()
        };
        drive(&mut state, &mut motion, &mut contact, &wall, brake, 64);
        state
            .apply(VehicleCommand::RequestGear(Gear::Drive))
            .unwrap();
        assert_eq!(
            drive(
                &mut state,
                &mut motion,
                &mut contact,
                &wall,
                throttle(),
                256
            )
            .len(),
            1
        );
    }

    /// 측면 벽에 1 cm 간격으로 붙은 채 정면 벽에서 떨어졌다 다시 닿으면 새 충돌로 보고하는지 확인한다.
    ///
    /// 중복 방지를 장애물마다 따로 하지 않으면, 측면 벽 때문에 접촉이 계속 유지돼 재충돌을 놓친다.
    #[test]
    fn returning_to_wall_beside_another_reports_new_impact() {
        let obstacles = [
            wall_ahead(1.0, 0.5),
            // 차체 오른쪽 면(x=0.9)에서 1 cm 떨어진, 진행 방향으로 긴 측면 벽(z 8..-12).
            GroundRect::axis_aligned(Vec2::new(0.9 + 0.01 + 0.25, -2.0), Vec2::new(0.5, 20.0)),
        ];
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        let mut contact = ContactState::default();
        let first = drive(
            &mut state,
            &mut motion,
            &mut contact,
            &obstacles,
            throttle(),
            192,
        );
        assert_eq!(first.len(), 1, "{first:?}");

        state
            .apply(VehicleCommand::RequestGear(Gear::Reverse))
            .unwrap();
        assert!(
            drive(
                &mut state,
                &mut motion,
                &mut contact,
                &obstacles,
                throttle(),
                64
            )
            .is_empty()
        );
        let front = VEHICLE_FOOTPRINT.rect_at(&motion);
        assert!(
            !front.overlaps(&obstacles[0], CONTACT_MARGIN_M),
            "{motion:?}"
        );
        // 측면 벽 때문에 접촉 표시는 계속 켜져 있다.
        assert!(contact.touching);

        let brake = VehicleInput {
            brake: 1.0,
            ..VehicleInput::default()
        };
        drive(&mut state, &mut motion, &mut contact, &obstacles, brake, 64);
        state
            .apply(VehicleCommand::RequestGear(Gear::Drive))
            .unwrap();
        let again = drive(
            &mut state,
            &mut motion,
            &mut contact,
            &obstacles,
            throttle(),
            256,
        );
        assert_eq!(again.len(), 1, "{again:?}");
    }

    /// 회전하며 비스듬히 다가가도 모서리가 장애물을 관통하지 않는지 확인한다.
    #[test]
    fn turning_into_pillar_does_not_penetrate() {
        let pillar = [GroundRect::axis_aligned(
            Vec2::new(4.0, -6.0),
            Vec2::new(1.0, 1.0),
        )];
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        let mut contact = ContactState::default();
        let driver = VehicleInput {
            throttle: 0.6,
            steering: 1.0,
            ..VehicleInput::default()
        };
        let impacts = drive(&mut state, &mut motion, &mut contact, &pillar, driver, 640);
        assert_eq!(impacts.len(), 1);
        assert_eq!(state.speed_mps(), 0.0);
    }

    /// 회전 중 검사 지점 사이에서만 모서리가 기둥을 스치는 경우도 막는지 확인한다(검토 지적 사례).
    ///
    /// 최고 속도·최대 조향각으로 도는 한 단계에서, 경로를 잘게 나눈 검사 지점은 모두 비어 있지만
    /// 그 사이(단계의 약 5/6 지점)에서 차체 모서리가 장면의 1 m 기둥을 수 mm 침범한다.
    #[test]
    fn turning_clip_between_samples_is_blocked() {
        // 이 자세는 Phase 2 기준 모델(5 m/s, 기구학 곡률)로 만든 사례라 그 값으로 재현한다.
        let params = DynamicsParams::phase2_reference();
        let pillar = [GroundRect::axis_aligned(Vec2::new(6.0, -14.0), Vec2::ONE)];
        let depth = |motion: &Motion| VEHICLE_FOOTPRINT.rect_at(motion).penetration(&pillar[0]);
        let mut state = ready_in(Gear::Drive);
        state.set_speed_mps(params.max_forward_speed_mps);
        state.set_input(VehicleInput {
            throttle: 1.0,
            steering: 1.0,
            ..VehicleInput::default()
        });
        let from = Motion {
            x_m: 4.839558,
            z_m: -10.219388,
            heading_rad: -0.7,
            steer_angle_rad: params.max_steer_angle_deg.to_radians(),
            ..Motion::default()
        };
        let mut to = from;
        dynamics::step(&mut state, &mut to, &params, DT);
        // 전제: 양 끝은 비어 있고, 경로 중간에서만 수 mm 겹친다.
        let deepest = |end: f32| {
            (0..=2000)
                .map(|i| depth(&pose_between(&from, &to, end * i as f32 / 2000.0)))
                .fold(f32::NEG_INFINITY, f32::max)
        };
        assert!(depth(&from) < 0.0 && depth(&to) < 0.0);
        assert!(deepest(1.0) > 0.003, "deepest={}", deepest(1.0));

        let result = sweep(&VEHICLE_FOOTPRINT, &from, &to, &pillar);
        assert!(result.blocked(), "{result:?}");
        assert!(deepest(result.fraction) <= 0.0, "{result:?}");
    }

    /// 장애물과 겹친 채 시작해도 갇히지 않고 빠져나갈 수 있는지 확인한다.
    #[test]
    fn starting_inside_obstacle_can_escape() {
        let block = [GroundRect::axis_aligned(
            Vec2::new(0.0, -1.4),
            Vec2::new(1.0, 1.0),
        )];
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        let mut contact = ContactState::default();
        state.set_input(throttle());
        for _ in 0..320 {
            step_with_obstacles(
                &mut state,
                &mut motion,
                &DynamicsParams::default(),
                DT,
                &VEHICLE_FOOTPRINT,
                &block,
                &mut contact,
            );
        }
        assert!(motion.z_m < -5.0);
        assert!(!contact.touching);
    }

    /// 장애물과 겹친 채 시작하면 더 깊이 들어가는 이동은 막고(충돌 1회), 빠져나가는 이동은 허용하는지 확인한다.
    #[test]
    fn starting_overlap_cannot_push_deeper() {
        // 앞범퍼(z=-3.5)가 1 m 상자(z -4.3..-3.3) 안으로 0.2 m 들어가 있다.
        let block = [GroundRect::axis_aligned(
            Vec2::new(0.0, -3.8),
            Vec2::new(1.0, 1.0),
        )];
        let depth = |motion: &Motion| VEHICLE_FOOTPRINT.rect_at(motion).penetration(&block[0]);
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        let mut contact = ContactState::default();
        let start = depth(&motion);
        assert!((start - 0.2).abs() < 1e-5);

        let impacts = drive_unchecked(
            &mut state,
            &mut motion,
            &mut contact,
            &block,
            throttle(),
            64,
        );
        assert_eq!(impacts.len(), 1);
        assert!(depth(&motion) <= start, "{motion:?}");
        assert_eq!(state.speed_mps(), 0.0);

        state
            .apply(VehicleCommand::RequestGear(Gear::Reverse))
            .unwrap();
        drive_unchecked(
            &mut state,
            &mut motion,
            &mut contact,
            &block,
            throttle(),
            128,
        );
        assert!(depth(&motion) < -CONTACT_MARGIN_M, "{motion:?}");
        assert!(!contact.touching);
    }

    /// 한 장애물과 겹친 채 시작해도 다른 장애물은 평소처럼 막는지 확인한다.
    #[test]
    fn starting_overlap_still_blocks_other_obstacles() {
        // 차체 한가운데를 지나는 긴 띠(z 5..-25)와 계속 겹친 채로, 앞범퍼(z=-3.5)에서 6 m 앞의
        // 벽 앞면(z=-9.5)에 닿는다. 띠와의 겹침 깊이는 전진해도 그대로다.
        let obstacles = [
            GroundRect::axis_aligned(Vec2::new(0.0, -10.0), Vec2::new(1.0, 30.0)),
            GroundRect::axis_aligned(Vec2::new(0.0, -10.0), Vec2::new(6.0, 1.0)),
        ];
        let mut state = ready_in(Gear::Drive);
        let mut motion = Motion::default();
        let mut contact = ContactState::default();
        let mut impacts = Vec::new();
        for _ in 0..64 * 8 {
            impacts.extend(drive_unchecked(
                &mut state,
                &mut motion,
                &mut contact,
                &obstacles,
                throttle(),
                1,
            ));
            assert!(
                motion.z_m > -6.0 - 1e-3,
                "passed the wall: z={}",
                motion.z_m
            );
        }
        assert_eq!(impacts.len(), 1);
        assert!(motion.z_m < -6.0 + 0.01, "stopped early: z={}", motion.z_m);
        assert_eq!(state.speed_mps(), 0.0);
    }

    /// 장애물이 없으면 운동 모델 결과와 똑같이 움직이는지 확인한다.
    #[test]
    fn no_obstacles_matches_plain_dynamics() {
        let params = DynamicsParams::default();
        let mut plain_state = ready_in(Gear::Drive);
        let mut plain = Motion::default();
        let mut state = plain_state.clone();
        let mut motion = plain;
        let mut contact = ContactState::default();
        let driver = VehicleInput {
            throttle: 1.0,
            steering: 0.5,
            ..VehicleInput::default()
        };
        plain_state.set_input(driver);
        state.set_input(driver);
        for _ in 0..128 {
            dynamics::step(&mut plain_state, &mut plain, &params, DT);
            let impact = step_with_obstacles(
                &mut state,
                &mut motion,
                &params,
                DT,
                &VEHICLE_FOOTPRINT,
                &[],
                &mut contact,
            );
            assert!(impact.is_none());
        }
        assert_eq!((state, motion), (plain_state, plain));
    }
}
