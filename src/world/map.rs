//! OSM 중심선 → 주행 코스 변환(Phase 3-4 파이프라인, 대상 통로와 무관한 범용 부분).
//!
//! 저장해 둔 Overpass JSON(`out geom`)과 코스 선택 파일(TOML)을 읽어 [`CourseDef`]를 만든다.
//! 실행 중 네트워크로 내려받지 않는다. 변환은 모두 메모리에서 끝낸 뒤 결과를 돌려주며,
//! 실패하면 오류만 반환해 일부만 만든 코스가 남지 않는다.
//!
//! 좌표 규칙: 동쪽 +X, 북쪽 -Z, 높이 y=0, 단위 m. 원점은 선택 파일의 기준 위경도다.
//! 투영은 기준 위도에서의 등장방형 근사이며 원점에서 수 km 안(코스 상한 500 × 300 m)에서만 쓴다.
//! 도로 폭은 OSM에 값이 없으므로 선택 파일의 **훈련용 임시 폭**을 쓴다(실제 폭 아님).

use std::path::Path;

use bevy::prelude::*;
use serde::Deserialize;

use super::course::{Collider, ColliderKind, CourseDef};
use crate::vehicle::{GroundRect, Motion, VEHICLE_FOOTPRINT};

/// 지구 평균 반지름(m).
const EARTH_RADIUS_M: f64 = 6_371_008.8;
/// 이 거리(m)보다 가까운 연속 점은 중복으로 보고 하나만 남긴다.
const MIN_POINT_SPACING_M: f32 = 0.05;
/// way 끝점끼리 이 거리(m) 안이면 이어진 것으로 본다.
const JOIN_TOLERANCE_M: f32 = 0.5;
/// 한 점에서 허용하는 최대 방향 변화(deg). 이보다 급하게 꺾이면 변환을 거부한다.
///
/// 구간마다 연석 직사각형을 두므로 바깥쪽 모서리에 쐐기 모양 틈이 생긴다. 틈 폭은 대략
/// `2 × (폭/2 + 연석 두께) × tan(꺾임/2)`이며, 20°·최대 폭 12 m에서 약 2.2 m로 차체 폭보다 넓을 수 있다.
/// 그래서 폭 상한과 함께 [`max_outer_gap_m`]로 차체 폭보다 좁은지 따로 검사한다.
pub const MAX_TURN_DEG: f32 = 20.0;
/// 임시 폭 허용 범위(m).
pub const WIDTH_RANGE_M: std::ops::RangeInclusive<f32> = 2.5..=12.0;
/// 연석 두께(m)와 표시 높이(m).
const CURB_THICKNESS_M: f32 = 0.3;
const CURB_HEIGHT_M: f32 = 0.25;
/// 끝 벽 두께(m)와 표시 높이(m).
const END_WALL_THICKNESS_M: f32 = 0.5;
const END_WALL_HEIGHT_M: f32 = 1.2;
/// 시작 자세에서 차체 뒤끝과 시작 쪽 끝 벽 사이 거리(m).
const START_REAR_CLEARANCE_M: f32 = 5.0;
/// 노면 직사각형을 구간 양 끝에서 더 늘리는 길이(m). 꺾이는 곳의 노면 틈을 덮는다.
const SURFACE_OVERLAP_M: f32 = 0.5;
/// 지도 코스에 늘 표시하는 출처 표기.
pub const OSM_ATTRIBUTION: &str =
    "Map data (c) OpenStreetMap contributors, ODbL - openstreetmap.org/copyright";

/// 위경도(deg).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

/// 기준점 `origin`에서 본 지역 평면 좌표(x=동쪽 m, z=남쪽 m)로 바꾼다.
///
/// 기준 위도에서의 등장방형 근사: x = R·Δλ·cos φ0, 북쪽 = R·Δφ, z = -북쪽.
pub fn project(origin: LatLon, point: LatLon) -> Vec2 {
    let cos_lat0 = origin.lat.to_radians().cos();
    let east = EARTH_RADIUS_M * (point.lon - origin.lon).to_radians() * cos_lat0;
    let north = EARTH_RADIUS_M * (point.lat - origin.lat).to_radians();
    Vec2::new(east as f32, -north as f32)
}

