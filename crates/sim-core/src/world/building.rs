//! 3D 건물 절차적 생성 (PLAN 4-12).
//!
//! 경로 양옆 150m 안의 건물 윤곽을 평평한 지붕 상자(로우폴리)로 세운다.
//! 높이는 `height` → `building:levels` × 층 높이(기본 3.0m) → 건물 종류별 기본값 순서로 정하고,
//! 기본값을 쓴 건물이 있으면 "실제 높이 아님"을 표시한다 (한국 OSM: 층수 약 16%, 높이 약 6%만 있음).
//! 도로 면(폭 + 여유)과 겹치는 건물은 빼거나 제외한다.

use std::collections::BTreeMap;
use std::path::Path;

use glam::Vec2;

use crate::vehicle::GroundRect;

use super::course::{Area, BuildingDef, Strip};
use super::map::{LatLon, project};
use super::places::{self, Place};

/// 건물 층당 기본 높이 (m).
pub const DEFAULT_LEVEL_HEIGHT_M: f32 = 3.0;

/// 경로 중심선으로부터 건물을 포함할 기본 복도 거리 (m).
pub const MAX_BUILDING_CORRIDOR_DISTANCE_M: f32 = 150.0;

/// 도로 면과의 최소 이격 거리 (m). 도로 면과 겹치는 건물을 제외할 때 여유를 둔다.
pub const ROAD_CLEARANCE_MARGIN_M: f32 = 0.5;

/// 실제 높이/층수 태그가 없어 기본값으로 추정된 건물이 포함되어 있을 때 코스 notices에 추가되는 안내 문구 (PLAN 4-12).
pub const ESTIMATED_HEIGHT_NOTICE: &str =
    "Some building heights are estimated defaults (not surveyed heights)";

/// 피트(ft) 단위를 미터(m)로 변환하는 계수.
const FEET_TO_METERS: f32 = 0.3048;

/// 문자열에서 `height` 태그의 미터 단위 높이를 파싱한다.
pub fn parse_height_tag(raw: &str) -> Option<f32> {
    let s = raw.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }
    if let Some(num) = s.strip_suffix("meters").or_else(|| s.strip_suffix('m')) {
        let val = num.trim().parse::<f32>().ok()?;
        return if val > 0.0 && val <= 1000.0 {
            Some(val)
        } else {
            None
        };
    }
    if let Some(num) = s.strip_suffix("feet").or_else(|| s.strip_suffix("ft")) {
        let val = num.trim().parse::<f32>().ok()?;
        let meters = val * FEET_TO_METERS;
        return if meters > 0.0 && meters <= 1000.0 {
            Some(meters)
        } else {
            None
        };
    }
    let val = s.parse::<f32>().ok()?;
    if val > 0.0 && val <= 1000.0 {
        Some(val)
    } else {
        None
    }
}

/// 문자열에서 `building:levels` 태그의 층수를 파싱한다.
pub fn parse_levels_tag(raw: &str) -> Option<f32> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let val = s.parse::<f32>().ok()?;
    if val > 0.0 && val <= 200.0 {
        Some(val)
    } else {
        None
    }
}

/// 건물 태그로부터 기본 높이(m)를 종류별로 산정한다.
fn default_height_by_kind(kind: &str, tags: &BTreeMap<String, String>) -> f32 {
    let building_tag = tags.get("building").map(|s| s.as_str()).unwrap_or("");
    let kind_lower = kind.to_lowercase();
    let b_lower = building_tag.to_lowercase();

    if b_lower == "apartments" || kind_lower.contains("apartments") {
        18.0
    } else if b_lower == "commercial"
        || b_lower == "retail"
        || b_lower == "office"
        || b_lower == "hotel"
        || b_lower == "supermarket"
        || kind_lower.contains("commercial")
        || kind_lower.contains("office")
        || kind_lower.contains("shop")
    {
        12.0
    } else if b_lower == "school"
        || b_lower == "university"
        || b_lower == "hospital"
        || b_lower == "civic"
        || b_lower == "public"
        || kind_lower.contains("school")
        || kind_lower.contains("hospital")
    {
        10.0
    } else if b_lower == "industrial"
        || b_lower == "warehouse"
        || b_lower == "hangar"
        || kind_lower.contains("industrial")
    {
        8.0
    } else if b_lower == "house"
        || b_lower == "residential"
        || b_lower == "detached"
        || b_lower == "terrace"
        || kind_lower.contains("residential")
        || kind_lower.contains("house")
    {
        6.0
    } else if b_lower == "garage"
        || b_lower == "garages"
        || b_lower == "shed"
        || b_lower == "kiosk"
        || kind_lower.contains("garage")
    {
        3.0
    } else {
        6.0
    }
}

