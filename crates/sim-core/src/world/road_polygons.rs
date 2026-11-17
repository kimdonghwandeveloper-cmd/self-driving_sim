//! 사용자 제작 맵의 도로 중심선으로 만드는 주행 코스(여수 엑스포 "내 맵 도로", PLAN 4-11b).
//!
//! 사용자가 Blender로 만든 맵의 도로 띠에서 뽑은 중심선·폭(`tools/blender/yeosu_background.py`가 내보낸
//! `road-lines-v1` JSON)을 읽어, 폭을 넓히고(배율·최소 폭) 꺾이는 곳을 둥글게 다듬은 뒤 도로 면 다각형을 만든다.
//! 겹친 면들을 합친 영역의 바깥 경계선을 따라 연석을 세우며, 교차로처럼 면이 겹친 곳의 안쪽 변에는 연석이
//! 생기지 않는다. 막다른 끝은 반원으로 둥글게 하고 연석 없이 열어 둔다(사용자 결정 2026-11-13).
//! 연석은 저속에서 넘을 수 있는 낮은 연석([`ColliderKind::LowCurb`])이다(사용자 결정 2026-11-13).
//!
//! 경계 계산: 각 다각형 변을 다른 변과의 교차점에서 자르고, 조각 가운데에서 양옆으로 [`SIDE_PROBE_M`]만큼
//! 떨어진 두 점 중 한쪽만 도로 영역(다각형 합집합) 안이면 경계로 본다. 연석은 바깥쪽에 붙인다.
//! 바깥쪽 [`GAP_BRIDGE_M`] 안에 다시 도로가 있으면 이음 틈으로 보고 연석을 두지 않는다.
//! 이어지는 경계 조각은 [`CURB_MERGE_TOLERANCE_M`] 안에서 하나로 합쳐 연석 이음매를 줄인다.
//! 좌표는 코스 지면 (x = 동쪽, z = 남쪽) m이다. 배율·최소 폭·둥글게 깎는 길이는 모두 **임시값**이다.

use std::collections::HashMap;
use std::path::Path;

use glam::Vec2;
use serde::Deserialize;

use super::course::{Area, Collider, ColliderKind, CourseDef};
use super::road_geom::{heading_of, right_of};
use crate::vehicle::{GroundRect, Motion, VEHICLE_FOOTPRINT};

/// 코스 파일(TOML)과 중심선 자료(JSON)의 형식 이름.
pub const ROAD_LINES_FORMAT: &str = "road-lines-v1";
/// 연석 두께(m)와 표시 높이(m). 지도 코스와 같은 값.
const CURB_THICKNESS_M: f32 = super::map::CURB_THICKNESS_M;
const CURB_HEIGHT_M: f32 = 0.25;
/// 연석 직사각형을 변 양 끝으로 더 늘리는 길이(m). 이웃 연석과의 틈을 덮는다.
const CURB_OVERLAP_M: f32 = 0.02;
/// 이어지는 경계 조각을 하나의 연석으로 합칠 때 허용하는 직선과의 거리(m).
const CURB_MERGE_TOLERANCE_M: f32 = 0.03;
/// 경계 판정에 쓰는 변 양옆 탐침 거리(m).
const SIDE_PROBE_M: f32 = 0.05;
/// 경계 바깥쪽으로 이 거리(m) 안에 다시 도로가 있으면 틈으로 보고 연석을 두지 않는다.
/// 이보다 좁게 떨어진 나란한 두 도로는 사이가 막히지 않는다.
const GAP_BRIDGE_M: f32 = 1.0;
/// 이보다 짧은 경계 조각(m)은 버린다.
const MIN_SEGMENT_M: f32 = 0.02;
/// 다각형 넓이가 이보다 작으면(m²) 버린다.
const MIN_POLYGON_AREA_M2: f32 = 0.01;
/// 공간 격자 칸 크기(m).
const GRID_CELL_M: f32 = 20.0;
/// 점이 원점에서 떨어질 수 있는 최대 거리(m)와 중심선 개수 상한.
const MAX_DISTANCE_M: f32 = 5_000.0;
const MAX_LINES: usize = 10_000;
/// 넓힌 폭의 허용 범위(m).
const WIDTH_LIMIT_M: std::ops::RangeInclusive<f32> = 1.0..=30.0;
/// 중심선을 둥글게 다듬는 모서리 깎기 반복 횟수, 깎는 비율(구간 길이 대비)과 최대 길이(m, 임시값).
const SMOOTH_ITERATIONS: usize = 4;
const SMOOTH_CUT_RATIO: f32 = 0.25;
const SMOOTH_MAX_CUT_M: f32 = 8.0;
/// 이보다 작은 꺾임(rad)은 깎지 않는다.
const SMOOTH_MIN_TURN_RAD: f32 = 0.02;
/// 이보다 가까운 연속 점(m)은 하나로 합친다.
const MIN_POINT_SPACING_M: f32 = 0.05;
/// 막다른 끝 반원을 나누는 조각 수.
const CAP_SEGMENTS: usize = 12;
/// 두 도로의 끝점이 이 거리(m) 안이면 한 도로로 잇는다(맵에서 way가 끝끼리 이어진 곳).
const JOIN_TOLERANCE_M: f32 = 1.0;
/// 끝점이 다른 도로 면에서 이 거리(m) 안이면 막다른 끝이 아니다(연석을 두고 둥근 이음으로 쓴다).
const DEAD_END_CLEARANCE_M: f32 = 0.5;
/// 다듬은 중심선에서 직선과 이 거리(m) 안인 사이 점은 지운다.
const SMOOTH_SIMPLIFY_M: f32 = 0.05;

