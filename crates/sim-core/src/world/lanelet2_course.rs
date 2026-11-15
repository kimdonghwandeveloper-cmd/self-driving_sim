//! Lanelet2 geometry의 연결 그래프에서 경로를 골라 주행 가능한 [`CourseDef`]로 조립한다.
//!
//! 이 단계는 파일 입출력과 Godot 연결을 하지 않는다. 선택 경로의 실제 좌우 경계를 노면과 연석에
//! 사용하고, 연결 중심선을 시작·목표 자세 및 안내선으로 제공한다.

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::course::{Collider, ColliderKind, CourseDef, Strip};
use super::lanelet2::LaneletDirection;
use super::lanelet2_geometry::{LaneletGeometry, LaneletGeometryMap};
use super::{map, road_geom, route};
use crate::vehicle::GroundRect;

/// Lanelet2 코스 설정 파일의 형식 이름.
pub const LANELET2_COURSE_FORMAT: &str = "lanelet2-course-v1";
/// 하위 호환용 Lanelet2 코스 형식 이름.
pub const LANELET2_COURSE_FORMAT_COMPAT: &str = "lanelet2-v1";

/// Lanelet2 코스 설정 파일(TOML).
///
/// Lanelet2 .osm 지도 파일, 좌표 원점 및 시작·목표 lanelet relation ID와 출처/이용조건/안내 문구를 정의한다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lanelet2CourseFile {
    /// 형식 이름([`LANELET2_COURSE_FORMAT`] 또는 [`LANELET2_COURSE_FORMAT_COMPAT`]).
    pub format: String,
    /// 로그 및 표시에 사용하는 코스 이름.
    pub name: String,
    /// Lanelet2 .osm XML 파일 상대 경로 (설정 파일 위치 기준).
    #[serde(alias = "source")]
    pub osm: String,
    /// 지역 좌표 원점 [위도, 경도](deg).
    pub origin: [f64; 2],
    /// 시작 lanelet relation ID.
    #[serde(alias = "start")]
    pub start_lanelet: i64,
    /// 목표 lanelet relation ID.
    #[serde(alias = "goal")]
    pub goal_lanelet: i64,
    /// 지도 출처 표기 (선택 사항).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
    /// 이용 조건 안내 (선택 사항).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terms: Option<String>,
    /// 주행 화면에 표시할 추가 안내 문구 목록.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<String>,
}

impl Lanelet2CourseFile {
    /// 설정 파일 필드의 유효성(형식 이름, 코스 이름, 파일 경로, 원점 위경도, 시작/목표 ID)을 검사한다.
    pub fn validate(&self) -> Result<(), String> {
        if self.format != LANELET2_COURSE_FORMAT && self.format != LANELET2_COURSE_FORMAT_COMPAT {
            return Err(format!(
                "unsupported format `{}`: expected `{LANELET2_COURSE_FORMAT}`",
                self.format
            ));
        }
        if self.name.trim().is_empty() {
            return Err("course name cannot be empty".to_owned());
        }
        if self.osm.trim().is_empty() {
            return Err("osm file path cannot be empty".to_owned());
        }
        let [lat, lon] = self.origin;
        if !(lat.is_finite()
            && lon.is_finite()
            && (-85.0..=85.0).contains(&lat)
            && (-180.0..=180.0).contains(&lon))
        {
            return Err(format!(
                "invalid origin [{lat}, {lon}]: latitude must be in [-85, 85] and longitude in [-180, 180]"
            ));
        }
        if self.start_lanelet <= 0 {
            return Err(format!(
                "start_lanelet ID must be a positive integer, got {}",
                self.start_lanelet
            ));
        }
        if self.goal_lanelet <= 0 {
            return Err(format!(
                "goal_lanelet ID must be a positive integer, got {}",
                self.goal_lanelet
            ));
        }
        Ok(())
    }
}