/// OSM 태그와 종류로부터 건물의 최종 높이(m)와 기본값 추정 여부를 결정한다.
pub fn determine_building_height(tags: &BTreeMap<String, String>, kind: &str) -> (f32, bool) {
    if let Some(h) = tags.get("height").and_then(|s| parse_height_tag(s)) {
        return (h, false);
    }
    if let Some(levels) = tags
        .get("building:levels")
        .and_then(|s| parse_levels_tag(s))
    {
        return (levels * DEFAULT_LEVEL_HEIGHT_M, false);
    }
    (default_height_by_kind(kind, tags), true)
}

/// 2D 다각형의 부호 있는 면적(Shoelace formula)을 계산한다.
pub fn signed_polygon_area(vertices: &[Vec2]) -> f32 {
    let n = vertices.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let (a, b) = (vertices[i], vertices[(i + 1) % n]);
        sum += a.x * b.y - b.x * a.y;
    }
    sum * 0.5
}

/// 점 p가 2D 삼각형 (a, b, c) 내부에 있는지 검사한다.
pub fn point_in_triangle_2d(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let cross_ab = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let cross_bc = (c.x - b.x) * (p.y - b.y) - (c.y - b.y) * (p.x - b.x);
    let cross_ca = (a.x - c.x) * (p.y - c.y) - (a.y - c.y) * (p.x - c.x);

    let has_neg = cross_ab < -1e-5 || cross_bc < -1e-5 || cross_ca < -1e-5;
    let has_pos = cross_ab > 1e-5 || cross_bc > 1e-5 || cross_ca > 1e-5;

    !(has_neg && has_pos)
}

/// 2D 단순 다각형을 귀 잘라내기(Ear-clipping) 알고리즘으로 삼각분할하여 인덱스 목록을 반환한다.
pub fn triangulate_polygon_2d(vertices: &[Vec2]) -> Vec<[usize; 3]> {
    let n = vertices.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }

    let mut indices: Vec<usize> = (0..n).collect();
    let mut triangles = Vec::with_capacity(n - 2);

    let area = signed_polygon_area(vertices);
    let ccw = area > 0.0;

    let mut iterations = 0;
    let max_iterations = n * 4;

    while indices.len() > 3 && iterations < max_iterations {
        iterations += 1;
        let mut ear_found = false;
        let len = indices.len();

        for i in 0..len {
            let prev = indices[(i + len - 1) % len];
            let curr = indices[i];
            let next = indices[(i + 1) % len];

            let a = vertices[prev];
            let b = vertices[curr];
            let c = vertices[next];

            let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            let is_convex = if ccw { cross > 1e-6 } else { cross < -1e-6 };
            if !is_convex {
                continue;
            }

            let mut has_point_inside = false;
            for &p_idx in &indices {
                if p_idx == prev || p_idx == curr || p_idx == next {
                    continue;
                }
                if point_in_triangle_2d(vertices[p_idx], a, b, c) {
                    has_point_inside = true;
                    break;
                }
            }

            if !has_point_inside {
                triangles.push([prev, curr, next]);
                indices.remove(i);
                ear_found = true;
                break;
            }
        }

        if !ear_found {
            break;
        }
    }

    if indices.len() == 3 {
        triangles.push([indices[0], indices[1], indices[2]]);
    } else if triangles.is_empty() {
        // 볼록 다각형 단순 팬 방식 폴백
        for i in 1..n - 1 {
            triangles.push([0, i, i + 1]);
        }
    }

    triangles
}

/// 점 p에서 선분 a-b까지의 최단 거리(m)를 계산한다.
pub fn distance_point_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq <= 1e-6 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    let projection = a + ab * t;
    (p - projection).length()
}

/// 점 p에서 도로 중심선 목록까지의 최단 거리(m)를 계산한다.
pub fn distance_to_centerlines(p: Vec2, centerlines: &[Vec<Vec2>]) -> f32 {
    let mut min_dist = f32::INFINITY;
    for line in centerlines {
        for window in line.windows(2) {
            let dist = distance_point_to_segment(p, window[0], window[1]);
            if dist < min_dist {
                min_dist = dist;
            }
        }
    }
    min_dist
}

