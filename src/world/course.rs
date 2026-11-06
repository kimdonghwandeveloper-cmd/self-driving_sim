//! 개발용 코스 정의(순수 데이터, Phase 3-2).
//!
//! 코스 하나의 시작 자세·노면·표시선·충돌 경계를 [`CourseDef`] 한 곳에서 정의한다.
//! 화면 메시와 충돌 판정은 같은 [`Collider`] 값을 쓰므로 둘이 어긋나지 않는다.
//! 좌표는 지면 (x, z) m이며 시작 자세의 전방은 -Z다. 모든 치수는 **훈련용 임시값**이며
//! 실제 도로·공원 통로의 실측값이 아니다.

use bevy::prelude::*;

use crate::vehicle::{GroundRect, Motion, VEHICLE_FOOTPRINT};

/// 훈련 코스의 차로 반폭(m). 차로 폭 6 m는 임시값이다.
pub const LANE_HALF_WIDTH_M: f32 = 3.0;
/// 좁은 통로에서 차체 양옆에 두는 여유(m). 임시값.
pub const NARROW_SIDE_MARGIN_M: f32 = 0.6;
/// 좁은 통로 폭(m) = 차체 폭 + 양옆 여유.
pub const NARROW_WIDTH_M: f32 = VEHICLE_FOOTPRINT.width_m + 2.0 * NARROW_SIDE_MARGIN_M;
/// 훈련 코스 뒤쪽 끝(m). 시작 자세 차체 뒤끝(z=+0.7)에서 약 5 m 뒤다.
const TRAINING_REAR_Z_M: f32 = 6.0;
/// 훈련 코스 앞쪽 끝(m).
const TRAINING_FRONT_Z_M: f32 = -66.0;
/// 정지선 위치(m).
const STOP_LINE_Z_M: f32 = -20.0;
/// 좁은 통로 시작·끝(m).
const NARROW_START_Z_M: f32 = -42.0;
const NARROW_END_Z_M: f32 = -54.0;
/// 차로 오른쪽에 둔 고정 장애물 중심(x, z; m)과 한 변 길이(m).
const SIDE_OBSTACLE_CENTER_M: Vec2 = Vec2::new(1.9, -32.0);
const SIDE_OBSTACLE_SIZE_M: f32 = 0.8;
/// 연석 두께(m)와 표시 높이(m).
const CURB_THICKNESS_M: f32 = 0.3;
const CURB_HEIGHT_M: f32 = 0.25;
/// 끝 벽 두께(m)와 표시 높이(m).
const END_WALL_THICKNESS_M: f32 = 0.5;
const END_WALL_HEIGHT_M: f32 = 1.2;
/// 좁은 통로 가벽·장애물 표시 높이(m).
const BARRIER_HEIGHT_M: f32 = 1.0;
/// 차로 가장자리 표시선 폭(m)과 연석 안쪽으로 들인 거리(m).
const EDGE_LINE_WIDTH_M: f32 = 0.12;
const EDGE_LINE_INSET_M: f32 = 0.2;
/// 정지선 두께(앞뒤, m).
const STOP_LINE_DEPTH_M: f32 = 0.4;

/// 선택 가능한 개발 코스.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CourseId {
    /// 직선·정지선·고정 장애물·좁은 통로가 있는 기본 조작 훈련 코스.
    #[default]
    Training,
    /// 가속·제동·회전 보정용 넓은 평지(Phase 2-5의 기존 장애물 배치).
    Calibration,
}

impl CourseId {
    /// 명령줄 이름(`training`, `calibration`)을 코스로 바꾼다. 모르는 이름이면 `None`.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim() {
            "training" => Some(Self::Training),
            "calibration" => Some(Self::Calibration),
            _ => None,
        }
    }

    /// 명령줄·로그에 쓰는 이름을 반환한다.
    pub fn name(self) -> &'static str {
        match self {
            Self::Training => "training",
            Self::Calibration => "calibration",
        }
    }

    /// 코스 정의를 만든다.
    pub fn build(self) -> CourseDef {
        match self {
            Self::Training => training_course(),
            Self::Calibration => calibration_course(),
        }
    }
}