/// Lanelet2 코스 설정 파일 구조체와 기준 디렉터리 경로를 받아 OSM 파일을 파싱·기하 변환하여 코스를 생성한다.
pub fn build_lanelet2_course_from_file(
    file: &Lanelet2CourseFile,
    base_dir: &Path,
) -> Result<LaneletCourse, String> {
    file.validate()?;
    let osm_path = base_dir.join(&file.osm);
    let bytes = std::fs::read(&osm_path)
        .map_err(|error| format!("cannot read osm file {}: {error}", osm_path.display()))?;
    let map = super::lanelet2::parse_lanelet2(&bytes).map_err(|error| {
        format!(
            "failed to parse Lanelet2 OSM ({}): {error}",
            osm_path.display()
        )
    })?;
    let origin = map::LatLon {
        lat: file.origin[0],
        lon: file.origin[1],
    };
    let geometry = super::lanelet2_geometry::build_lanelet_geometry(&map, origin)
        .map_err(|error| format!("failed to build Lanelet2 geometry: {error}"))?;

    let mut notices = Vec::new();
    if let Some(attr) = file.attribution.as_deref().filter(|s| !s.trim().is_empty()) {
        notices.push(attr.to_owned());
    }
    if let Some(terms) = file.terms.as_deref().filter(|s| !s.trim().is_empty()) {
        notices.push(terms.to_owned());
    }
    for notice in &file.notices {
        if !notice.trim().is_empty() {
            notices.push(notice.clone());
        }
    }

    build_lanelet_course(
        &geometry,
        &file.name,
        file.start_lanelet,
        file.goal_lanelet,
        notices,
    )
}

/// Lanelet2 코스 설정 파일 TOML 문자열과 기준 디렉터리를 받아 코스 정의(`CourseDef`)를 반환한다(기존 map 로더 연동용).
pub fn load_lanelet2_course(text: &str, base_dir: &Path) -> Result<CourseDef, String> {
    load_lanelet2_course_full(text, base_dir).map(|lanelet_course| lanelet_course.course)
}

/// Lanelet2 코스 설정 파일 TOML 문자열과 기준 디렉터리를 받아 코스 정의와 안내선(`LaneletCourse`)을 반환한다.
pub fn load_lanelet2_course_full(text: &str, base_dir: &Path) -> Result<LaneletCourse, String> {
    let file: Lanelet2CourseFile =
        toml::from_str(text).map_err(|error| format!("invalid Lanelet2 course TOML: {error}"))?;
    build_lanelet2_course_from_file(&file, base_dir)
}

/// 지정한 경로의 Lanelet2 코스 설정 파일(TOML)을 읽어 코스(`LaneletCourse`)를 생성한다.
pub fn load_lanelet2_course_file(path: &Path) -> Result<LaneletCourse, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let base_dir = path.parent().unwrap_or(Path::new("."));
    load_lanelet2_course_full(&text, base_dir)
}

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

    /// 파일 및 설정으로부터 직선 Lanelet2 코스를 로드하고 검증 및 주행 시험을 수행한다.
    #[test]
    fn loads_straight_course_from_file_and_osm() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let toml_text = r#"