/// 점 c가 선분 a-b 위에 있는지 검사한다.
fn point_on_segment(c: Vec2, a: Vec2, b: Vec2) -> bool {
    let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    if cross.abs() > 1e-4 {
        return false;
    }
    let dot = (c.x - a.x) * (b.x - a.x) + (c.y - a.y) * (b.y - a.y);
    if dot < 0.0 {
        return false;
    }
    let len_sq = (b.x - a.x).powi(2) + (b.y - a.y).powi(2);
    dot <= len_sq
}

/// 두 선분 p1-p2와 q1-q2가 교차하는지 검사한다.
pub fn segments_intersect(p1: Vec2, p2: Vec2, q1: Vec2, q2: Vec2) -> bool {
    fn ccw(a: Vec2, b: Vec2, c: Vec2) -> f32 {
        (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
    }
    let d1 = ccw(p1, p2, q1);
    let d2 = ccw(p1, p2, q2);
    let d3 = ccw(q1, q2, p1);
    let d4 = ccw(q1, q2, p2);

    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    if d1.abs() < 1e-5 && point_on_segment(q1, p1, p2) {
        return true;
    }
    if d2.abs() < 1e-5 && point_on_segment(q2, p1, p2) {
        return true;
    }
    if d3.abs() < 1e-5 && point_on_segment(p1, q1, q2) {
        return true;
    }
    if d4.abs() < 1e-5 && point_on_segment(p2, q1, q2) {
        return true;
    }
    false
}

/// 두 다각형이 겹치는지(꼭짓점 포함, 변 교차 또는 이격 거리 이내) 검사한다.
pub fn polygon_overlaps_polygon(poly1: &[Vec2], poly2: &[Vec2], clearance: f32) -> bool {
    let n1 = poly1.len();
    let n2 = poly2.len();
    if n1 < 3 || n2 < 3 {
        return false;
    }
    // 1. AABB 1차 빠른 거부 검사
    let min_x1 = poly1.iter().map(|p| p.x).fold(f32::INFINITY, f32::min) - clearance;
    let max_x1 = poly1.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max) + clearance;
    let min_z1 = poly1.iter().map(|p| p.y).fold(f32::INFINITY, f32::min) - clearance;
    let max_z1 = poly1.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max) + clearance;

    let min_x2 = poly2.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
    let max_x2 = poly2.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
    let min_z2 = poly2.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    let max_z2 = poly2.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);

    if max_x1 < min_x2 || min_x1 > max_x2 || max_z1 < min_z2 || min_z1 > max_z2 {
        return false;
    }

    // 2. 꼭짓점 포함 검사 (한 다각형의 꼭짓점이 다른 다각형 내부에 있는지)
    for &p in poly1 {
        if point_inside_polygon(p, poly2) {
            return true;
        }
    }
    for &p in poly2 {
        if point_inside_polygon(p, poly1) {
            return true;
        }
    }

    // 3. 변끼리의 교차 검사 (십자형 겹침 검출)
    for i in 0..n1 {
        let p1 = poly1[i];
        let p2 = poly1[(i + 1) % n1];
        for j in 0..n2 {
            let q1 = poly2[j];
            let q2 = poly2[(j + 1) % n2];
            if segments_intersect(p1, p2, q1, q2) {
                return true;
            }
        }
    }

    // 4. 이격 거리(clearance) 검사
    if clearance > 0.0 {
        for &p in poly1 {
            for j in 0..n2 {
                if distance_point_to_segment(p, poly2[j], poly2[(j + 1) % n2]) <= clearance {
                    return true;
                }
            }
        }
        for &p in poly2 {
            for i in 0..n1 {
                if distance_point_to_segment(p, poly1[i], poly1[(i + 1) % n1]) <= clearance {
                    return true;
                }
            }
        }
    }

    false
}