/// way ID와 노드 좌표 목록. 응답에 없는 노드는 `None`이다.
pub type OsmWay = (u64, Vec<Option<LatLon>>);

/// Overpass JSON 응답 중 필요한 부분.
#[derive(Debug, Deserialize)]
struct OverpassResponse {
    elements: Vec<OverpassElement>,
}

/// Overpass 요소. way가 아니면 무시한다.
#[derive(Debug, Deserialize)]
struct OverpassElement {
    #[serde(rename = "type")]
    kind: String,
    id: u64,
    /// `out geom`의 노드 좌표. 응답에 없는 노드는 `null`로 온다.
    #[serde(default)]
    geometry: Vec<Option<LatLon>>,
}

/// Overpass JSON 문자열에서 way ID와 노드 좌표 목록을 읽는다.
pub fn parse_overpass_ways(json: &str) -> Result<Vec<OsmWay>, String> {
    let response: OverpassResponse =
        serde_json::from_str(json).map_err(|error| format!("invalid Overpass JSON: {error}"))?;
    Ok(response
        .elements
        .into_iter()
        .filter(|element| element.kind == "way")
        .map(|element| (element.id, element.geometry))
        .collect())
}

/// 선택한 way들을 주어진 순서로 이어 하나의 중심선(지역 좌표)으로 만든다.
///
/// 빠진 노드와 유한하지 않은 좌표는 버린다. 각 way는 앞 중심선의 끝에 이어져야 하며,
/// 필요하면 뒤집어 붙인다(첫 way도 두 번째 way와 맞도록 뒤집을 수 있다). 끊겨 있으면 오류다.
pub fn chain_ways(ways: &[OsmWay], order: &[u64], origin: LatLon) -> Result<Vec<Vec2>, String> {
    if order.is_empty() {
        return Err("no ways selected".to_owned());
    }
    let mut line: Vec<Vec2> = Vec::new();
    for (index, id) in order.iter().enumerate() {
        let (_, nodes) = ways
            .iter()
            .find(|(way_id, _)| way_id == id)
            .ok_or_else(|| format!("way {id} not found in source data"))?;
        let mut points: Vec<Vec2> = nodes
            .iter()
            .flatten()
            .filter(|node| node.lat.is_finite() && node.lon.is_finite())
            .map(|node| project(origin, *node))
            .collect();
        if points.len() < 2 {
            return Err(format!("way {id} has fewer than 2 usable nodes"));
        }
        if index == 0 {
            line = points;
            continue;
        }
        let close = |a: Vec2, b: Vec2| a.distance(b) <= JOIN_TOLERANCE_M;
        let (first, last) = (points[0], points[points.len() - 1]);
        let end = line[line.len() - 1];
        if index == 1 && !close(end, first) && !close(end, last) {
            // 첫 way의 방향이 반대일 수 있다.
            line.reverse();
        }
        let end = line[line.len() - 1];
        if close(end, last) && !close(end, first) {
            points.reverse();
        } else if !close(end, first) {
            return Err(format!("way {id} is not connected to the previous way"));
        }
        line.extend(points.into_iter().skip(1));
    }
    Ok(line)
}

/// 연속 중복점을 지우고, 점 개수·길이·꺾임 각도를 검사한다.
pub fn clean_polyline(points: &[Vec2]) -> Result<Vec<Vec2>, String> {
    let mut cleaned: Vec<Vec2> = Vec::with_capacity(points.len());
    for point in points {
        if !point.is_finite() {
            return Err(format!("non-finite point {point:?}"));
        }
        if cleaned
            .last()
            .is_none_or(|last: &Vec2| last.distance(*point) >= MIN_POINT_SPACING_M)
        {
            cleaned.push(*point);
        }
    }
    if cleaned.len() < 2 {
        return Err("centerline has fewer than 2 distinct points (zero length)".to_owned());
    }
    for window in cleaned.windows(3) {
        let turn = turn_deg(window[1] - window[0], window[2] - window[1]);
        if turn > MAX_TURN_DEG {
            return Err(format!(
                "centerline turns {turn:.1} deg at {:?} (limit {MAX_TURN_DEG} deg)",
                window[1]
            ));
        }
    }
    Ok(cleaned)
}