format = "lanelet2-course-v1"
name = "synthetic-straight-course"
osm = "testdata/lanelet2/synthetic-straight.osm"
origin = [34.0, 127.0]
start_lanelet = 301
goal_lanelet = 301
attribution = "Synthetic Lanelet2 test dataset"
terms = "For simulation and testing only"
notices = ["Drive within lane limits"]
"#;
        let loaded = load_lanelet2_course_full(toml_text, root).unwrap();
        assert_eq!(loaded.course.name, "synthetic-straight-course");
        assert_eq!(loaded.traversal.len(), 1);
        assert_eq!(loaded.traversal[0].id, 301);
        assert!(!loaded.course.strips.is_empty());
        assert!(loaded.course.goal.is_some());
        assert!(
            loaded
                .course
                .notices
                .iter()
                .any(|n| n.contains("Synthetic Lanelet2"))
        );
        assert!(
            loaded
                .course
                .notices
                .iter()
                .any(|n| n.contains("Drive within lane limits"))
        );
        loaded.course.validate().unwrap();

        // map 로더 연동 함수를 통한 CourseDef 로드 검증
        let course_def = load_lanelet2_course(toml_text, root).unwrap();
        assert_eq!(course_def.name, "synthetic-straight-course");
        course_def.validate().unwrap();

        let driven = super::super::test_drive::follow_path(&loaded.course, &loaded.guide, 3.0);
        assert_eq!(driven.impacts, 0, "{driven:?}");
    }

    /// 곡선 Lanelet2 OSM 데이터로부터 코스를 로드하고 유효성을 검증한다.
    #[test]
    fn loads_curved_course_from_osm() {
        let temp_dir =
            std::env::temp_dir().join(format!("sds-curved-lanelet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 90도 곡선 2개 연결 OSM 생성
        let curved_osm = r#"<?xml version="1.0" encoding="UTF-8"?>
<osm version="0.6" generator="test">
  <node id="1" lat="34.0000000" lon="127.0000000"/>
  <node id="2" lat="34.0001000" lon="127.0000200"/>
  <node id="3" lat="34.0002000" lon="127.0000600"/>
  <node id="4" lat="34.0000000" lon="127.0000400"/>
  <node id="5" lat="34.0001000" lon="127.0000600"/>
  <node id="6" lat="34.0002000" lon="127.0001000"/>
  <node id="7" lat="34.0000000" lon="127.0000200"/>
  <node id="8" lat="34.0001000" lon="127.0000400"/>
  <node id="9" lat="34.0002000" lon="127.0000800"/>
  <way id="201">
    <nd ref="1"/>
    <nd ref="2"/>
    <nd ref="3"/>
  </way>
  <way id="202">
    <nd ref="4"/>
    <nd ref="5"/>
    <nd ref="6"/>
  </way>
  <way id="203">
    <nd ref="7"/>
    <nd ref="8"/>
    <nd ref="9"/>
  </way>
  <relation id="501">
    <member type="way" ref="201" role="left"/>
    <member type="way" ref="202" role="right"/>
    <member type="way" ref="203" role="centerline"/>
    <tag k="type" v="lanelet"/>
    <tag k="subtype" v="road"/>
    <tag k="one_way" v="yes"/>
  </relation>
</osm>
"#;
        let osm_path = temp_dir.join("curved.osm");
        std::fs::write(&osm_path, curved_osm).unwrap();

        let toml_content = r#"
format = "lanelet2-course-v1"
name = "curved-sample"
osm = "curved.osm"
origin = [34.0, 127.0]
start_lanelet = 501
goal_lanelet = 501
"#;
        let toml_path = temp_dir.join("course.toml");
        std::fs::write(&toml_path, toml_content).unwrap();

        let loaded = load_lanelet2_course_file(&toml_path).unwrap();
        assert_eq!(loaded.course.name, "curved-sample");
        loaded.course.validate().unwrap();

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    /// 동일한 설정 파일을 다시 열었을 때 완벽히 동일한 코스가 생성되는지 결정성을 검증한다.
    #[test]
    fn deterministic_reloading_produces_identical_course() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let toml_text = r#"
format = "lanelet2-course-v1"
name = "deterministic-test"
osm = "testdata/lanelet2/synthetic-straight.osm"
origin = [34.0, 127.0]
start_lanelet = 301
goal_lanelet = 301
"#;
        let first = load_lanelet2_course_full(toml_text, root).unwrap();
        let second = load_lanelet2_course_full(toml_text, root).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.course, second.course);
        assert_eq!(first.guide, second.guide);
        assert_eq!(first.traversal, second.traversal);
    }

    /// 잘못된 입력(누락된 파일, 비정상 위경도, 잘못된 ID, 지원되지 않는 버전 등)을 명확한 원인 오류로 거부한다.
    #[test]
    fn rejects_invalid_inputs_with_informative_errors() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));

        // 1. 존재하지 않는 설정 파일 경로
        let missing_path = root.join("assets/maps/non-existent-course.toml");
        assert!(load_lanelet2_course_file(&missing_path).is_err());

        // 2. 존재하지 않는 OSM 파일 참조
        let missing_osm_toml = r#"
