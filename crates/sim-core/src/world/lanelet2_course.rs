//! Lanelet2 geometry의 연결 그래프에서 경로를 골라 주행 가능한 [`CourseDef`]로 조립한다.
//!
//! 이 단계는 파일 입출력과 Godot 연결을 하지 않는다. 선택 경로의 실제 좌우 경계를 노면과 연석에
//! 사용하고, 연결 중심선을 시작·목표 자세 및 안내선으로 제공한다.

use std::collections::{BTreeMap, VecDeque};

use glam::Vec2;

use super::course::{Collider, ColliderKind, CourseDef, Strip};
use super::lanelet2::LaneletDirection;
use super::lanelet2_geometry::{LaneletGeometry, LaneletGeometryMap};
use super::{map, road_geom, route};
use crate::vehicle::GroundRect;

/// 지도 코스와 같이 사용하는 연석 두께(m).
const CURB_THICKNESS_M: f32 = 0.3;
/// 연석 표시 높이(m).
const CURB_HEIGHT_M: f32 = 0.25;
/// 경로 양 끝을 막는 벽 두께와 높이(m).
const END_WALL_THICKNESS_M: f32 = 0.5;
const END_WALL_HEIGHT_M: f32 = 1.2;
/// 이음부 연석 틈을 막기 위해 각 선분 양 끝에 더하는 길이(m).
const CURB_OVERLAP_M: f32 = 0.02;
/// 연결된 lanelet 중심선 끝점이 떨어질 수 있는 최대 거리(m).
const MAX_GUIDE_JOIN_M: f32 = 7.0;
/// 같은 안내선 점으로 합칠 거리(m).
const GUIDE_DEDUP_M: f32 = 0.05;

/// 경로에서 lanelet 하나를 어느 방향으로 통과하는지 나타낸다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LaneletTraversal {
    /// Lanelet2 relation ID.
    pub id: i64,
    /// `true`면 relation 경계·중심선의 반대 방향으로 통과한다.
    pub reversed: bool,
}

/// Lanelet2 geometry에서 조립한 코스와 경로 부가 정보.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneletCourse {
    /// 렌더링과 충돌에 사용하는 코스 정의.
    pub course: CourseDef,
    /// 자동 주행·목표 표시에 사용할 연결 중심선.
    pub guide: Vec<Vec2>,
    /// 시작부터 목표까지의 lanelet 통과 순서와 방향.
    pub traversal: Vec<LaneletTraversal>,
}

/// 시작·목표 lanelet 사이의 유일한 최단 경로를 찾아 `CourseDef`로 조립한다.
pub fn build_lanelet_course(
    geometry: &LaneletGeometryMap,
    name: &str,
    start_lanelet: i64,
    goal_lanelet: i64,
    mut notices: Vec<String>,
) -> Result<LaneletCourse, String> {
    validate_connections(geometry)?;
    let traversal = find_unique_shortest_path(geometry, start_lanelet, goal_lanelet)?;
    let guide = build_guide(geometry, &traversal)?;
    let start = map::start_pose(&guide)?;
    let goal = map::goal_pose(&guide)?;
    let mut strips = Vec::with_capacity(traversal.len());
    let mut colliders = Vec::new();
    for step in &traversal {
        let lanelet = &geometry.lanelets[&step.id];
        strips.push(oriented_surface(lanelet, step.reversed));
        colliders.extend(boundary_curbs(lanelet));
    }
    colliders.push(end_wall(geometry, traversal[0], true)?);
    colliders.push(end_wall(
        geometry,
        *traversal.last().expect("path contains a goal"),
        false,
    )?);
    notices.extend(geometry.warnings.iter().cloned());
    let mut course = map::finish_course(
        name,
        notices,
        start,
        strips,
        Vec::new(),
        colliders,
        route::ROUTE_MAX_EXTENT_M,
    )?;
    course.goal = Some(goal);
    if crate::vehicle::collision::in_contact(
        &crate::vehicle::VEHICLE_FOOTPRINT,
        &goal,
        &course.collision_rects(),
    ) {
        return Err("Lanelet2 goal overlaps a collider".to_owned());
    }
    course
        .validate()
        .map_err(|error| format!("invalid Lanelet2 course: {error}"))?;
    Ok(LaneletCourse {
        course,
        guide,
        traversal,
    })
}