/// 충돌 경계 종류. 화면 색과 높이만 다르고 판정 방식은 같다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColliderKind {
    /// 차로 양옆의 낮은 연석(주행 경계).
    Curb,
    /// 코스 끝을 막는 벽.
    Wall,
    /// 좁은 통로를 만드는 가벽.
    Barrier,
    /// 고정 장애물.
    Obstacle,
}

/// 차량이 통과할 수 없는 지면 직사각형과 그 표시 높이.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Collider {
    /// 충돌 판정 영역. 표시 상자의 바닥면과 같다.
    pub rect: GroundRect,
    /// 표시 높이(m). 충돌 판정은 높이를 보지 않는다.
    pub height_m: f32,
    /// 종류.
    pub kind: ColliderKind,
}

/// 노면 표시 종류. 표시선은 화면에만 있고 충돌 판정이 없다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkingKind {
    /// 차로 가장자리 흰 선(시각 표시; 실제 경계는 연석).
    EdgeLine,
    /// 정지선.
    StopLine,
}

/// 지면에 칠한 표시.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marking {
    /// 표시 영역.
    pub rect: GroundRect,
    /// 종류.
    pub kind: MarkingKind,
}

/// 코스 하나의 전체 정의.
#[derive(Debug, Clone, PartialEq)]
pub struct CourseDef {
    /// 코스 종류.
    pub id: CourseId,
    /// 시작·리셋 자세.
    pub start: Motion,
    /// 포장 노면 영역(시각 표시).
    pub surfaces: Vec<GroundRect>,
    /// 노면 표시.
    pub markings: Vec<Marking>,
    /// 충돌 경계.
    pub colliders: Vec<Collider>,
}

impl CourseDef {
    /// 충돌 판정에 쓰는 영역 목록을 반환한다.
    pub fn collision_rects(&self) -> Vec<GroundRect> {
        self.colliders
            .iter()
            .map(|collider| collider.rect)
            .collect()
    }
}

/// x·z 범위로 회전 없는 직사각형을 만든다. 범위의 순서는 상관없다.
fn rect_between(x: (f32, f32), z: (f32, f32)) -> GroundRect {
    let center = Vec2::new((x.0 + x.1) / 2.0, (z.0 + z.1) / 2.0);
    let size = Vec2::new((x.1 - x.0).abs(), (z.1 - z.0).abs());
    GroundRect::axis_aligned(center, size)
}

/// 기본 조작 훈련 코스를 만든다.
///
/// 원점에서 -Z로 직진하면 정지선(z=-20) → 오른쪽 고정 장애물(z=-32) → 좁은 통로(z=-42..-54)
/// → 끝 벽(z=-66) 순으로 지난다. 차로 중앙으로 직진하면 장애물과 통로 가벽에 닿지 않고,
/// 후진하면 뒤쪽 벽(z=+6)에서 멈춘다.
pub fn training_course() -> CourseDef {
    let half = LANE_HALF_WIDTH_M;
    let (rear, front) = (TRAINING_REAR_Z_M, TRAINING_FRONT_Z_M);
    let curb_x = half + CURB_THICKNESS_M;
    let curb = |side: f32| Collider {
        rect: rect_between((side * half, side * curb_x), (rear, front)),
        height_m: CURB_HEIGHT_M,
        kind: ColliderKind::Curb,
    };
    let end_wall = |z: f32, outward: f32| Collider {
        rect: rect_between((-curb_x, curb_x), (z, z + outward * END_WALL_THICKNESS_M)),
        height_m: END_WALL_HEIGHT_M,
        kind: ColliderKind::Wall,
    };
    let narrow_half = NARROW_WIDTH_M / 2.0;
    let barrier = |side: f32| Collider {
        rect: rect_between(
            (side * narrow_half, side * half),
            (NARROW_START_Z_M, NARROW_END_Z_M),
        ),
        height_m: BARRIER_HEIGHT_M,
        kind: ColliderKind::Barrier,
    };
    let edge_x = half - EDGE_LINE_INSET_M;
    // 가장자리 선은 좁은 통로 가벽 아래를 지나지 않도록 통로 앞뒤 두 구간으로 나눈다.
    let edge_line = |side: f32, z: (f32, f32)| Marking {
        rect: rect_between(
            (
                side * (edge_x - EDGE_LINE_WIDTH_M / 2.0),
                side * (edge_x + EDGE_LINE_WIDTH_M / 2.0),
            ),
            z,
        ),
        kind: MarkingKind::EdgeLine,
    };
    CourseDef {
        id: CourseId::Training,
        start: Motion::default(),
        surfaces: vec![rect_between((-half, half), (rear, front))],
        markings: vec![
            edge_line(-1.0, (rear, NARROW_START_Z_M)),
            edge_line(1.0, (rear, NARROW_START_Z_M)),
            edge_line(-1.0, (NARROW_END_Z_M, front)),
            edge_line(1.0, (NARROW_END_Z_M, front)),
            Marking {
                rect: rect_between(
                    (-edge_x, edge_x),
                    (
                        STOP_LINE_Z_M + STOP_LINE_DEPTH_M / 2.0,
                        STOP_LINE_Z_M - STOP_LINE_DEPTH_M / 2.0,
                    ),
                ),
                kind: MarkingKind::StopLine,
            },
        ],
        colliders: vec![
            curb(-1.0),
            curb(1.0),
            end_wall(rear, 1.0),
            end_wall(front, -1.0),
            barrier(-1.0),
            barrier(1.0),
            Collider {
                rect: GroundRect::axis_aligned(
                    SIDE_OBSTACLE_CENTER_M,
                    Vec2::splat(SIDE_OBSTACLE_SIZE_M),
                ),
                height_m: BARRIER_HEIGHT_M,
                kind: ColliderKind::Obstacle,
            },
        ],
    }
}