/// 맵 도로 코스 파일(TOML). 중심선 자료는 따로 둔다.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadLinesCourseFile {
    /// 형식 이름([`ROAD_LINES_FORMAT`]).
    pub format: String,
    /// 로그에 쓰는 코스 이름.
    pub name: String,
    /// 중심선 자료 JSON 경로. 이 TOML 파일 위치 기준 상대 경로다.
    pub lines: String,
    /// 맵 폭에 곱하는 배율(임시값).
    pub width_scale: f32,
    /// 넓힌 뒤의 최소 폭(m, 임시값).
    pub min_width_m: f32,
    /// 시작 자세의 후륜축 중심 [x, z](m).
    pub start: [f32; 2],
    /// 시작 방향(deg, +Y축 회전, 0이면 -Z(북쪽)를 본다).
    pub start_heading_deg: f32,
    /// 주행 화면에 계속 표시할 안내.
    pub notices: Vec<String>,
}

/// 중심선 자료(JSON).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoadLinesData {
    /// 형식 이름([`ROAD_LINES_FORMAT`]).
    format: String,
    /// 좌표 원점 [위도, 경도](deg). 기록용이며 변환에는 쓰지 않는다.
    #[allow(dead_code)]
    origin: [f64; 2],
    /// 만든 곳 설명. 기록용.
    #[allow(dead_code)]
    source: String,
    /// 도로 중심선 목록.
    lines: Vec<RoadLineData>,
}

/// 중심선 자료의 도로 하나.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoadLineData {
    /// 중심선 [x, z] 점(진행 순서, 2개 이상).
    points: Vec<[f32; 2]>,
    /// 맵의 도로 폭(m).
    width_m: f32,
    /// 닫힌 고리(회전 교차로 등)이면 `true`.
    closed: bool,
}

/// 코스를 만들 도로 하나: 중심선, 넓힌 폭, 닫힌 고리 여부.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadLine {
    /// 중심선 점(진행 순서).
    pub points: Vec<Vec2>,
    /// 노면 폭(m).
    pub width_m: f32,
    /// 닫힌 고리이면 `true`.
    pub closed: bool,
}

/// 도로 영역 경계 조각 하나: 양 끝과 바깥쪽(도로 밖) 단위 법선.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundarySegment {
    /// 시작점.
    pub a: Vec2,
    /// 끝점.
    pub b: Vec2,
    /// 도로 밖을 향하는 단위 법선.
    pub outward: Vec2,
}

/// 코스 파일 내용과 그 파일이 있는 폴더로 맵 도로 코스를 만든다.
pub fn load_road_lines_course(text: &str, base_dir: &Path) -> Result<CourseDef, String> {
    let file: RoadLinesCourseFile =
        toml::from_str(text).map_err(|error| format!("invalid road lines course: {error}"))?;
    if file.format != ROAD_LINES_FORMAT {
        return Err(format!("unsupported format {:?}", file.format));
    }
    if !(file.width_scale.is_finite() && file.width_scale > 0.0)
        || !WIDTH_LIMIT_M.contains(&file.min_width_m)
    {
        return Err(format!(
            "width_scale must be positive and min_width_m within {WIDTH_LIMIT_M:?} m"
        ));
    }
    let path = base_dir.join(&file.lines);
    let json = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let lines = parse_lines(&json, file.width_scale, file.min_width_m)?;
    let [x, z] = file.start;
    let start = Motion {
        x_m: x,
        z_m: z,
        heading_rad: file.start_heading_deg.to_radians(),
        ..Motion::default()
    };
    let mut notices = file.notices;
    notices.push(format!(
        "도로 폭은 맵 폭 x{:.1}(최소 {:.1} m)로 넓힌 훈련용 임시값입니다",
        file.width_scale, file.min_width_m
    ));
    build_road_lines_course(&file.name, &lines, start, notices)
}

/// 중심선 자료 JSON을 읽어 검사하고(형식, 점 개수, 유한·거리 범위) 폭을 `scale`배, 최소 `min_width_m`로 넓힌다.
fn parse_lines(json: &str, scale: f32, min_width_m: f32) -> Result<Vec<RoadLine>, String> {
    let data: RoadLinesData =
        serde_json::from_str(json).map_err(|error| format!("invalid road lines: {error}"))?;
    if data.format != ROAD_LINES_FORMAT {
        return Err(format!("unsupported lines format {:?}", data.format));
    }
    if data.lines.is_empty() || data.lines.len() > MAX_LINES {
        return Err(format!(
            "road line count {} is outside 1..={MAX_LINES}",
            data.lines.len()
        ));
    }
    let mut lines = Vec::with_capacity(data.lines.len());
    for (index, raw) in data.lines.iter().enumerate() {
        let points: Vec<Vec2> = raw.points.iter().map(|&[x, z]| Vec2::new(x, z)).collect();
        if !points
            .iter()
            .all(|p| p.is_finite() && p.length() <= MAX_DISTANCE_M)
        {
            return Err(format!(
                "line {index} has a non-finite point or one beyond {MAX_DISTANCE_M} m"
            ));
        }
        if !(raw.width_m.is_finite() && raw.width_m > 0.0) {
            return Err(format!("line {index} has an invalid width {}", raw.width_m));
        }
        let width_m = (raw.width_m * scale).max(min_width_m);
        if !WIDTH_LIMIT_M.contains(&width_m) {
            return Err(format!(
                "line {index} width {width_m:.1} m is outside {WIDTH_LIMIT_M:?} m"
            ));
        }
        let points = dedup_points(&points, raw.closed);
        // 맵에서 길이가 거의 0인 띠(점 1개)는 도로로 쓰지 않는다.
        if points.len() < 2 {
            continue;
        }
        let closed = raw.closed && points.len() >= 3;
        lines.push(RoadLine {
            points,
            width_m,
            closed,
        });
    }
    Ok(lines)
}

/// 연속 중복점(닫힌 고리는 끝과 처음 사이 포함)을 지운다.
fn dedup_points(points: &[Vec2], closed: bool) -> Vec<Vec2> {
    let mut result: Vec<Vec2> = Vec::with_capacity(points.len());
    for &point in points {
        if result
            .last()
            .is_none_or(|last| last.distance(point) >= MIN_POINT_SPACING_M)
        {
            result.push(point);
        }
    }
    if closed
        && result.len() > 2
        && result[0].distance(result[result.len() - 1]) < MIN_POINT_SPACING_M
    {
        result.pop();
    }
    result
}