/// 두 진행 방향 사이의 각도(deg, 0..=180)를 구한다.
fn turn_deg(a: Vec2, b: Vec2) -> f32 {
    a.angle_to(b).abs().to_degrees()
}

/// 꺾임이 가장 큰 점에서 바깥쪽 연석 사이에 생기는 쐐기 틈의 최대 폭(m)을 근사한다.
pub fn max_outer_gap_m(points: &[Vec2], width_m: f32) -> f32 {
    points
        .windows(3)
        .map(|window| {
            let half_turn =
                turn_deg(window[1] - window[0], window[2] - window[1]).to_radians() / 2.0;
            2.0 * (width_m / 2.0 + CURB_THICKNESS_M) * half_turn.tan()
        })
        .fold(0.0, f32::max)
}

/// 방향 벡터 `direction`(x, z)을 바라보는 heading(rad)을 구한다. [`Motion`]과 같은 규칙이다.
fn heading_of(direction: Vec2) -> f32 {
    (-direction.x).atan2(-direction.y)
}

/// 중심선 구간 `a→b`를 따라 놓인 직사각형을 만든다. `lateral`은 오른쪽으로의 중심 이동(m)이다.
fn segment_rect(a: Vec2, b: Vec2, width: f32, lateral: f32, extend: f32) -> GroundRect {
    let direction = (b - a).normalize();
    let heading = heading_of(direction);
    let right = Vec2::new(heading.cos(), -heading.sin());
    GroundRect {
        center: (a + b) / 2.0 + right * lateral,
        half_size: Vec2::new(width / 2.0, a.distance(b) / 2.0 + extend),
        heading_rad: heading,
    }
}