/// 보정용 넓은 평지를 만든다. Phase 2-5에서 쓰던 장애물 4개 배치를 그대로 둔다.
pub fn calibration_course() -> CourseDef {
    let obstacle = |center: Vec2, size: Vec2| Collider {
        rect: GroundRect::axis_aligned(center, size),
        height_m: END_WALL_HEIGHT_M,
        kind: ColliderKind::Obstacle,
    };
    CourseDef {
        id: CourseId::Calibration,
        start: Motion::default(),
        surfaces: Vec::new(),
        markings: Vec::new(),
        colliders: vec![
            // 정면 30 m 앞의 벽
            obstacle(Vec2::new(0.0, -30.0), Vec2::new(12.0, 0.5)),
            // 오른쪽 앞 기둥
            obstacle(Vec2::new(6.0, -14.0), Vec2::new(1.0, 1.0)),
            // 왼쪽 앞 상자
            obstacle(Vec2::new(-7.0, -10.0), Vec2::new(2.0, 2.0)),
            // 후진 시험용 뒤쪽 벽
            obstacle(Vec2::new(0.0, 15.0), Vec2::new(8.0, 0.5)),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::collision::{ContactState, in_contact, step_with_obstacles};
    use crate::vehicle::{
        DynamicsParams, EngineState, Gear, VehicleCommand, VehicleInput, VehicleState,
    };

    /// 테스트용 고정 시간 간격(s).
    const DT: f32 = 1.0 / 64.0;

    /// 시동 Ready, 지정 기어인 차량 상태를 만든다.
    fn ready_in(gear: Gear) -> VehicleState {
        let mut state = VehicleState::default();
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        state.apply(VehicleCommand::RequestGear(gear)).unwrap();
        assert_eq!(state.engine(), EngineState::Ready);
        state
    }

    /// 시작 자세에서 최대 가속으로 직진해 처음 막힐 때까지 몰고, 멈춘 자세를 반환한다.
    fn drive_until_blocked(course: &CourseDef, gear: Gear) -> Motion {
        let obstacles = course.collision_rects();
        let params = DynamicsParams::default();
        let mut state = ready_in(gear);
        state.set_input(VehicleInput {
            throttle: 1.0,
            ..VehicleInput::default()
        });
        let mut motion = course.start;
        let mut contact = ContactState::default();
        for _ in 0..64 * 120 {
            if step_with_obstacles(
                &mut state,
                &mut motion,
                &params,
                DT,
                &VEHICLE_FOOTPRINT,
                &obstacles,
                &mut contact,
            )
            .is_some()
            {
                return motion;
            }
        }
        panic!("vehicle was never blocked: {motion:?}");
    }

    /// 모든 코스의 시작 자세 차체가 어떤 충돌 경계와도 닿지 않는지 확인한다.
    #[test]
    fn start_pose_is_clear_on_every_course() {
        for id in [CourseId::Training, CourseId::Calibration] {
            let course = id.build();
            assert!(
                !in_contact(&VEHICLE_FOOTPRINT, &course.start, &course.collision_rects()),
                "{id:?}"
            );
        }
    }

    /// 좁은 통로 폭이 차체 폭 + 양옆 여유이고, 가벽 안쪽 면 사이가 그 폭인지 확인한다.
    #[test]
    fn narrow_passage_width_is_footprint_plus_margin() {
        assert!((NARROW_WIDTH_M - 3.0).abs() < 1e-6);
        let course = training_course();
        let barriers: Vec<&Collider> = course
            .colliders
            .iter()
            .filter(|collider| collider.kind == ColliderKind::Barrier)
            .collect();
        assert_eq!(barriers.len(), 2);
        let inner = |collider: &Collider| collider.rect.center.x.abs() - collider.rect.half_size.x;
        for barrier in barriers {
            assert!((inner(barrier) - NARROW_WIDTH_M / 2.0).abs() < 1e-5);
        }
    }

    /// 차로 중앙으로 직진하면 장애물·좁은 통로를 지나 앞쪽 끝 벽에서 처음 막히는지 확인한다.
    #[test]
    fn driving_straight_reaches_front_wall_only() {
        let course = training_course();
        let stopped = drive_until_blocked(&course, Gear::Drive);
        let front_bumper_z = stopped.z_m
            - VEHICLE_FOOTPRINT.rear_axle_to_center_m
            - VEHICLE_FOOTPRINT.length_m / 2.0;
        assert!(
            (front_bumper_z - TRAINING_FRONT_Z_M).abs() < 0.05,
            "front bumper stopped at {front_bumper_z}"
        );
    }

    /// 시작 자세에서 후진하면 뒤쪽 끝 벽에서 막히는지 확인한다.
    #[test]
    fn reversing_from_start_hits_rear_wall() {
        let course = training_course();
        let stopped = drive_until_blocked(&course, Gear::Reverse);
        let rear_bumper_z = stopped.z_m - VEHICLE_FOOTPRINT.rear_axle_to_center_m
            + VEHICLE_FOOTPRINT.length_m / 2.0;
        assert!(
            (rear_bumper_z - TRAINING_REAR_Z_M).abs() < 0.05,
            "rear bumper stopped at {rear_bumper_z}"
        );
    }

    /// 표시선이 노면 안에 있고 충돌 경계와 겹치지 않는지 확인한다(시각 표시와 실제 경계 구분).
    #[test]
    fn markings_lie_on_road_and_off_colliders() {
        let course = training_course();
        let road = course.surfaces[0];
        for marking in &course.markings {
            let (low, high) = (
                marking.rect.center - marking.rect.half_size,
                marking.rect.center + marking.rect.half_size,
            );
            let (road_low, road_high) =
                (road.center - road.half_size, road.center + road.half_size);
            assert!(
                low.cmpge(road_low - 1e-5).all() && high.cmple(road_high + 1e-5).all(),
                "{marking:?} leaves the road"
            );
            for collider in &course.colliders {
                assert!(
                    !marking.rect.overlaps(&collider.rect, 0.0),
                    "{marking:?} overlaps {collider:?}"
                );
            }
        }
    }

    /// 모든 영역이 유한하고 크기가 양수인지 확인한다.
    #[test]
    fn course_geometry_is_finite_and_non_degenerate() {
        for id in [CourseId::Training, CourseId::Calibration] {
            let course = id.build();
            let rects = course
                .surfaces
                .iter()
                .chain(course.markings.iter().map(|marking| &marking.rect))
                .chain(course.colliders.iter().map(|collider| &collider.rect));
            for rect in rects {
                assert!(rect.center.is_finite() && rect.heading_rad.is_finite());
                assert!(rect.half_size.min_element() > 0.0, "{id:?}: {rect:?}");
            }
            assert!(
                course
                    .colliders
                    .iter()
                    .all(|collider| collider.height_m > 0.0)
            );
        }
    }

    /// 코스 이름을 읽고, 모르는 이름은 거부하는지 확인한다.
    #[test]
    fn course_names_round_trip() {
        for id in [CourseId::Training, CourseId::Calibration] {
            assert_eq!(CourseId::parse(id.name()), Some(id));
        }
        assert_eq!(CourseId::parse("yeosu"), None);
        assert_eq!(CourseId::default(), CourseId::Training);
    }
}
