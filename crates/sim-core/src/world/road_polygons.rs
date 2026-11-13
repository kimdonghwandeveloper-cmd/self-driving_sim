//! 도로 면 다각형으로 만드는 주행 코스(사용자 제작 맵 도로, 여수 엑스포).
//!
//! 사용자가 Blender로 만든 맵의 도로 바닥면(`tools/blender/yeosu_background.py`가 내보낸 `road-polygons-v1` JSON)을
//! 그대로 노면으로 쓰고, 겹친 면들을 합친 영역의 바깥 경계선을 따라 연석을 세운다. 교차로처럼 면이 겹친 곳의
//! 안쪽 변에는 연석이 생기지 않으므로 어느 도로로든 들어갈 수 있다. 중심선·필렛 생성(4-1)은 쓰지 않는다.
//!
//! 경계 계산: 각 다각형 변을 다른 변과의 교차점에서 자르고, 조각 가운데에서 양옆으로 [`SIDE_PROBE_M`]만큼
//! 떨어진 두 점 중 한쪽만 도로 영역(다각형 합집합) 안이면 경계로 본다. 연석은 바깥쪽에 붙인다.
//! 바깥쪽 [`GAP_BRIDGE_M`] 안에 다시 도로가 있으면 맵의 이음 틈(띠 끝이 비스듬히 만난 V자 틈 등)으로 보고 연석을 두지 않는다.
//! 좌표는 코스 지면 (x = 동쪽, z = 남쪽) m이다.

use std::collections::HashMap;
use std::path::Path;

use glam::Vec2;
use serde::Deserialize;

use super::course::{Area, Collider, ColliderKind, CourseDef};
use super::road_geom::{heading_of, right_of};
use crate::vehicle::{GroundRect, Motion, VEHICLE_FOOTPRINT};

/// 코스 파일(TOML)과 다각형 자료(JSON)의 형식 이름.
pub const ROAD_POLYGONS_FORMAT: &str = "road-polygons-v1";
/// 연석 두께(m)와 표시 높이(m). 지도 코스와 같은 값.
const CURB_THICKNESS_M: f32 = super::map::CURB_THICKNESS_M;
const CURB_HEIGHT_M: f32 = 0.25;
/// 연석 직사각형을 변 양 끝으로 더 늘리는 길이(m). 이웃 연석과의 틈을 덮는다.
const CURB_OVERLAP_M: f32 = 0.02;
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
/// 다각형 점이 원점에서 떨어질 수 있는 최대 거리(m)와 다각형 개수 상한.
const MAX_DISTANCE_M: f32 = 5_000.0;
const MAX_POLYGONS: usize = 50_000;

/// 도로 면 코스 파일(TOML). 다각형 자료는 따로 둔다.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadPolygonsCourseFile {
    /// 형식 이름([`ROAD_POLYGONS_FORMAT`]).
    pub format: String,
    /// 로그에 쓰는 코스 이름.
    pub name: String,
    /// 다각형 자료 JSON 경로. 이 TOML 파일 위치 기준 상대 경로다.
    pub polygons: String,
    /// 시작 자세의 후륜축 중심 [x, z](m).
    pub start: [f32; 2],
    /// 시작 방향(deg, +Y축 회전, 0이면 -Z(북쪽)를 본다).
    pub start_heading_deg: f32,
    /// 주행 화면에 계속 표시할 안내.
    pub notices: Vec<String>,
}

/// 다각형 자료(JSON).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoadPolygonsData {
    /// 형식 이름([`ROAD_POLYGONS_FORMAT`]).
    format: String,
    /// 좌표 원점 [위도, 경도](deg). 기록용이며 변환에는 쓰지 않는다.
    #[allow(dead_code)]
    origin: [f64; 2],
    /// 만든 곳 설명. 기록용.
    #[allow(dead_code)]
    source: String,
    /// 도로 면 다각형 목록. 각 다각형은 [x, z] 점 3개 이상(닫는 점 중복 없음).
    polygons: Vec<Vec<[f32; 2]>>,
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