/// 깨끗한 중심선과 임시 폭으로 노면·연석·끝 벽과 시작 자세를 만든다.
///
/// 시작 자세는 첫 구간 위에서 시작 쪽 끝 벽과 차체 뒤끝 사이를 [`START_REAR_CLEARANCE_M`]만큼 둔다.
pub fn build_road_course(
    name: &str,
    centerline: &[Vec2],
    width_m: f32,
    notices: Vec<String>,
) -> Result<CourseDef, String> {
    if !WIDTH_RANGE_M.contains(&width_m) {
        return Err(format!(
            "temporary width {width_m} m must be within {}..={} m",
            WIDTH_RANGE_M.start(),
            WIDTH_RANGE_M.end()
        ));
    }
    let points = clean_polyline(centerline)?;
    let gap = max_outer_gap_m(&points, width_m);
    if gap >= VEHICLE_FOOTPRINT.width_m {
        return Err(format!(
            "outer curb gap {gap:.2} m at a bend is not narrower than the vehicle ({} m)",
            VEHICLE_FOOTPRINT.width_m
        ));
    }
    let rear_overhang = VEHICLE_FOOTPRINT.length_m / 2.0 - VEHICLE_FOOTPRINT.rear_axle_to_center_m;
    let front_reach = VEHICLE_FOOTPRINT.rear_axle_to_center_m + VEHICLE_FOOTPRINT.length_m / 2.0;
    let start_offset = START_REAR_CLEARANCE_M + rear_overhang;
    let first_length = points[0].distance(points[1]);
    if first_length < start_offset + front_reach {
        return Err(format!(
            "first segment is {first_length:.1} m; at least {:.1} m is needed for the start pose",
            start_offset + front_reach
        ));
    }

    let curb_offset = width_m / 2.0 + CURB_THICKNESS_M / 2.0;
    let mut surfaces = Vec::new();
    let mut colliders = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        surfaces.push(segment_rect(a, b, width_m, 0.0, SURFACE_OVERLAP_M));
        for side in [-1.0, 1.0] {
            colliders.push(Collider {
                rect: segment_rect(a, b, CURB_THICKNESS_M, side * curb_offset, 0.0),
                height_m: CURB_HEIGHT_M,
                kind: ColliderKind::Curb,
            });
        }
    }
    // 양 끝 벽: 끝점 바깥쪽으로 벽 두께 절반만큼 물려 둔다.
    let wall_width = width_m + 2.0 * CURB_THICKNESS_M;
    let last = points.len() - 1;
    for (end, inner) in [(points[0], points[1]), (points[last], points[last - 1])] {
        let outward = (end - inner).normalize();
        let center = end + outward * (END_WALL_THICKNESS_M / 2.0);
        colliders.push(Collider {
            rect: GroundRect {
                center,
                half_size: Vec2::new(wall_width / 2.0, END_WALL_THICKNESS_M / 2.0),
                heading_rad: heading_of(outward),
            },
            height_m: END_WALL_HEIGHT_M,
            kind: ColliderKind::Wall,
        });
    }

    let direction = (points[1] - points[0]).normalize();
    let axle = points[0] + direction * start_offset;
    let start = Motion {
        x_m: axle.x,
        z_m: axle.y,
        heading_rad: heading_of(direction),
        ..Motion::default()
    };
    let rects: Vec<GroundRect> = colliders.iter().map(|collider| collider.rect).collect();
    if crate::vehicle::collision::in_contact(&VEHICLE_FOOTPRINT, &start, &rects) {
        return Err("start pose touches a curb or wall".to_owned());
    }
    Ok(CourseDef {
        name: name.to_owned(),
        notices,
        start,
        surfaces,
        markings: Vec::new(),
        colliders,
    })
}

/// 지도 코스 선택 파일(TOML). 대상 통로를 정한 뒤 `assets/maps/<이름>/course.toml`로 만든다.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapCourseFile {
    /// 출처 Overpass JSON 경로. 이 TOML 파일 위치 기준 상대 경로다.
    pub source: String,
    /// 지역 좌표 원점 [위도, 경도](deg).
    pub origin: [f64; 2],
    /// 이어 붙일 way ID 목록(진행 순서).
    pub ways: Vec<u64>,
    /// 훈련용 임시 도로 폭(m). OSM 폭이 아니다.
    pub temporary_width_m: f32,
}

/// 선택 파일 내용과 그 파일이 있는 폴더로 지도 코스를 만든다.
pub fn build_map_course(file: &MapCourseFile, base_dir: &Path) -> Result<CourseDef, String> {
    let [lat, lon] = file.origin;
    if !(lat.is_finite() && lon.is_finite() && lat.abs() <= 85.0 && lon.abs() <= 180.0) {
        return Err(format!("invalid origin {:?}", file.origin));
    }
    let source = base_dir.join(&file.source);
    let json = std::fs::read_to_string(&source)
        .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
    let ways = parse_overpass_ways(&json)?;
    let centerline = chain_ways(&ways, &file.ways, LatLon { lat, lon })?;
    let notices = vec![
        OSM_ATTRIBUTION.to_owned(),
        "Virtual training course from OSM centerline (not a surveyed road)".to_owned(),
        format!(
            "Road width {:.1} m is a temporary training value",
            file.temporary_width_m
        ),
    ];
    build_road_course("map", &centerline, file.temporary_width_m, notices)
}