/// 모든 연결 ID가 존재하고 predecessor/successor가 서로 대칭인지 검사한다.
fn validate_connections(geometry: &LaneletGeometryMap) -> Result<(), String> {
    for (&id, lanelet) in &geometry.lanelets {
        for successor in &lanelet.successors {
            let target = geometry
                .lanelets
                .get(successor)
                .ok_or_else(|| format!("lanelet {id} references missing successor {successor}"))?;
            if !target.predecessors.contains(&id) {
                return Err(format!(
                    "lanelet {id} successor {successor} is missing the matching predecessor"
                ));
            }
        }
        for predecessor in &lanelet.predecessors {
            let source = geometry.lanelets.get(predecessor).ok_or_else(|| {
                format!("lanelet {id} references missing predecessor {predecessor}")
            })?;
            if !source.successors.contains(&id) {
                return Err(format!(
                    "lanelet {id} predecessor {predecessor} is missing the matching successor"
                ));
            }
        }
    }
    Ok(())
}

/// traversal 상태에서 통행 규칙상 이어 갈 수 있는 다음 상태를 ID 순으로 만든다.
fn neighbors(geometry: &LaneletGeometryMap, state: LaneletTraversal) -> Vec<LaneletTraversal> {
    let lanelet = &geometry.lanelets[&state.id];
    if state.reversed {
        lanelet
            .predecessors
            .iter()
            .filter(|id| geometry.lanelets[id].direction == LaneletDirection::Bidirectional)
            .map(|id| LaneletTraversal {
                id: *id,
                reversed: true,
            })
            .collect()
    } else {
        lanelet
            .successors
            .iter()
            .map(|id| LaneletTraversal {
                id: *id,
                reversed: false,
            })
            .collect()
    }
}

/// 시작·목표 ID 사이에서 유일한 최단 traversal을 찾고 동률 경로는 오류로 돌려준다.
fn find_unique_shortest_path(
    geometry: &LaneletGeometryMap,
    start_id: i64,
    goal_id: i64,
) -> Result<Vec<LaneletTraversal>, String> {
    let start = geometry
        .lanelets
        .get(&start_id)
        .ok_or_else(|| format!("start lanelet {start_id} not found"))?;
    if !geometry.lanelets.contains_key(&goal_id) {
        return Err(format!("goal lanelet {goal_id} not found"));
    }
    let mut queue = VecDeque::new();
    let mut distance: BTreeMap<LaneletTraversal, usize> = BTreeMap::new();
    let mut path_count: BTreeMap<LaneletTraversal, u8> = BTreeMap::new();
    let mut previous: BTreeMap<LaneletTraversal, LaneletTraversal> = BTreeMap::new();
    let forward = LaneletTraversal {
        id: start_id,
        reversed: false,
    };
    queue.push_back(forward);
    distance.insert(forward, 0);
    path_count.insert(forward, 1);
    if start.direction == LaneletDirection::Bidirectional && start_id != goal_id {
        let reverse = LaneletTraversal {
            id: start_id,
            reversed: true,
        };
        queue.push_back(reverse);
        distance.insert(reverse, 0);
        path_count.insert(reverse, 1);
    }
    while let Some(state) = queue.pop_front() {
        let next_distance = distance[&state] + 1;
        for next in neighbors(geometry, state) {
            match distance.get(&next).copied() {
                None => {
                    distance.insert(next, next_distance);
                    path_count.insert(next, path_count[&state]);
                    previous.insert(next, state);
                    queue.push_back(next);
                }
                Some(known) if known == next_distance => {
                    let count = path_count[&next].saturating_add(path_count[&state]).min(2);
                    path_count.insert(next, count);
                }
                Some(_) => {}
            }
        }
    }
    let mut goals: Vec<LaneletTraversal> = distance
        .keys()
        .copied()
        .filter(|state| state.id == goal_id)
        .collect();
    if goals.is_empty() {
        return Err(format!(
            "no directed Lanelet2 path from {start_id} to {goal_id}"
        ));
    }
    let shortest = goals
        .iter()
        .map(|state| distance[state])
        .min()
        .expect("goals is not empty");
    goals.retain(|state| distance[state] == shortest);
    let total_paths: u8 = goals
        .iter()
        .map(|state| path_count[state])
        .fold(0u8, |total, count| total.saturating_add(count).min(2));
    if total_paths != 1 {
        return Err(format!(
            "multiple shortest Lanelet2 paths from {start_id} to {goal_id}"
        ));
    }
    let mut state = goals[0];
    let mut path = vec![state];
    while let Some(parent) = previous.get(&state).copied() {
        path.push(parent);
        state = parent;
    }
    path.reverse();
    Ok(path)
}

