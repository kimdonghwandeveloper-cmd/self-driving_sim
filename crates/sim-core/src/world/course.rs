//! 개발용 코스 정의(순수 데이터, Phase 3-2).
//!
//! 코스 하나의 시작 자세·노면·표시선·충돌 경계를 [`CourseDef`] 한 곳에서 정의한다.
//! 화면 메시와 충돌 판정은 같은 [`Collider`] 값을 쓰므로 둘이 어긋나지 않는다.
//! 좌표는 지면 (x, z) m이며 시작 자세의 전방은 -Z다. 모든 치수는 **훈련용 임시값**이며
//! 실제 도로·공원 통로의 실측값이 아니다.

use glam::Vec2;

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

/// 띠 메시 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripKind {
    /// 포장 노면.
    Road,
    /// 차로 가장자리 흰 선(시각 표시).
    EdgeLine,
}

/// 두 경계선 사이를 잇는 띠(시각 표시, 충돌 판정 없음). 같은 순번의 점끼리 마주 본다(Phase 4-1 굽은 길).
#[derive(Debug, Clone, PartialEq)]
pub struct Strip {
    /// 종류.
    pub kind: StripKind,
    /// 진행 방향 왼쪽 경계선.
    pub left: Vec<Vec2>,
    /// 진행 방향 오른쪽 경계선. `left`와 점 개수가 같다.
    pub right: Vec<Vec2>,
}

/// 중심에서 본 별 모양 다각형 노면(교차로 면, Phase 4-2). 중심에서 부채꼴 삼각형으로 그린다.
#[derive(Debug, Clone, PartialEq)]
pub struct Area {
    /// 부채꼴 중심(다각형 안, 모든 경계점이 보이는 점).
    pub center: Vec2,
    /// 경계 꼭짓점(반시계 순서).
    pub boundary: Vec<Vec2>,
}

/// 코스 하나의 전체 정의.
#[derive(Debug, Clone, PartialEq)]
pub struct CourseDef {
    /// 로그에 쓰는 코스 이름.
    pub name: String,
    /// 주행 화면에 계속 표시할 안내(지도 출처, 임시 폭 등). 없으면 표시하지 않는다.
    pub notices: Vec<String>,
    /// 시작·리셋 자세.
    pub start: Motion,
    /// 경로 끝 목표 자세(표시용, 4-6). 지도 코스만 있고 개발 코스는 `None`이다.
    pub goal: Option<Motion>,
    /// 포장 노면 영역(시각 표시).
    pub surfaces: Vec<GroundRect>,
    /// 굽은 길 노면·표시선 띠(시각 표시). 지도 코스에서 쓴다.
    pub strips: Vec<Strip>,
    /// 교차로 면 노면(시각 표시).
    pub areas: Vec<Area>,
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
        name: CourseId::Training.name().to_owned(),
        notices: Vec::new(),
        start: Motion::default(),
        goal: None,
        surfaces: vec![rect_between((-half, half), (rear, front))],
        strips: Vec::new(),
        areas: Vec::new(),
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
        name: CourseId::Calibration.name().to_owned(),
        notices: Vec::new(),
        start: Motion::default(),
        goal: None,
        surfaces: Vec::new(),
        strips: Vec::new(),
        areas: Vec::new(),
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

/// 코스 형상이 표시 계약(Godot 인터페이스 3절)을 어긴 이유.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeError {
    /// 좌표·크기·각도에 유한하지 않은 값이 있음.
    NonFinite { part: &'static str, index: usize },
    /// 직사각형 크기가 0 이하.
    EmptyRect { part: &'static str, index: usize },
    /// 띠의 점이 2개 미만이거나 양쪽 점 개수가 다름.
    BadStrip {
        index: usize,
        left: usize,
        right: usize,
    },
    /// 교차로 면 경계점이 3개 미만.
    TooFewBoundaryPoints { index: usize, count: usize },
    /// 중심에서 본 부채꼴 삼각형이 반시계가 아님(중심이 모든 경계점을 볼 수 없음).
    FanNotVisible { index: usize, edge: usize },
}

impl std::fmt::Display for ShapeError {
    /// 사람이 읽는 한국어 오류 문구를 쓴다.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite { part, index } => {
                write!(
                    f,
                    "코스 형상 오류: {part}[{index}]에 유한하지 않은 값이 있습니다"
                )
            }
            Self::EmptyRect { part, index } => {
                write!(
                    f,
                    "코스 형상 오류: {part}[{index}] 직사각형 크기가 0 이하입니다"
                )
            }
            Self::BadStrip { index, left, right } => write!(
                f,
                "코스 형상 오류: strips[{index}] 점 개수가 맞지 않습니다(왼쪽 {left}, 오른쪽 {right}, 각각 2개 이상이고 같아야 함)"
            ),
            Self::TooFewBoundaryPoints { index, count } => write!(
                f,
                "코스 형상 오류: areas[{index}] 경계점이 {count}개입니다(3개 이상 필요)"
            ),
            Self::FanNotVisible { index, edge } => write!(
                f,
                "코스 형상 오류: areas[{index}] 중심에서 경계 {edge}번 변이 반시계로 보이지 않습니다"
            ),
        }
    }
}

