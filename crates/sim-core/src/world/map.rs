//! OSM 중심선 → 주행 코스 변환(Phase 3-4 파이프라인, 대상 통로와 무관한 범용 부분).
//!
//! 저장해 둔 Overpass JSON(`out geom`)과 코스 선택 파일(TOML)을 읽어 [`CourseDef`]를 만든다.
//! 실행 중 네트워크로 내려받지 않는다. 변환은 모두 메모리에서 끝낸 뒤 결과를 돌려주며,
//! 실패하면 오류만 반환해 일부만 만든 코스가 남지 않는다.
//!
//! 좌표 규칙: 동쪽 +X, 북쪽 -Z, 높이 y=0, 단위 m. 원점은 선택 파일의 기준 위경도다.
//! 투영은 기준 위도에서의 등장방형 근사이며, 모든 점이 원점에서 [`MAX_ORIGIN_DISTANCE_M`] 안에 있어야 한다.
//! 생성한 코스 전체는 [`MAX_EXTENT_M`](개발상 임시 상한 500 × 300 m) 안에 들어야 한다.
//! 도로 폭은 OSM에 값이 없으므로 선택 파일의 **훈련용 임시 폭**을 쓴다(실제 폭 아님).

use std::path::Path;

use glam::Vec2;
use serde::Deserialize;

use super::course::{Area, Collider, ColliderKind, CourseDef, Strip, StripKind};
use super::road_geom::{self, RoadSpec};
use crate::vehicle::{DynamicsParams, GroundRect, Motion, VEHICLE_FOOTPRINT};

/// 지구 평균 반지름(m).
const EARTH_RADIUS_M: f64 = 6_371_008.8;
/// 이 거리(m)보다 가까운 연속 점은 중복으로 보고 하나만 남긴다.
const MIN_POINT_SPACING_M: f32 = 0.05;
/// way 끝점끼리 이 거리(m) 안이면 이어진 것으로 본다.
const JOIN_TOLERANCE_M: f32 = 0.5;
/// 임시 폭 허용 범위(m).
pub const WIDTH_RANGE_M: std::ops::RangeInclusive<f32> = 2.5..=12.0;
/// 연석 두께(m)와 표시 높이(m).
pub const CURB_THICKNESS_M: f32 = 0.3;
const CURB_HEIGHT_M: f32 = 0.25;
/// 끝 벽 두께(m)와 표시 높이(m).
const END_WALL_THICKNESS_M: f32 = 0.5;
const END_WALL_HEIGHT_M: f32 = 1.2;
/// 시작 자세에서 차체 뒤끝과 시작 쪽 끝 벽 사이 거리(m).
const START_REAR_CLEARANCE_M: f32 = 5.0;
/// 차로 가장자리 표시선 폭(m)과 연석 안쪽으로 들인 거리(m). 개발 코스와 같은 값.
const EDGE_LINE_WIDTH_M: f32 = 0.12;
const EDGE_LINE_INSET_M: f32 = 0.2;
/// 투영한 점이 원점에서 떨어질 수 있는 최대 거리(m). 이 안에서 등장방형 근사의 거리 오차는 무시할 수 있다.
pub const MAX_ORIGIN_DISTANCE_M: f32 = 1000.0;
/// 생성한 코스(연석·끝 벽 포함)의 x·z 축 정렬 범위 상한(긴 변, 짧은 변; m). PLAN 3-3의 개발상 임시 상한.
pub const MAX_EXTENT_M: (f32, f32) = (500.0, 300.0);
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
/// 선택한 way에 빠진 노드(`null`), 유한하지 않거나 위경도 범위를 벗어난 좌표,
/// 원점에서 [`MAX_ORIGIN_DISTANCE_M`]보다 먼 점이 하나라도 있으면 오류다. 빠진 노드 양옆을 이으면
/// 원본에 없는 도로가 생기므로 건너뛰지 않는다(원본 추출 범위를 넓혀 다시 받는다).
/// 각 way는 앞 중심선의 끝에 이어져야 하며, 필요하면 뒤집어 붙인다
/// (첫 way도 두 번째 way와 맞도록 뒤집을 수 있다). 끊겨 있으면 오류다.
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
        let mut points: Vec<Vec2> = Vec::with_capacity(nodes.len());
        for (position, node) in nodes.iter().enumerate() {
            let node = node
                .ok_or_else(|| format!("way {id} node #{position} is missing from source data"))?;
            if !valid_lat_lon(node) {
                return Err(format!(
                    "way {id} node #{position} has invalid coordinates {node:?}"
                ));
            }
            let point = project(origin, node);
            // 위경도를 이미 검사했으므로 투영 결과는 유한하다.
            if point.length() > MAX_ORIGIN_DISTANCE_M {
                return Err(format!(
                    "way {id} node #{position} is {:.0} m from the origin (limit {MAX_ORIGIN_DISTANCE_M} m)",
                    point.length()
                ));
            }
            points.push(point);
        }
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