/// traversal 순서와 방향에 맞게 중심선을 연결하고 큰 이음 간격을 거부한다.
fn build_guide(
    geometry: &LaneletGeometryMap,
    traversal: &[LaneletTraversal],
) -> Result<Vec<Vec2>, String> {
    let mut guide: Vec<Vec2> = Vec::new();
    for step in traversal {
        let lanelet = &geometry.lanelets[&step.id];
        let points: Box<dyn Iterator<Item = Vec2>> = if step.reversed {
            Box::new(lanelet.centerline.iter().rev().copied())
        } else {
            Box::new(lanelet.centerline.iter().copied())
        };
        for (index, point) in points.enumerate() {
            if let Some(previous) = guide.last().copied() {
                let gap = previous.distance(point);
                if index == 0 && gap > MAX_GUIDE_JOIN_M {
                    return Err(format!(
                        "lanelet {} guide starts {gap:.2} m from the previous lanelet",
                        step.id
                    ));
                }
                if gap < GUIDE_DEDUP_M {
                    continue;
                }
            }
            guide.push(point);
        }
    }
    if guide.len() < 2 {
        return Err("Lanelet2 route guide has fewer than 2 points".to_owned());
    }
    Ok(guide)
}

/// traversal 방향에 맞게 좌우를 바꿔 도로 면을 반환한다.
fn oriented_surface(lanelet: &LaneletGeometry, reversed: bool) -> Strip {
    if reversed {
        Strip {
            kind: lanelet.surface.kind,
            left: lanelet.surface.right.iter().rev().copied().collect(),
            right: lanelet.surface.left.iter().rev().copied().collect(),
        }
    } else {
        lanelet.surface.clone()
    }
}

/// lanelet의 실제 좌우 경계를 따라 양쪽 연석 직사각형을 만든다.
fn boundary_curbs(lanelet: &LaneletGeometry) -> Vec<Collider> {
    let mut colliders = curb_side(&lanelet.surface.left, -1.0);
    colliders.extend(curb_side(&lanelet.surface.right, 1.0));
    colliders
}

/// 경계선의 지정한 바깥쪽에 연석 직사각형을 배치한다.
fn curb_side(boundary: &[Vec2], outward_side: f32) -> Vec<Collider> {
    boundary
        .windows(2)
        .filter_map(|pair| {
            let delta = pair[1] - pair[0];
            let length = delta.length();
            if length <= 0.0 {
                return None;
            }
            let forward = delta / length;
            Some(Collider {
                rect: GroundRect {
                    center: (pair[0] + pair[1]) * 0.5
                        + road_geom::right_of(forward) * outward_side * (CURB_THICKNESS_M * 0.5),
                    half_size: Vec2::new(CURB_THICKNESS_M * 0.5, length * 0.5 + CURB_OVERLAP_M),
                    heading_rad: road_geom::heading_of(forward),
                },
                height_m: CURB_HEIGHT_M,
                kind: ColliderKind::Curb,
            })
        })
        .collect()
}