/// 건물 윤곽이 도로 영역(GroundRect)과 겹치는지 검사한다.
pub fn polygon_overlaps_ground_rect(footprint: &[Vec2], rect: &GroundRect, clearance: f32) -> bool {
    let half_w = rect.half_size.x;
    let half_h = rect.half_size.y;
    let corners_local = [
        Vec2::new(-half_w, -half_h),
        Vec2::new(half_w, -half_h),
        Vec2::new(half_w, half_h),
        Vec2::new(-half_w, half_h),
    ];
    let (world_sin, world_cos) = rect.heading_rad.sin_cos();
    let rect_corners_world = [
        Vec2::new(
            rect.center.x + corners_local[0].x * world_cos - corners_local[0].y * world_sin,
            rect.center.y + corners_local[0].x * world_sin + corners_local[0].y * world_cos,
        ),
        Vec2::new(
            rect.center.x + corners_local[1].x * world_cos - corners_local[1].y * world_sin,
            rect.center.y + corners_local[1].x * world_sin + corners_local[1].y * world_cos,
        ),
        Vec2::new(
            rect.center.x + corners_local[2].x * world_cos - corners_local[2].y * world_sin,
            rect.center.y + corners_local[2].x * world_sin + corners_local[2].y * world_cos,
        ),
        Vec2::new(
            rect.center.x + corners_local[3].x * world_cos - corners_local[3].y * world_sin,
            rect.center.y + corners_local[3].x * world_sin + corners_local[3].y * world_cos,
        ),
    ];
    polygon_overlaps_polygon(footprint, &rect_corners_world, clearance)
}

/// 건물 윤곽이 굽은 길 차로 띠(Strip)의 노면과 겹치는지 검사한다.
pub fn polygon_overlaps_strip(footprint: &[Vec2], strip: &Strip, clearance: f32) -> bool {
    let n = strip.left.len().min(strip.right.len());
    if n < 2 {
        return false;
    }
    for i in 0..(n - 1) {
        let quad = [
            strip.left[i],
            strip.left[i + 1],
            strip.right[i + 1],
            strip.right[i],
        ];
        if polygon_overlaps_polygon(footprint, &quad, clearance) {
            return true;
        }
    }
    false
}

/// 건물 윤곽이 교차로 다각형(Area) 노면과 겹치는지 검사한다.
pub fn polygon_overlaps_area(footprint: &[Vec2], area: &Area, clearance: f32) -> bool {
    polygon_overlaps_polygon(footprint, &area.boundary, clearance)
}