/// 위경도가 유한하고 위도 ±90°, 경도 ±180° 안인지 확인한다.
fn valid_lat_lon(point: LatLon) -> bool {
    point.lat.is_finite()
        && point.lon.is_finite()
        && point.lat.abs() <= 90.0
        && point.lon.abs() <= 180.0
}

/// 연속 중복점을 지우고, 점 개수·길이를 검사한다. 꺾임은 [`road_geom::build_geometry`]가 필렛으로 처리한다.
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
    Ok(cleaned)
}

/// 직사각형들의 모든 모서리와 점들을 덮는 x·z 축 정렬 범위의 크기(m)를 구한다.
fn extent_of<'a>(
    rects: impl Iterator<Item = &'a GroundRect>,
    points: impl Iterator<Item = &'a Vec2>,
) -> Vec2 {
    let (mut low, mut high) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
    for point in points {
        low = low.min(*point);
        high = high.max(*point);
    }
    for rect in rects {
        let (sin, cos) = rect.heading_rad.sin_cos();
        let (right, forward) = (Vec2::new(cos, -sin), Vec2::new(-sin, -cos));
        for (sx, sz) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            let corner =
                rect.center + right * sx * rect.half_size.x + forward * sz * rect.half_size.y;
            low = low.min(corner);
            high = high.max(corner);
        }
    }
    (high - low).max(Vec2::ZERO)
}

/// 지도 코스에 쓰는 차량 최소 회전 반경(m). 기본 운동 모델 값(후륜축 기준)이다.
pub fn vehicle_min_radius_m() -> f32 {
    crate::vehicle::dynamics::min_turn_radius_m(&DynamicsParams::default())
}

/// 중심선과 임시 폭으로 굽은 도로 코스를 만든다(기본 원호 분할 각도). [`build_road_course_with`] 참고.
pub fn build_road_course(
    name: &str,
    centerline: &[Vec2],
    width_m: f32,
    notices: Vec<String>,
) -> Result<CourseDef, String> {
    let spec = RoadSpec {
        width_m,
        curb_thickness_m: CURB_THICKNESS_M,
        vehicle_min_radius_m: vehicle_min_radius_m(),
        arc_step_deg: road_geom::ARC_STEP_DEG,
    };
    build_road_course_with(name, centerline, &spec, notices)
}

/// 중심선과 형상 조건으로 노면·표시선 띠, 연석, 끝 벽, 시작 자세를 만든다(Phase 4-1).
///
/// 꺾이는 점마다 필렛을 넣고([`road_geom::build_geometry`]), 연석 바깥선 다각형이 단순 다각형인지 검사한다.
/// 시작 자세는 중심선을 따라 시작 쪽 끝 벽에서 차체 뒤끝까지 [`START_REAR_CLEARANCE_M`]인 곳에 두고,
/// 방향은 그곳 중심선 방향이다. 실패하면 일부만 만든 코스 없이 오류만 반환한다.
pub fn build_road_course_with(
    name: &str,
    centerline: &[Vec2],
    spec: &RoadSpec,
    notices: Vec<String>,
) -> Result<CourseDef, String> {
    check_width(spec.width_m)?;
    let piece = road_piece(centerline, spec, [true, true])?;
    let start = start_pose(&piece.centerline)?;
    let mut course = finish_course(
        name,
        notices,
        start,
        piece.strips,
        Vec::new(),
        piece.colliders,
        MAX_EXTENT_M,
    )?;
    course.goal = Some(goal_pose(&piece.centerline)?);
    Ok(course)
}

/// 임시 폭이 허용 범위([`WIDTH_RANGE_M`]) 안인지 검사한다.
pub fn check_width(width_m: f32) -> Result<(), String> {
    if !WIDTH_RANGE_M.contains(&width_m) {
        return Err(format!(
            "temporary width {width_m} m must be within {}..={} m",
            WIDTH_RANGE_M.start(),
            WIDTH_RANGE_M.end()
        ));
    }
    Ok(())
}