/// 중심선들을 둥글게 다듬고 넓힌 도로 면으로 만들어 노면·연석·시작 자세를 갖춘 코스를 만든다.
pub fn build_road_lines_course(
    name: &str,
    lines: &[RoadLine],
    start: Motion,
    notices: Vec<String>,
) -> Result<CourseDef, String> {
    let smoothed: Vec<RoadLine> = join_lines(lines)
        .into_iter()
        .map(|line| RoadLine {
            points: thin_line(&smooth_line(&line.points, line.closed), line.closed),
            ..line
        })
        .collect();
    let mut polygons = Vec::new();
    let mut open = Vec::new();
    for (index, line) in smoothed.iter().enumerate() {
        let dead_ends = if line.closed {
            [false, false]
        } else {
            let ends = [line.points[0], line.points[line.points.len() - 1]];
            ends.map(|end| is_dead_end(end, index, &smoothed))
        };
        for (polygon, cap_end) in road_shapes(&line.points, line.closed, line.width_m) {
            polygons.push(polygon);
            open.push(cap_end.is_some_and(|end| dead_ends[end]));
        }
    }
    build_course_from_polygons(name, &polygons, &open, start, notices)
}

/// 도로 면 다각형으로 코스를 만든다. 모든 바깥 경계에 연석을 세운다(시험·다른 자료용).
pub fn build_road_polygons_course(
    name: &str,
    polygons: &[Vec<Vec2>],
    start: Motion,
    notices: Vec<String>,
) -> Result<CourseDef, String> {
    build_course_from_polygons(name, polygons, &vec![false; polygons.len()], start, notices)
}

/// 도로 면 다각형과 "열린 끝" 표시로 노면(면)·연석·시작 자세를 갖춘 코스를 만든다.
/// 열린 끝 다각형의 바깥 경계에는 연석을 두지 않는다. 시작 자세가 도로 밖이거나 연석에 닿으면 오류.
fn build_course_from_polygons(
    name: &str,
    polygons: &[Vec<Vec2>],
    open: &[bool],
    start: Motion,
    notices: Vec<String>,
) -> Result<CourseDef, String> {
    let index = PolygonIndex::new(polygons);
    if !index.contains(Vec2::new(start.x_m, start.z_m)) {
        return Err("start pose is not on a road".to_owned());
    }
    let segments = merge_segments(&union_boundary(polygons, open, &index));
    let colliders: Vec<Collider> = segments
        .iter()
        .map(|segment| Collider {
            rect: curb_rect(segment),
            height_m: CURB_HEIGHT_M,
            kind: ColliderKind::LowCurb,
        })
        .collect();
    let rects: Vec<GroundRect> = colliders.iter().map(|collider| collider.rect).collect();
    if crate::vehicle::collision::in_contact(&VEHICLE_FOOTPRINT, &start, &rects) {
        return Err("start pose touches a curb".to_owned());
    }
    Ok(CourseDef {
        name: name.to_owned(),
        notices,
        start,
        goal: None,
        surfaces: Vec::new(),
        strips: Vec::new(),
        areas: polygons
            .iter()
            .flat_map(|polygon| areas_of(polygon))
            .collect(),
        markings: Vec::new(),
        colliders,
        buildings: Vec::new(),
    })
}

/// 끝점끼리 [`JOIN_TOLERANCE_M`] 안에서 딱 둘만 만나는 도로들을 한 도로로 잇는다(방향을 맞춰 뒤집기도 한다).
/// 이어진 끝이 처음으로 돌아오면 닫힌 고리가 된다. 폭은 이어진 도로 중 가장 넓은 값을 쓴다.
pub fn join_lines(lines: &[RoadLine]) -> Vec<RoadLine> {
    // 끝점 (도로 번호, 0 = 시작 / 1 = 끝).
    let end_point = |(line, end): (usize, usize)| {
        let points = &lines[line].points;
        if end == 0 {
            points[0]
        } else {
            points[points.len() - 1]
        }
    };
    let ends: Vec<(usize, usize)> = (0..lines.len())
        .filter(|&i| !lines[i].closed)
        .flat_map(|i| [(i, 0), (i, 1)])
        .collect();
    // 각 끝과 짝지을 수 있는 다른 끝(그 자리에 끝이 정확히 둘일 때만).
    let partner = |end: (usize, usize)| {
        let here = end_point(end);
        let near: Vec<(usize, usize)> = ends
            .iter()
            .copied()
            .filter(|&other| other != end && end_point(other).distance(here) <= JOIN_TOLERANCE_M)
            .collect();
        (near.len() == 1 && near[0].0 != end.0).then(|| near[0])
    };
    let mut used = vec![false; lines.len()];
    let mut joined = Vec::new();
    for first in 0..lines.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let mut line = lines[first].clone();
        if !line.closed {
            // 시작 쪽으로 더 이어지는 도로가 있으면 거꾸로 붙인다.
            for side in [1, 0] {
                let mut end = (first, side);
                while let Some((other, other_end)) = partner(end) {
                    if used[other] || partner((other, other_end)) != Some(end) {
                        break;
                    }
                    used[other] = true;
                    let mut points = lines[other].points.clone();
                    // 붙일 도로의 이어지는 끝이 우리 쪽에 오도록 방향을 맞춘다.
                    if (side == 1) == (other_end == 1) {
                        points.reverse();
                    }
                    if side == 1 {
                        line.points.extend(points.into_iter().skip(1));
                    } else {
                        points.pop();
                        points.extend(line.points);
                        line.points = points;
                    }
                    line.width_m = line.width_m.max(lines[other].width_m);
                    end = (other, 1 - other_end);
                }
            }
            let n = line.points.len();
            if n > 3 && line.points[0].distance(line.points[n - 1]) <= JOIN_TOLERANCE_M {
                line.points.pop();
                line.closed = true;
            }
        }
        joined.push(line);
    }
    joined
}