/// 점 p가 단순 다각형 내부에 있는지 검사한다 (Ray-casting).
fn point_inside_polygon(p: Vec2, polygon: &[Vec2]) -> bool {
    let mut inside = false;
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[j]);
        if ((a.y > p.y) != (b.y > p.y)) && (p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 장소가 실제 3D 건물인지 여부를 판별한다 (공원, 운동장 등 비건물 제외).
pub fn is_building_place(kind: &str, tags: &BTreeMap<String, String>) -> bool {
    if let Some(b) = tags.get("building") {
        if b == "no" {
            return false;
        }
        if !b.is_empty() {
            return true;
        }
    }
    if tags.contains_key("leisure")
        || tags.contains_key("landuse")
        || tags.contains_key("natural")
        || tags.contains_key("boundary")
    {
        return false;
    }
    let k = kind.to_lowercase();
    if k == "building" || k.starts_with("building=") || k.starts_with("building:") {
        return true;
    }
    if k.starts_with("leisure=")
        || k.starts_with("landuse=")
        || k.starts_with("natural=")
        || k.starts_with("boundary=")
        || k.contains("park")
        || k.contains("pitch")
        || k.contains("garden")
    {
        return false;
    }
    if tags.contains_key("building:levels") || tags.contains_key("height") {
        return true;
    }
    false
}

/// 장소 목록에서 경로 양옆 복도(150m) 내의 건물을 추출하고 도로 겹침을 방지하여 건물 목록을 만든다.
pub fn build_course_buildings(
    places: &[Place],
    origin: LatLon,
    road_centerlines: &[Vec<Vec2>],
    road_surfaces: &[GroundRect],
    road_strips: &[Strip],
    road_areas: &[Area],
    max_corridor_dist_m: f32,
) -> (Vec<BuildingDef>, bool) {
    let mut buildings = Vec::new();
    let mut has_estimated = false;

    for place in places {
        // 윤곽이 없는 점 장소는 3D 건물로 생성하지 않음
        if place.outline.len() < 3 {
            continue;
        }

        // 건물이 아닌 장소(공원 leisure=park, 운동장, 주차장 등)는 제외
        if !is_building_place(&place.kind, &place.tags) {
            continue;
        }

        // 지면 로컬 (x, z) 좌표로 투영
        let footprint: Vec<Vec2> = place.outline.iter().map(|&p| project(origin, p)).collect();

        // 중심점 계산
        let count = footprint.len() as f32;
        let center = Vec2::new(
            footprint.iter().map(|p| p.x).sum::<f32>() / count,
            footprint.iter().map(|p| p.y).sum::<f32>() / count,
        );

        // 경로 중심선과의 복도 거리 검사
        if !road_centerlines.is_empty() {
            let dist = distance_to_centerlines(center, road_centerlines);
            if dist > max_corridor_dist_m {
                continue;
            }
        }

        // 도로 면(GroundRect), 차로 띠(Strip), 교차로 다각형(Area)과의 중첩 검사
        let overlaps_surface = road_surfaces
            .iter()
            .any(|rect| polygon_overlaps_ground_rect(&footprint, rect, ROAD_CLEARANCE_MARGIN_M));
        if overlaps_surface {
            continue;
        }

        let overlaps_strip = road_strips
            .iter()
            .any(|strip| polygon_overlaps_strip(&footprint, strip, ROAD_CLEARANCE_MARGIN_M));
        if overlaps_strip {
            continue;
        }

        let overlaps_area = road_areas
            .iter()
            .any(|area| polygon_overlaps_area(&footprint, area, ROAD_CLEARANCE_MARGIN_M));
        if overlaps_area {
            continue;
        }

        // 높이 산정
        let (height_m, is_est) = determine_building_height(&place.tags, &place.kind);
        if is_est {
            has_estimated = true;
        }

        buildings.push(BuildingDef {
            id: place.id.clone(),
            name: place.name.clone(),
            kind: place.kind.clone(),
            height_m,
            is_estimated: is_est,
            footprint,
        });
    }

    // 일관된 순서를 위해 ID로 정렬
    buildings.sort_by(|a, b| a.id.cmp(&b.id));
    (buildings, has_estimated)
}

/// 기준 디렉터리 주변에서 동반 장소/건물 파일(`places.osm.json`, `places-synthetic.osm.json`, `*.places.osm.json`)을 찾아 건물을 생성한다.
pub fn load_companion_buildings(
    base_dir: &Path,
    origin: LatLon,
    road_centerlines: &[Vec<Vec2>],
    road_surfaces: &[GroundRect],
    road_strips: &[Strip],
    road_areas: &[Area],
) -> (Vec<BuildingDef>, bool) {
    let candidate_names = ["places.osm.json", "places-synthetic.osm.json"];

    for name in &candidate_names {
        let path = base_dir.join(name);
        if !path.exists() {
            continue;
        }
        let Ok(places) = places::load_places(&path) else {
            continue;
        };
        if places.is_empty() {
            continue;
        }
        let (buildings, est) = build_course_buildings(
            &places,
            origin,
            road_centerlines,
            road_surfaces,
            road_strips,
            road_areas,
            MAX_BUILDING_CORRIDOR_DISTANCE_M,
        );
        if !buildings.is_empty() {
            return (buildings, est);
        }
    }

    // 디렉터리 내의 *.places.osm.json 정렬 탐색
    if let Ok(entries) = std::fs::read_dir(base_dir) {
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|s| s.ends_with(".places.osm.json"))
            })
            .collect();
        paths.sort();
        for path in paths {
            if let Ok(places) = places::load_places(&path) {
                let (buildings, est) = build_course_buildings(
                    &places,
                    origin,
                    road_centerlines,
                    road_surfaces,
                    road_strips,
                    road_areas,
                    MAX_BUILDING_CORRIDOR_DISTANCE_M,
                );
                if !buildings.is_empty() {
                    return (buildings, est);
                }
            }
        }
    }

    // 전국 자료 폴더(data/korea 또는 testdata/korea-yeosu)에서 장소 추출 시도
    let data_dirs = [Path::new("data/korea"), Path::new("testdata/korea-yeosu")];
    for data_dir in &data_dirs {
        if let Ok(korea) = super::korea_data::KoreaData::open(data_dir) {
            let radius_m = 800.0;
            let lat_delta = radius_m / 111_000.0;
            let cos_lat = origin.lat.to_radians().cos().abs().max(0.1);
            let lon_delta = radius_m / (111_000.0 * cos_lat);
            let bbox = super::map_fetch::Bbox {
                south: origin.lat - lat_delta,
                west: origin.lon - lon_delta,
                north: origin.lat + lat_delta,
                east: origin.lon + lon_delta,
            };
            if let Ok(region) = korea.region(&bbox)
                && !region.places.is_empty()
            {
                let (buildings, est) = build_course_buildings(
                    &region.places,
                    origin,
                    road_centerlines,
                    road_surfaces,
                    road_strips,
                    road_areas,
                    MAX_BUILDING_CORRIDOR_DISTANCE_M,
                );
                if !buildings.is_empty() {
                    return (buildings, est);
                }
            }
        }
    }

    (Vec::new(), false)
}