format = "lanelet2-course-v1"
name = "missing-osm"
osm = "testdata/lanelet2/non-existent.osm"
origin = [34.0, 127.0]
start_lanelet = 301
goal_lanelet = 301
"#;
        let err = load_lanelet2_course_full(missing_osm_toml, root).unwrap_err();
        assert!(err.contains("cannot read osm file"), "{err}");

        // 3. 잘못된 TOML 구문
        let bad_toml = "invalid toml {[[";
        assert!(load_lanelet2_course_full(bad_toml, root).is_err());

        // 4. 지원하지 않는 형식 버전
        let bad_format = r#"
format = "lanelet2-v999"
name = "bad-format"
osm = "testdata/lanelet2/synthetic-straight.osm"
origin = [34.0, 127.0]
start_lanelet = 301
goal_lanelet = 301
"#;
        let err = load_lanelet2_course_full(bad_format, root).unwrap_err();
        assert!(err.contains("unsupported format"), "{err}");

        // 5. 비정상적인 원점 위도/경도
        let bad_lat = r#"
format = "lanelet2-course-v1"
name = "bad-lat"
osm = "testdata/lanelet2/synthetic-straight.osm"
origin = [90.5, 127.0]
start_lanelet = 301
goal_lanelet = 301
"#;
        let err = load_lanelet2_course_full(bad_lat, root).unwrap_err();
        assert!(err.contains("invalid origin"), "{err}");

        // 6. 음수 또는 0인 lanelet ID
        let bad_id = r#"
format = "lanelet2-course-v1"
name = "bad-id"
osm = "testdata/lanelet2/synthetic-straight.osm"
origin = [34.0, 127.0]
start_lanelet = 0
goal_lanelet = 301
"#;
        let err = load_lanelet2_course_full(bad_id, root).unwrap_err();
        assert!(err.contains("start_lanelet ID"), "{err}");

        // 7. OSM 내에 존재하지 않는 lanelet ID 참조
        let not_found_id = r#"
format = "lanelet2-course-v1"
name = "not-found-id"
osm = "testdata/lanelet2/synthetic-straight.osm"
origin = [34.0, 127.0]
start_lanelet = 9999
goal_lanelet = 9999
"#;
        let err = load_lanelet2_course_full(not_found_id, root).unwrap_err();
        assert!(err.contains("9999 not found"), "{err}");
    }

    /// map 모듈의 `load_map_course`에서 새 Lanelet2 형식과 기존 형식들이 정상 분기 처리되는지 검증한다.
    #[test]
    fn map_course_loader_dispatches_lanelet2_and_existing_formats() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let temp_dir =
            std::env::temp_dir().join(format!("sds-map-dispatch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 테스트용 OSM 파일을 임시 폴더로 복사
        let osm_src = root.join("testdata/lanelet2/synthetic-straight.osm");
        let osm_dest = temp_dir.join("straight.osm");
        std::fs::copy(&osm_src, &osm_dest).unwrap();

        // 1. Lanelet2 형식 파일
        let lanelet2_toml = r#"
format = "lanelet2-course-v1"
name = "dispatch-lanelet2"
osm = "straight.osm"
origin = [34.0, 127.0]
start_lanelet = 301
goal_lanelet = 301
"#;
        let lanelet2_path = temp_dir.join("course.toml");
        std::fs::write(&lanelet2_path, lanelet2_toml).unwrap();
        let course = super::super::map::load_map_course(&lanelet2_path).unwrap();
        assert_eq!(course.name, "dispatch-lanelet2");
        course.validate().unwrap();

        // 2. 하위 호환 lanelet2-v1 형식
        let compat_toml = r#"
format = "lanelet2-v1"
name = "compat-lanelet2"
osm = "straight.osm"
origin = [34.0, 127.0]
start_lanelet = 301
goal_lanelet = 301
"#;
        let compat_path = temp_dir.join("compat.toml");
        std::fs::write(&compat_path, compat_toml).unwrap();
        let compat_course = super::super::map::load_map_course(&compat_path).unwrap();
        assert_eq!(compat_course.name, "compat-lanelet2");
        compat_course.validate().unwrap();

        // 3. 기존 testdata 코스(route-junctions.toml) 회귀 검증
        let existing_path = root.join("testdata/yeosu-expo/route-junctions.toml");
        let existing_course = super::super::map::load_map_course(&existing_path).unwrap();
        existing_course.validate().unwrap();

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