/// 도로 `index`의 끝점 `end`가 다른 도로 면(폭/2 + [`DEAD_END_CLEARANCE_M`]) 밖이면 막다른 끝이다.
fn is_dead_end(end: Vec2, index: usize, lines: &[RoadLine]) -> bool {
    !lines.iter().enumerate().any(|(other, line)| {
        if other == index {
            return false;
        }
        let reach = line.width_m / 2.0 + DEAD_END_CLEARANCE_M;
        let n = line.points.len();
        let count = if line.closed { n } else { n - 1 };
        (0..count).any(|i| distance_to_line(end, line.points[i], line.points[(i + 1) % n]) <= reach)
    })
}

/// 다듬은 중심선에서 앞뒤 점을 잇는 직선과 [`SMOOTH_SIMPLIFY_M`] 안인 사이 점을 지운다(닫힌 고리는 첫 점을 남긴다).
fn thin_line(points: &[Vec2], closed: bool) -> Vec<Vec2> {
    let mut result = vec![points[0]];
    let mut anchor = 0;
    for i in 1..points.len() {
        let next = if i + 1 < points.len() {
            points[i + 1]
        } else if closed {
            points[0]
        } else {
            result.push(points[i]);
            break;
        };
        let keep = points[anchor + 1..=i]
            .iter()
            .any(|p| distance_to_line(*p, points[anchor], next) > SMOOTH_SIMPLIFY_M);
        if keep {
            result.push(points[i]);
            anchor = i;
        }
    }
    result
}

/// 중심선 꺾임을 모서리 깎기(Chaikin)로 둥글게 다듬는다. 한 번에 꼭짓점 양옆 구간의
/// [`SMOOTH_CUT_RATIO`](최대 [`SMOOTH_MAX_CUT_M`])만큼 물러난 두 점으로 바꾸며, 열린 선의 양 끝은 그대로 둔다.
pub fn smooth_line(points: &[Vec2], closed: bool) -> Vec<Vec2> {
    let mut current = points.to_vec();
    for _ in 0..SMOOTH_ITERATIONS {
        let n = current.len();
        if n < 3 {
            break;
        }
        let mut next = Vec::with_capacity(n * 2);
        for i in 0..n {
            let interior = closed || (i > 0 && i < n - 1);
            if !interior {
                next.push(current[i]);
                continue;
            }
            let (prev, point, after) = (current[(i + n - 1) % n], current[i], current[(i + 1) % n]);
            let (into, out) = (point - prev, after - point);
            let (len_in, len_out) = (into.length(), out.length());
            if len_in <= 0.0 || len_out <= 0.0 || into.angle_to(out).abs() < SMOOTH_MIN_TURN_RAD {
                next.push(point);
                continue;
            }
            let cut_in = (len_in * SMOOTH_CUT_RATIO).min(SMOOTH_MAX_CUT_M);
            let cut_out = (len_out * SMOOTH_CUT_RATIO).min(SMOOTH_MAX_CUT_M);
            next.push(point - into / len_in * cut_in);
            next.push(point + out / len_out * cut_out);
        }
        current = dedup_points(&next, closed);
    }
    current
}

/// 다듬은 중심선과 폭으로 도로 면 다각형들을 만든다: 구간마다 직사각형, 꺾이는 점의 바깥쪽 틈을 메우는 삼각형,
/// 열린 선의 양 끝 반원(`Some(0)` 시작 / `Some(1)` 끝). 면끼리 겹쳐도 된다(합집합으로 다룬다).
fn road_shapes(points: &[Vec2], closed: bool, width_m: f32) -> Vec<(Vec<Vec2>, Option<usize>)> {
    let half = width_m / 2.0;
    let n = points.len();
    let segment_count = if closed { n } else { n - 1 };
    let direction = |i: usize| (points[(i + 1) % n] - points[i]).normalize();
    let mut shapes =
        Vec::with_capacity(segment_count + if closed { n } else { n.saturating_sub(2) } + 2);
    for i in 0..segment_count {
        let (a, b) = (points[i], points[(i + 1) % n]);
        let side = right_of(direction(i)) * half;
        shapes.push((vec![a - side, b - side, b + side, a + side], None));
    }
    let joints_range = if closed { 0..n } else { 1..n - 1 };
    for i in joints_range {
        let before = direction((i + segment_count - 1) % segment_count);
        let after = direction(i % segment_count);
        let turn = before.angle_to(after);
        if turn.abs() < 1.0e-3 {
            continue;
        }
        // right_of 쪽으로 꺾이면(각이 양수) 반대쪽이 바깥이다.
        let outer = if turn > 0.0 { -1.0 } else { 1.0 };
        let p = points[i];
        shapes.push((
            vec![
                p,
                p + right_of(before) * outer * half,
                p + right_of(after) * outer * half,
            ],
            None,
        ));
    }
    if !closed {
        shapes.push((cap(points[0], -direction(0), half), Some(0)));
        shapes.push((cap(points[n - 1], direction(n - 2), half), Some(1)));
    }
    shapes
}

/// 끝점 `end`에서 바깥 방향 `outward`로 볼록한 반지름 `radius`의 반원 다각형을 만든다.
fn cap(end: Vec2, outward: Vec2, radius: f32) -> Vec<Vec2> {
    let side = right_of(outward);
    let mut points = Vec::with_capacity(CAP_SEGMENTS + 1);
    for k in 0..=CAP_SEGMENTS {
        let angle = std::f32::consts::PI * (k as f32 / CAP_SEGMENTS as f32 - 0.5);
        points.push(end + (outward * angle.cos() + side * angle.sin()) * radius);
    }
    points
}

/// 경계 조각 바깥쪽에 붙는 연석 직사각형을 만든다(긴 축이 조각 방향).
fn curb_rect(segment: &BoundarySegment) -> GroundRect {
    let length = segment.a.distance(segment.b);
    let forward = (segment.b - segment.a) / length;
    GroundRect {
        center: (segment.a + segment.b) / 2.0 + segment.outward * (CURB_THICKNESS_M / 2.0),
        half_size: Vec2::new(CURB_THICKNESS_M / 2.0, length / 2.0 + CURB_OVERLAP_M),
        heading_rad: heading_of(forward),
    }
}