/// 선택 파일 경로에서 지도 코스를 읽는다. 파일 없음·TOML 오류·변환 오류는 모두 `Err`다.
pub fn load_map_course(path: &Path) -> Result<CourseDef, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let file: MapCourseFile =
        toml::from_str(&text).map_err(|error| format!("invalid {}: {error}", path.display()))?;
    build_map_course(&file, path.parent().unwrap_or(Path::new(".")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::collision::{ContactState, in_contact, step_with_obstacles};
    use crate::vehicle::{DynamicsParams, Gear, VehicleCommand, VehicleInput, VehicleState};

    /// 테스트 기준점(여수 부근 임의 좌표; 실제 대상 통로 아님).
    const ORIGIN: LatLon = LatLon {
        lat: 34.75,
        lon: 127.75,
    };

    /// 기준점에서 동쪽 `east`, 북쪽 `north`(m)만큼 떨어진 위경도를 만든다(투영의 역변환).
    fn offset(east: f64, north: f64) -> LatLon {
        LatLon {
            lat: ORIGIN.lat + (north / EARTH_RADIUS_M).to_degrees(),
            lon: ORIGIN.lon
                + (east / (EARTH_RADIUS_M * ORIGIN.lat.to_radians().cos())).to_degrees(),
        }
    }

    /// 원점은 (0, 0)으로, 북쪽은 -Z로, 동쪽은 +X로 투영되는지 확인한다.
    #[test]
    fn projection_axes_and_scale() {
        assert_eq!(project(ORIGIN, ORIGIN), Vec2::ZERO);
        let north = project(ORIGIN, offset(0.0, 100.0));
        assert!(
            north.x.abs() < 1e-3 && (north.y + 100.0).abs() < 1e-3,
            "{north:?}"
        );
        let east = project(ORIGIN, offset(250.0, 0.0));
        assert!(
            (east.x - 250.0).abs() < 1e-3 && east.y.abs() < 1e-3,
            "{east:?}"
        );
        // 위도 0.001°는 약 111.2 m다.
        let degree = project(
            ORIGIN,
            LatLon {
                lat: ORIGIN.lat + 0.001,
                lon: ORIGIN.lon,
            },
        );
        assert!((degree.y + 111.19).abs() < 0.05, "{degree:?}");
    }

    /// way만 읽고, 응답에 없는 노드(null)는 None으로 남기는지 확인한다.
    #[test]
    fn overpass_parsing_keeps_missing_nodes() {
        let json = r#"{"elements":[
            {"type":"node","id":1,"lat":34.7,"lon":127.7},
            {"type":"way","id":7,"tags":{"highway":"pedestrian"},
             "geometry":[{"lat":34.75,"lon":127.75},null,{"lat":34.751,"lon":127.75}]}
        ]}"#;
        let ways = parse_overpass_ways(json).unwrap();
        assert_eq!(ways.len(), 1);
        assert_eq!(ways[0].0, 7);
        assert_eq!(ways[0].1.len(), 3);
        assert!(ways[0].1[1].is_none());
        assert!(parse_overpass_ways("not json").is_err());
    }

    /// 거꾸로 저장된 way도 뒤집어 잇고, 끊긴 way·없는 way·노드 부족 way는 거부하는지 확인한다.
    #[test]
    fn chaining_reverses_and_rejects_bad_ways() {
        let a = (1, vec![Some(offset(0.0, 0.0)), Some(offset(0.0, 50.0))]);
        let b = (
            2,
            vec![Some(offset(0.0, 100.0)), None, Some(offset(0.0, 50.0))],
        );
        let far = (3, vec![Some(offset(500.0, 0.0)), Some(offset(600.0, 0.0))]);
        let short = (4, vec![Some(offset(0.0, 50.0)), None]);
        let ways = vec![a, b, far, short];
        let line = chain_ways(&ways, &[1, 2], ORIGIN).unwrap();
        assert_eq!(line.len(), 3);
        assert!((line[2].y + 100.0).abs() < 1e-2, "{line:?}");
        // 두 번째 way에 맞춰 첫 way를 뒤집는 경우.
        let line = chain_ways(&ways, &[2, 1], ORIGIN).unwrap();
        assert!(line[0].y.abs() > 99.0 && line[2].y.abs() < 1e-2, "{line:?}");
        assert!(chain_ways(&ways, &[1, 3], ORIGIN).is_err());
        assert!(chain_ways(&ways, &[9], ORIGIN).is_err());
        assert!(chain_ways(&ways, &[4], ORIGIN).is_err());
        assert!(chain_ways(&ways, &[], ORIGIN).is_err());
    }

    /// 연속 중복점은 하나로 줄이고, 길이 0·NaN·급한 꺾임은 거부하는지 확인한다.
    #[test]
    fn polyline_cleanup_handles_degenerate_input() {
        let line = [Vec2::ZERO, Vec2::new(0.0, -0.01), Vec2::new(0.0, -30.0)];
        assert_eq!(clean_polyline(&line).unwrap().len(), 2);
        assert!(clean_polyline(&[Vec2::ONE, Vec2::ONE]).is_err());
        assert!(clean_polyline(&[Vec2::ZERO]).is_err());
        assert!(clean_polyline(&[Vec2::ZERO, Vec2::new(f32::NAN, 0.0)]).is_err());
        let sharp = [Vec2::ZERO, Vec2::new(0.0, -30.0), Vec2::new(30.0, -30.0)];
        assert!(clean_polyline(&sharp).is_err());
        let gentle = [Vec2::ZERO, Vec2::new(0.0, -30.0), Vec2::new(3.0, -60.0)];
        assert_eq!(clean_polyline(&gentle).unwrap().len(), 3);
    }

    /// 시동 Ready, 지정 기어인 차량 상태를 만든다.
    fn ready_in(gear: Gear) -> VehicleState {
        let mut state = VehicleState::default();
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        state.apply(VehicleCommand::RequestGear(gear)).unwrap();
        state
    }

    /// 중심선을 따라 북동쪽으로 뻗은 직선 도로에서 시작 자세가 비접촉이고,
    /// 직진하면 반대쪽 끝 벽에서 처음 막히며, 모든 영역이 유한·양수 크기인지 확인한다.
    #[test]
    fn straight_road_course_is_drivable_end_to_end() {
        let direction = Vec2::new(1.0, -1.0).normalize();
        let line = [Vec2::new(5.0, 3.0), Vec2::new(5.0, 3.0) + direction * 80.0];
        let course = build_road_course("test", &line, 4.0, Vec::new()).unwrap();
        let obstacles = course.collision_rects();
        for rect in course.surfaces.iter().chain(obstacles.iter()) {
            assert!(rect.center.is_finite() && rect.heading_rad.is_finite());
            assert!(rect.half_size.min_element() > 0.0, "{rect:?}");
        }
        assert!(!in_contact(&VEHICLE_FOOTPRINT, &course.start, &obstacles));
        let (x, z) = course.start.forward();
        assert!((Vec2::new(x, z) - direction).length() < 1e-4);

        let mut state = ready_in(Gear::Drive);
        state.set_input(VehicleInput {
            throttle: 1.0,
            ..VehicleInput::default()
        });
        let mut motion = course.start;
        let mut contact = ContactState::default();
        let params = DynamicsParams::default();
        let impact = (0..64 * 120).find_map(|_| {
            step_with_obstacles(
                &mut state,
                &mut motion,
                &params,
                1.0 / 64.0,
                &VEHICLE_FOOTPRINT,
                &obstacles,
                &mut contact,
            )
        });
        assert!(impact.is_some(), "never blocked");
        let front = Vec2::new(motion.x_m, motion.z_m)
            + direction
                * (VEHICLE_FOOTPRINT.rear_axle_to_center_m + VEHICLE_FOOTPRINT.length_m / 2.0);
        assert!(
            front.distance(line[1]) < 0.05,
            "stopped with front at {front:?}"
        );
    }

    /// 완만하게 꺾인 도로는 만들어지고 시작 자세가 비접촉인지, 폭·첫 구간 길이 오류는 거부하는지 확인한다.
    #[test]
    fn bent_road_and_invalid_parameters() {
        let line = [Vec2::ZERO, Vec2::new(0.0, -40.0), Vec2::new(5.0, -80.0)];
        let course = build_road_course("bent", &line, 4.0, Vec::new()).unwrap();
        assert!(!in_contact(
            &VEHICLE_FOOTPRINT,
            &course.start,
            &course.collision_rects()
        ));
        assert_eq!(course.surfaces.len(), 2);
        assert!(build_road_course("narrow", &line, 1.0, Vec::new()).is_err());
        assert!(build_road_course("nan", &line, f32::NAN, Vec::new()).is_err());
        let short = [Vec2::ZERO, Vec2::new(0.0, -8.0), Vec2::new(0.0, -80.0)];
        assert!(build_road_course("short", &short, 4.0, Vec::new()).is_err());
    }

    /// 꺾이는 점의 바깥 연석 틈이 차체 폭 이상이면 거부하는지 확인한다.
    #[test]
    fn wide_road_with_bend_is_rejected_when_gap_fits_vehicle() {
        // 19° 꺾임, 폭 12 m: 틈 ≈ 2 × 6.3 × tan(9.5°) ≈ 2.1 m > 차체 폭 1.8 m.
        let bend = 19f32.to_radians();
        let line = [
            Vec2::ZERO,
            Vec2::new(0.0, -40.0),
            Vec2::new(0.0, -40.0) + Vec2::new(bend.sin(), -bend.cos()) * 40.0,
        ];
        assert!(max_outer_gap_m(&line, 12.0) > VEHICLE_FOOTPRINT.width_m);
        assert!(build_road_course("wide", &line, 12.0, Vec::new()).is_err());
        assert!(build_road_course("normal", &line, 4.0, Vec::new()).is_ok());
    }

    /// 테스트마다 따로 쓰는 임시 폴더를 만든다.
    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sds-map-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 선택 파일과 원본으로 지도 코스를 읽고 출처 안내를 붙이며, 파일 없음·TOML 오류·원본 오류는 거부하는지 확인한다.
    #[test]
    fn map_course_file_loading_and_failures() {
        let dir = temp_dir("load");
        let a = offset(0.0, 0.0);
        let b = offset(0.0, 60.0);
        std::fs::write(
            dir.join("src.json"),
            format!(
                r#"{{"elements":[{{"type":"way","id":10,"geometry":[{{"lat":{},"lon":{}}},{{"lat":{},"lon":{}}}]}}]}}"#,
                a.lat, a.lon, b.lat, b.lon
            ),
        )
        .unwrap();
        let course_toml = format!(
            "source = \"src.json\"\norigin = [{}, {}]\nways = [10]\ntemporary_width_m = 4.0\n",
            ORIGIN.lat, ORIGIN.lon
        );
        std::fs::write(dir.join("course.toml"), &course_toml).unwrap();
        let course = load_map_course(&dir.join("course.toml")).unwrap();
        assert_eq!(course.name, "map");
        assert!(
            course
                .notices
                .iter()
                .any(|notice| notice == OSM_ATTRIBUTION)
        );
        assert!(
            course
                .notices
                .iter()
                .any(|notice| notice.contains("temporary"))
        );

        assert!(load_map_course(&dir.join("missing.toml")).is_err());
        std::fs::write(dir.join("bad.toml"), "source = [").unwrap();
        assert!(load_map_course(&dir.join("bad.toml")).is_err());
        std::fs::write(
            dir.join("unknown.toml"),
            format!("{course_toml}extra = 1\n"),
        )
        .unwrap();
        assert!(load_map_course(&dir.join("unknown.toml")).is_err());
        std::fs::write(
            dir.join("nosrc.toml"),
            course_toml.replace("src.json", "none.json"),
        )
        .unwrap();
        assert!(load_map_course(&dir.join("nosrc.toml")).is_err());
        std::fs::write(dir.join("garbage.json"), "{").unwrap();
        std::fs::write(
            dir.join("garbage.toml"),
            course_toml.replace("src.json", "garbage.json"),
        )
        .unwrap();
        assert!(load_map_course(&dir.join("garbage.toml")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