/// 도로 조각 하나(경로 조각·교차로 곁가지 공통)의 생성 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadPiece {
    /// 필렛을 넣은 매끄러운 중심선.
    pub centerline: Vec<Vec2>,
    /// 노면 띠와 양쪽 가장자리 표시선 띠.
    pub strips: Vec<Strip>,
    /// 양옆 연석과 요청한 끝 벽.
    pub colliders: Vec<Collider>,
    /// 연석 바깥선으로 닫은 다각형(왼쪽 앞으로, 오른쪽 거꾸로). 조각끼리 겹침 검사에 쓴다.
    pub outline: Vec<Vec2>,
}

/// 중심선 점들로 도로 조각을 만든다. `walls`는 [시작 쪽, 끝 쪽]에 끝 벽을 세울지다.
///
/// 중복점 정리, 필렛, 연석 바깥선 단순 다각형 검사를 거친다. 교차로에 붙는 쪽은 벽 없이 열어 둔다.
pub fn road_piece(points: &[Vec2], spec: &RoadSpec, walls: [bool; 2]) -> Result<RoadPiece, String> {
    let points = clean_polyline(points)?;
    let geometry = road_geom::build_geometry(&points, spec)?;
    let center = geometry.centerline;
    let (width_m, curb) = (spec.width_m, spec.curb_thickness_m);
    road_geom::check_simple_boundary(&center, width_m / 2.0 + curb)?;
    let half = width_m / 2.0;
    let line_outer = half - EDGE_LINE_INSET_M;
    let line_inner = line_outer - EDGE_LINE_WIDTH_M;
    let strip = |kind, left: f32, right: f32| Strip {
        kind,
        left: road_geom::offset_line(&center, left),
        right: road_geom::offset_line(&center, right),
    };
    let strips = vec![
        strip(StripKind::Road, -half, half),
        strip(StripKind::EdgeLine, -line_outer, -line_inner),
        strip(StripKind::EdgeLine, line_inner, line_outer),
    ];
    let mut colliders: Vec<Collider> = road_geom::curb_rects(&center, width_m, curb)
        .into_iter()
        .map(|rect| Collider {
            rect,
            height_m: CURB_HEIGHT_M,
            kind: ColliderKind::Curb,
        })
        .collect();
    // 끝 벽: 끝점 바깥쪽으로 벽 두께 절반만큼 물려 둔다.
    let wall_width = width_m + 2.0 * curb;
    let last = center.len() - 1;
    let ends = [(center[0], center[1]), (center[last], center[last - 1])];
    for (wanted, (end, inner)) in walls.into_iter().zip(ends) {
        if !wanted {
            continue;
        }
        let outward = (end - inner).normalize();
        colliders.push(Collider {
            rect: GroundRect {
                center: end + outward * (END_WALL_THICKNESS_M / 2.0),
                half_size: Vec2::new(wall_width / 2.0, END_WALL_THICKNESS_M / 2.0),
                heading_rad: road_geom::heading_of(outward),
            },
            height_m: END_WALL_HEIGHT_M,
            kind: ColliderKind::Wall,
        });
    }
    let reach = half + curb;
    let mut outline = road_geom::offset_line(&center, -reach);
    outline.extend(road_geom::offset_line(&center, reach).into_iter().rev());
    Ok(RoadPiece {
        centerline: center,
        strips,
        colliders,
        outline,
    })
}

/// 중심선 시작 쪽 끝 벽에서 차체 뒤끝까지 [`START_REAR_CLEARANCE_M`]인 곳의 시작 자세를 만든다. 짧으면 오류.
pub fn start_pose(center: &[Vec2]) -> Result<Motion, String> {
    let rear_overhang = VEHICLE_FOOTPRINT.length_m / 2.0 - VEHICLE_FOOTPRINT.rear_axle_to_center_m;
    let front_reach = VEHICLE_FOOTPRINT.rear_axle_to_center_m + VEHICLE_FOOTPRINT.length_m / 2.0;
    let start_offset = START_REAR_CLEARANCE_M + rear_overhang;
    let length = road_geom::polyline_length(center);
    if length < start_offset + front_reach {
        return Err(format!(
            "course is {length:.1} m long; at least {:.1} m is needed for the start pose",
            start_offset + front_reach
        ));
    }
    let (axle, direction) = road_geom::point_at_distance(center, start_offset)
        .ok_or_else(|| "course is too short for the start pose".to_owned())?;
    Ok(Motion {
        x_m: axle.x,
        z_m: axle.y,
        heading_rad: road_geom::heading_of(direction),
        ..Motion::default()
    })
}