/// 이어지는 경계 조각들을 사슬로 묶고, 직선에서 [`CURB_MERGE_TOLERANCE_M`] 안에 드는 구간을 하나로 합친다.
/// 모든 조각을 바깥이 right_of 쪽이 되게 돌려 놓은 뒤 끝점(1 mm 단위)으로 다음 조각을 찾는다.
pub fn merge_segments(segments: &[BoundarySegment]) -> Vec<BoundarySegment> {
    let key = |p: Vec2| ((p.x * 1000.0).round() as i64, (p.y * 1000.0).round() as i64);
    let oriented: Vec<(Vec2, Vec2)> = segments
        .iter()
        .map(|s| {
            if right_of(s.b - s.a).dot(s.outward) >= 0.0 {
                (s.a, s.b)
            } else {
                (s.b, s.a)
            }
        })
        .collect();
    let mut starts: HashMap<(i64, i64), Vec<usize>> = HashMap::with_capacity(oriented.len());
    for (index, (a, _)) in oriented.iter().enumerate() {
        starts.entry(key(*a)).or_default().push(index);
    }
    let mut has_previous = vec![false; oriented.len()];
    for (_, b) in &oriented {
        for &index in starts.get(&key(*b)).into_iter().flatten() {
            has_previous[index] = true;
        }
    }
    // 앞 조각이 없는 사슬 시작부터 걷고, 남은 것(고리)은 그다음에 걷는다.
    let order = (0..oriented.len())
        .filter(|&i| !has_previous[i])
        .chain((0..oriented.len()).filter(|&i| has_previous[i]));
    let mut used = vec![false; oriented.len()];
    let mut merged = Vec::new();
    for first in order {
        if used[first] {
            continue;
        }
        used[first] = true;
        let mut chain = vec![oriented[first].0, oriented[first].1];
        while let Some(&next) = starts
            .get(&key(chain[chain.len() - 1]))
            .and_then(|candidates| candidates.iter().find(|&&i| !used[i]))
        {
            used[next] = true;
            chain.push(oriented[next].1);
        }
        for (a, b) in simplify(&chain) {
            if a.distance(b) >= MIN_SEGMENT_M {
                merged.push(BoundarySegment {
                    a,
                    b,
                    outward: right_of((b - a).normalize()),
                });
            }
        }
    }
    merged
}

/// 꺾은선을 앞에서부터 이어 붙이며, 사이 점들이 직선에서 [`CURB_MERGE_TOLERANCE_M`] 안이면 한 구간으로 합친다.
fn simplify(chain: &[Vec2]) -> Vec<(Vec2, Vec2)> {
    let mut pieces = Vec::new();
    let mut start = 0;
    while start + 1 < chain.len() {
        let mut end = start + 1;
        while end + 1 < chain.len()
            && (chain[end + 1] - chain[end]).dot(chain[end] - chain[start]) > 0.0
            && chain[start + 1..=end].iter().all(|p| {
                distance_to_line(*p, chain[start], chain[end + 1]) <= CURB_MERGE_TOLERANCE_M
            })
        {
            end += 1;
        }
        pieces.push((chain[start], chain[end]));
        start = end;
    }
    pieces
}