#[cfg(test)]
mod tests {
    use super::super::course::StripKind;
    use super::*;

    #[test]
    fn parse_height_tag_variations() {
        assert_eq!(parse_height_tag("15"), Some(15.0));
        assert_eq!(parse_height_tag("15.5m"), Some(15.5));
        assert_eq!(parse_height_tag(" 12.0 meters "), Some(12.0));
        assert_eq!(parse_height_tag("50ft"), Some(50.0 * 0.3048));
        assert_eq!(parse_height_tag("0"), None);
        assert_eq!(parse_height_tag("-5"), None);
        assert_eq!(parse_height_tag("invalid"), None);
    }

    #[test]
    fn parse_levels_tag_variations() {
        assert_eq!(parse_levels_tag("4"), Some(4.0));
        assert_eq!(parse_levels_tag(" 3.5 "), Some(3.5));
        assert_eq!(parse_levels_tag("0"), None);
        assert_eq!(parse_levels_tag("-2"), None);
        assert_eq!(parse_levels_tag("abc"), None);
    }

    #[test]
    fn building_height_priority_rule() {
        let mut tags = BTreeMap::new();
        // 1. 아무것도 없을 때 기본값 (종류별)
        let (h, est) = determine_building_height(&tags, "apartments");
        assert_eq!(h, 18.0);
        assert!(est);

        // 2. levels가 들어왔을 때
        tags.insert("building:levels".to_owned(), "5".to_owned());
        let (h, est) = determine_building_height(&tags, "apartments");
        assert_eq!(h, 15.0);
        assert!(!est);

        // 3. height가 들어왔을 때 (최우선)
        tags.insert("height".to_owned(), "22.5m".to_owned());
        let (h, est) = determine_building_height(&tags, "apartments");
        assert_eq!(h, 22.5);
        assert!(!est);
    }

    #[test]
    fn polygon_triangulation_convex_and_concave() {
        // 사각형
        let quad = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        let tris = triangulate_polygon_2d(&quad);
        assert_eq!(tris.len(), 2);

        // L자형 오목 다각형 (6개 꼭짓점)
        let l_shape = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 5.0),
            Vec2::new(5.0, 5.0),
            Vec2::new(5.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        let tris_l = triangulate_polygon_2d(&l_shape);
        assert_eq!(tris_l.len(), 4);
    }

    #[test]
    fn road_overlap_and_corridor_filtering() {
        let origin = LatLon {
            lat: 34.75,
            lon: 127.75,
        };
        let mut tags = BTreeMap::new();
        tags.insert("building".to_owned(), "residential".to_owned());

        // 1. 도로와 겹치는 건물 (x=0, z=-20 근처)
        let overlapping_place = Place {
            id: "way/101".to_owned(),
            name: "중첩 건물".to_owned(),
            kind: "building".to_owned(),
            tags: tags.clone(),
            outline: vec![
                LatLon {
                    lat: 34.75,
                    lon: 127.75,
                },
                LatLon {
                    lat: 34.75,
                    lon: 127.7501,
                },
                LatLon {
                    lat: 34.7501,
                    lon: 127.7501,
                },
                LatLon {
                    lat: 34.7501,
                    lon: 127.75,
                },
            ],
            center: origin,
            entrances: Vec::new(),
        };

        let road_surface = GroundRect::axis_aligned(Vec2::ZERO, Vec2::new(10.0, 50.0));
        let road_centerlines = vec![vec![Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)]];

        let (buildings, _) = build_course_buildings(
            &[overlapping_place],
            origin,
            &road_centerlines,
            &[road_surface],
            &[],
            &[],
            150.0,
        );