/// 중심선 끝 쪽 끝 벽에서 차체 앞끝까지 [`START_REAR_CLEARANCE_M`]인 곳의 목표 자세(진행 방향)를 만든다. 짧으면 오류.
///
/// 시작 자세와 같은 여유를 끝 쪽에 둔 것이며, 경로 끝 목표 표시(4-6)에 쓴다.
pub fn goal_pose(center: &[Vec2]) -> Result<Motion, String> {
    let front_reach = VEHICLE_FOOTPRINT.rear_axle_to_center_m + VEHICLE_FOOTPRINT.length_m / 2.0;
    let length = road_geom::polyline_length(center);
    let distance = length - START_REAR_CLEARANCE_M - front_reach;
    let too_short = || format!("course is {length:.1} m long; too short for the goal pose");
    if distance < 0.0 {
        return Err(too_short());
    }
    let (axle, direction) = road_geom::point_at_distance(center, distance).ok_or_else(too_short)?;
    Ok(Motion {
        x_m: axle.x,
        z_m: axle.y,
        heading_rad: road_geom::heading_of(direction),
        ..Motion::default()
    })
}

/// 시작 자세 비접촉과 코스 크기 상한 `max_extent_m`(긴 변, 짧은 변; m)을 검사하고 코스 정의를 완성한다.
///
/// 단일 통로 코스는 [`MAX_EXTENT_M`], 경로 코스는 [`super::route::ROUTE_MAX_EXTENT_M`]을 쓴다.
pub fn finish_course(
    name: &str,
    notices: Vec<String>,
    start: Motion,
    strips: Vec<Strip>,
    areas: Vec<Area>,
    colliders: Vec<Collider>,
    max_extent_m: (f32, f32),
) -> Result<CourseDef, String> {
    let rects: Vec<GroundRect> = colliders.iter().map(|collider| collider.rect).collect();
    if crate::vehicle::collision::in_contact(&VEHICLE_FOOTPRINT, &start, &rects) {
        return Err("start pose touches a curb or wall".to_owned());
    }
    let size = extent_of(
        rects.iter(),
        strips
            .iter()
            .flat_map(|s| s.left.iter().chain(&s.right))
            .chain(areas.iter().flat_map(|a| a.boundary.iter())),
    );
    let (long, short) = (size.max_element(), size.min_element());
    if long > max_extent_m.0 || short > max_extent_m.1 {
        return Err(format!(
            "course spans {:.0} x {:.0} m (x by z); limit is {} x {} m",
            size.x, size.y, max_extent_m.0, max_extent_m.1
        ));
    }
    Ok(CourseDef {
        name: name.to_owned(),
        notices,
        start,
        goal: None,
        surfaces: Vec::new(),
        strips,
        areas,
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
    // 극지방에서는 cos φ0가 0에 가까워 동서 축척이 무너지므로 위도 ±85°까지만 받는다.
    if !(valid_lat_lon(LatLon { lat, lon }) && lat.abs() <= 85.0) {
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
///
/// 최상위에 `format` 키가 있으면 경로 파일([`super::route_plan::RouteFile`], Phase 4-3)로,
/// 없으면 단일 통로 선택 파일([`MapCourseFile`])로 읽는다.
pub fn load_map_course(path: &Path) -> Result<CourseDef, String> {
    load_map_course_with_patch_dir(
        path,
        Path::new(super::map_patch::PATCH_DIR),
        super::map_patch::PatchAudience::Distribution,
    )
}

/// [`load_map_course`]와 같되 지역 보정 경로가 참조할 보정 폴더와 실행 대상을 명시한다.
pub fn load_map_course_with_patch_dir(
    path: &Path,
    patch_dir: &Path,
    audience: super::map_patch::PatchAudience,
) -> Result<CourseDef, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let invalid = |error: toml::de::Error| format!("invalid {}: {error}", path.display());
    let table: toml::Table = toml::from_str(&text).map_err(invalid)?;
    let base_dir = path.parent().unwrap_or(Path::new("."));
    if table.contains_key("format") {
        let file: super::route_plan::RouteFile = toml::from_str(&text).map_err(invalid)?;
        return super::route_plan::load_route_course_with_patch_dir(
            &file, base_dir, patch_dir, audience,
        );
    }
    let file: MapCourseFile = toml::from_str(&text).map_err(invalid)?;
    build_map_course(&file, base_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::collision::{ContactState, in_contact, step_with_obstacles};
    use crate::vehicle::{DynamicsParams, Gear, VehicleCommand, VehicleInput, VehicleState};
    use crate::world::test_drive::{FollowResult, follow_path};

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
            vec![
                Some(offset(0.0, 100.0)),
                Some(offset(0.0, 75.0)),
                Some(offset(0.0, 50.0)),
            ],
        );
        let far = (3, vec![Some(offset(500.0, 0.0)), Some(offset(600.0, 0.0))]);
        let short = (4, vec![Some(offset(0.0, 50.0))]);
        let ways = vec![a, b, far, short];
        let line = chain_ways(&ways, &[1, 2], ORIGIN).unwrap();
        assert_eq!(line.len(), 4);
        assert!((line[3].y + 100.0).abs() < 1e-2, "{line:?}");
        // 두 번째 way에 맞춰 첫 way를 뒤집는 경우.
        let line = chain_ways(&ways, &[2, 1], ORIGIN).unwrap();
        assert!(line[0].y.abs() > 99.0 && line[3].y.abs() < 1e-2, "{line:?}");
        assert!(chain_ways(&ways, &[1, 3], ORIGIN).is_err());
        assert!(chain_ways(&ways, &[9], ORIGIN).is_err());
        assert!(chain_ways(&ways, &[4], ORIGIN).is_err());
        assert!(chain_ways(&ways, &[], ORIGIN).is_err());
    }

    /// 선택 way의 빠진 노드(앞·가운데·끝)는 양옆을 잇지 않고 오류로 거부하는지 확인한다(Codex 재현 사례 포함).
    #[test]
    fn missing_nodes_are_rejected_not_bridged() {
        let (a, b) = (
            LatLon {
                lat: 34.75,
                lon: 127.75,
            },
            LatLon {
                lat: 34.751,
                lon: 127.75,
            },
        );
        for nodes in [
            vec![Some(a), None, Some(b)],
            vec![None, Some(a), Some(b)],
            vec![Some(a), Some(b), None],
        ] {
            let error = chain_ways(&[(5, nodes.clone())], &[5], ORIGIN).unwrap_err();
            assert!(error.contains("missing"), "{nodes:?}: {error}");
        }
    }

    /// 위경도 범위 밖·NaN 좌표와 원점에서 먼 점은 거부하는지 확인한다.
    #[test]
    fn invalid_or_distant_coordinates_are_rejected() {
        let bad = [
            LatLon {
                lat: 95.0,
                lon: 127.75,
            },
            LatLon {
                lat: 34.75,
                lon: 190.0,
            },
            LatLon {
                lat: f64::NAN,
                lon: 127.75,
            },
        ];
        for point in bad {
            let way = (6, vec![Some(offset(0.0, 0.0)), Some(point)]);
            assert!(chain_ways(&[way], &[6], ORIGIN).is_err(), "{point:?}");
        }
        // 짧은 직선(30 m)이지만 원점에서 약 5 km 떨어져 있다.
        let distant = (
            7,
            vec![Some(offset(5000.0, 0.0)), Some(offset(5000.0, 30.0))],
        );
        let error = chain_ways(&[distant], &[7], ORIGIN).unwrap_err();
        assert!(error.contains("from the origin"), "{error}");
        // 경계 안쪽(원점에서 900 m)은 받는다.
        let near = (8, vec![Some(offset(900.0, 0.0)), Some(offset(900.0, 30.0))]);
        assert!(chain_ways(&[near], &[8], ORIGIN).is_ok());
    }

    /// 코스 전체가 500 × 300 m 임시 상한을 넘으면 거부하고, 안이면 받는지 확인한다.
    #[test]
    fn course_extent_is_limited() {
        // 남북 약 2224 m 직선(Codex 재현 사례): 원점 거리 검사 전에 크기 검사로도 거부되어야 한다.
        let long = [Vec2::ZERO, Vec2::new(0.0, -2224.0)];
        let error = build_road_course("long", &long, 4.0, Vec::new()).unwrap_err();
        assert!(error.contains("spans"), "{error}");
        // 긴 변 500 m 초과.
        let over = [Vec2::ZERO, Vec2::new(0.0, -505.0)];
        assert!(build_road_course("over", &over, 4.0, Vec::new()).is_err());
        // 대각선 300 × 300 m에 가까운 경로: 짧은 변도 300 m를 넘으면 거부.
        let diagonal = [Vec2::ZERO, Vec2::new(305.0, -305.0)];
        assert!(build_road_course("diag", &diagonal, 4.0, Vec::new()).is_err());
        let ok = [Vec2::ZERO, Vec2::new(0.0, -480.0)];
        assert!(build_road_course("ok", &ok, 4.0, Vec::new()).is_ok());
    }

    /// 선택 파일 단계에서도 긴 경로는 training 대체 대상이 되도록 오류를 내는지 확인한다.
    #[test]
    fn oversize_map_file_fails_to_load() {
        let dir = temp_dir("oversize");
        // 위도 34.75 → 34.77(약 2224 m)인 직선. 원점은 가운데에 두어 원점 거리 1 km 안팎이 되게 한다.
        std::fs::write(
            dir.join("src.json"),
            r#"{"elements":[{"type":"way","id":1,"geometry":[{"lat":34.75,"lon":127.75},{"lat":34.77,"lon":127.75}]}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("course.toml"),
            "source = \"src.json\"\norigin = [34.76, 127.75]\nways = [1]\ntemporary_width_m = 4.0\n",
        )
        .unwrap();
        assert!(load_map_course(&dir.join("course.toml")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 연속 중복점은 하나로 줄이고 길이 0·NaN은 거부하며, 꺾임은 정리 단계에서 거부하지 않는지 확인한다.
    #[test]
    fn polyline_cleanup_handles_degenerate_input() {
        let line = [Vec2::ZERO, Vec2::new(0.0, -0.01), Vec2::new(0.0, -30.0)];
        assert_eq!(clean_polyline(&line).unwrap().len(), 2);
        assert!(clean_polyline(&[Vec2::ONE, Vec2::ONE]).is_err());
        assert!(clean_polyline(&[Vec2::ZERO]).is_err());
        assert!(clean_polyline(&[Vec2::ZERO, Vec2::new(f32::NAN, 0.0)]).is_err());
        let sharp = [Vec2::ZERO, Vec2::new(0.0, -30.0), Vec2::new(30.0, -30.0)];
        assert_eq!(clean_polyline(&sharp).unwrap().len(), 3);
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
        for rect in &obstacles {
            assert!(rect.center.is_finite() && rect.heading_rad.is_finite());
            assert!(rect.half_size.min_element() > 0.0, "{rect:?}");
        }
        for strip in &course.strips {
            assert_eq!(strip.left.len(), strip.right.len());
            assert!(strip.left.iter().chain(&strip.right).all(|p| p.is_finite()));
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

    /// 꺾인 도로는 만들어지고 시작 자세가 비접촉인지, 폭 오류와 시작 자세가 들어가지 않는 짧은 코스는 거부하는지 확인한다.
    #[test]
    fn bent_road_and_invalid_parameters() {
        let line = [Vec2::ZERO, Vec2::new(0.0, -40.0), Vec2::new(5.0, -80.0)];
        let course = build_road_course("bent", &line, 4.0, Vec::new()).unwrap();
        assert!(!in_contact(
            &VEHICLE_FOOTPRINT,
            &course.start,
            &course.collision_rects()
        ));
        // 노면 띠 하나와 양쪽 가장자리 표시선 띠 둘.
        assert_eq!(course.strips.len(), 3);
        assert!(course.surfaces.is_empty());
        assert!(build_road_course("narrow", &line, 1.0, Vec::new()).is_err());
        assert!(build_road_course("nan", &line, f32::NAN, Vec::new()).is_err());
        let short = [Vec2::ZERO, Vec2::new(0.0, -8.0)];
        let error = build_road_course("short", &short, 4.0, Vec::new()).unwrap_err();
        assert!(error.contains("start pose"), "{error}");
    }

    /// 저장소에 고정한 여수 가상 교육 코스(OSM way 532526349)가 읽히고, 출처 안내가 붙고,
    /// 시작 자세가 비접촉이며, 시작 위치에서 조향 없이 직진해도 도중에 막히지 않고 끝 부근까지 가는지 확인한다.
    #[test]
    fn committed_yeosu_course_is_drivable() {
        let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("testdata/yeosu-expo/course.toml");
        let course = load_map_course(&path).unwrap();
        assert!(
            course
                .notices
                .iter()
                .any(|notice| notice == OSM_ATTRIBUTION)
        );
        let obstacles = course.collision_rects();
        assert!(!in_contact(&VEHICLE_FOOTPRINT, &course.start, &obstacles));
        // 첫 노드에서 남서쪽(-X 서쪽, +Z 남쪽)으로 출발한다.
        let (x, z) = course.start.forward();
        assert!(x < 0.0 && z > 0.0, "forward ({x}, {z})");

        let mut state = ready_in(Gear::Drive);
        state.set_input(VehicleInput {
            throttle: 1.0,
            ..VehicleInput::default()
        });
        let mut motion = course.start;
        let mut contact = ContactState::default();
        let params = DynamicsParams::default();
        let impact = (0..64 * 240).find_map(|_| {
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
        // 마지막 노드(위도 34.7488786, 경도 127.7474190)의 지역 좌표 근처에서 멈춘다.
        let end = project(
            LatLon {
                lat: 34.7509700,
                lon: 127.7493575,
            },
            LatLon {
                lat: 34.7488786,
                lon: 127.7474190,
            },
        );
        let (fx, fz) = motion.forward();
        let front = Vec2::new(motion.x_m, motion.z_m)
            + Vec2::new(fx, fz)
                * (VEHICLE_FOOTPRINT.rear_axle_to_center_m + VEHICLE_FOOTPRINT.length_m / 2.0);
        // OSM 중심선은 구간마다 최대 약 0.6° 꺾인다. 조향 없이 첫 구간 방향으로만 달리면 끝까지
        // 약 1 m 옆으로 밀려 끝 부근에서 모서리가 먼저 연석에 닿을 수 있으므로 2 m 안을 기준으로 본다.
        assert!(front.distance(end) < 2.0, "front {front:?}, end {end:?}");
    }

    /// 예전(3-4) 20° 상한을 넘는 꺾임도 넓은 도로에서 필렛으로 만들어지는지 확인한다(연석 틈은 road_geom 시험).
    #[test]
    fn sharp_bend_on_wide_road_is_built_with_fillet() {
        let bend = 60f32.to_radians();
        let line = [
            Vec2::ZERO,
            Vec2::new(0.0, -40.0),
            Vec2::new(0.0, -40.0) + Vec2::new(bend.sin(), -bend.cos()) * 40.0,
        ];
        for width in [4.0, 12.0] {
            let course = build_road_course("wide", &line, width, Vec::new()).unwrap();
            assert!(!in_contact(
                &VEHICLE_FOOTPRINT,
                &course.start,
                &course.collision_rects()
            ));
        }
    }

    /// 코스 노면 띠(첫 띠)의 가운데 선을 경로 추종 조향기로 따라간다.
    fn follow_course(course: &CourseDef, target_speed_mps: f32) -> FollowResult {
        let road = &course.strips[0];
        let path: Vec<Vec2> = road
            .left
            .iter()
            .zip(&road.right)
            .map(|(l, r)| (*l + *r) / 2.0)
            .collect();
        follow_path(course, &path, target_speed_mps)
    }

    /// 띠 메시가 유한한 점만 쓰고, 이웃한 두 쌍의 점이 만드는 사각형이 모두 같은 방향으로 감겨
    /// 뒤집히거나 넓이가 0인 삼각형이 없는지 확인한다.
    fn assert_strips_valid(course: &CourseDef) {
        for strip in &course.strips {
            assert_eq!(strip.left.len(), strip.right.len());
            assert!(strip.left.iter().chain(&strip.right).all(|p| p.is_finite()));
            for i in 0..strip.left.len() - 1 {
                let (l0, r0, l1, r1) = (
                    strip.left[i],
                    strip.right[i],
                    strip.left[i + 1],
                    strip.right[i + 1],
                );
                for (a, b, c) in [(l0, r0, l1), (r0, r1, l1)] {
                    // 왼쪽→오른쪽→앞 순서는 지면 (x, z)에서 늘 음의 방향으로 감긴다. 부호가 바뀌면 뒤집힌 삼각형이다.
                    let area = (b - a).perp_dot(c - a);
                    assert!(
                        area < -1.0e-6,
                        "inverted or empty triangle at {a} (area {area})"
                    );
                }
            }
        }
    }

    /// 합성 자료를 그래프로 읽어 첫 way의 노드 순서대로 지역 좌표 중심선을 만든다.
    fn fixture_line(ways: Vec<crate::world::osm_graph::fixtures::FixtureWay>) -> Vec<Vec2> {
        use crate::world::osm_graph::{fixtures, parse_overpass_graph};
        let parsed = parse_overpass_graph(&fixtures::overpass_json(&ways)).unwrap();
        parsed[0]
            .nodes
            .iter()
            .map(|node| project(fixtures::ORIGIN, node.pos))
            .collect()
    }

    /// 폴리라인 각 구간을 `spacing` m 이하 조각으로 나눈다(정점 간격 바꾸기 시험용).
    fn densify(points: &[Vec2], spacing: f32) -> Vec<Vec2> {
        let mut out = vec![points[0]];
        for pair in points.windows(2) {
            let pieces = (pair[0].distance(pair[1]) / spacing).ceil().max(1.0) as usize;
            for k in 1..=pieces {
                out.push(pair[0].lerp(pair[1], k as f32 / pieces as f32));
            }
        }
        out
    }

    /// 합성 자료: 굽은 길·S자는 만들어져 경로 추종 조향기로 충돌 없이 끝까지 가고,
    /// 넓은 도로+짧은 구간과 처음 길 위로 되돌아오는 길은 거부되는지 확인한다(PLAN 4-1 합성 완료 기준).
    #[test]
    fn synthetic_fixtures_are_built_or_rejected() {
        use crate::world::osm_graph::fixtures;
        for (name, ways) in [
            ("curved", fixtures::curved_road()),
            ("s-curve", fixtures::s_curve()),
        ] {
            let course = build_road_course(name, &fixture_line(ways), 4.0, Vec::new()).unwrap();
            assert_strips_valid(&course);
            let result = follow_course(&course, 4.0);
            assert_eq!(result.impacts, 0, "{name}: {result:?}");
            assert!(result.remaining_m < 12.5, "{name}: {result:?}");
            assert!(result.max_offset_m < 1.0, "{name}: {result:?}");
        }
        let zigzag = fixture_line(fixtures::wide_short_zigzag());
        let error = build_road_course("zigzag", &zigzag, 12.0, Vec::new()).unwrap_err();
        assert!(error.contains("too sharp"), "{error}");
        let looped = fixture_line(fixtures::loop_overlap());
        let error = build_road_course("loop", &looped, 4.0, Vec::new()).unwrap_err();
        assert!(error.contains("overlaps"), "{error}");
    }

    /// 정점 간격(0.25·1·5 m)과 원호 분할 각도(1°·3°·10°)를 바꿔도 정상 코스는 모두 만들어지고,
    /// 실제로 겹치는 길은 어느 설정에서도 거부되는지 확인한다(PLAN 4-1 회귀 기준).
    #[test]
    fn results_do_not_depend_on_sampling() {
        use crate::world::osm_graph::fixtures;
        let normal = [
            fixture_line(fixtures::curved_road()),
            fixture_line(fixtures::s_curve()),
        ];
        let looped = fixture_line(fixtures::loop_overlap());
        for spacing in [0.25, 1.0, 5.0] {
            for step in [1.0, 3.0, 10.0] {
                let spec = RoadSpec {
                    width_m: 4.0,
                    curb_thickness_m: CURB_THICKNESS_M,
                    vehicle_min_radius_m: vehicle_min_radius_m(),
                    arc_step_deg: step,
                };
                for line in &normal {
                    let dense = densify(line, spacing);
                    assert!(
                        build_road_course_with("ok", &dense, &spec, Vec::new()).is_ok(),
                        "spacing {spacing}, step {step}"
                    );
                }
                let dense = densify(&looped, spacing);
                assert!(
                    build_road_course_with("loop", &dense, &spec, Vec::new()).is_err(),
                    "loop accepted at spacing {spacing}, step {step}"
                );
            }
        }
    }

    /// 실제 스냅샷: 문서에 기록한 굽은 경로(way 529495041, 약 321 m)로 폭 4 m 코스를 만들고,
    /// 경로 추종 조향기가 충돌 없이 끝 부근까지 가는지 확인한다(PLAN 4-1 실제 스냅샷 완료 기준).
    #[test]
    fn real_curved_way_is_drivable() {
        use crate::world::osm_graph::parse_overpass_graph;
        let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("testdata/yeosu-expo/graph-2026-11-06.osm.json");
        let ways = parse_overpass_graph(&std::fs::read_to_string(path).unwrap()).unwrap();
        let way = ways.iter().find(|way| way.id == 529_495_041).unwrap();
        let origin = way.nodes[0].pos;
        let line: Vec<Vec2> = way.nodes.iter().map(|n| project(origin, n.pos)).collect();
        let course = build_road_course("way-529495041", &line, 4.0, Vec::new()).unwrap();
        assert_strips_valid(&course);
        // 저장소의 굽은 길 확인 코스 파일도 같은 way로 읽히고 출처 안내가 붙는다.
        let file = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("testdata/yeosu-expo/curved-course.toml");
        let from_file = load_map_course(&file).unwrap();
        assert!(from_file.notices.iter().any(|n| n == OSM_ATTRIBUTION));
        assert_eq!(from_file.start, course.start);
        assert!(!in_contact(
            &VEHICLE_FOOTPRINT,
            &course.start,
            &course.collision_rects()
        ));
        let result = follow_course(&course, 5.0);
        assert_eq!(result.impacts, 0, "{result:?}");
        assert!(result.remaining_m < 12.5, "{result:?}");
        assert!(result.max_offset_m < 1.0, "{result:?}");
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