/// 경로 시작 또는 끝의 횡단 경계 바깥쪽에 벽을 만든다.
fn end_wall(
    geometry: &LaneletGeometryMap,
    step: LaneletTraversal,
    at_start: bool,
) -> Result<Collider, String> {
    let lanelet = &geometry.lanelets[&step.id];
    let surface = oriented_surface(lanelet, step.reversed);
    let index = if at_start { 0 } else { surface.left.len() - 1 };
    let left = surface.left[index];
    let right = surface.right[index];
    let across = right - left;
    let width = across.length();
    if width <= 0.0 {
        return Err(format!("lanelet {} has a zero-width end", step.id));
    }
    let centerline = if step.reversed {
        lanelet.centerline.iter().rev().copied().collect::<Vec<_>>()
    } else {
        lanelet.centerline.clone()
    };
    let forward = if at_start {
        (centerline[1] - centerline[0]).normalize()
    } else {
        let last = centerline.len() - 1;
        (centerline[last] - centerline[last - 1]).normalize()
    };
    let outward = if at_start { -forward } else { forward };
    Ok(Collider {
        rect: GroundRect {
            center: (left + right) * 0.5 + outward * (END_WALL_THICKNESS_M * 0.5),
            half_size: Vec2::new(width * 0.5 + CURB_THICKNESS_M, END_WALL_THICKNESS_M * 0.5),
            heading_rad: road_geom::heading_of(forward),
        },
        height_m: END_WALL_HEIGHT_M,
        kind: ColliderKind::Wall,
    })
}

#[cfg(test)]
mod tests {
    use super::super::course::StripKind;
    use super::super::lanelet2_geometry::LaneletGeometryMap;
    use super::super::map::LatLon;
    use super::*;

    /// 직선 lanelet geometry 하나를 만든다.
    fn lanelet(id: i64, z_start: f32, z_end: f32, direction: LaneletDirection) -> LaneletGeometry {
        LaneletGeometry {
            id,
            surface: Strip {
                kind: StripKind::Road,
                left: vec![Vec2::new(-2.0, z_start), Vec2::new(-2.0, z_end)],
                right: vec![Vec2::new(2.0, z_start), Vec2::new(2.0, z_end)],
            },
            centerline: vec![Vec2::new(0.0, z_start), Vec2::new(0.0, z_end)],
            mean_width_m: 4.0,
            direction,
            predecessors: Vec::new(),
            successors: Vec::new(),
        }
    }

    /// lanelet 목록으로 원점 고정 geometry 지도를 만든다.
    fn geometry(lanelets: impl IntoIterator<Item = LaneletGeometry>) -> LaneletGeometryMap {
        LaneletGeometryMap {
            origin: LatLon {
                lat: 34.0,
                lon: 127.0,
            },
            lanelets: lanelets
                .into_iter()
                .map(|lanelet| (lanelet.id, lanelet))
                .collect(),
            warnings: vec!["synthetic Lanelet2 geometry".to_owned()],
        }
    }

    /// 연결 두 lanelet을 도로 면·연석·벽·시작·목표가 있는 코스로 만든다.
    #[test]
    fn builds_connected_course() {
        let mut first = lanelet(1, 0.0, -30.0, LaneletDirection::OneWay);
        first.successors = vec![2];
        let mut second = lanelet(2, -30.0, -60.0, LaneletDirection::OneWay);
        second.predecessors = vec![1];
        let built = build_lanelet_course(
            &geometry([first, second]),
            "lanelet-test",
            1,
            2,
            vec!["source notice".to_owned()],
        )
        .unwrap();
        assert_eq!(
            built.traversal,
            [
                LaneletTraversal {
                    id: 1,
                    reversed: false
                },
                LaneletTraversal {
                    id: 2,
                    reversed: false
                },
            ]
        );
        assert_eq!(built.course.strips.len(), 2);
        assert_eq!(
            built
                .course
                .colliders
                .iter()
                .filter(|c| c.kind == ColliderKind::Wall)
                .count(),
            2
        );
        assert!(built.course.goal.is_some());
        assert_eq!(built.course.notices.len(), 2);
        built.course.validate().unwrap();
        let driven = super::super::test_drive::follow_path(&built.course, &built.guide, 3.0);
        assert_eq!(driven.impacts, 0, "{driven:?}");
        assert!(driven.remaining_m < 12.0, "{driven:?}");
    }