        // 원점의 도로는 겹치므로 제외되어야 함
        assert!(buildings.is_empty());
    }

    #[test]
    fn cross_shaped_overlap_detected() {
        // 도로: 중심 (0, 0), 폭 10 (half 5), 길이 40 (half 20), heading 0
        let road = GroundRect::axis_aligned(Vec2::ZERO, Vec2::new(10.0, 40.0));
        // 건물: 가로로 길게 도로를 가로지르는 직사각형 (x: -20..20, z: -2..2)
        // 꼭짓점들은 도로 직사각형 밖에 있지만 변끼리 십자형으로 교차함
        let cross_building = vec![
            Vec2::new(-20.0, -2.0),
            Vec2::new(20.0, -2.0),
            Vec2::new(20.0, 2.0),
            Vec2::new(-20.0, 2.0),
        ];
        assert!(polygon_overlaps_ground_rect(&cross_building, &road, 0.5));
    }

    #[test]
    fn building_inside_strip_and_area_excluded() {
        // 1. 차로 띠 (Strip) 안에 있는 건물
        let strip = Strip {
            kind: StripKind::Road,
            left: vec![Vec2::new(-3.0, 0.0), Vec2::new(-3.0, 50.0)],
            right: vec![Vec2::new(3.0, 0.0), Vec2::new(3.0, 50.0)],
        };
        let building_in_strip = vec![
            Vec2::new(-1.0, 10.0),
            Vec2::new(1.0, 10.0),
            Vec2::new(1.0, 20.0),
            Vec2::new(-1.0, 20.0),
        ];
        assert!(polygon_overlaps_strip(&building_in_strip, &strip, 0.5));

        // 2. 교차로 다각형 (Area) 안에 있는 건물
        let area = Area {
            center: Vec2::ZERO,
            boundary: vec![
                Vec2::new(-10.0, -10.0),
                Vec2::new(10.0, -10.0),
                Vec2::new(10.0, 10.0),
                Vec2::new(-10.0, 10.0),
            ],
        };
        let building_in_area = vec![
            Vec2::new(-2.0, -2.0),
            Vec2::new(2.0, -2.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(-2.0, 2.0),
        ];
        assert!(polygon_overlaps_area(&building_in_area, &area, 0.5));
    }

    #[test]
    fn korea_data_bbox_radius_within_2000m_limit() {
        let origin = LatLon {
            lat: 34.75,
            lon: 127.75,
        };
        let radius_m = 800.0;
        let lat_delta = radius_m / 111_000.0;
        let cos_lat = origin.lat.to_radians().cos().abs().max(0.1);
        let lon_delta = radius_m / (111_000.0 * cos_lat);
        let bbox = super::super::map_fetch::Bbox {
            south: origin.lat - lat_delta,
            west: origin.lon - lon_delta,
            north: origin.lat + lat_delta,
            east: origin.lon + lon_delta,
        };
        assert!(
            bbox.validate().is_ok(),
            "800m 반경 bbox는 2000m 상한을 만족해야 함"
        );
    }

    #[test]
    fn non_building_places_are_filtered_out() {
        let origin = LatLon {
            lat: 34.75,
            lon: 127.75,
        };
        let mut park_tags = BTreeMap::new();
        park_tags.insert("leisure".to_owned(), "park".to_owned());
        park_tags.insert("name".to_owned(), "중앙공원".to_owned());

        let park_place = Place {
            id: "way/201".to_owned(),
            name: "중앙공원".to_owned(),
            kind: "leisure=park".to_owned(),
            tags: park_tags,
            outline: vec![
                LatLon {
                    lat: 34.751,
                    lon: 127.751,
                },
                LatLon {
                    lat: 34.751,
                    lon: 127.752,
                },
                LatLon {
                    lat: 34.752,
                    lon: 127.752,
                },
                LatLon {
                    lat: 34.752,
                    lon: 127.751,
                },
            ],
            center: LatLon {
                lat: 34.7515,
                lon: 127.7515,
            },
            entrances: Vec::new(),
        };

        let road_centerlines = vec![vec![Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)]];
        let (buildings, _) = build_course_buildings(
            &[park_place],
            origin,
            &road_centerlines,
            &[],
            &[],
            &[],
            150.0,
        );
        assert!(buildings.is_empty(), "공원은 건물로 생성되지 않아야 합니다");
    }
}