/// 점 `p`와 선분 a-b 사이 거리(m).
fn distance_to_line(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

/// 다각형들의 합집합 영역 바깥 경계 조각을 구한다. 겹친 면 안쪽 변, 맞닿은 공유 변, 좁은 틈의 변,
/// 열린 끝(`open[다각형]`) 다각형의 변은 빠진다.
pub fn union_boundary(
    polygons: &[Vec<Vec2>],
    open: &[bool],
    index: &PolygonIndex,
) -> Vec<BoundarySegment> {
    let mut segments = Vec::new();
    for (owner, polygon) in polygons.iter().enumerate() {
        if open[owner] {
            continue;
        }
        for i in 0..polygon.len() {
            let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            let length = a.distance(b);
            if length < MIN_SEGMENT_M {
                continue;
            }
            // 이 변을 다른 변과의 교차점에서 자른다.
            let mut cuts = vec![0.0, 1.0];
            for (other, j) in index.edges_near(a, b) {
                if other == owner
                    && (j == i || (j + 1) % polygon.len() == i || (i + 1) % polygon.len() == j)
                {
                    continue;
                }
                let target = &polygons[other];
                let (c, d) = (target[j], target[(j + 1) % target.len()]);
                if let Some(t) = crossing(a, b, c, d) {
                    cuts.push(t);
                }
            }
            cuts.sort_by(f32::total_cmp);
            let normal = right_of((b - a) / length);
            for pair in cuts.windows(2) {
                let (p, q) = (a.lerp(b, pair[0]), a.lerp(b, pair[1]));
                if p.distance(q) < MIN_SEGMENT_M {
                    continue;
                }
                let middle = (p + q) / 2.0;
                let right_in = index.contains(middle + normal * SIDE_PROBE_M);
                let left_in = index.contains(middle - normal * SIDE_PROBE_M);
                if right_in != left_in {
                    let outward = if right_in { -normal } else { normal };
                    if index.contains(middle + outward * GAP_BRIDGE_M) {
                        continue;
                    }
                    segments.push(BoundarySegment {
                        a: p,
                        b: q,
                        outward,
                    });
                }
            }
        }
    }
    segments
}

/// 선분 a-b와 c-d가 엇갈리면 a-b 위의 비율(0..1, 양 끝 제외)을 돌려준다. 평행하면 `None`.
fn crossing(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<f32> {
    let (r, s) = (b - a, d - c);
    let denominator = r.perp_dot(s);
    if denominator.abs() < 1.0e-9 {
        return None;
    }
    let t = (c - a).perp_dot(s) / denominator;
    let u = (c - a).perp_dot(r) / denominator;
    (t > 0.0 && t < 1.0 && (0.0..=1.0).contains(&u)).then_some(t)
}

/// 다각형의 부호 있는 넓이(m², perp_dot 기준 반시계가 양수).
fn signed_area(points: &[Vec2]) -> f32 {
    let n = points.len();
    (0..n)
        .map(|i| points[i].perp_dot(points[(i + 1) % n]))
        .sum::<f32>()
        / 2.0
}

/// 다각형을 표시용 면(부채꼴이 뒤집히지 않는 반시계 점 순서)으로 바꾼다. 볼록이면 하나, 아니면 귀 자르기 삼각형들.
fn areas_of(polygon: &[Vec2]) -> Vec<Area> {
    let mut points = polygon.to_vec();
    if signed_area(&points) < 0.0 {
        points.reverse();
    }
    let n = points.len();
    let convex = (0..n).all(|i| {
        let (p, q, r) = (points[i], points[(i + 1) % n], points[(i + 2) % n]);
        (q - p).perp_dot(r - q) > 0.0
    });
    if convex {
        let center = points.iter().copied().sum::<Vec2>() / n as f32;
        return vec![Area {
            center,
            boundary: points,
        }];
    }
    triangulate(&points)
        .into_iter()
        .map(|[a, b, c]| Area {
            center: (a + b + c) / 3.0,
            boundary: vec![a, b, c],
        })
        .collect()
}

/// 반시계 단순 다각형을 귀 자르기로 삼각형들로 나눈다. 넓이가 거의 0인 삼각형은 뺀다.
fn triangulate(points: &[Vec2]) -> Vec<[Vec2; 3]> {
    let mut remaining: Vec<Vec2> = points.to_vec();
    let mut triangles = Vec::with_capacity(points.len().saturating_sub(2));
    while remaining.len() > 3 {
        let n = remaining.len();
        let ear = (0..n).find(|&i| {
            let (a, b, c) = (
                remaining[(i + n - 1) % n],
                remaining[i],
                remaining[(i + 1) % n],
            );
            (b - a).perp_dot(c - b) > 0.0
                && remaining
                    .iter()
                    .filter(|p| **p != a && **p != b && **p != c)
                    .all(|p| !in_triangle(*p, a, b, c))
        });
        // 귀를 못 찾으면(자기 교차 등) 남은 점을 부채꼴로 처리한다.
        let i = ear.unwrap_or(0);
        let (a, b, c) = (
            remaining[(i + n - 1) % n],
            remaining[i],
            remaining[(i + 1) % n],
        );
        triangles.push([a, b, c]);
        remaining.remove(i);
    }
    triangles.push([remaining[0], remaining[1], remaining[2]]);
    triangles
        .into_iter()
        .filter(|[a, b, c]| (b - a).perp_dot(c - a) > 2.0 * MIN_POLYGON_AREA_M2)
        .collect()
}

/// 점 `p`가 반시계 삼각형 a-b-c 안(경계 포함)에 있는지 본다.
fn in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    (b - a).perp_dot(p - a) >= 0.0
        && (c - b).perp_dot(p - b) >= 0.0
        && (a - c).perp_dot(p - c) >= 0.0
}

/// 짝홀 규칙으로 점이 다각형 안에 있는지 본다.
fn polygon_contains(polygon: &[Vec2], point: Vec2) -> bool {
    let n = polygon.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + n - 1) % n]);
        if (a.y > point.y) != (b.y > point.y) {
            let x = a.x + (point.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if point.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// 다각형과 변을 찾기 위한 공간 격자.
pub struct PolygonIndex<'a> {
    /// 원본 다각형.
    polygons: &'a [Vec<Vec2>],
    /// 칸 → 그 칸과 범위가 겹치는 다각형 번호.
    polygon_cells: HashMap<(i32, i32), Vec<usize>>,
    /// 칸 → 그 칸과 범위가 겹치는 (다각형 번호, 변 번호).
    edge_cells: HashMap<(i32, i32), Vec<(usize, usize)>>,
}

impl<'a> PolygonIndex<'a> {
    /// 다각형 목록으로 격자를 만든다.
    pub fn new(polygons: &'a [Vec<Vec2>]) -> Self {
        let mut polygon_cells: HashMap<(i32, i32), Vec<usize>> =
            HashMap::with_capacity(polygons.len() * 2);
        let mut edge_cells: HashMap<(i32, i32), Vec<(usize, usize)>> =
            HashMap::with_capacity(polygons.len() * 8);
        for (owner, polygon) in polygons.iter().enumerate() {
            let low = polygon.iter().copied().fold(Vec2::INFINITY, Vec2::min);
            let high = polygon.iter().copied().fold(Vec2::NEG_INFINITY, Vec2::max);
            for cell in cells_between(low, high) {
                polygon_cells.entry(cell).or_default().push(owner);
            }
            for i in 0..polygon.len() {
                let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
                for cell in cells_between(a.min(b), a.max(b)) {
                    edge_cells.entry(cell).or_default().push((owner, i));
                }
            }
        }
        Self {
            polygons,
            polygon_cells,
            edge_cells,
        }
    }

    /// 점이 어느 다각형 안(합집합 영역)에 있는지 본다.
    pub fn contains(&self, point: Vec2) -> bool {
        self.polygon_cells
            .get(&cell_of(point))
            .is_some_and(|owners| {
                owners
                    .iter()
                    .any(|&owner| polygon_contains(&self.polygons[owner], point))
            })
    }

    /// 선분 a-b의 범위와 겹치는 칸에 든 변(다각형 번호, 변 번호)을 중복 없이 돌려준다.
    fn edges_near(&self, a: Vec2, b: Vec2) -> Vec<(usize, usize)> {
        let mut edges: Vec<(usize, usize)> = cells_between(a.min(b), a.max(b))
            .filter_map(|cell| self.edge_cells.get(&cell))
            .flatten()
            .copied()
            .collect();
        edges.sort_unstable();
        edges.dedup();
        edges
    }
}

/// 점이 속한 격자 칸.
fn cell_of(point: Vec2) -> (i32, i32) {
    (
        (point.x / GRID_CELL_M).floor() as i32,
        (point.y / GRID_CELL_M).floor() as i32,
    )
}

/// 범위 low..high와 겹치는 격자 칸들.
fn cells_between(low: Vec2, high: Vec2) -> impl Iterator<Item = (i32, i32)> {
    let (x0, z0) = cell_of(low);
    let (x1, z1) = cell_of(high);
    (x0..=x1).flat_map(move |x| (z0..=z1).map(move |z| (x, z)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::collision::{ContactState, step_with_obstacles};
    use crate::vehicle::{DynamicsParams, Gear, VehicleCommand, VehicleInput, VehicleState};

    /// 축 정렬 직사각형 다각형(x0..x1, z0..z1)을 만든다.
    fn rect(x0: f32, z0: f32, x1: f32, z1: f32) -> Vec<Vec2> {
        vec![
            Vec2::new(x0, z0),
            Vec2::new(x1, z0),
            Vec2::new(x1, z1),
            Vec2::new(x0, z1),
        ]
    }

    /// 십자 교차로(가로·세로 띠 두 개)의 경계에는 교차 안쪽 변이 없고, 둘레 길이가 맞다.
    #[test]
    fn crossing_strips_leave_junction_open() {
        let polygons = vec![rect(-20.0, -3.0, 20.0, 3.0), rect(-3.0, -20.0, 3.0, 20.0)];
        let index = PolygonIndex::new(&polygons);
        let boundary = union_boundary(&polygons, &[false; 2], &index);
        let total: f32 = boundary.iter().map(|s| s.a.distance(s.b)).sum();
        // 십자 바깥 둘레: 팔 4개 × (17 + 6 + 17) = 160 m.
        assert!((total - 160.0).abs() < 0.01, "boundary length {total}");
        for segment in &boundary {
            let middle = (segment.a + segment.b) / 2.0;
            assert!(
                !(middle.x.abs() < 2.9 && middle.y.abs() < 2.9),
                "junction edge {segment:?}"
            );
            assert!(!index.contains(middle + segment.outward * 0.1));
            assert!(index.contains(middle - segment.outward * 0.1));
        }
    }

    /// 한 띠를 잘게 나눈 면들 사이의 공유 변은 경계가 아니다.
    #[test]
    fn shared_edges_are_interior() {
        let polygons = vec![rect(0.0, 0.0, 10.0, 4.0), rect(10.0, 0.0, 20.0, 4.0)];
        let index = PolygonIndex::new(&polygons);
        let boundary = union_boundary(&polygons, &[false; 2], &index);
        let total: f32 = boundary.iter().map(|s| s.a.distance(s.b)).sum();
        assert!((total - 48.0).abs() < 0.01, "boundary length {total}");
    }

    /// 띠 끝이 비스듬히 만나 생긴 V자 틈과 0.3 m 틈에는 도로를 가로지르는 연석이 없다.
    #[test]
    fn narrow_gaps_between_pieces_stay_open() {
        // 0.3 m 틈을 두고 이어지는 두 띠.
        let gapped = vec![rect(0.0, 0.0, 10.0, 6.0), rect(10.3, 0.0, 20.0, 6.0)];
        // 위쪽이 0.6 m 벌어진 V자 틈(아래쪽은 맞닿음).
        let wedge = vec![
            rect(0.0, 0.0, 10.0, 6.0),
            vec![
                Vec2::new(10.0, 6.0),
                Vec2::new(20.0, 6.0),
                Vec2::new(20.0, 0.0),
                Vec2::new(10.6, 0.0),
            ],
        ];
        for polygons in [gapped, wedge] {
            let index = PolygonIndex::new(&polygons);
            for segment in union_boundary(&polygons, &[false; 2], &index) {
                let middle = (segment.a + segment.b) / 2.0;
                assert!(
                    !(9.9..=10.7).contains(&middle.x) || middle.y <= 0.0 || middle.y >= 6.0,
                    "curb across the gap: {segment:?}"
                );
            }
        }
    }

    /// 오목 다각형도 반시계 삼각형 면으로 나뉘어 넓이가 보존되고 코스 검사를 통과하며, 도로 밖 시작은 거부한다.
    #[test]
    fn concave_polygons_become_valid_areas() {
        let l_shape = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, 10.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(10.0, 6.0),
            Vec2::new(4.0, 6.0),
            Vec2::new(4.0, 0.0),
        ];
        let areas = areas_of(&l_shape);
        let total: f32 = areas.iter().map(|a| signed_area(&a.boundary)).sum();
        assert!((total - signed_area(&l_shape).abs()).abs() < 0.01);
        let start = |x_m, z_m| Motion {
            x_m,
            z_m,
            ..Motion::default()
        };
        let course = build_road_polygons_course(
            "test",
            std::slice::from_ref(&l_shape),
            start(2.0, 8.0),
            Vec::new(),
        )
        .unwrap();
        course.validate().unwrap();
        // 오목한 안쪽 모서리 밖(빈 곳)에서 시작하면 거부한다.
        assert!(
            build_road_polygons_course("test", &[l_shape], start(8.0, 2.0), Vec::new()).is_err()
        );
    }

    /// 넓은 직선 도로에서 시작해 정면 연석에 부딪혀 멈추고, 차체 기준점이 도로 밖으로 나가지 않는다.
    #[test]
    fn vehicle_is_kept_inside_by_curbs() {
        let polygons = vec![rect(-5.0, -40.0, 5.0, 10.0)];
        let course =
            build_road_polygons_course("test", &polygons, Motion::default(), Vec::new()).unwrap();
        course.validate().unwrap();
        let rects = course.collision_rects();
        let params = DynamicsParams::default();
        let mut state = VehicleState::default();
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        state
            .apply(VehicleCommand::RequestGear(Gear::Drive))
            .unwrap();
        state.set_input(VehicleInput {
            throttle: 0.6,
            ..VehicleInput::default()
        });
        let mut motion = course.start;
        let mut contact = ContactState::default();
        let index = PolygonIndex::new(&polygons);
        let impact = (0..64 * 15).find_map(|_| {
            let impact = step_with_obstacles(
                &mut state,
                &mut motion,
                &params,
                1.0 / 64.0,
                &VEHICLE_FOOTPRINT,
                &rects,
                &mut contact,
            );
            assert!(index.contains(Vec2::new(motion.x_m, motion.z_m)));
            impact
        });
        assert!(impact.is_some(), "never blocked");
        assert!(motion.z_m < -30.0, "{motion:?}");
    }

    /// 90도로 꺾인 중심선은 둥글게 다듬어져, 모든 꺾임이 작아지고 양 끝은 그대로다.
    #[test]
    fn smoothing_rounds_corners_and_keeps_ends() {
        let line = [Vec2::ZERO, Vec2::new(0.0, -40.0), Vec2::new(40.0, -40.0)];
        let smooth = smooth_line(&line, false);
        assert_eq!(smooth[0], line[0]);
        assert_eq!(*smooth.last().unwrap(), line[2]);
        let max_turn = smooth
            .windows(3)
            .map(|w| (w[1] - w[0]).angle_to(w[2] - w[1]).abs().to_degrees())
            .fold(0.0, f32::max);
        assert!(max_turn < 15.0, "max turn {max_turn}");
        // 깎는 길이 상한(8 m) 때문에 모서리에서 멀리 벗어나지 않는다.
        let corner_gap = smooth
            .iter()
            .map(|p| p.distance(line[1]))
            .fold(f32::INFINITY, f32::min);
        assert!(
            corner_gap > 1.0 && corner_gap < 8.0,
            "corner gap {corner_gap}"
        );
    }

    /// 막다른 끝은 연석 없이 열려 있어 차가 도로 밖으로 나갈 수 있고, 옆면에는 연석이 있다.
    #[test]
    fn dead_ends_are_open_and_sides_are_curbed() {
        let lines = [RoadLine {
            points: vec![Vec2::new(0.0, 10.0), Vec2::new(0.0, -30.0)],
            width_m: 7.0,
            closed: false,
        }];
        let course =
            build_road_lines_course("test", &lines, Motion::default(), Vec::new()).unwrap();
        course.validate().unwrap();
        let rects = course.collision_rects();
        assert!(!rects.is_empty());
        // 연석은 모두 옆면(x = ±3.5 바깥)에 있고, 끝(z < -30, z > 10)을 가로지르지 않는다.
        for rect in &rects {
            assert!(rect.center.x.abs() > 3.4, "{rect:?}");
        }
        let params = DynamicsParams::default();
        let mut state = VehicleState::default();
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        state
            .apply(VehicleCommand::RequestGear(Gear::Drive))
            .unwrap();
        state.set_input(VehicleInput {
            throttle: 0.6,
            ..VehicleInput::default()
        });
        let mut motion = course.start;
        let mut contact = ContactState::default();
        for _ in 0..(64 * 30) {
            let impact = step_with_obstacles(
                &mut state,
                &mut motion,
                &params,
                1.0 / 64.0,
                &VEHICLE_FOOTPRINT,
                &rects,
                &mut contact,
            );
            assert!(impact.is_none(), "blocked at {motion:?}");
        }
        assert!(motion.z_m < -40.0, "did not leave the open end: {motion:?}");
    }

    /// 끝끼리 만나는 두 도로는 하나로 이어지고(방향 맞춤), 이은 자리의 끝은 열린 끝이 아니다.
    #[test]
    fn end_to_end_roads_are_joined() {
        let line = |points: Vec<Vec2>| RoadLine {
            points,
            width_m: 7.0,
            closed: false,
        };
        // 두 번째 도로는 반대 방향으로 저장되어 있다.
        let lines = [
            line(vec![Vec2::new(0.0, 10.0), Vec2::new(0.0, -30.0)]),
            line(vec![Vec2::new(30.0, -60.0), Vec2::new(0.3, -30.2)]),
        ];
        let joined = join_lines(&lines);
        assert_eq!(joined.len(), 1, "{joined:?}");
        assert_eq!(joined[0].points.len(), 3);
        assert_eq!(joined[0].points[2], Vec2::new(30.0, -60.0));
        // 이은 꺾임 바깥쪽에 연석 틈이 없도록, 막다른 끝은 양쪽 끝뿐이다.
        let course =
            build_road_lines_course("test", &lines, Motion::default(), Vec::new()).unwrap();
        course.validate().unwrap();
        // 끝이 다른 도로 옆면에 닿는 T자 길은 막다른 끝이 아니다.
        let t_lines = [
            line(vec![Vec2::new(-30.0, -20.0), Vec2::new(30.0, -20.0)]),
            line(vec![Vec2::new(0.0, 10.0), Vec2::new(0.0, -20.0)]),
        ];
        assert!(!is_dead_end(Vec2::new(0.0, -20.0), 1, &t_lines));
        assert!(is_dead_end(Vec2::new(0.0, 10.0), 1, &t_lines));
    }

    /// 잘게 나뉜 경계 조각은 길이 합을 지키며 적은 수의 연석으로 합쳐진다.
    #[test]
    fn boundary_pieces_are_merged() {
        let pieces: Vec<BoundarySegment> = (0..10)
            .map(|i| BoundarySegment {
                a: Vec2::new(0.0, -(i as f32)),
                b: Vec2::new(0.0, -(i as f32) - 1.0),
                outward: Vec2::new(1.0, 0.0),
            })
            .collect();
        let merged = merge_segments(&pieces);
        assert_eq!(merged.len(), 1, "{merged:?}");
        assert!((merged[0].a.distance(merged[0].b) - 10.0).abs() < 1e-4);
        assert!(merged[0].outward.distance(Vec2::new(1.0, 0.0)) < 1e-4);
    }

    /// 저장소의 여수 맵 도로 코스가 열리고 형상 검사를 통과하며, 폭이 넓어졌다.
    #[test]
    fn yeosu_custom_roads_course_loads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/yeosu-expo/custom-roads.toml");
        let course = super::super::map::load_map_course(&path).unwrap();
        course.validate().unwrap();
        assert!(course.colliders.len() > 500, "{}", course.colliders.len());
        assert!(!course.areas.is_empty());
        assert!(course.notices.iter().any(|notice| notice.contains("x1.5")));
    }
}