impl std::error::Error for ShapeError {}

/// 점의 두 좌표가 모두 유한한지 확인한다.
fn finite_point(point: Vec2) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

/// 직사각형의 중심·크기·각도가 유한하고 크기가 양수인지 확인한다.
fn check_rect(rect: &GroundRect, part: &'static str, index: usize) -> Result<(), ShapeError> {
    if !finite_point(rect.center) || !finite_point(rect.half_size) || !rect.heading_rad.is_finite()
    {
        return Err(ShapeError::NonFinite { part, index });
    }
    if rect.half_size.x <= 0.0 || rect.half_size.y <= 0.0 {
        return Err(ShapeError::EmptyRect { part, index });
    }
    Ok(())
}

impl CourseDef {
    /// 형상이 표시 계약을 지키는지 검사한다. 어기면 첫 번째 위반을 돌려준다.
    ///
    /// 교차로 면은 경계점 3개 이상, 중심에서 각 변 (b[i], b[i+1])을 잇는 삼각형이 (x, z) 평면에서
    /// 반시계(외적 > 0)여야 한다. 그래야 부채꼴 삼각형이 겹치거나 뒤집히지 않는다. 닫는 점은 중복하지 않는다고 본다.
    pub fn validate(&self) -> Result<(), ShapeError> {
        if !finite_point(Vec2::new(self.start.x_m, self.start.z_m))
            || !self.start.heading_rad.is_finite()
        {
            return Err(ShapeError::NonFinite {
                part: "start",
                index: 0,
            });
        }
        for (index, rect) in self.surfaces.iter().enumerate() {
            check_rect(rect, "surfaces", index)?;
        }
        for (index, marking) in self.markings.iter().enumerate() {
            check_rect(&marking.rect, "markings", index)?;
        }
        for (index, collider) in self.colliders.iter().enumerate() {
            check_rect(&collider.rect, "colliders", index)?;
            if !collider.height_m.is_finite() {
                return Err(ShapeError::NonFinite {
                    part: "colliders",
                    index,
                });
            }
        }
        for (index, strip) in self.strips.iter().enumerate() {
            let (left, right) = (strip.left.len(), strip.right.len());
            if left < 2 || left != right {
                return Err(ShapeError::BadStrip { index, left, right });
            }
            if !strip
                .left
                .iter()
                .chain(&strip.right)
                .all(|p| finite_point(*p))
            {
                return Err(ShapeError::NonFinite {
                    part: "strips",
                    index,
                });
            }
        }
        for (index, area) in self.areas.iter().enumerate() {
            let count = area.boundary.len();
            if count < 3 {
                return Err(ShapeError::TooFewBoundaryPoints { index, count });
            }
            if !finite_point(area.center) || !area.boundary.iter().all(|p| finite_point(*p)) {
                return Err(ShapeError::NonFinite {
                    part: "areas",
                    index,
                });
            }
            for edge in 0..count {
                let a = area.boundary[edge] - area.center;
                let b = area.boundary[(edge + 1) % count] - area.center;
                if a.perp_dot(b) <= 0.0 {
                    return Err(ShapeError::FanNotVisible { index, edge });
                }
            }
        }
        Ok(())
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

    /// 모든 코스의 시작 자세 차체가 어떤 충돌 경계와도 닿지 않고, 개발 코스에는 경로 끝 목표가 없는지 확인한다.
    #[test]
    fn start_pose_is_clear_on_every_course() {
        for id in [CourseId::Training, CourseId::Calibration] {
            let course = id.build();
            assert!(course.goal.is_none(), "{id:?}");
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

    /// 원점을 중심으로 한 정사각형 교차로 면을 만든다. `ccw`가 거짓이면 경계 순서를 뒤집는다.
    fn square_area(center: Vec2, ccw: bool) -> Area {
        let mut boundary = vec![
            Vec2::new(1.0, -1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
            Vec2::new(-1.0, -1.0),
        ];
        if !ccw {
            boundary.reverse();
        }
        Area { center, boundary }
    }

    /// 개발 코스 두 개가 형상 계약 검사를 통과하는지 확인한다.
    #[test]
    fn development_courses_pass_shape_validation() {
        for id in [CourseId::Training, CourseId::Calibration] {
            assert_eq!(id.build().validate(), Ok(()), "{}", id.name());
        }
    }

    /// atan2(z, x)가 커지는 순서의 교차로 면만 받고, 반대 순서나 바깥 중심은 거부하는지 확인한다.
    #[test]
    fn junction_area_must_be_ccw_fan_from_center() {
        let mut course = training_course();
        course.areas = vec![square_area(Vec2::ZERO, true)];
        assert_eq!(course.validate(), Ok(()));
        course.areas = vec![square_area(Vec2::ZERO, false)];
        assert!(matches!(
            course.validate(),
            Err(ShapeError::FanNotVisible { index: 0, .. })
        ));
        course.areas = vec![square_area(Vec2::new(3.0, 0.0), true)];
        assert!(matches!(
            course.validate(),
            Err(ShapeError::FanNotVisible { index: 0, .. })
        ));
        course.areas = vec![Area {
            center: Vec2::ZERO,
            boundary: vec![Vec2::X, Vec2::Y],
        }];
        assert_eq!(
            course.validate(),
            Err(ShapeError::TooFewBoundaryPoints { index: 0, count: 2 })
        );
    }

    /// 띠의 양쪽 점 개수가 다르거나 2개 미만이면 거부하는지 확인한다.
    #[test]
    fn strip_needs_matching_point_counts() {
        let mut course = training_course();
        course.strips = vec![Strip {
            kind: StripKind::Road,
            left: vec![Vec2::ZERO, Vec2::Y],
            right: vec![Vec2::X],
        }];
        assert_eq!(
            course.validate(),
            Err(ShapeError::BadStrip {
                index: 0,
                left: 2,
                right: 1
            })
        );
    }

    /// 충돌 경계에 NaN이 있으면 거부하고, 오류 문구가 한국어 사유를 담는지 확인한다.
    #[test]
    fn non_finite_collider_is_rejected() {
        let mut course = training_course();
        course.colliders[0].rect.center.x = f32::NAN;
        let error = course.validate().unwrap_err();
        assert_eq!(
            error,
            ShapeError::NonFinite {
                part: "colliders",
                index: 0
            }
        );
        assert!(error.to_string().contains("colliders[0]"));
    }
}