/// 코스 파일 내용과 그 파일이 있는 폴더로 도로 면 코스를 만든다.
pub fn load_road_polygons_course(text: &str, base_dir: &Path) -> Result<CourseDef, String> {
    let file: RoadPolygonsCourseFile =
        toml::from_str(text).map_err(|error| format!("invalid road polygons course: {error}"))?;
    if file.format != ROAD_POLYGONS_FORMAT {
        return Err(format!("unsupported format {:?}", file.format));
    }
    let path = base_dir.join(&file.polygons);
    let json = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let polygons = parse_polygons(&json)?;
    let [x, z] = file.start;
    let start = Motion {
        x_m: x,
        z_m: z,
        heading_rad: file.start_heading_deg.to_radians(),
        ..Motion::default()
    };
    build_road_polygons_course(&file.name, &polygons, start, file.notices)
}

/// 다각형 자료 JSON을 읽어 검사한다(형식, 점 개수, 유한·거리 범위). 넓이가 거의 0인 다각형은 뺀다.
fn parse_polygons(json: &str) -> Result<Vec<Vec<Vec2>>, String> {
    let data: RoadPolygonsData =
        serde_json::from_str(json).map_err(|error| format!("invalid road polygons: {error}"))?;
    if data.format != ROAD_POLYGONS_FORMAT {
        return Err(format!("unsupported polygons format {:?}", data.format));
    }
    if data.polygons.is_empty() || data.polygons.len() > MAX_POLYGONS {
        return Err(format!(
            "road polygons count {} is outside 1..={MAX_POLYGONS}",
            data.polygons.len()
        ));
    }
    let mut polygons = Vec::with_capacity(data.polygons.len());
    for (index, raw) in data.polygons.iter().enumerate() {
        if raw.len() < 3 {
            return Err(format!("polygon {index} has fewer than 3 points"));
        }
        let points: Vec<Vec2> = raw.iter().map(|&[x, z]| Vec2::new(x, z)).collect();
        if !points
            .iter()
            .all(|p| p.is_finite() && p.length() <= MAX_DISTANCE_M)
        {
            return Err(format!(
                "polygon {index} has a non-finite point or one beyond {MAX_DISTANCE_M} m"
            ));
        }
        if signed_area(&points).abs() >= MIN_POLYGON_AREA_M2 {
            polygons.push(points);
        }
    }
    Ok(polygons)
}

/// 도로 면 다각형으로 노면(면)·연석·시작 자세를 갖춘 코스를 만든다. 시작 자세가 도로 밖이거나 연석에 닿으면 오류.
pub fn build_road_polygons_course(
    name: &str,
    polygons: &[Vec<Vec2>],
    start: Motion,
    notices: Vec<String>,
) -> Result<CourseDef, String> {
    let index = PolygonIndex::new(polygons);
    if !index.contains(Vec2::new(start.x_m, start.z_m)) {
        return Err("start pose is not on a road polygon".to_owned());
    }
    let colliders: Vec<Collider> = union_boundary(polygons, &index)
        .iter()
        .map(|segment| Collider {
            rect: curb_rect(segment),
            height_m: CURB_HEIGHT_M,
            kind: ColliderKind::Curb,
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
    })
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

/// 다각형들의 합집합 영역 바깥 경계 조각을 구한다. 겹친 면 안쪽 변, 맞닿은 공유 변, 좁은 틈의 변은 빠진다.
pub fn union_boundary(polygons: &[Vec<Vec2>], index: &PolygonIndex) -> Vec<BoundarySegment> {
    let mut segments = Vec::new();
    for (owner, polygon) in polygons.iter().enumerate() {
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
    let mut triangles = Vec::new();
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
        let mut polygon_cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        let mut edge_cells: HashMap<(i32, i32), Vec<(usize, usize)>> = HashMap::new();
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
        let boundary = union_boundary(&polygons, &index);
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
        let boundary = union_boundary(&polygons, &index);
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
            for segment in union_boundary(&polygons, &index) {
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

    /// 저장소의 여수 맵 도로 코스가 열리고 형상 검사를 통과한다.
    #[test]
    fn yeosu_custom_roads_course_loads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/yeosu-expo/custom-roads.toml");
        let course = super::super::map::load_map_course(&path).unwrap();
        course.validate().unwrap();
        assert!(course.colliders.len() > 1000, "{}", course.colliders.len());
        assert!(!course.areas.is_empty());
    }
}