    /// 동률인 두 최단 경로는 ID가 작다는 이유로 임의 선택하지 않는다.
    #[test]
    fn rejects_ambiguous_shortest_route() {
        let mut first = lanelet(1, 0.0, -20.0, LaneletDirection::OneWay);
        first.successors = vec![2, 3];
        let mut left = lanelet(2, -20.0, -40.0, LaneletDirection::OneWay);
        left.predecessors = vec![1];
        left.successors = vec![4];
        let mut right = lanelet(3, -20.0, -40.0, LaneletDirection::OneWay);
        right.predecessors = vec![1];
        right.successors = vec![4];
        let mut goal = lanelet(4, -40.0, -60.0, LaneletDirection::OneWay);
        goal.predecessors = vec![2, 3];
        let error = build_lanelet_course(
            &geometry([first, left, right, goal]),
            "ambiguous",
            1,
            4,
            Vec::new(),
        )
        .unwrap_err();
        assert!(error.contains("multiple shortest"));
    }

    /// 양방향 lanelet 연결은 목표에서 시작 쪽으로 역주행 경로를 만들 수 있다.
    #[test]
    fn supports_reverse_bidirectional_route() {
        let mut first = lanelet(1, 0.0, -30.0, LaneletDirection::Bidirectional);
        first.successors = vec![2];
        let mut second = lanelet(2, -30.0, -60.0, LaneletDirection::Bidirectional);
        second.predecessors = vec![1];
        let built =
            build_lanelet_course(&geometry([first, second]), "reverse", 2, 1, Vec::new()).unwrap();
        assert!(built.traversal.iter().all(|step| step.reversed));
        assert!(built.guide.first().unwrap().y < built.guide.last().unwrap().y);
    }

    /// 끊긴 경로와 비대칭 연결 자료를 명확한 오류로 거부한다.
    #[test]
    fn rejects_disconnected_and_inconsistent_graphs() {
        let disconnected = geometry([
            lanelet(1, 0.0, -30.0, LaneletDirection::OneWay),
            lanelet(2, -40.0, -70.0, LaneletDirection::OneWay),
        ]);
        assert!(
            build_lanelet_course(&disconnected, "none", 1, 2, Vec::new())
                .unwrap_err()
                .contains("no directed")
        );
        let mut bad = lanelet(1, 0.0, -30.0, LaneletDirection::OneWay);
        bad.successors = vec![2];
        let inconsistent = geometry([bad, lanelet(2, -30.0, -60.0, LaneletDirection::OneWay)]);
        assert!(
            build_lanelet_course(&inconsistent, "bad", 1, 2, Vec::new())
                .unwrap_err()
                .contains("matching predecessor")
        );
    }

    /// 같은 양방향 차선을 시작과 목표로 고르면 정방향을 사용한다.
    #[test]
    fn single_bidirectional_lanelet_uses_forward_direction() {
        let built = build_lanelet_course(
            &geometry([lanelet(1, 0.0, -30.0, LaneletDirection::Bidirectional)]),
            "single",
            1,
            1,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(
            built.traversal,
            vec![LaneletTraversal {
                id: 1,
                reversed: false
            }]
        );
    }

    /// 차선 내부의 긴 선분은 허용하고 서로 떨어진 차선 연결은 거부한다.
    #[test]
    fn rejects_gap_between_lanelets() {
        let mut first = lanelet(1, 0.0, -30.0, LaneletDirection::OneWay);
        first.successors = vec![2];
        let mut second = lanelet(2, -40.0, -70.0, LaneletDirection::OneWay);
        second.predecessors = vec![1];
        assert!(
            build_lanelet_course(&geometry([first, second]), "gap", 1, 2, Vec::new())
                .unwrap_err()
                .contains("guide starts")
        );
    }
}
